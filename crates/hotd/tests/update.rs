//! PL: Aktualizacja Horizona (`hotd::update`, D18) z atrapą świata: każda droga porażki zostawia
//! serwer na starej wersji i mówi dlaczego; dobra droga kończy się na nowej.
//! EN: The Horizon update (`hotd::update`, D18) with a fake world: every failure path leaves the
//! server on the old version and says why; the good path ends on the new one.

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::DiagError;
use hotd::codes;
use hotd::update::{Config, Env, Health, Phase, Update, Upstream};
use serde_json::{json, Value};

const OLD: &str = "66f3981f522b24c250b21523a70080965b940424";
const NEW: &str = "9f42064aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LOCK_OK: &str = "[[package]]\nname = \"libloading\"\nversion = \"0.8.9\"\n";
const HOUR: u64 = 3600;

struct Fake {
    old: PathBuf,
    new: PathBuf,
    current: PathBuf,
    lock: Result<String, ()>,
    free: u64,
    build_fails: bool,
    trial_fails: Option<hot_diag::Code>,
    swap_fails: bool,
    health: VecDeque<Health>,
    calm: Health,
    calls: Vec<String>,
    said: Vec<String>,
    kept: Vec<PathBuf>,
}

fn engine_dir(root: &Path, name: &str, rev: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("native-mods.json"),
        json!({ "horizon": rev }).to_string(),
    )
    .unwrap();
    fs::write(dir.join("horizon-engine"), "").unwrap();
    dir.join("horizon-engine")
}

fn fake(name: &str) -> (Fake, PathBuf) {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("update-{name}"));
    let _ = fs::remove_dir_all(&root);
    let old = engine_dir(&root, "old", OLD);
    let new = engine_dir(&root, "new", NEW);
    let calm = Health {
        pid: Some(100),
        p95: Some(10.0),
        players: Some(3),
    };
    (
        Fake {
            current: old.clone(),
            old,
            new,
            lock: Ok(LOCK_OK.into()),
            free: u64::MAX,
            build_fails: false,
            trial_fails: None,
            swap_fails: false,
            health: VecDeque::new(),
            calm,
            calls: Vec::new(),
            said: Vec::new(),
            kept: Vec::new(),
        },
        root,
    )
}

fn failure(code: hot_diag::Code, what: &str) -> DiagError {
    DiagError::new(code, what, "fix")
}

impl Env for Fake {
    fn newest(&mut self) -> Result<Option<Upstream>, DiagError> {
        self.calls.push("newest".into());
        Ok(Some(Upstream {
            tag: "nightly-source/2026-11-13-9f42064".into(),
            rev: NEW.into(),
        }))
    }
    fn cargo_lock(&mut self, rev: &str) -> Result<String, DiagError> {
        self.calls.push(format!("lock {rev}"));
        self.lock
            .clone()
            .map_err(|()| failure(codes::HORIZON_CHECK_FAILED, "no network"))
    }
    fn free_bytes(&mut self) -> Option<u64> {
        Some(self.free)
    }
    fn current_engine(&mut self) -> Option<PathBuf> {
        Some(self.current.clone())
    }
    fn build(&mut self, rev: &str) -> Result<PathBuf, DiagError> {
        self.calls.push(format!("build {rev}"));
        if self.build_fails {
            return Err(failure(codes::ENGINE_BUILD_FAILED, "mixin does not apply"));
        }
        Ok(self.new.clone())
    }
    fn trial(&mut self, old: &Path, new: &Path) -> Result<Value, DiagError> {
        self.calls.push("trial".into());
        assert_eq!((old, new), (self.old.as_path(), self.new.as_path()));
        if let Some(code) = self.trial_fails {
            return Err(failure(code, "p95 30.0 ms against 10.0 ms"));
        }
        Ok(json!({ "ok": true }))
    }
    fn health(&mut self) -> Health {
        self.health.pop_front().unwrap_or(self.calm)
    }
    fn say(&mut self, text: &str) {
        self.said.push(text.into());
    }
    fn swap(&mut self, engine: &Path) -> Result<Value, DiagError> {
        self.calls.push(format!("swap {}", engine.display()));
        if self.swap_fails {
            return Err(failure(
                codes::SWAP_NOT_STARTED,
                "the new engine never got ready",
            ));
        }
        self.current = engine.to_path_buf();
        Ok(json!({ "freeze_ms": 700 }))
    }
    fn cleanup(&mut self, keep: &[PathBuf]) {
        self.kept = keep.to_vec();
    }
    fn info(&mut self, _: &str, _: Value) {}
    fn error(&mut self, _: &DiagError) {}
}

fn trusted() -> Config {
    Config {
        trust: true,
        ..Config::default()
    }
}

fn builds(f: &Fake) -> usize {
    f.calls.iter().filter(|c| c.starts_with("build")).count()
}

#[test]
fn a_revision_without_libloading_is_refused_once_and_the_server_stays() {
    let (mut f, _) = fake("libloading");
    f.lock = Ok("[[package]]\nname = \"wgpu\"\n".into());
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    assert_eq!(u.phase, Phase::Refused);
    assert_eq!(u.code.as_deref(), Some("HOT-E0701"));
    assert_eq!(u.current.as_deref(), Some(OLD));
    assert!(u.reason.as_deref().unwrap().contains("libloading"));
    assert_eq!(u.bad, [NEW]);
    assert_eq!(builds(&f), 0);
    u.step(&mut f, &trusted(), 11 * HOUR);
    assert_eq!(u.phase, Phase::Refused, "the outcome stays visible");
    assert_eq!(
        f.calls.iter().filter(|c| c.starts_with("lock")).count(),
        1,
        "a bad revision is never fetched again"
    );
}

#[test]
fn checks_run_once_an_hour() {
    let (mut f, _) = fake("hourly");
    f.lock = Err(());
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    assert_eq!(u.phase, Phase::Idle);
    assert_eq!(u.code.as_deref(), Some("HOT-E0704"));
    assert!(u.bad.is_empty(), "a network failure is no verdict");
    u.step(&mut f, &trusted(), 10 * HOUR + 60);
    assert_eq!(f.calls.iter().filter(|c| *c == "newest").count(), 1);
    u.step(&mut f, &trusted(), 11 * HOUR);
    assert_eq!(f.calls.iter().filter(|c| *c == "newest").count(), 2);
}

#[test]
fn without_consent_or_disk_nothing_is_built() {
    let (mut f, _) = fake("consent");
    let mut u = Update::default();
    u.step(&mut f, &Config::default(), 10 * HOUR);
    assert_eq!(u.phase, Phase::NeedsConsent);
    assert!(u.reason.as_deref().unwrap().contains("--trust-native-code"));
    f.free = 1024;
    u.step(&mut f, &trusted(), 11 * HOUR);
    assert_eq!(u.phase, Phase::LowDisk);
    assert_eq!(u.code.as_deref(), Some("HOT-E0211"));
    assert_eq!(builds(&f), 0);
    assert!(u.bad.is_empty());
}

#[test]
fn a_failed_build_or_trial_marks_the_revision_bad_and_never_swaps() {
    let (mut f, _) = fake("build");
    f.build_fails = true;
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    assert_eq!(u.phase, Phase::BuildFailed);
    assert_eq!(u.code.as_deref(), Some("HOT-E0301"));
    assert_eq!(u.bad, [NEW]);

    let (mut f, _) = fake("trial");
    f.trial_fails = Some(codes::HORIZON_TRIAL_FAILED);
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    assert_eq!(u.phase, Phase::TrialFailed);
    assert_eq!(u.code.as_deref(), Some("HOT-E0702"));
    assert!(u.reason.as_deref().unwrap().contains("p95 30.0 ms"));
    assert_eq!(u.bad, [NEW]);
    assert!(!f.calls.iter().any(|c| c.starts_with("swap")));
}

#[test]
fn a_broken_trial_is_retried_and_does_not_mark_the_revision_bad() {
    let (mut f, _) = fake("trial-setup");
    f.trial_fails = Some(codes::HORIZON_CHECK_FAILED);
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    assert_eq!(u.phase, Phase::TrialFailed);
    assert!(u.bad.is_empty(), "the trial broke, not the revision");
    assert!(u.reason.as_deref().unwrap().contains("ponowię"));
    f.trial_fails = None;
    u.step(&mut f, &trusted(), 11 * HOUR);
    assert_eq!(u.phase, Phase::Waiting, "an hour later it passes");
    assert!(!f.calls.iter().any(|c| c.starts_with("swap")));
}

#[test]
fn the_good_path_waits_for_the_window_swaps_watches_and_keeps() {
    let (mut f, root) = fake("good");
    let cfg = trusted();
    let mut u = Update::default();
    let t0 = 10 * HOUR;
    u.step(&mut f, &cfg, t0);
    assert_eq!(u.phase, Phase::Waiting);
    assert_eq!(f.said.len(), 1, "the deadline is announced");
    assert!(f.said[0].contains("24 h"));
    // PL: Stan przetrwa restart `hotd`. EN: The state survives a `hotd` restart.
    u.save(&root).unwrap();
    let mut u = Update::load(&root);
    assert_eq!(u.phase, Phase::Waiting);
    for minute in 1..=60 {
        u.step(&mut f, &cfg, t0 + minute * 60);
    }
    assert!(
        !f.calls.iter().any(|c| c.starts_with("swap")),
        "players are online"
    );
    u.step(&mut f, &cfg, t0 + cfg.window - 300);
    assert_eq!(f.said.len(), 2, "the last call");
    u.step(&mut f, &cfg, t0 + cfg.window);
    assert_eq!(u.phase, Phase::Observing);
    assert_eq!(f.current, f.new);
    let swapped = t0 + cfg.window;
    u.step(&mut f, &cfg, swapped + 300);
    assert_eq!(u.phase, Phase::Observing);
    u.step(&mut f, &cfg, swapped + cfg.observe);
    assert_eq!(u.phase, Phase::Kept);
    assert_eq!(u.current.as_deref(), Some(NEW));
    assert_eq!(
        f.kept,
        [f.new.clone(), f.old.clone()],
        "the new and the way back stay"
    );
    assert!(hotd::update::status_lines(&root).is_some());
}

#[test]
fn nobody_online_swaps_at_once() {
    let (mut f, _) = fake("empty");
    f.calm.players = Some(0);
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    u.step(&mut f, &trusted(), 10 * HOUR + 60);
    assert_eq!(u.phase, Phase::Observing);
}

#[test]
fn a_slower_engine_goes_back_and_is_marked_bad() {
    let (mut f, _) = fake("slower");
    let cfg = trusted();
    let mut u = Update::default();
    u.step(&mut f, &cfg, 10 * HOUR);
    f.calm.players = Some(0);
    u.step(&mut f, &cfg, 10 * HOUR + 60);
    assert_eq!(u.phase, Phase::Observing);
    f.calm.p95 = Some(20.0);
    let swapped = u.swapped_at;
    for minute in 1..=10 {
        u.step(&mut f, &cfg, swapped + minute * 60);
    }
    assert_eq!(u.phase, Phase::RolledBack);
    assert_eq!(u.code.as_deref(), Some("HOT-E0703"));
    assert_eq!(f.current, f.old, "back on the old engine");
    assert_eq!(u.bad, [NEW]);
    assert_eq!(u.current.as_deref(), Some(OLD));
}

#[test]
fn a_small_slowdown_is_noise_and_stays() {
    let (mut f, _) = fake("noise");
    let cfg = trusted();
    let mut u = Update::default();
    u.step(&mut f, &cfg, 10 * HOUR);
    f.calm = Health {
        pid: Some(100),
        p95: Some(1.0),
        players: Some(0),
    };
    u.step(&mut f, &cfg, 10 * HOUR + 60);
    f.calm.p95 = Some(2.0);
    let swapped = u.swapped_at;
    u.step(&mut f, &cfg, swapped + cfg.observe);
    assert_eq!(u.phase, Phase::Kept, "1 ms to 2 ms is not worse");
}

#[test]
fn a_crash_after_the_swap_goes_back() {
    let (mut f, _) = fake("crash");
    let cfg = trusted();
    let mut u = Update::default();
    f.calm.players = Some(0);
    u.step(&mut f, &cfg, 10 * HOUR);
    u.step(&mut f, &cfg, 10 * HOUR + 60);
    assert_eq!(u.phase, Phase::Observing);
    f.calm.pid = Some(200);
    u.step(&mut f, &cfg, 10 * HOUR + 120);
    assert_eq!(u.phase, Phase::RolledBack);
    assert_eq!(f.current, f.old);
}

#[test]
fn a_failed_swap_leaves_the_players_on_the_old_engine() {
    let (mut f, root) = fake("swapfail");
    f.calm.players = Some(0);
    f.swap_fails = true;
    let mut u = Update::default();
    u.step(&mut f, &trusted(), 10 * HOUR);
    u.step(&mut f, &trusted(), 10 * HOUR + 60);
    assert_eq!(u.phase, Phase::SwapFailed);
    assert_eq!(u.code.as_deref(), Some("HOT-E0606"));
    assert_eq!(u.bad, [NEW]);
    assert_eq!(f.current, f.old);
    u.save(&root).unwrap();
    let lines = hotd::update::status_lines(&root).unwrap();
    assert!(lines.contains("podmiana się nie udała"), "{lines}");
    assert!(lines.contains("HOT-E0606"), "{lines}");
}
