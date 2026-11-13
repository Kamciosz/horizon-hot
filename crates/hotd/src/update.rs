//! PL: Aktualizacja Horizona (D18, M9.1). `hotd horizon` raz na godzinę sprawdza najnowszą
//! rewizję Nightly (tag `nightly-source/…`) i prowadzi ją przez kolejne kroki. Bramka: `libloading`
//! w `Cargo.lock` tej rewizji, bo bez niego `hot-host` nie wczyta `hot-core`. Dalej: miejsce na
//! dysku, zgoda właściciela na kod natywny, budowa (`build.rs`), test na kopii świata (`trial.rs`),
//! okno zgodności klientów (maks. 24 h, ogłoszenie na czacie, wcześniej, gdy nikt nie gra),
//! podmiana silnika (D11, przez `hotd engine`), 10 minut obserwacji i powrót, gdy jest gorzej.
//! Rewizja, która sama zawiodła (brak `libloading`, nieudana budowa, nowy silnik gorszy w teście,
//! nieudana podmiana, powrót), trafia na listę `bad` i nie wraca. Gdy test na kopii świata nie
//! odbył się z naszej winy (`HOT-E0704`), rewizja nie trafia na listę i krok wraca za godzinę.
//! Stan leży w `hot-state/horizon.json`, a `hot status` go pokazuje.
//!
//! Logika jest tutaj, a świat zewnętrzny za cechą [`Env`], więc testy podstawiają atrapę. Prawdziwą
//! implementację daje `update_env.rs`.
//!
//! EN: The Horizon update (D18, M9.1). `hotd horizon` checks the newest Nightly revision (the
//! `nightly-source/…` tag) once an hour and takes it through the steps. The gate: `libloading` in
//! that revision's `Cargo.lock`, as `hot-host` cannot load `hot-core` without it. Then: disk space,
//! the owner's consent to native code, the build (`build.rs`), a trial on a copy of the world
//! (`trial.rs`), the client window (at most 24 h, announced in chat, earlier when nobody plays),
//! the engine swap (D11, through `hotd engine`), 10 minutes of watching and going back when it is
//! worse. A revision that failed by itself (no `libloading`, a failed build, a worse new engine in
//! the trial, a failed swap, a rollback) goes on the `bad` list and never comes back. When the
//! trial on the world copy did not happen through our fault (`HOT-E0704`), the revision stays off
//! the list and the step comes back in an hour. The state lives in `hot-state/horizon.json` and
//! `hot status` shows it.
//!
//! The logic is here and the outside world behind the [`Env`] trait, so tests plug in a fake. The
//! real implementation is in `update_env.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::{Code, DiagError};
use serde_json::{json, Value};

use crate::codes;
use crate::state::write_atomic;

/// PL: Nowa rewizja na górze. EN: A new revision upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// PL: Np. `nightly-source/2026-11-13-9f42064`. EN: E.g. `nightly-source/2026-11-13-9f42064`.
    pub tag: String,
    pub rev: String,
}

/// PL: Stan silnika w danej chwili. EN: The engine's state at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    /// PL: PID procesu silnika; `None`, gdy nie działa. EN: The engine PID; `None` when down.
    pub pid: Option<u64>,
    /// PL: p95 czasu ticku w ms. EN: The p95 tick time in ms.
    pub p95: Option<f64>,
    pub players: Option<u64>,
}

/// PL: Co aktualizacja potrzebuje od świata. EN: What the update needs from the world.
pub trait Env {
    /// PL: Najnowszy tag `nightly-source/…`. EN: The newest `nightly-source/…` tag.
    ///
    /// # Errors
    /// PL: `HOT-E0704`. EN: `HOT-E0704`.
    fn newest(&mut self) -> Result<Option<Upstream>, DiagError>;
    /// PL: `Cargo.lock` rewizji. EN: The revision's `Cargo.lock`.
    ///
    /// # Errors
    /// PL: `HOT-E0704`. EN: `HOT-E0704`.
    fn cargo_lock(&mut self, rev: &str) -> Result<String, DiagError>;
    /// PL: Wolne bajty tam, gdzie powstaje silnik. EN: Free bytes where the engine is built.
    fn free_bytes(&mut self) -> Option<u64>;
    /// PL: Plik działającego silnika. EN: The running engine's file.
    fn current_engine(&mut self) -> Option<PathBuf>;
    /// PL: Buduje silnik rewizji. EN: Builds the revision's engine.
    ///
    /// # Errors
    /// PL: `HOT-E0301`. EN: `HOT-E0301`.
    fn build(&mut self, rev: &str) -> Result<PathBuf, DiagError>;
    /// PL: Test nowego silnika na kopii świata, w porównaniu ze starym. EN: The new engine's trial
    /// on a copy of the world, compared with the old one.
    ///
    /// # Errors
    /// PL: `HOT-E0702`. EN: `HOT-E0702`.
    fn trial(&mut self, old: &Path, new: &Path) -> Result<Value, DiagError>;
    fn health(&mut self) -> Health;
    /// PL: Wiadomość na czacie gry. EN: A message in the game chat.
    fn say(&mut self, text: &str);
    /// PL: Podmiana silnika (D11) na plik. EN: An engine swap (D11) to the file.
    ///
    /// # Errors
    /// PL: `HOT-E0606`, `HOT-E0607`. EN: `HOT-E0606`, `HOT-E0607`.
    fn swap(&mut self, engine: &Path) -> Result<Value, DiagError>;
    /// PL: Usuwa stare silniki poza tymi. EN: Removes old engines except these.
    fn cleanup(&mut self, keep: &[PathBuf]);
    fn info(&mut self, what: &str, context: Value);
    fn error(&mut self, error: &DiagError);
}

/// PL: Ustawienia. EN: Settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// PL: Zgoda właściciela na kod natywny. EN: The owner's consent to native code.
    pub trust: bool,
    pub check_every: u64,
    /// PL: Okno zgodności klientów w s (D18: 24 h). EN: The client window in s (D18: 24 h).
    pub window: u64,
    /// PL: Obserwacja po podmianie w s (D18: 10 min). EN: Watching after the swap in s.
    pub observe: u64,
    /// PL: Minimum wolnego miejsca na budowę. EN: The free space a build needs.
    pub min_free: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            trust: false,
            check_every: 60 * 60,
            window: 24 * 60 * 60,
            observe: 10 * 60,
            min_free: MIN_FREE,
        }
    }
}

/// PL: Minimum wolnego miejsca: nowa budowa `native-build` i drugi silnik. HIPOTEZA z rozmiaru
/// `native/` launchera na Macu, do sprawdzenia na ThinkCentre.
/// EN: The minimum free space: a new `native-build` and a second engine. A HYPOTHESIS from the size
/// of the launcher's `native/` on a Mac, to check on the ThinkCentre.
pub const MIN_FREE: u64 = 8 * 1024 * 1024 * 1024;
/// PL: p95 po podmianie gorsze o tyle razy niż przed nią = gorzej. EN: p95 after the swap this many
/// times the one before = worse.
pub const WORSE_RATIO: f64 = 1.2;
/// PL: …i co najmniej o tyle ms (szum przy małych liczbach). EN: …and by at least this many ms.
pub const WORSE_MS: f64 = 5.0;
/// PL: Próbki p95 sprzed podmiany, które tworzą punkt odniesienia. EN: p95 samples before the swap
/// that make the baseline.
const BASELINE_SAMPLES: usize = 30;
/// PL: Czas obserwacji w opisie dla `hot status` (D18). EN: The watching time in the status text.
const OBSERVE_NOTE_MIN: u64 = 10;
/// PL: Ostatnie ogłoszenie tyle s przed końcem okna. EN: The last notice this long before the end.
const LAST_CALL: u64 = 10 * 60;

/// PL: Etap aktualizacji. EN: The update's stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// PL: Bramka odrzuciła rewizję (np. brak `libloading`). EN: The gate refused the revision.
    Refused,
    /// PL: Czeka na zgodę właściciela. EN: Waits for the owner's consent.
    NeedsConsent,
    /// PL: Mało miejsca; ponowi za godzinę. EN: Low disk; tries again in an hour.
    LowDisk,
    BuildFailed,
    TrialFailed,
    /// PL: Okno zgodności klientów. EN: The client window.
    Waiting,
    /// PL: Podmiana się nie udała; gracze na starym. EN: The swap failed; players on the old one.
    SwapFailed,
    Observing,
    Kept,
    RolledBack,
}

impl Phase {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Refused => "refused",
            Self::NeedsConsent => "needs_consent",
            Self::LowDisk => "low_disk",
            Self::BuildFailed => "build_failed",
            Self::TrialFailed => "trial_failed",
            Self::Waiting => "waiting",
            Self::SwapFailed => "swap_failed",
            Self::Observing => "observing",
            Self::Kept => "kept",
            Self::RolledBack => "rolled_back",
        }
    }

    fn from_name(name: &str) -> Self {
        [
            Self::Refused,
            Self::NeedsConsent,
            Self::LowDisk,
            Self::BuildFailed,
            Self::TrialFailed,
            Self::Waiting,
            Self::SwapFailed,
            Self::Observing,
            Self::Kept,
            Self::RolledBack,
        ]
        .into_iter()
        .find(|p| p.name() == name)
        .unwrap_or(Self::Idle)
    }
}

/// PL: Stan w `hot-state/horizon.json`. EN: The state in `hot-state/horizon.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub phase: Phase,
    /// PL: Rewizja działającego silnika. EN: The running engine's revision.
    pub current: Option<String>,
    pub candidate: Option<String>,
    pub tag: Option<String>,
    /// PL: Plik silnika kandydata. EN: The candidate's engine file.
    pub engine: Option<String>,
    /// PL: Plik silnika sprzed podmiany (powrót). EN: The engine file before the swap (way back).
    pub previous: Option<String>,
    /// PL: Zdanie po polsku dla `hot status`. EN: A Polish sentence for `hot status`.
    pub reason: Option<String>,
    pub code: Option<String>,
    /// PL: Rewizje, które nie przeszły; nigdy nie wracają. EN: Revisions that failed; never retried.
    pub bad: Vec<String>,
    pub checked_at: u64,
    pub deadline: u64,
    pub swapped_at: u64,
    /// PL: p95 sprzed podmiany (ostatnie próbki). EN: p95 before the swap (the last samples).
    pub before: Vec<f64>,
    /// PL: p95 po podmianie. EN: p95 after the swap.
    pub after: Vec<f64>,
    /// PL: PID silnika tuż po podmianie: inny = silnik padł i wstał. EN: The engine PID right after
    /// the swap: another one = the engine crashed and came back.
    pub pid: Option<u64>,
    pub last_call: bool,
    pub trial: Value,
}

impl Default for Update {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            current: None,
            candidate: None,
            tag: None,
            engine: None,
            previous: None,
            reason: None,
            code: None,
            bad: Vec::new(),
            checked_at: 0,
            deadline: 0,
            swapped_at: 0,
            before: Vec::new(),
            after: Vec::new(),
            pid: None,
            last_call: false,
            trial: Value::Null,
        }
    }
}

/// PL: Plik stanu. EN: The state file.
#[must_use]
pub fn state_file(home: &Path) -> PathBuf {
    home.join("hot-state").join("horizon.json")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn numbers(v: &Value) -> Vec<f64> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect()
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

impl Update {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        Self {
            phase: Phase::from_name(v["phase"].as_str().unwrap_or("")),
            current: text(&v["current"]),
            candidate: text(&v["candidate"]),
            tag: text(&v["tag"]),
            engine: text(&v["engine"]),
            previous: text(&v["previous"]),
            reason: text(&v["reason"]),
            code: text(&v["code"]),
            bad: strings(&v["bad"]),
            checked_at: v["checked_at"].as_u64().unwrap_or(0),
            deadline: v["deadline"].as_u64().unwrap_or(0),
            swapped_at: v["swapped_at"].as_u64().unwrap_or(0),
            before: numbers(&v["before"]),
            after: numbers(&v["after"]),
            pid: v["pid"].as_u64(),
            last_call: v["last_call"] == true,
            trial: v["trial"].clone(),
        }
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "phase": self.phase.name(),
            "current": self.current,
            "candidate": self.candidate,
            "tag": self.tag,
            "engine": self.engine,
            "previous": self.previous,
            "reason": self.reason,
            "code": self.code,
            "bad": self.bad,
            "checked_at": self.checked_at,
            "deadline": self.deadline,
            "swapped_at": self.swapped_at,
            "before": self.before,
            "after": self.after,
            "pid": self.pid,
            "last_call": self.last_call,
            "trial": self.trial,
        })
    }

    /// PL: Czyta stan; brak pliku = stan początkowy. EN: Reads the state; no file = the start.
    #[must_use]
    pub fn load(home: &Path) -> Self {
        fs::read(state_file(home))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .map_or_else(Self::default, |v| Self::from_json(&v))
    }

    /// PL: Zapisuje stan. EN: Saves the state.
    ///
    /// # Errors
    /// PL: `HOT-E0207`. EN: `HOT-E0207`.
    pub fn save(&self, home: &Path) -> Result<(), DiagError> {
        write_atomic(
            &state_file(home),
            format!("{:#}\n", self.to_json()).as_bytes(),
        )
    }

    fn stop(&mut self, phase: Phase, code: Option<Code>, reason: String) {
        self.phase = phase;
        self.code = code.map(|c| c.as_str().to_owned());
        self.reason = Some(reason);
    }

    fn mark_bad(&mut self, rev: &str) {
        if !self.bad.iter().any(|b| b == rev) {
            self.bad.push(rev.to_owned());
        }
    }

    /// PL: Jeden krok; wywoływany co minutę. Sprawdzenie nowej rewizji idzie co `check_every`,
    /// okno i obserwacja przy każdym kroku. EN: One step; called every minute. The check for a new
    /// revision runs every `check_every`, the window and the watching at every step.
    pub fn step(&mut self, env: &mut dyn Env, cfg: &Config, now: u64) {
        match self.phase {
            Phase::Waiting => self.window(env, now),
            Phase::Observing => self.watch(env, cfg, now),
            _ if now >= self.checked_at.saturating_add(cfg.check_every) => {
                self.check(env, cfg, now);
            }
            _ => {}
        }
    }

    fn check(&mut self, env: &mut dyn Env, cfg: &Config, now: u64) {
        self.checked_at = now;
        let engine = env.current_engine();
        if let Some(rev) = engine.as_deref().and_then(crate::build::engine_revision) {
            self.current = Some(rev);
        }
        let newest = match env.newest() {
            Ok(Some(n)) => n,
            Ok(None) => return,
            Err(e) => {
                env.error(&e);
                return;
            }
        };
        let fresh = self.current.as_deref() != Some(&newest.rev) && !self.bad.contains(&newest.rev);
        if !fresh {
            // PL: Wynik ostatniej próby zostaje widoczny. EN: The last attempt's outcome stays.
            if self.candidate.as_deref() != Some(&newest.rev) && self.phase != Phase::Kept {
                self.phase = Phase::Idle;
                self.reason = None;
                self.code = None;
            }
            return;
        }
        let Some(old) = engine else {
            self.stop(
                Phase::Idle,
                None,
                "nie wiem, który silnik działa (engine.path); aktualizacja czeka".into(),
            );
            return;
        };
        self.candidate = Some(newest.rev.clone());
        self.tag = Some(newest.tag.clone());
        self.trial = Value::Null;
        env.info(
            "hotd horizon found a new Horizon revision",
            json!({ "tag": newest.tag, "rev": newest.rev, "current": self.current }),
        );
        let lock = match env.cargo_lock(&newest.rev) {
            Ok(lock) => lock,
            Err(e) => {
                env.error(&e);
                self.stop(
                    Phase::Idle,
                    Some(e.code()),
                    format!(
                        "nie da się pobrać Cargo.lock {}; ponowię za godzinę",
                        newest.tag
                    ),
                );
                return;
            }
        };
        if !has_libloading(&lock) {
            let e = DiagError::new(
                codes::HORIZON_REFUSED,
                format!("Horizon {} has no libloading in Cargo.lock: hot-host could not load hot-core", newest.tag),
                "Nothing to do on the server: it stays on its Horizon. hot-host needs a new way to load hot-core (D2) before this revision",
            )
            .context("rev", newest.rev.as_str())
            .state("the server stays on its Horizon revision");
            env.error(&e);
            self.mark_bad(&newest.rev);
            self.stop(
                Phase::Refused,
                Some(e.code()),
                format!(
                    "{} nie ma libloading w Cargo.lock, hot-host nie wczytałby hot-core; serwer zostaje na swojej wersji",
                    newest.tag
                ),
            );
            return;
        }
        let free = env.free_bytes().unwrap_or(u64::MAX);
        if free < cfg.min_free {
            self.stop(
                Phase::LowDisk,
                Some(codes::DISK_LOW),
                format!(
                    "mało miejsca na budowę {}: wolne {} MB, potrzeba {} MB; ponowię za godzinę",
                    newest.tag,
                    free / 1_048_576,
                    cfg.min_free / 1_048_576
                ),
            );
            return;
        }
        if !cfg.trust {
            self.stop(
                Phase::NeedsConsent,
                None,
                format!(
                    "nowy Horizon {} czeka na zgodę właściciela na kod natywny (hotd horizon --trust-native-code)",
                    newest.tag
                ),
            );
            return;
        }
        let new = match env.build(&newest.rev) {
            Ok(new) => new,
            Err(e) => {
                env.error(&e);
                self.mark_bad(&newest.rev);
                self.stop(
                    Phase::BuildFailed,
                    Some(e.code()),
                    format!("silnik z {} się nie zbudował (HOT-E0301 w logu); serwer zostaje na swojej wersji", newest.tag),
                );
                return;
            }
        };
        self.engine = Some(new.display().to_string());
        match env.trial(&old, &new) {
            Ok(report) => self.trial = report,
            Err(e) => {
                env.error(&e);
                self.trial = Value::Object(e.fields());
                // PL: Winny nowy silnik: rewizja nie wraca. Zepsuty sam test (brak kopii świata,
                // stary silnik nie wstał): ponowienie za godzinę, z gotowym już silnikiem.
                // EN: The new engine is at fault: the revision never comes back. The trial itself
                // broke (no world copy, the old engine did not start): retried in an hour, with the
                // engine already built.
                let reason = if e.code() == codes::HORIZON_TRIAL_FAILED {
                    self.mark_bad(&newest.rev);
                    format!(
                        "{} nie przeszedł testu na kopii świata: {}",
                        newest.tag,
                        e_what(&e)
                    )
                } else {
                    format!(
                        "test {} na kopii świata się nie odbył: {}; ponowię za godzinę",
                        newest.tag,
                        e_what(&e)
                    )
                };
                self.stop(Phase::TrialFailed, Some(e.code()), reason);
                return;
            }
        }
        self.deadline = now + cfg.window;
        self.last_call = false;
        self.before.clear();
        self.stop(
            Phase::Waiting,
            None,
            format!(
                "{} przeszedł test; podmiana, gdy nikt nie gra, najpóźniej za {} h",
                newest.tag,
                cfg.window / 3600
            ),
        );
        env.say(&format!(
            "Nowa wersja Horizona na serwerze najpóźniej za {} h. Zaktualizuj aplikację gracza przed kolejną grą.",
            cfg.window / 3600
        ));
    }

    fn window(&mut self, env: &mut dyn Env, now: u64) {
        let health = env.health();
        if let Some(p95) = health.p95 {
            self.before.push(p95);
            let extra = self.before.len().saturating_sub(BASELINE_SAMPLES);
            self.before.drain(..extra);
        }
        let nobody = health.players == Some(0);
        if !nobody && now < self.deadline {
            if !self.last_call && self.deadline - now <= LAST_CALL {
                self.last_call = true;
                env.say("Za 10 minut nowa wersja Horizona: krótka pauza, bez rozłączania.");
            }
            return;
        }
        let (Some(new), Some(rev)) = (self.engine.clone(), self.candidate.clone()) else {
            self.stop(Phase::Idle, None, "brak pliku kandydata".into());
            return;
        };
        self.previous = env.current_engine().map(|p| p.display().to_string());
        match env.swap(Path::new(&new)) {
            Ok(report) => {
                env.info(
                    "hotd horizon swapped to the new Horizon",
                    json!({ "rev": rev, "report": report }),
                );
                self.phase = Phase::Observing;
                self.swapped_at = now;
                self.after.clear();
                self.pid = env.health().pid;
                self.reason = Some(format!(
                    "{} działa; obserwacja przez {} min po podmianie",
                    self.tag.as_deref().unwrap_or(&rev),
                    OBSERVE_NOTE_MIN
                ));
            }
            Err(e) => {
                env.error(&e);
                self.mark_bad(&rev);
                self.stop(
                    Phase::SwapFailed,
                    Some(e.code()),
                    format!(
                        "podmiana na nowy Horizon się nie udała, gracze są na starym: {}",
                        e_what(&e)
                    ),
                );
            }
        }
    }

    fn watch(&mut self, env: &mut dyn Env, cfg: &Config, now: u64) {
        let health = env.health();
        if health.pid.is_none() || (self.pid.is_some() && health.pid != self.pid) {
            self.roll_back(env, "silnik padł albo nie działa po podmianie".into());
            return;
        }
        if let Some(p95) = health.p95 {
            self.after.push(p95);
        }
        if now < self.swapped_at + cfg.observe {
            return;
        }
        if let (Some(before), Some(after)) = (mean(&self.before), mean(&self.after)) {
            if after > before * WORSE_RATIO && after - before > WORSE_MS {
                self.roll_back(
                    env,
                    format!("p95 ticku po podmianie {after:.1} ms, przed {before:.1} ms"),
                );
                return;
            }
        }
        let rev = self.candidate.clone().unwrap_or_default();
        self.current = Some(rev.clone());
        self.phase = Phase::Kept;
        self.code = None;
        self.reason = Some(format!(
            "serwer działa na {}",
            self.tag.as_deref().unwrap_or(&rev)
        ));
        let keep: Vec<PathBuf> = [&self.engine, &self.previous]
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .collect();
        env.cleanup(&keep);
        env.info(
            "hotd horizon keeps the new Horizon",
            json!({ "rev": rev, "before_p95": self.before, "after_p95": self.after }),
        );
    }

    fn roll_back(&mut self, env: &mut dyn Env, why: String) {
        let rev = self.candidate.clone().unwrap_or_default();
        self.mark_bad(&rev);
        let back = self.previous.clone().map(PathBuf::from);
        let result = match &back {
            Some(file) => env.swap(file).map(|_| ()),
            None => Err(DiagError::new(
                codes::HORIZON_ROLLED_BACK,
                "No previous engine file is recorded",
                "Set engine.path back by hand",
            )),
        };
        let mut e = DiagError::new(
            codes::HORIZON_ROLLED_BACK,
            format!("The new Horizon is worse than the old one: {why}"),
            "Nothing to do: the server is back on the old engine and the revision is marked bad (hot-state/horizon.json, bad). Read the engine log around the swap to report it to Horizon",
        )
        .context("rev", rev.as_str())
        .context("before_p95", json!(self.before))
        .context("after_p95", json!(self.after));
        let back_ok = result.is_ok();
        if let Err(err) = result {
            e = e
                .chain_line(format!("the way back failed: {}", e_what(&err)))
                .state("the server may still run the new engine: run hotd swap --to <previous engine> by hand");
        } else {
            e = e.state("the server is back on the old engine");
        }
        env.error(&e);
        self.stop(
            Phase::RolledBack,
            Some(e.code()),
            if back_ok {
                format!("powrót na stary silnik: {why}")
            } else {
                format!(
                    "POWRÓT SIĘ NIE UDAŁ ({why}); hotd swap --to {} ręcznie",
                    self.previous.as_deref().unwrap_or("?")
                )
            },
        );
    }
}

fn e_what(e: &DiagError) -> String {
    e.fields()
        .get("what")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn mean(v: &[f64]) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)] // PL: kilkaset próbek. EN: a few hundred samples.
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// PL: Czy `Cargo.lock` ma pakiet `libloading`. EN: Whether `Cargo.lock` has the `libloading` package.
#[must_use]
pub fn has_libloading(cargo_lock: &str) -> bool {
    cargo_lock
        .lines()
        .any(|l| l.trim() == "name = \"libloading\"")
}

/// PL: Linie `hot status`. EN: The `hot status` lines.
#[must_use]
pub fn status_lines(home: &Path) -> Option<String> {
    let v: Value = serde_json::from_slice(&fs::read(state_file(home)).ok()?).ok()?;
    let u = Update::from_json(&v);
    let short = |r: &Option<String>| {
        r.as_deref()
            .map_or_else(|| "?".to_owned(), |s| s.get(..7).unwrap_or(s).to_owned())
    };
    let mut out = format!(
        "Horizon {}: {}",
        short(&u.current),
        match u.phase {
            Phase::Idle => "aktualny",
            Phase::Refused => "nowa wersja odrzucona",
            Phase::NeedsConsent => "nowa wersja czeka na zgodę",
            Phase::LowDisk => "nowa wersja czeka na miejsce",
            Phase::BuildFailed => "nowa wersja się nie zbudowała",
            Phase::TrialFailed => "test nowej wersji się nie powiódł",
            Phase::Waiting => "nowa wersja czeka na okno",
            Phase::SwapFailed => "podmiana się nie udała",
            Phase::Observing => "obserwacja po podmianie",
            Phase::Kept => "zaktualizowany",
            Phase::RolledBack => "powrót na starą wersję",
        }
    );
    if let Some(reason) = &u.reason {
        out.push_str(&format!("\n  {reason}"));
    }
    if let Some(code) = &u.code {
        out.push_str(&format!(" ({code})"));
    }
    Some(out + "\n")
}
