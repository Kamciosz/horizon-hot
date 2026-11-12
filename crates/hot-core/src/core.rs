//! PL: Rdzeń: lista aktywnych modów, przełączanie generacji na granicy ticku, powrót do
//! poprzedniej generacji po panice (D4), próba 6000 ticków przed uznaniem generacji za dobrą
//! (D13), komenda `hot` w konsoli. Wszystko poza wczytaniem pliku dzieje się w wątku serwera.
//!
//! EN: The core: the active mod list, generation switches at the tick boundary, falling back to
//! the previous generation after a panic (D4), a 6000-tick probation before a generation counts as
//! good (D13), the `hot` console command. Everything except file loading runs on the server thread.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use hot_abi::{flow, kind, Buf, HostApi, Str};
use hot_diag::{DiagError, LogConfig, Logger, Redactor};
use hot_sdk::{Flow, Host, HotMod};
use serde_json::{json, Value};

use crate::loader::{self, Gen, Outcome};
use crate::store::{self, ModRecord, ModsFile, Paths};
use crate::wrap::ModCall;
use crate::{codes, VERSION};

/// PL: Generacja jest dobra po 5 minutach bez paniki (D13): 5 × 60 × 20 TPS.
/// EN: A generation is good after 5 minutes without a panic (D13): 5 × 60 × 20 TPS.
pub const GOOD_AFTER_TICKS: u64 = 6000;

/// PL: Ile ostatnich błędów trzyma `status.json`. EN: How many recent errors `status.json` keeps.
const RECENT_ERRORS: usize = 10;

struct Active {
    gen: Gen,
    generation: u64,
    instance: *mut c_void,
    /// PL: Generacja sprzed bieżącej, cel powrotu po panice. EN: Fallback after a panic.
    previous: Option<(Gen, u64)>,
    /// PL: Ostatnia generacja po próbie (D13). EN: The last generation that passed probation.
    good: Option<(Gen, u64)>,
    /// PL: Stan, który dostała bieżąca generacja; wraca do poprzedniej po panice.
    /// EN: The state the current generation got; handed to the previous one after a panic.
    snapshot: Vec<u8>,
    ok_ticks: u64,
}

/// PL: Stan rdzenia; jedna instancja na generację `hot-core`. EN: Core state, one per generation.
pub struct Core {
    paths: Paths,
    log: Arc<Logger>,
    mods: Vec<Active>,
    tx: Sender<Outcome>,
    rx: Receiver<Outcome>,
    seq: u64,
    pending: Vec<PathBuf>,
    errors: Vec<Value>,
    tick: u64,
}

impl HotMod for Core {
    fn init(host: &mut Host<'_>, _state: &[u8]) -> Self {
        let mut home = host.data_dir();
        if home.as_os_str().is_empty() {
            home = std::env::current_dir().unwrap_or_default();
        }
        let paths = Paths { home };
        let log = Arc::new(open_logger(&paths, host));
        // PL: Hak paniki tej kopii `hot-core` (każda generacja rdzenia ma własną bibliotekę
        // standardową); raz, bo w testach ta sama kopia startuje wiele razy.
        // EN: The panic hook of this `hot-core` copy (every core generation has its own standard
        // library); once, as tests start the same copy many times.
        static HOOK: std::sync::Once = std::sync::Once::new();
        HOOK.call_once(|| hot_diag::install_panic_hook(Arc::clone(&log)));
        let (tx, rx) = channel();
        let mut core = Core {
            paths,
            log,
            mods: Vec::new(),
            tx,
            rx,
            seq: 0,
            pending: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        };
        core.restore(host.raw());
        core
    }

    fn on_raw_event(&mut self, host: &mut Host<'_>, event: &hot_abi::Event) -> Flow {
        let api = host.raw();
        if event.kind == kind::CONSOLE_COMMAND {
            // PL: Serwer bez graczy stoi w pauzie i nie woła ticku, ale komendy konsoli działają;
            // przełączenie tutaj też jest między tickami, więc `hot status` domyka wczytanie.
            // EN: An empty server is paused and fires no tick, but console commands still run;
            // switching here is also between ticks, so `hot status` completes a load.
            self.apply_loaded(api);
            // SAFETY: napis ważny w wywołaniu. EN: the string is valid for the call.
            let line = unsafe { event.text.as_str() }.unwrap_or("").to_owned();
            if let Some(args) = hot_args(&line) {
                self.command(api, args, event);
                return Flow::Cancel;
            }
        }
        if event.kind == kind::SERVER_TICK {
            self.tick = event.number;
            self.apply_loaded(api);
        }
        let flow = self.dispatch(api, event);
        if event.kind == kind::SERVER_TICK {
            self.count_tick();
        }
        flow
    }

    fn save(&mut self, host: &mut Host<'_>) -> Vec<u8> {
        let api = host.raw();
        for i in 0..self.mods.len() {
            let state = self.deactivate(api, i);
            let id = self.mods[i].gen.id.clone();
            if let Err(e) = store::write_atomic(&self.paths.saved(&id), &state) {
                self.error(&e);
            }
        }
        if !self.pending.is_empty() {
            self.log.warn(
                "hot-core stopped while hot mod files were still loading; run hot load again for them",
                json!({ "pending": self.pending.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() }),
            );
        }
        self.write_mods(true);
        self.mods.clear();
        self.write_status();
        self.log.info("hot-core stopped cleanly", json!({}));
        Vec::new()
    }
}

/// PL: `hot ...` albo `/hot ...` → reszta po słowie `hot`. EN: Arguments after the `hot` word.
fn hot_args(line: &str) -> Option<&str> {
    let line = line.trim().trim_start_matches('/');
    let rest = line.strip_prefix("hot")?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

fn open_logger(paths: &Paths, host: &mut Host<'_>) -> Logger {
    let primary = Logger::open(
        LogConfig::new("hot-core", VERSION, paths.logs()),
        Redactor::new(),
    );
    match primary {
        Ok(l) => l,
        Err(e) => {
            let dir = std::env::temp_dir().join("hot-core-logs");
            host.error(&format!(
                "{e}; hot-core logs go to {} instead",
                dir.display()
            ));
            match Logger::open(LogConfig::new("hot-core", VERSION, &dir), Redactor::new()) {
                Ok(l) => l,
                // PL: Panika w `init` wraca do `hot-host` jako nieudany start rdzenia z tym opisem.
                // EN: A panic in `init` reaches `hot-host` as a failed core start with this text.
                Err(e) => panic!("hot-core cannot open any log file (logs/ and {}): {e}; check disk space and permissions", dir.display()),
            }
        }
    }
}

impl Core {
    // ---------- PL: start i zatrzymanie / EN: start and stop ----------

    fn restore(&mut self, api: &HostApi) {
        let file = match store::read_mods(&self.paths) {
            Ok(f) => f,
            Err(e) => {
                self.error(&e);
                ModsFile::default()
            }
        };
        for record in &file.mods {
            self.restore_one(api, record, file.clean_stop);
        }
        let keep: Vec<PathBuf> = self
            .mods
            .iter()
            .flat_map(|m| {
                [
                    Some(m.gen.file.clone()),
                    m.good.as_ref().map(|(g, _)| g.file.clone()),
                ]
            })
            .flatten()
            .collect();
        store::prune_generations(&self.paths, &keep);
        // PL: Od teraz każde zatrzymanie bez `save` to awaria. EN: From now on, a stop without
        // `save` counts as a crash.
        self.write_mods(false);
        self.write_status();
        self.log.info(
            "hot-core started",
            json!({ "home": self.paths.home.display().to_string(), "mods": self.mods.iter().map(|m| m.gen.id.clone()).collect::<Vec<_>>(), "after_clean_stop": file.clean_stop }),
        );
    }

    /// PL: Po czystym zatrzymaniu wraca bieżąca generacja, po awarii ostatnia dobra (D13).
    /// EN: After a clean stop the current generation returns, after a crash the last good one.
    fn restore_one(&mut self, api: &HostApi, record: &ModRecord, clean_stop: bool) {
        let (path, generation) = if clean_stop {
            (record.current.clone(), record.generation)
        } else if let Some((good, n)) = &record.good {
            if *good != record.current {
                self.log.warn(
                    &format!(
                        "The server stopped without a clean shutdown; hot mod \"{}\" starts from its last good generation {n} instead of the unproven generation {}",
                        record.id, record.generation
                    ),
                    json!({ "mod": record.id, "skipped": record.current.display().to_string(), "loaded": good.display().to_string() }),
                );
            }
            (good.clone(), *n)
        } else {
            self.log.warn(
                &format!(
                    "The server stopped without a clean shutdown; hot mod \"{}\" has no generation that passed probation and stays off until the next hot load",
                    record.id
                ),
                json!({ "mod": record.id, "skipped": record.current.display().to_string() }),
            );
            return;
        };
        let gen = match loader::open(&path, &record.source) {
            Ok(g) => g,
            Err(e) => return self.error(&e),
        };
        let good = match &record.good {
            Some((g, n)) if *g == path => Some((gen.clone(), *n)),
            Some((g, n)) => match loader::open(g, &record.source) {
                Ok(x) => Some((x, *n)),
                Err(e) => {
                    self.error(&e);
                    None
                }
            },
            None => None,
        };
        let proven = good.as_ref().is_some_and(|(g, _)| g.file == gen.file);
        let state = store::take_saved(&self.paths, &gen.id);
        let mut candidates = vec![(gen, generation)];
        if !proven {
            candidates.extend(good.clone());
        }
        for (k, (gen, generation)) in candidates.iter().enumerate() {
            if let Some(instance) = self.init_gen(api, gen, *generation, &state) {
                let is_good = good.as_ref().is_some_and(|(g, _)| g.file == gen.file);
                self.mods.push(Active {
                    gen: gen.clone(),
                    generation: *generation,
                    instance,
                    previous: if k == 0 && !proven {
                        good.clone()
                    } else {
                        None
                    },
                    good: good.clone(),
                    snapshot: state,
                    ok_ticks: if is_good { GOOD_AFTER_TICKS } else { 0 },
                });
                return;
            }
        }
        if let Err(e) = store::write_atomic(&self.paths.saved(&record.id), &state) {
            self.error(&e);
        }
    }

    // ---------- PL: wywołania modów / EN: mod calls ----------

    /// PL: Wywołuje mod przez tabelę z jego id; zwraca wynik i opis paniki od `hot-sdk`.
    /// EN: Calls the mod through its own table; returns the result and the `hot-sdk` panic report.
    fn with_call<R>(
        &self,
        outer: &HostApi,
        gen: &Gen,
        generation: u64,
        f: impl FnOnce(&HostApi) -> R,
    ) -> (R, Option<String>) {
        let data_dir = self.paths.data(&gen.id);
        let mut call = ModCall {
            outer,
            id: &gen.id,
            generation,
            data_dir: &data_dir,
            log: &self.log,
            panic: None,
        };
        let api = call.api();
        let result = f(&api);
        (result, call.panic)
    }

    fn init_gen(
        &mut self,
        outer: &HostApi,
        gen: &Gen,
        generation: u64,
        state: &[u8],
    ) -> Option<*mut c_void> {
        let (instance, panic) = self.with_call(outer, gen, generation, |api| {
            (gen.vtable.init)(api, Str::from_bytes(state))
        });
        if instance.is_null() {
            self.panicked(gen, generation, "init", panic, "");
            None
        } else {
            Some(instance)
        }
    }

    fn deactivate(&mut self, outer: &HostApi, i: usize) -> Vec<u8> {
        let (gen, generation, instance) = {
            let m = &self.mods[i];
            (m.gen.clone(), m.generation, m.instance)
        };
        let (buf, panic) = self.with_call(outer, &gen, generation, |api| {
            (gen.vtable.deactivate)(instance, api)
        });
        let state = take_buf(&gen, buf);
        if panic.is_some() {
            self.panicked(
                &gen,
                generation,
                "deactivate",
                panic,
                "The mod's state from this generation is lost; the next generation starts empty",
            );
        }
        state
    }

    fn dispatch(&mut self, outer: &HostApi, event: &hot_abi::Event) -> Flow {
        let mut i = 0;
        while i < self.mods.len() {
            let (result, stayed) = self.call_one(outer, i, event);
            if result == flow::CANCEL {
                return Flow::Cancel;
            }
            if stayed {
                i += 1;
            }
        }
        Flow::Continue
    }

    /// PL: Zdarzenie dla jednego moda. Zwraca wynik moda i to, czy mod nadal jest na liście
    /// (po panice może zniknąć). EN: An event for one mod. Returns the mod's result and whether
    /// the mod is still listed (it may be gone after a panic).
    fn call_one(&mut self, outer: &HostApi, i: usize, event: &hot_abi::Event) -> (u32, bool) {
        let (gen, generation, instance) = {
            let m = &self.mods[i];
            (m.gen.clone(), m.generation, m.instance)
        };
        let (result, panic) = self.with_call(outer, &gen, generation, |api| {
            (gen.vtable.on_event)(instance, api, event)
        });
        if result == flow::PANICKED {
            let stayed = self.fall_back(outer, i, event.kind, panic);
            return (result, stayed);
        }
        (result, true)
    }

    /// PL: Po panice w `on_event`: poprzednia generacja dostaje stan sprzed bieżącej. Gdy jej nie
    /// ma albo też padnie, mod się wyłącza, a stan trafia do `saved/`. Zwraca, czy mod został.
    /// EN: After a panic in `on_event`: the previous generation gets the state from before the
    /// current one. With no previous generation, or when it fails too, the mod turns off and the
    /// state goes to `saved/`. Returns whether the mod stayed.
    fn fall_back(
        &mut self,
        outer: &HostApi,
        i: usize,
        event_kind: u32,
        panic: Option<String>,
    ) -> bool {
        let gen = self.mods[i].gen.clone();
        let generation = self.mods[i].generation;
        let snapshot = std::mem::take(&mut self.mods[i].snapshot);
        let previous = self.mods[i].previous.take();
        let hook = format!("on_event (kind {event_kind})");
        if let Some((prev, prev_generation)) = previous {
            self.panicked(
                &gen,
                generation,
                &hook,
                panic,
                &format!("Generation {prev_generation} of the mod is active again with the state it had before generation {generation}"),
            );
            if let Some(instance) = self.init_gen(outer, &prev, prev_generation, &snapshot) {
                let m = &mut self.mods[i];
                m.ok_ticks = if m.good.as_ref().is_some_and(|(g, _)| g.file == prev.file) {
                    GOOD_AFTER_TICKS
                } else {
                    0
                };
                m.gen = prev;
                m.generation = prev_generation;
                m.instance = instance;
                m.snapshot = snapshot;
                self.write_mods(false);
                self.write_status();
                return true;
            }
        } else {
            self.panicked(
                &gen,
                generation,
                &hook,
                panic,
                "The mod has no earlier generation in this run, so it is off until the next hot load; its state is in hot-state/saved/",
            );
        }
        if let Err(e) = store::write_atomic(&self.paths.saved(&gen.id), &snapshot) {
            self.error(&e);
        }
        self.mods.remove(i);
        self.write_mods(false);
        self.write_status();
        false
    }

    fn count_tick(&mut self) {
        let mut promoted = Vec::new();
        for m in &mut self.mods {
            m.ok_ticks = m.ok_ticks.saturating_add(1);
            if m.ok_ticks == GOOD_AFTER_TICKS {
                m.good = Some((m.gen.clone(), m.generation));
                promoted.push((m.gen.id.clone(), m.generation));
            }
        }
        if !promoted.is_empty() {
            for (id, generation) in &promoted {
                self.log.info(
                    &format!("Hot mod \"{id}\" generation {generation} passed probation and is now the last good generation"),
                    json!({ "mod": id, "generation": generation, "ticks": GOOD_AFTER_TICKS }),
                );
            }
            self.write_mods(false);
            self.write_status();
        }
    }

    // ---------- PL: nowe generacje / EN: new generations ----------

    fn start_load(&mut self, source: PathBuf) {
        self.seq += 1;
        self.pending.push(source.clone());
        loader::spawn(source, self.paths.gen_dir(), self.seq, self.tx.clone());
        self.write_status();
    }

    fn apply_loaded(&mut self, outer: &HostApi) {
        while let Ok(outcome) = self.rx.try_recv() {
            match outcome {
                Outcome::Ready(gen) => {
                    self.pending.retain(|p| *p != gen.source);
                    self.switch(outer, gen);
                }
                Outcome::Failed(source, e) => {
                    self.pending.retain(|p| *p != source);
                    self.error(&e);
                }
            }
            self.write_status();
        }
    }

    /// PL: Nowa generacja na miejsce starej (albo nowy mod). Stan starej przechodzi do nowej.
    /// EN: A new generation replaces the old one (or a new mod). The old state passes over.
    fn switch(&mut self, outer: &HostApi, gen: Gen) {
        let Some(i) = self.mods.iter().position(|m| m.gen.id == gen.id) else {
            if let Some(instance) = self.init_gen(outer, &gen, 1, &[]) {
                self.log.info(
                    &format!("Hot mod \"{}\" {} is active as generation 1", gen.id, gen.version),
                    json!({ "mod": gen.id, "generation": 1, "file": gen.file.display().to_string(), "source": gen.source.display().to_string() }),
                );
                self.mods.push(Active {
                    gen,
                    generation: 1,
                    instance,
                    previous: None,
                    good: None,
                    snapshot: Vec::new(),
                    ok_ticks: 0,
                });
                self.write_mods(false);
            }
            return;
        };
        let state = self.deactivate(outer, i);
        let old = self.mods[i].gen.clone();
        let old_generation = self.mods[i].generation;
        let generation = old_generation + 1;
        if let Some(instance) = self.init_gen(outer, &gen, generation, &state) {
            self.log.info(
                &format!(
                    "Hot mod \"{}\" {} is active as generation {generation}, replacing generation {old_generation}",
                    gen.id, gen.version
                ),
                json!({ "mod": gen.id, "generation": generation, "file": gen.file.display().to_string(), "state_bytes": state.len() }),
            );
            let m = &mut self.mods[i];
            m.previous = Some((old, old_generation));
            m.gen = gen;
            m.generation = generation;
            m.instance = instance;
            m.snapshot = state;
            m.ok_ticks = 0;
        } else if let Some(instance) = self.init_gen(outer, &old, old_generation, &state) {
            self.mods[i].instance = instance;
            self.mods[i].snapshot = state;
        } else {
            if let Err(e) = store::write_atomic(&self.paths.saved(&old.id), &state) {
                self.error(&e);
            }
            self.mods.remove(i);
        }
        self.write_mods(false);
    }

    // ---------- PL: komenda `hot` / EN: the `hot` command ----------

    /// PL: `hot …` z konsoli albo od operatora; `from` to zdarzenie z komendą (gracz, jeśli był).
    /// EN: `hot …` from the console or an op; `from` is the command event (with its player, if any).
    fn command(&mut self, api: &HostApi, args: &str, from: &hot_abi::Event) {
        let (verb, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        let rest = rest.trim();
        let mut out: Vec<String> = Vec::new();
        match verb {
            "load" if !rest.is_empty() => {
                let mut path = PathBuf::from(rest);
                if path.is_relative() {
                    path = self.paths.home.join(path);
                }
                if path.is_file() {
                    out.push(format!(
                        "Ładuję {} w tle; przełączenie przy najbliższym ticku.",
                        path.display()
                    ));
                    self.start_load(path);
                } else {
                    out.push(format!("Nie ma pliku {}.", path.display()));
                }
            }
            "unload" if !rest.is_empty() => {
                if let Some(i) = self.mods.iter().position(|m| m.gen.id == rest) {
                    let state = self.deactivate(api, i);
                    if let Err(e) = store::write_atomic(&self.paths.saved(rest), &state) {
                        self.error(&e);
                    }
                    self.mods.remove(i);
                    self.write_mods(false);
                    self.write_status();
                    self.log.info(
                        &format!("Hot mod \"{rest}\" was unloaded by the hot unload command"),
                        json!({ "mod": rest }),
                    );
                    out.push(format!(
                        "Mod {rest} wyłączony; stan zapisany na następne ładowanie."
                    ));
                } else {
                    out.push(format!("Nie ma aktywnego moda {rest}."));
                }
            }
            "list" | "status" => {
                out.push(format!(
                    "hot-core {VERSION}, ABI {}, mody: {}, w kolejce: {}",
                    hot_abi::ABI_MAJOR,
                    self.mods.len(),
                    self.pending.len()
                ));
                for m in &self.mods {
                    let probation = if m.ok_ticks >= GOOD_AFTER_TICKS {
                        "dobra".to_owned()
                    } else {
                        format!("próba {}/{GOOD_AFTER_TICKS}", m.ok_ticks)
                    };
                    out.push(format!(
                        "  {} {} — generacja {}, {probation}",
                        m.gen.id, m.gen.version, m.generation
                    ));
                }
                out.extend(hotd_lines(&self.paths.state().join("hotd.json")));
            }
            _ if self.mods.iter().any(|m| m.gen.id == verb) => {
                // PL: `hot <mod> …` trafia tylko do tego moda jako komenda konsoli `<mod> …` (D5).
                // EN: `hot <mod> …` goes only to that mod as the console command `<mod> …` (D5).
                let i = self.mods.iter().position(|m| m.gen.id == verb).unwrap_or(0);
                let mut event = *from;
                event.text = Str::from_str(args);
                match self.call_one(api, i, &event).0 {
                    flow::CANCEL => {}
                    flow::PANICKED => out.push(format!(
                        "Mod {verb} spanikował przy tej komendzie; szczegóły w logs/hot-core.jsonl (HOT-E0403)."
                    )),
                    _ => out.push(format!("Mod {verb} nie obsłużył komendy „{args}”.")),
                }
            }
            _ => {
                out.push(
                    "Użycie: hot load <plik> | hot unload <mod> | hot list | hot <mod> [argumenty]"
                        .to_owned(),
                );
            }
        }
        let mut host = Host::new(api);
        for line in out {
            host.reply(&line);
        }
    }

    // ---------- PL: pliki stanu i błędy / EN: state files and errors ----------

    fn panicked(
        &mut self,
        gen: &Gen,
        generation: u64,
        hook: &str,
        report: Option<String>,
        state: &str,
    ) {
        let report = report
            .unwrap_or_else(|| "panicked at unknown place: no report from hot-sdk".to_owned());
        let mut lines = report.lines();
        let first = lines.next().unwrap_or("").to_owned();
        let mut e = DiagError::new(
            codes::MOD_PANICKED,
            format!("Hot mod \"{}\" generation {generation} panicked in {hook}", gen.id),
            "Fix the panic at the place named in cause (the file is in the mod's source in the server repository), add a test for it and push; hotd loads the fixed build",
        )
        .expected(format!("{hook} returns without a panic"))
        .cause(first.clone())
        .context("mod", gen.id.clone())
        .context("version", gen.version.clone())
        .context("generation", generation)
        .context("file", gen.file.display().to_string())
        .context("source", gen.source.display().to_string())
        .context("tick", self.tick)
        .chain_line(lines.collect::<Vec<_>>().join("\n"));
        if let Some((file, line)) = panic_place(&first) {
            e = e.source_at(&file, line);
        }
        if !state.is_empty() {
            e = e.state(state);
        }
        self.error(&e);
    }

    fn error(&mut self, e: &DiagError) {
        self.log.error(e);
        let f = e.fields();
        self.errors
            .push(json!({ "code": f["code"], "what": f["what"], "context": f["context"] }));
        if self.errors.len() > RECENT_ERRORS {
            self.errors.remove(0);
        }
    }

    fn write_mods(&mut self, clean_stop: bool) {
        let file = ModsFile {
            clean_stop,
            mods: self
                .mods
                .iter()
                .map(|m| ModRecord {
                    id: m.gen.id.clone(),
                    version: m.gen.version.clone(),
                    generation: m.generation,
                    source: m.gen.source.clone(),
                    current: m.gen.file.clone(),
                    good: m.good.as_ref().map(|(g, n)| (g.file.clone(), *n)),
                })
                .collect(),
        };
        if let Err(e) = store::write_mods(&self.paths, &file) {
            self.error(&e);
        }
    }

    fn write_status(&mut self) {
        let mods: Vec<Value> = self
            .mods
            .iter()
            .map(|m| {
                json!({
                    "id": m.gen.id,
                    "version": m.gen.version,
                    "generation": m.generation,
                    "good": m.ok_ticks >= GOOD_AFTER_TICKS,
                    "probation_ticks": m.ok_ticks.min(GOOD_AFTER_TICKS),
                    "file": m.gen.file.display().to_string(),
                    "source": m.gen.source.display().to_string(),
                })
            })
            .collect();
        let v = json!({
            "component": "hot-core",
            "version": VERSION,
            "abi": hot_abi::ABI_MAJOR,
            "updated": hot_diag::utc_timestamp(std::time::SystemTime::now()),
            "tick": self.tick,
            "mods": mods,
            "pending": self.pending.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "recent_errors": self.errors,
        });
        if let Err(e) = store::write_atomic(&self.paths.status(), format!("{v:#}\n").as_bytes()) {
            // PL: Bez `error`, żeby nie zapętlić zapisu statusu. EN: Not `error`, to avoid a loop.
            self.log.error(&e);
        }
    }
}

/// PL: Stan `hotd` z `hot-state/hotd.json` dla `/hot status` w grze (D21): commit repo
/// i zastosowany, na co czeka, ostatni nieudany commit. Brak pliku = `hotd` nie działa.
/// EN: `hotd` state from `hot-state/hotd.json` for `/hot status` in game (D21).
fn hotd_lines(path: &std::path::Path) -> Vec<String> {
    let Some(d) = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    else {
        return vec!["hotd: brak stanu — hotd nie działa albo nie śledzi repo".to_owned()];
    };
    let short = |v: &Value| {
        v.as_str()
            .map_or_else(|| "brak".to_owned(), |s| s.get(..7).unwrap_or(s).to_owned())
    };
    let mut out = vec![format!(
        "hotd: repo {}, zastosowany {}",
        short(&d["head"]),
        short(&d["applied"])
    )];
    if let Some(w) = d["waiting"].as_str() {
        out.push(format!("  czeka: {w}"));
    }
    if let Some(f) = d["failed"].as_array().and_then(|a| a.last()) {
        if f["commit"] == d["head"] {
            out.push(format!(
                "  commit {} nie wszedł: {}",
                short(&f["commit"]),
                f["reason"].as_str().unwrap_or("")
            ));
        }
    }
    out
}

/// PL: `panicked at src/lib.rs:12:5: ...` → (`src/lib.rs`, 12). EN: Parses the panic place.
fn panic_place(first: &str) -> Option<(String, u32)> {
    let rest = first.strip_prefix("panicked at ")?;
    let place = rest.split(": ").next()?;
    let mut parts = place.rsplitn(3, ':');
    let _column = parts.next()?;
    let line = parts.next()?.parse().ok()?;
    let file = parts.next()?.to_owned();
    Some((file, line))
}

fn take_buf(gen: &Gen, buf: Buf) -> Vec<u8> {
    if buf.ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: bufor z `deactivate` tego moda, ważny do `free_buf`. EN: valid until `free_buf`.
    let bytes = unsafe { std::slice::from_raw_parts(buf.ptr, buf.len) }.to_vec();
    (gen.vtable.free_buf)(buf);
    bytes
}
