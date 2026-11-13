//! PL: Rdzeń: lista aktywnych modów, przełączanie generacji na granicy ticku, powrót do
//! poprzedniej generacji po panice (D4), próba 6000 ticków przed uznaniem generacji za dobrą
//! (D13), komenda `hot` w konsoli. Wszystko poza wczytaniem pliku dzieje się w wątku serwera.
//!
//! EN: The core: the active mod list, generation switches at the tick boundary, falling back to
//! the previous generation after a panic (D4), a 6000-tick probation before a generation counts as
//! good (D13), the `hot` console command. Everything except file loading runs on the server thread.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use hot_abi::{flow, kind, Buf, Event, HostApi, Str, Uuid};
use hot_diag::{DiagError, LogConfig, Logger, Redactor};
use hot_sdk::{Flow, Host, HotMod};
use serde_json::{json, Value};

use crate::loader::{self, Gen, Outcome};
use crate::slots::SlotNames;
use crate::store::{self, ModRecord, ModsFile, Paths};
use crate::wrap::{Later, ModCall};
use crate::{codes, VERSION};

/// PL: Generacja jest dobra po 5 minutach bez paniki (D13): 5 × 60 × 20 TPS.
/// EN: A generation is good after 5 minutes without a panic (D13): 5 × 60 × 20 TPS.
pub const GOOD_AFTER_TICKS: u64 = 6000;

/// PL: Awaria w ciągu tylu ms od startu liczy się jako kolejna z rzędu (D13, tyle co próba).
/// EN: A crash within this many ms of the start counts as one more in a row (D13, the probation).
pub const CRASH_LOOP_WINDOW_MS: u64 = 5 * 60 * 1000;

/// PL: Od tylu awarii z rzędu mody gorące śpią (tryb bezpieczny, M7.1): pierwsza awaria daje
/// ostatnie dobre generacje, druga znaczy, że i one nie pomagają.
/// EN: From this many crashes in a row hot mods sleep (safe mode, M7.1): the first crash brings
/// the last good generations, the second means they do not help either.
pub const SAFE_MODE_AFTER: u32 = 2;

/// PL: Ile ostatnich błędów trzyma `status.json`. EN: How many recent errors `status.json` keeps.
const RECENT_ERRORS: usize = 10;

/// PL: Najwięcej odłożonych komend jednego gracza (D6a). EN: The most deferred commands per player.
pub const DEFERRED_MAX: usize = 16;

/// PL: Komenda gracza czekająca na jego tick (D6a). EN: A player command waiting for their tick.
struct Deferred {
    player: Uuid,
    name: String,
    command: String,
}

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
    /// PL: Komendy modów na następny tick (`run_later`, zapasowy `tell`). EN: Next-tick commands.
    later: RefCell<Vec<Later>>,
    /// PL: Komendy zadeklarowane przez mody, czekające na tick gracza (D6a).
    /// EN: Commands the mods declared, waiting for the player's tick (D6a).
    deferred: Vec<Deferred>,
    /// PL: Gracz odłożonej komendy w trakcie jej wykonania. EN: The deferred command's player.
    reply_to: Option<Uuid>,
    /// PL: Mod, który ostatnio przerwał zdarzenie (`Cancel`). EN: The mod that last cancelled.
    cancelled_by: Option<String>,
    /// PL: Pary (mod, komenda), o których już ostrzegano. EN: (mod, command) pairs already warned.
    warned: BTreeSet<(String, String)>,
    /// PL: Nazwy bloków modów z `hot-state/slots.json` (M5.1). EN: Mod block names (M5.1).
    slots: SlotNames,
    /// PL: Mody uśpione w trybie bezpiecznym; wracają po czystym restarcie albo `hot load`.
    /// EN: Mods asleep in safe mode; they return after a clean restart or `hot load`.
    dormant: Vec<ModRecord>,
    started_ms: u64,
    crash_starts: u32,
    /// PL: Świat trzyma inny silnik (rozgrzany silnik przy podmianie, D11): stan z `hot-state`
    /// czyta dopiero pierwsze zdarzenie, a wszystkie przychodzą po zajęciu świata.
    /// EN: Another engine holds the world (a warm engine during a swap, D11): the first event
    /// reads the `hot-state` state, and every event comes after the world is taken.
    restore_pending: bool,
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
            later: RefCell::new(Vec::new()),
            deferred: Vec::new(),
            reply_to: None,
            cancelled_by: None,
            warned: BTreeSet::new(),
            slots: SlotNames::default(),
            dormant: Vec::new(),
            started_ms: 0,
            crash_starts: 0,
            restore_pending: false,
        };
        core.read_slots();
        if world_in_use(&core.paths.home) {
            // PL: `hot-state` należy jeszcze do działającego silnika: jego `mods.json` ma
            // `clean_stop = false`, więc odczyt teraz policzyłby podmianę jako awarię.
            // EN: `hot-state` still belongs to the running engine: its `mods.json` says
            // `clean_stop = false`, so reading it now would count the swap as a crash.
            core.restore_pending = true;
            core.log.info(
                "hot-core waits for the world: another engine holds it (engine swap, D11)",
                json!({ "home": core.paths.home.display().to_string() }),
            );
        } else {
            core.restore(host.raw());
        }
        core
    }

    fn on_raw_event(&mut self, host: &mut Host<'_>, event: &hot_abi::Event) -> Flow {
        let api = host.raw();
        if self.restore_pending {
            self.restore_pending = false;
            self.restore(api);
        }
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
            self.run_later(api);
        }
        match event.kind {
            kind::PLAYER_TICK => {
                self.run_deferred(api, event.player);
                return Flow::Continue;
            }
            kind::PLAYER_COMMAND if self.defer(api, event) => return Flow::Cancel,
            kind::PLAYER_LEFT => self.deferred.retain(|d| d.player != event.player),
            _ => {}
        }
        // PL: Blok ze slotu mod widzi pod swoją nazwą; napis żyje do końca tej funkcji.
        // EN: A slot block reaches mods under their own name; the string lives until the end here.
        // SAFETY: napis ważny w wywołaniu. EN: the string is valid for the call.
        let engine_block = unsafe { event.block.as_str() }.unwrap_or("");
        let mod_block = self.slots.to_mod(engine_block).to_owned();
        let mut translated = *event;
        if mod_block != engine_block {
            translated.block = Str::from_str(&mod_block);
        }
        let flow = self.dispatch(api, &translated);
        if event.kind == kind::PLAYER_COMMAND && flow == Flow::Cancel {
            self.warn_undeclared(event);
        }
        if event.kind == kind::SERVER_TICK {
            self.count_tick();
        }
        flow
    }

    fn save(&mut self, host: &mut Host<'_>) -> Vec<u8> {
        if self.restore_pending {
            // PL: Przerwana podmiana: ten silnik nie zajął świata, `hot-state` zostaje bez zmian.
            // EN: An aborted swap: this engine never took the world; `hot-state` stays as it is.
            self.log.info(
                "hot-core stopped before its engine took the world; hot-state is left to the running engine",
                json!({}),
            );
            return Vec::new();
        }
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
        let now = now_ms();
        self.started_ms = now;
        self.crash_starts = crash_starts(&file, now);
        if self.crash_starts >= SAFE_MODE_AFTER && !file.mods.is_empty() {
            self.enter_safe_mode(&file);
        } else {
            for record in &file.mods {
                self.restore_one(api, record, file.clean_stop);
            }
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
            .chain(self.dormant.iter().flat_map(|r| {
                [
                    Some(r.current.clone()),
                    r.good.as_ref().map(|(g, _)| g.clone()),
                ]
            }))
            .flatten()
            .collect();
        store::prune_generations(&self.paths, &keep);
        // PL: Od teraz każde zatrzymanie bez `save` to awaria. EN: From now on, a stop without
        // `save` counts as a crash.
        self.write_mods(false);
        self.write_status();
        self.log.info(
            "hot-core started",
            json!({ "home": self.paths.home.display().to_string(), "mods": self.mods.iter().map(|m| m.gen.id.clone()).collect::<Vec<_>>(), "after_clean_stop": file.clean_stop, "crash_starts": self.crash_starts }),
        );
    }

    /// PL: Tryb bezpieczny (M7.1): silnik padł drugi raz z rzędu krótko po starcie, mimo
    /// ostatnich dobrych generacji. Żaden mod gorący nie startuje, żeby przerwać pętlę awarii;
    /// wpisy zostają w `mods.json` i wracają po czystym restarcie albo `hot load`.
    /// EN: Safe mode (M7.1): the engine crashed a second time in a row soon after starting, even
    /// with the last good generations. No hot mod starts, to break the crash loop; the records
    /// stay in `mods.json` and return after a clean restart or `hot load`.
    fn enter_safe_mode(&mut self, file: &ModsFile) {
        self.dormant.clone_from(&file.mods);
        let ids: Vec<&str> = file.mods.iter().map(|r| r.id.as_str()).collect();
        let e = DiagError::new(
            codes::CRASH_LOOP,
            format!(
                "The server crashed {} times in a row, each within {} min of starting; hot mods {} stay off (safe mode) to stop a crash loop",
                self.crash_starts,
                CRASH_LOOP_WINDOW_MS / 60_000,
                ids.join(", ")
            ),
            "Read logs/latest.log (the crash and the lines before it) and logs/hot-core.jsonl. If a hot mod crashed: fix it and push a commit, hotd loads it; or run hot load <file> for each mod you trust. If the crash is not from a hot mod: fix that first. A clean restart of the server (systemctl restart horizon-hot-server) loads all of these mods again",
        )
        .expected("The server runs longer than the probation after a start")
        .context("mods", ids.join(","))
        .context("crash_starts", self.crash_starts)
        .context("previous_start_ms", file.started_ms)
        .state("hot-core runs without hot mods; the records stay in hot-state/mods.json");
        self.error(&e);
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
            later: &self.later,
            reply_to: self.reply_to,
            slots: &self.slots,
        };
        let api = call.api();
        let result = f(&api);
        (result, call.panic)
    }

    /// PL: Komendy odłożone przez mody w poprzednim ticku. Odmowa silnika trafia do logu z id moda.
    /// EN: Commands the mods put off in the previous tick. An engine refusal is logged with the mod.
    fn run_later(&mut self, outer: &HostApi) {
        let queued = self.later.take();
        for later in queued {
            if !(outer.run_command)(outer.ctx, Str::from_str(&later.line)) {
                self.log.warn(
                    &format!(
                        "The engine refused a command hot mod \"{}\" queued for the next tick. Fix: run the same command in the server console to see why",
                        later.id
                    ),
                    json!({ "mod": later.id, "command": later.line }),
                );
            }
        }
    }

    // ---------- PL: odłożone komendy graczy (D6a) / EN: deferred player commands (D6a) ----------

    /// PL: Indeks moda, który deklaruje pierwszy wyraz komendy. EN: The mod declaring the command.
    fn owner(&self, command: &str) -> Option<usize> {
        let word = command.split_whitespace().next()?;
        self.mods
            .iter()
            .position(|m| m.gen.commands.iter().any(|c| c == word))
    }

    /// PL: Komenda gracza zadeklarowana przez mod trafia do kolejki na jego tick; zwraca, czy
    /// została odłożona. W haku komendy silnik nie widzi gracza (D6a), więc mod nie dostaje jej tu.
    /// EN: A player command a mod declared is queued for the player's tick; returns whether it was
    /// deferred. In the command hook the engine does not see the player (D6a), so the mod does not
    /// get it here.
    fn defer(&mut self, outer: &HostApi, event: &Event) -> bool {
        // SAFETY: napisy zdarzenia ważne w wywołaniu. EN: the event's strings are valid for the call.
        let command = unsafe { event.text.as_str() }.unwrap_or("").to_owned();
        if self.owner(&command).is_none() {
            return false;
        }
        let waiting = self
            .deferred
            .iter()
            .filter(|d| d.player == event.player)
            .count();
        if waiting >= DEFERRED_MAX {
            (outer.reply)(
                outer.ctx,
                Str::from_str("Za dużo komend naraz; poczekaj chwilę i spróbuj ponownie."),
            );
            return true;
        }
        self.deferred.push(Deferred {
            player: event.player,
            // SAFETY: jak wyżej. EN: as above.
            name: unsafe { event.name.as_str() }.unwrap_or("").to_owned(),
            command,
        });
        true
    }

    /// PL: Mod obsłużył komendę gracza w haku komendy, gdzie silnik nie widzi gracza (D6a). Jedno
    /// ostrzeżenie na parę (mod, komenda), bo blokada komendy silnika też jest poprawna.
    /// EN: A mod handled a player command in the command hook, where the engine does not see the
    /// player (D6a). One warning per (mod, command) pair, as blocking an engine command is valid.
    fn warn_undeclared(&mut self, event: &Event) {
        let Some(id) = self.cancelled_by.take() else {
            return;
        };
        // SAFETY: napis zdarzenia ważny w wywołaniu. EN: the event's string is valid for the call.
        let line = unsafe { event.text.as_str() }.unwrap_or("");
        let word = line.split_whitespace().next().unwrap_or("").to_owned();
        if !self.warned.insert((id.clone(), word.clone())) {
            return;
        }
        self.log.warn(
            &format!(
                "Hot mod \"{id}\" answered the player command /{word} in the command hook, where the engine does not see the player: players() leaves them out and tell() and give() do not reach them. Fix: if the mod needs the player, declare the command with hot_mod!(..., commands = \"{word}\") so it runs in the player's tick"
            ),
            json!({ "mod": id, "command": word }),
        );
    }

    /// PL: Ostrzeżenie, gdy nowy mod deklaruje komendę innego aktywnego moda; wygrywa ten, który
    /// był wcześniej na liście. EN: A warning when a new mod declares another active mod's
    /// command; the one listed earlier wins.
    fn warn_shared_commands(&self, gen: &Gen) {
        for m in self.mods.iter().filter(|m| m.gen.id != gen.id) {
            for c in gen.commands.iter().filter(|c| m.gen.commands.contains(c)) {
                self.log.warn(
                    &format!(
                        "Hot mods \"{}\" and \"{}\" both declare the command /{c}; \"{}\" gets it. Fix: remove /{c} from commands = \"...\" in one of them",
                        m.gen.id, gen.id, m.gen.id
                    ),
                    json!({ "command": c, "owner": m.gen.id, "ignored": gen.id }),
                );
            }
        }
    }

    /// PL: Odłożone komendy gracza, w jego ticku. Właściciela szuka się teraz, bo mod mógł zostać
    /// przeładowany albo usunięty. EN: The player's deferred commands, in their tick. The owner is
    /// looked up now, as the mod may have been reloaded or removed meanwhile.
    fn run_deferred(&mut self, outer: &HostApi, player: Uuid) {
        if !self.deferred.iter().any(|d| d.player == player) {
            return;
        }
        let (mine, rest): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut self.deferred)
            .into_iter()
            .partition(|d| d.player == player);
        self.deferred = rest;
        for d in mine {
            let Some(i) = self.owner(&d.command) else {
                let line = format!("Komendy /{} nie obsługuje już żaden mod.", d.command);
                (outer.tell)(outer.ctx, &player, Str::from_str(&line));
                continue;
            };
            let mut event = Event::new(kind::PLAYER_COMMAND);
            event.player = player;
            event.name = Str::from_str(&d.name);
            event.text = Str::from_str(&d.command);
            self.reply_to = Some(player);
            self.call_one(outer, i, &event);
            self.reply_to = None;
        }
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
                self.cancelled_by = self.mods.get(i).map(|m| m.gen.id.clone());
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
        self.warn_shared_commands(&gen);
        // PL: `hot load` uśpionego moda kończy dla niego tryb bezpieczny. EN: `hot load` of a
        // sleeping mod ends safe mode for it.
        self.dormant.retain(|r| r.id != gen.id);
        let Some(i) = self.mods.iter().position(|m| m.gen.id == gen.id) else {
            // PL: Stan zostawiony przez `hot unload` albo wyłączenie po panice. Gdy start się nie
            // uda, wraca do `saved/`. EN: The state left by `hot unload` or a turn-off after a
            // panic. If the start fails, it goes back to `saved/`.
            let state = store::take_saved(&self.paths, &gen.id);
            let Some(instance) = self.init_gen(outer, &gen, 1, &state) else {
                if let Err(e) = store::write_atomic(&self.paths.saved(&gen.id), &state) {
                    self.error(&e);
                }
                return;
            };
            self.log.info(
                &format!("Hot mod \"{}\" {} is active as generation 1", gen.id, gen.version),
                json!({ "mod": gen.id, "generation": 1, "file": gen.file.display().to_string(), "source": gen.source.display().to_string(), "state_bytes": state.len() }),
            );
            self.mods.push(Active {
                gen,
                generation: 1,
                instance,
                previous: None,
                good: None,
                snapshot: state,
                ok_ticks: 0,
            });
            self.write_mods(false);
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
                } else if self.dormant.iter().any(|r| r.id == rest) {
                    self.dormant.retain(|r| r.id != rest);
                    self.write_mods(false);
                    self.write_status();
                    self.log.info(
                        &format!("Hot mod \"{rest}\", asleep in safe mode, was removed by the hot unload command"),
                        json!({ "mod": rest }),
                    );
                    out.push(format!(
                        "Mod {rest} spał w trybie bezpiecznym; usunięty z listy, nie wróci po restarcie."
                    ));
                } else {
                    out.push(format!("Nie ma aktywnego moda {rest}."));
                }
            }
            "slots" => {
                self.read_slots();
                out.push(format!(
                    "Bloki modów: {} (hot-state/slots.json).",
                    self.slots.len()
                ));
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
                if !self.dormant.is_empty() {
                    let ids: Vec<&str> = self.dormant.iter().map(|r| r.id.as_str()).collect();
                    out.push(format!(
                        "Tryb bezpieczny: serwer padł {} razy z rzędu krótko po starcie, więc te mody śpią: {}. Wrócą po hot load albo po czystym restarcie serwera; opis: HOT-E0408.",
                        self.crash_starts,
                        ids.join(", ")
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
                    "Użycie: hot load <plik> | hot unload <mod> | hot list | hot slots | hot <mod> [argumenty]"
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

    /// PL: Przydział slotów od `hotd`; przy błędzie zostaje poprzedni. EN: The slot assignment
    /// from `hotd`; on an error the previous one stays.
    fn read_slots(&mut self) {
        match SlotNames::read(&self.paths.state().join("slots.json")) {
            Ok(names) => self.slots = names,
            Err(e) => self.error(&e),
        }
    }

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
                .chain(self.dormant.iter().cloned())
                .collect(),
            started_ms: self.started_ms,
            crash_starts: self.crash_starts,
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
            "safe_mode": (!self.dormant.is_empty()).then(|| json!({
                "crash_starts": self.crash_starts,
                "dormant": self.dormant.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            })),
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// PL: Która to awaria z rzędu (D13): 0 po czystym zatrzymaniu i bez zapisanego startu
/// (pierwszy start, plik sprzed M7.1); po awarii o jeden więcej niż poprzednio, gdy poprzedni
/// start był niedawno, inaczej 1. Zegar cofnięty w tył też liczy się jako „niedawno”.
/// EN: Which crash in a row this is (D13): 0 after a clean stop or with no recorded start (first
/// start, a file from before M7.1); after a crash one more than before when the previous start was
/// recent, otherwise 1. A clock set back counts as recent.
pub fn crash_starts(file: &ModsFile, now_ms: u64) -> u32 {
    if file.clean_stop || file.started_ms == 0 {
        return 0;
    }
    if now_ms.saturating_sub(file.started_ms) < CRASH_LOOP_WINDOW_MS {
        file.crash_starts.saturating_add(1)
    } else {
        1
    }
}

/// PL: Czy świat serwera z `home` (`level-name` z `server.properties`, inaczej `world`) trzyma
/// inny silnik: blokada `session.lock` jak `SessionLock` w Horizonie. Brak pliku: wolny.
/// EN: Whether another engine holds the server world in `home` (`level-name` from
/// `server.properties`, else `world`): the `session.lock` lock, like Horizon's `SessionLock`.
/// No file: free.
#[must_use]
pub fn world_in_use(home: &std::path::Path) -> bool {
    let name = std::fs::read_to_string(home.join("server.properties"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("level-name=").map(|v| v.trim().to_owned()))
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "world".to_owned());
    let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .open(home.join(name).join("session.lock"))
    else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(_)) => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(clean_stop: bool, started_ms: u64, crash_starts: u32) -> ModsFile {
        ModsFile {
            mods: Vec::new(),
            clean_stop,
            started_ms,
            crash_starts,
        }
    }

    #[test]
    fn crashes_in_a_row_count_only_soon_after_a_start() {
        let t = 10 * CRASH_LOOP_WINDOW_MS;
        assert_eq!(crash_starts(&file(true, t - 1, 5), t), 0);
        assert_eq!(crash_starts(&file(false, t - 1000, 0), t), 1);
        assert_eq!(crash_starts(&file(false, t - 1000, 1), t), 2);
        assert_eq!(
            crash_starts(&file(false, t - CRASH_LOOP_WINDOW_MS, 1), t),
            1
        );
        assert_eq!(crash_starts(&file(false, 0, 1), t), 0);
        assert_eq!(crash_starts(&file(false, t + 1000, 1), t), 2);
    }
}
