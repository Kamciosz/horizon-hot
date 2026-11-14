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

mod commands;
mod generations;
mod restore;
mod status;

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
    /// PL: Ile bibliotek było w `hot-state/libraries.json` przy ostatnim zapisie.
    /// EN: How many libraries `hot-state/libraries.json` had at the last write.
    libraries_written: usize,
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
            libraries_written: 0,
        };
        core.read_slots();
        core.inherit_libraries();
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
