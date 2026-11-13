//! PL: Samoaktualizacja horizon-hot (D19, M10.1, ADR 0014). `hotd upgrade` raz na dobę sprawdza
//! głowę gałęzi z `--hot-ref` i dzieli zmianę według plików ([`plan`]):
//! - tylko `crates/hot-core`: nowy rdzeń wchodzi od razu przez `hot core load`, przy graczach;
//! - pozostały kod: budowa w tle, pliki czekają w `bin/.next/`, a w pierwszej chwili bez graczy
//!   wszystkie programy zmieniają się naraz i usługi restartują się razem. Gdy któraś nie wstanie,
//!   wracają poprzednie pliki;
//! - `hot-host`, `hot-abi`, instalator, szablony, pula slotów: nic się nie wdraża, właściciel
//!   uruchamia instalator (`HOT-E0706`).
//!
//! Wersja, która sama zawiodła (rdzeń się nie wczytał, usługi nie wstały), trafia na listę `bad`.
//! Stan leży w `hot-state/upgrade.json`, a `hot status` go pokazuje. Logika jest tutaj, a świat
//! zewnętrzny za cechą [`Env`] (prawdziwy w `upgrade_env.rs`).
//!
//! EN: The horizon-hot self-update (D19, M10.1, ADR 0014). `hotd upgrade` checks the head of the
//! `--hot-ref` branch once a day and splits the change by files ([`plan`]):
//! - only `crates/hot-core`: the new core goes in at once through `hot core load`, with players;
//! - other code: built in the background, the files wait in `bin/.next/`, and at the first moment
//!   without players all programs change at once and the services restart together. When one does
//!   not come up, the previous files come back;
//! - `hot-host`, `hot-abi`, the installer, templates, the slot pool: nothing is deployed, the owner
//!   runs the installer (`HOT-E0706`).
//!
//! A version that failed by itself (the core did not load, the services did not come up) goes on
//! the `bad` list. The state lives in `hot-state/upgrade.json` and `hot status` shows it. The logic
//! is here and the outside world behind the [`Env`] trait (the real one in `upgrade_env.rs`).

use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::{Code, DiagError};
use serde_json::{json, Value};

use crate::codes;
use crate::state::write_atomic;

/// PL: Sprawdzanie raz na dobę (D19). EN: A check once a day (D19).
pub const EVERY: u64 = 24 * 3600;

/// PL: Jak wdrożyć nową wersję. EN: How to deploy a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// PL: Sam `hot-core`, przeładowanie przy graczach. EN: Only `hot-core`, a reload with players.
    Core,
    /// PL: Wszystkie programy naraz, bez graczy. EN: All programs at once, without players.
    Full,
}

/// PL: Wynik klasyfikacji zmienionych plików. EN: The result of classifying the changed files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// PL: Nic nie trafia na serwer. EN: Nothing reaches the server.
    Nothing,
    Deploy(Mode),
    /// PL: Same pliki binarne nie wystarczą; powód dla właściciela. EN: Binaries alone are not
    /// enough; the reason for the owner.
    Blocked(String),
}

/// PL: Pliki, które nie trafiają na serwer z pakietu `hotd upgrade`. EN: Files that do not reach
/// the server through `hotd upgrade`.
fn ignored(file: &str) -> bool {
    // PL: `crates/hot-play` i `scripts/install.*` to aplikacja gracza; na serwer nie trafia.
    // EN: `crates/hot-play` and `scripts/install.*` are the player app; it never reaches the server.
    const PREFIXES: [&str; 5] = [
        "docs/",
        "spikes/",
        ".github/",
        "examples/",
        "crates/hot-play/",
    ];
    const FILES: [&str; 7] = [
        "Dockerfile",
        ".dockerignore",
        "compose.yaml",
        "scripts/container-entrypoint.sh",
        "scripts/install.sh",
        "scripts/install.ps1",
        "LICENSE",
    ];
    PREFIXES.iter().any(|p| file.starts_with(p))
        || FILES.contains(&file)
        // PL: `hot-diag report` wbudowuje `AGENTS.md`. EN: `hot-diag report` embeds `AGENTS.md`.
        || (file.ends_with(".md") && file != "AGENTS.md")
        || file.contains("/tests/")
}

/// PL: Dzieli zmianę według plików (ADR 0014). EN: Splits the change by files (ADR 0014).
#[must_use]
pub fn plan(changed: &[String]) -> Plan {
    let mut mode = None;
    for file in changed.iter().map(String::as_str).filter(|f| !ignored(f)) {
        if file.starts_with("crates/hot-host/") || file.starts_with("crates/hot-abi/") {
            return Plan::Blocked(format!(
                "nowa wersja zmienia silnik ({file}): przeczytaj kod, uruchom instalator z --trust-native-code i ustaw nowy ref w hot.yml repo serwera"
            ));
        }
        if file == "scripts/server-install.sh"
            || file.starts_with("templates/")
            || file == "crates/hotd/src/slots.rs"
        {
            return Plan::Blocked(format!(
                "nowa wersja zmienia instalację ({file}): uruchom instalator ponownie"
            ));
        }
        // PL: `hot-sdk` trafia na serwer tylko w `hot-core` (`hot_mod!`). EN: `hot-sdk` reaches
        // the server only inside `hot-core` (`hot_mod!`).
        let core_only = file.starts_with("crates/hot-core/") || file.starts_with("crates/hot-sdk/");
        mode = match (mode, core_only) {
            (None | Some(Mode::Core), true) => Some(Mode::Core),
            _ => Some(Mode::Full),
        };
    }
    mode.map_or(Plan::Nothing, Plan::Deploy)
}

/// PL: Świat zewnętrzny. EN: The outside world.
pub trait Env {
    /// PL: Wersja na serwerze (`hot-version`). EN: The version on the server (`hot-version`).
    fn current(&mut self) -> Option<String>;
    /// PL: Głowa gałęzi; `None`, gdy `--hot-ref` to tag albo commit. EN: The branch head; `None`
    /// when `--hot-ref` is a tag or a commit.
    ///
    /// # Errors
    /// PL: `HOT-E0705`. EN: `HOT-E0705`.
    fn fetch(&mut self) -> Result<Option<String>, DiagError>;
    /// PL: Zmienione pliki. EN: The changed files.
    ///
    /// # Errors
    /// PL: `HOT-E0705`. EN: `HOT-E0705`.
    fn changed(&mut self, from: &str, to: &str) -> Result<Vec<String>, DiagError>;
    /// PL: Budowa i odłożenie plików obok obecnych. EN: Builds and puts the files beside the
    /// current ones.
    ///
    /// # Errors
    /// PL: `HOT-E0705`. EN: `HOT-E0705`.
    fn build(&mut self, rev: &str, mode: Mode) -> Result<(), DiagError>;
    /// PL: `hot core load` odłożonego rdzenia; po potwierdzeniu silnika zastępuje on stary plik.
    /// EN: `hot core load` of the staged core; once the engine confirms, it replaces the old file.
    ///
    /// # Errors
    /// PL: `HOT-E0707`, gdy silnik odrzucił rdzeń (wersja odrzucona); `HOT-E0705`, gdy nie
    /// potwierdził (ponowienie). EN: `HOT-E0707` when the engine rejected the core (the version is
    /// skipped); `HOT-E0705` when it did not confirm (a retry).
    fn load_core(&mut self) -> Result<(), DiagError>;
    fn players(&mut self) -> usize;
    /// PL: Odłożone pliki na miejsce, stare z końcówką `.previous`. EN: The staged files into
    /// place, the old ones with a `.previous` suffix.
    ///
    /// # Errors
    /// PL: `HOT-E0705`. EN: `HOT-E0705`.
    fn install(&mut self) -> Result<(), DiagError>;
    /// PL: Restart usług i czekanie, aż wstaną. EN: Restarts the services and waits for them.
    ///
    /// # Errors
    /// PL: `HOT-E0708`. EN: `HOT-E0708`.
    fn restart(&mut self) -> Result<(), DiagError>;
    /// PL: Pliki `.previous` z powrotem na miejsce. EN: The `.previous` files back into place.
    fn restore(&mut self);
    /// PL: Usuwa odłożone pliki. EN: Removes the staged files.
    fn discard(&mut self);
    /// PL: Zapis `hot-version`. EN: Writes `hot-version`.
    ///
    /// # Errors
    /// PL: `HOT-E0705`. EN: `HOT-E0705`.
    fn set_version(&mut self, rev: &str) -> Result<(), DiagError>;
    fn info(&mut self, what: &str, context: Value);
    fn error(&mut self, error: &DiagError);
}

/// PL: Etap. EN: The stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// PL: `--hot-ref` to tag albo commit. EN: `--hot-ref` is a tag or a commit.
    Pinned,
    CheckFailed,
    Blocked,
    CoreFailed,
    /// PL: Pliki gotowe, czeka na pusty serwer. EN: Files ready, waits for an empty server.
    Staged,
    Done,
    RolledBack,
}

const PHASES: [(Phase, &str); 8] = [
    (Phase::Idle, "idle"),
    (Phase::Pinned, "pinned"),
    (Phase::CheckFailed, "check_failed"),
    (Phase::Blocked, "blocked"),
    (Phase::CoreFailed, "core_failed"),
    (Phase::Staged, "staged"),
    (Phase::Done, "done"),
    (Phase::RolledBack, "rolled_back"),
];

impl Phase {
    #[must_use]
    pub fn name(self) -> &'static str {
        PHASES
            .iter()
            .find(|(p, _)| *p == self)
            .map_or("idle", |(_, n)| n)
    }

    fn from_name(name: &str) -> Self {
        PHASES
            .iter()
            .find(|(_, n)| *n == name)
            .map_or(Self::Idle, |(p, _)| *p)
    }
}

/// PL: Stan w `hot-state/upgrade.json`. EN: The state in `hot-state/upgrade.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Upgrade {
    pub phase: Phase,
    pub current: Option<String>,
    pub candidate: Option<String>,
    pub reason: Option<String>,
    pub code: Option<String>,
    pub bad: Vec<String>,
    pub checked_at: u64,
}

impl Default for Upgrade {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            current: None,
            candidate: None,
            reason: None,
            code: None,
            bad: Vec::new(),
            checked_at: 0,
        }
    }
}

fn state_file(home: &Path) -> PathBuf {
    home.join("hot-state").join("upgrade.json")
}

fn short(rev: &str) -> &str {
    rev.get(..7).unwrap_or(rev)
}

impl Upgrade {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        let text = |v: &Value| v.as_str().map(str::to_owned);
        Self {
            phase: Phase::from_name(v["phase"].as_str().unwrap_or_default()),
            current: text(&v["current"]),
            candidate: text(&v["candidate"]),
            reason: text(&v["reason"]),
            code: text(&v["code"]),
            bad: v["bad"]
                .as_array()
                .map(|a| a.iter().filter_map(text).collect())
                .unwrap_or_default(),
            checked_at: v["checked_at"].as_u64().unwrap_or(0),
        }
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "phase": self.phase.name(),
            "current": self.current,
            "candidate": self.candidate,
            "reason": self.reason,
            "code": self.code,
            "bad": self.bad,
            "checked_at": self.checked_at,
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

    fn fail(&mut self, env: &mut dyn Env, error: &DiagError, reason: String) {
        env.error(error);
        self.stop(
            Phase::CheckFailed,
            Some(codes::UPGRADE_CHECK_FAILED),
            reason,
        );
    }

    /// PL: Jeden krok (co 10 minut z timera). Sieć i budowa najwyżej raz na `every` sekund.
    /// EN: One step (every 10 minutes from the timer). Network and build at most once per `every`
    /// seconds.
    pub fn step(&mut self, env: &mut dyn Env, now: u64, every: u64) {
        if self.phase == Phase::Staged {
            self.deploy_when_empty(env);
            return;
        }
        if self.checked_at != 0 && now < self.checked_at.saturating_add(every) {
            return;
        }
        self.checked_at = now;
        self.current = env.current();
        let newest = match env.fetch() {
            Ok(Some(rev)) => rev,
            Ok(None) => {
                self.candidate = None;
                return self.stop(
                    Phase::Pinned,
                    None,
                    "--hot-ref to tag albo commit: serwer się nie aktualizuje".into(),
                );
            }
            Err(e) => {
                return self.fail(
                    env,
                    &e,
                    "nie da się sprawdzić nowej wersji; ponowię jutro".into(),
                )
            }
        };
        let Some(current) = self.current.clone() else {
            return self.stop(
                Phase::Blocked,
                Some(codes::UPGRADE_NEEDS_INSTALLER),
                "brak pliku hot-version: uruchom instalator ponownie".into(),
            );
        };
        if newest == current {
            self.candidate = None;
            return self.stop(Phase::Idle, None, "najnowsza wersja".into());
        }
        if self.bad.contains(&newest) {
            return;
        }
        self.candidate = Some(newest.clone());
        let changed = match env.changed(&current, &newest) {
            Ok(c) => c,
            Err(e) => {
                return self.fail(env, &e, "nie da się porównać wersji; ponowię jutro".into())
            }
        };
        match plan(&changed) {
            Plan::Nothing => {
                self.adopt(env, &newest, "nowa wersja nie zmienia niczego na serwerze")
            }
            Plan::Blocked(why) => {
                self.stop(Phase::Blocked, Some(codes::UPGRADE_NEEDS_INSTALLER), why);
            }
            Plan::Deploy(mode) => {
                if let Err(e) = env.build(&newest, mode) {
                    return self.fail(
                        env,
                        &e,
                        format!("wersja {} się nie zbudowała; ponowię jutro", short(&newest)),
                    );
                }
                match mode {
                    Mode::Core => self.core(env, &newest),
                    Mode::Full => {
                        self.stop(
                            Phase::Staged,
                            None,
                            format!(
                                "wersja {} gotowa; wejdzie, gdy nikt nie będzie grał",
                                short(&newest)
                            ),
                        );
                        self.deploy_when_empty(env);
                    }
                }
            }
        }
    }

    fn core(&mut self, env: &mut dyn Env, rev: &str) {
        match env.load_core() {
            Ok(()) => self.adopt(env, rev, "nowy hot-core wczytany bez restartu"),
            Err(e) if e.code() != codes::UPGRADE_CORE_FAILED => self.fail(
                env,
                &e,
                format!(
                    "silnik nie potwierdził nowego hot-core z wersji {}; ponowię jutro",
                    short(rev)
                ),
            ),
            Err(e) => {
                env.error(&e);
                self.mark_bad(rev);
                self.stop(
                    Phase::CoreFailed,
                    Some(codes::UPGRADE_CORE_FAILED),
                    format!(
                        "hot-core z wersji {} się nie wczytał; działa poprzedni",
                        short(rev)
                    ),
                );
            }
        }
    }

    fn deploy_when_empty(&mut self, env: &mut dyn Env) {
        let Some(rev) = self.candidate.clone() else {
            return self.stop(Phase::Idle, None, "brak wersji do wdrożenia".into());
        };
        // PL: Instalator mógł w międzyczasie zbudować tę samą wersję. EN: The installer may have
        // built the same version meanwhile.
        if env.current().as_deref() == Some(rev.as_str()) {
            env.discard();
            self.current = Some(rev);
            self.candidate = None;
            return self.stop(Phase::Idle, None, "najnowsza wersja".into());
        }
        if env.players() > 0 {
            return;
        }
        if let Err(e) = env.install() {
            env.restore();
            env.discard();
            return self.fail(env, &e, "nie da się podmienić plików; ponowię jutro".into());
        }
        if let Err(e) = env.restart() {
            env.error(&e);
            env.restore();
            if let Err(again) = env.restart() {
                env.error(&again);
            }
            self.mark_bad(&rev);
            return self.stop(
                Phase::RolledBack,
                Some(codes::UPGRADE_ROLLED_BACK),
                format!(
                    "po wdrożeniu wersji {} usługi nie wstały; wróciła poprzednia",
                    short(&rev)
                ),
            );
        }
        self.adopt(env, &rev, "wdrożona bez graczy");
    }

    fn adopt(&mut self, env: &mut dyn Env, rev: &str, how: &str) {
        if let Err(e) = env.set_version(rev) {
            env.error(&e);
        }
        env.info(
            "hotd upgrade adopted a version",
            json!({ "rev": rev, "how": how }),
        );
        self.current = Some(rev.to_owned());
        self.candidate = None;
        self.stop(Phase::Done, None, format!("wersja {}: {how}", short(rev)));
    }

    fn mark_bad(&mut self, rev: &str) {
        if !self.bad.iter().any(|b| b == rev) {
            self.bad.push(rev.to_owned());
        }
    }
}

/// PL: Wiersze do `hot status`. EN: Lines for `hot status`.
#[must_use]
pub fn status_lines(home: &Path) -> Option<String> {
    let v: Value = serde_json::from_slice(&fs::read(state_file(home)).ok()?).ok()?;
    let u = Upgrade::from_json(&v);
    let mut out = format!(
        "horizon-hot {}: {}",
        u.current.as_deref().map_or("?", short),
        match u.phase {
            Phase::Idle | Phase::Done => "aktualny",
            Phase::Pinned => "przypięty",
            Phase::CheckFailed => "sprawdzenie się nie udało",
            Phase::Blocked => "nowa wersja czeka na instalator",
            Phase::CoreFailed => "nowy hot-core odrzucony",
            Phase::Staged => "nowa wersja czeka na pusty serwer",
            Phase::RolledBack => "powrót na poprzednią wersję",
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

#[cfg(test)]
#[allow(clippy::unwrap_used)] // PL: testy. EN: tests.
mod tests {
    use super::*;

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_plan_follows_the_changed_files() {
        assert_eq!(
            plan(&files(&[
                "docs/a.md",
                "PLAN.md",
                "crates/hotd/tests/flow.rs",
                "crates/hot-play/src/game.rs",
                "scripts/install.sh",
                "scripts/install.ps1"
            ])),
            Plan::Nothing
        );
        assert_eq!(
            plan(&files(&["crates/hot-core/src/lib.rs", "README.md"])),
            Plan::Deploy(Mode::Core)
        );
        assert_eq!(
            plan(&files(&[
                "crates/hot-core/src/lib.rs",
                "crates/hotd/src/daemon.rs"
            ])),
            Plan::Deploy(Mode::Full)
        );
        assert_eq!(plan(&files(&["Cargo.lock"])), Plan::Deploy(Mode::Full));
        assert_eq!(
            plan(&files(&["crates/hot-proxy/src/link.rs"])),
            Plan::Deploy(Mode::Full)
        );
        for blocked in [
            "crates/hot-host/api/src/lib.rs",
            "crates/hot-abi/src/lib.rs",
            "scripts/server-install.sh",
            "templates/server-repo/.github/workflows/hot.yml",
            "crates/hotd/src/slots.rs",
        ] {
            assert!(
                matches!(
                    plan(&files(&["crates/hot-core/src/lib.rs", blocked])),
                    Plan::Blocked(_)
                ),
                "{blocked}"
            );
        }
        assert_eq!(
            plan(&files(&["Dockerfile", "examples/licznik/src/lib.rs"])),
            Plan::Nothing
        );
        assert_eq!(
            plan(&files(&["crates/hot-sdk/src/lib.rs"])),
            Plan::Deploy(Mode::Core)
        );
        assert_eq!(plan(&files(&["AGENTS.md"])), Plan::Deploy(Mode::Full));
    }

    #[test]
    fn the_state_survives_a_round_trip() {
        let u = Upgrade {
            phase: Phase::Staged,
            current: Some("a".repeat(40)),
            candidate: Some("b".repeat(40)),
            reason: Some("x".into()),
            code: Some("HOT-E0705".into()),
            bad: vec!["c".repeat(40)],
            checked_at: 7,
        };
        assert_eq!(Upgrade::from_json(&u.to_json()), u);
    }
}
