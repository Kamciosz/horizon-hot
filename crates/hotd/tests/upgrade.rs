//! PL: Samoaktualizacja horizon-hot (M10.1, ADR 0014) na fałszywym świecie: który tryb, kiedy
//! czeka, kiedy wraca i kiedy nie robi nic.
//! EN: The horizon-hot self-update (M10.1, ADR 0014) on a fake world: which mode, when it waits,
//! when it goes back and when it does nothing.
#![allow(clippy::unwrap_used)] // PL: testy. EN: tests.

use hot_diag::DiagError;
use hotd::codes;
use hotd::upgrade::{Env, Mode, Phase, Upgrade};
use serde_json::Value;

const DAY: u64 = 86_400;
const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

fn failure() -> DiagError {
    DiagError::new(codes::UPGRADE_CHECK_FAILED, "fake failure", "nothing")
}

#[derive(Default)]
struct Fake {
    version: Option<String>,
    head: Option<String>,
    files: Vec<String>,
    players: usize,
    fetch_fails: bool,
    build_fails: bool,
    core_fails: bool,
    core_unconfirmed: bool,
    restart_fails: bool,
    proxy_refuses: bool,
    proxy_dies: bool,
    calls: Vec<String>,
}

impl Fake {
    fn new(files: &[&str]) -> Self {
        Self {
            version: Some(OLD.into()),
            head: Some(NEW.into()),
            files: files.iter().map(|f| (*f).to_owned()).collect(),
            ..Self::default()
        }
    }

    fn did(&self, call: &str) -> bool {
        self.calls.iter().any(|c| c == call)
    }

    fn count(&self, call: &str) -> usize {
        self.calls.iter().filter(|c| *c == call).count()
    }
}

impl Env for Fake {
    fn current(&mut self) -> Option<String> {
        self.version.clone()
    }
    fn fetch(&mut self) -> Result<Option<String>, DiagError> {
        self.calls.push("fetch".into());
        if self.fetch_fails {
            return Err(failure());
        }
        Ok(self.head.clone())
    }
    fn changed(&mut self, _: &str, _: &str) -> Result<Vec<String>, DiagError> {
        Ok(self.files.clone())
    }
    fn build(&mut self, _: &str, mode: Mode) -> Result<(), DiagError> {
        self.calls.push(format!("build {mode:?}"));
        if self.build_fails {
            return Err(failure());
        }
        Ok(())
    }
    fn load_core(&mut self) -> Result<(), DiagError> {
        self.calls.push("load_core".into());
        if self.core_fails {
            return Err(DiagError::new(
                codes::UPGRADE_CORE_FAILED,
                "fake rejection",
                "nothing",
            ));
        }
        if self.core_unconfirmed {
            return Err(failure());
        }
        Ok(())
    }
    fn load_proxy(&mut self) -> Result<bool, DiagError> {
        self.calls.push("load_proxy".into());
        if self.proxy_dies {
            return Err(DiagError::new(
                codes::UPGRADE_ROLLED_BACK,
                "fake silence",
                "nothing",
            ));
        }
        Ok(!self.proxy_refuses)
    }
    fn players(&mut self) -> usize {
        self.players
    }
    fn install(&mut self) -> Result<(), DiagError> {
        self.calls.push("install".into());
        Ok(())
    }
    fn restart(&mut self) -> Result<(), DiagError> {
        self.calls.push("restart".into());
        if self.restart_fails && !self.did("restore") {
            return Err(failure());
        }
        Ok(())
    }
    fn restore(&mut self) {
        self.calls.push("restore".into());
    }
    fn discard(&mut self) {
        self.calls.push("discard".into());
    }
    fn set_version(&mut self, rev: &str) -> Result<(), DiagError> {
        self.version = Some(rev.into());
        Ok(())
    }
    fn info(&mut self, _: &str, _: Value) {}
    fn error(&mut self, _: &DiagError) {}
}

#[test]
fn a_core_change_goes_in_at_once_with_players() {
    let mut env = Fake::new(&["crates/hot-core/src/lib.rs"]);
    env.players = 5;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Done);
    assert_eq!(env.version.as_deref(), Some(NEW));
    assert!(env.did("build Core") && env.did("load_core"));
    assert!(!env.did("restart"), "a core update restarts nothing");
}

#[test]
fn a_core_that_does_not_load_is_skipped_for_good() {
    let mut env = Fake::new(&["crates/hot-core/src/lib.rs"]);
    env.core_fails = true;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::CoreFailed);
    assert_eq!(u.code.as_deref(), Some("HOT-E0707"));
    assert_eq!(env.version.as_deref(), Some(OLD));
    assert_eq!(u.bad, vec![NEW.to_owned()]);
    u.step(&mut env, 3 * DAY, DAY);
    assert_eq!(
        env.count("build Core"),
        1,
        "a bad version is not built again"
    );
}

#[test]
fn a_core_load_the_engine_did_not_confirm_is_retried() {
    let mut env = Fake::new(&["crates/hot-sdk/src/lib.rs"]);
    env.core_unconfirmed = true;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::CheckFailed);
    assert_eq!(u.code.as_deref(), Some("HOT-E0705"));
    assert!(u.bad.is_empty(), "no answer is not a rejection");
    env.core_unconfirmed = false;
    u.step(&mut env, 2 * DAY, DAY);
    assert_eq!(u.phase, Phase::Done);
    assert_eq!(env.count("build Core"), 2);
}

#[test]
fn other_code_waits_for_an_empty_server() {
    let mut env = Fake::new(&["crates/hotd/src/daemon.rs"]);
    env.players = 2;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Staged);
    assert!(env.did("build Full") && !env.did("install"));
    u.step(&mut env, DAY + 600, DAY);
    assert!(!env.did("install"), "players still in game");
    assert_eq!(
        env.count("fetch"),
        1,
        "a staged version does not fetch again"
    );
    env.players = 0;
    u.step(&mut env, DAY + 1200, DAY);
    assert_eq!(u.phase, Phase::Done);
    assert_eq!(env.version.as_deref(), Some(NEW));
    assert!(env.did("install") && env.did("restart") && !env.did("restore"));
}

#[test]
fn services_that_do_not_come_up_bring_the_old_files_back() {
    let mut env = Fake::new(&["crates/hotd/src/daemon.rs"]);
    env.restart_fails = true;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::RolledBack);
    assert_eq!(u.code.as_deref(), Some("HOT-E0708"));
    assert_eq!(
        env.calls[env.calls.len() - 3..],
        ["restart", "restore", "restart"],
        "the old files come back and the services restart again"
    );
    assert_eq!(env.version.as_deref(), Some(OLD));
    assert_eq!(u.bad, vec![NEW.to_owned()]);
}

#[test]
fn engine_and_installer_changes_wait_for_the_owner() {
    for file in [
        "crates/hot-host/api/src/lib.rs",
        "scripts/server-install.sh",
    ] {
        let mut env = Fake::new(&["crates/hot-core/src/lib.rs", file]);
        let mut u = Upgrade::default();
        u.step(&mut env, DAY, DAY);
        assert_eq!(u.phase, Phase::Blocked, "{file}");
        assert_eq!(u.code.as_deref(), Some("HOT-E0706"));
        assert!(!env.calls.iter().any(|c| c.starts_with("build")), "{file}");
        assert_eq!(env.version.as_deref(), Some(OLD));
    }
}

#[test]
fn docs_only_move_the_version_without_a_build() {
    let mut env = Fake::new(&["docs/a.md", "crates/hotd/tests/flow.rs"]);
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Done);
    assert_eq!(env.version.as_deref(), Some(NEW));
    assert!(!env.calls.iter().any(|c| c.starts_with("build")));
}

#[test]
fn a_tag_or_commit_pins_the_server() {
    let mut env = Fake::new(&["crates/hotd/src/daemon.rs"]);
    env.head = None;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Pinned);
    assert!(!env.calls.iter().any(|c| c.starts_with("build")));
}

#[test]
fn the_network_and_the_build_run_once_a_day() {
    let mut env = Fake::new(&["crates/hotd/src/daemon.rs"]);
    env.players = 1;
    env.fetch_fails = true;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::CheckFailed);
    assert_eq!(u.code.as_deref(), Some("HOT-E0705"));
    u.step(&mut env, DAY + 600, DAY);
    assert_eq!(env.count("fetch"), 1, "no second fetch within a day");
    env.fetch_fails = false;
    env.build_fails = true;
    u.step(&mut env, 2 * DAY, DAY);
    assert_eq!(u.phase, Phase::CheckFailed);
    assert!(
        u.bad.is_empty(),
        "a failed build is retried, not marked bad"
    );
    env.build_fails = false;
    u.step(&mut env, 3 * DAY, DAY);
    assert_eq!(u.phase, Phase::Staged);
}

#[test]
fn the_status_names_the_phase_and_the_code() {
    let home = std::env::temp_dir().join(format!("hot-upgrade-status-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let mut env = Fake::new(&["scripts/server-install.sh"]);
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    u.save(&home).unwrap();
    let text = hotd::upgrade::status_lines(&home).unwrap();
    assert!(
        text.starts_with("horizon-hot 1111111: nowa wersja czeka na instalator"),
        "{text}"
    );
    assert!(
        text.contains("uruchom instalator ponownie (HOT-E0706)"),
        "{text}"
    );
    assert_eq!(Upgrade::load(&home), u);
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_version_the_installer_already_built_is_not_deployed_again() {
    let mut env = Fake::new(&["crates/hotd/src/daemon.rs"]);
    env.players = 1;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Staged);
    env.version = Some(NEW.into());
    env.players = 0;
    u.step(&mut env, DAY + 600, DAY);
    assert_eq!(u.phase, Phase::Idle);
    assert!(env.did("discard") && !env.did("install") && !env.did("restart"));
}

#[test]
fn a_proxy_change_takes_the_players_over_with_players() {
    let mut env = Fake::new(&["crates/hot-proxy/src/link/mod.rs"]);
    env.players = 3;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Done);
    assert_eq!(env.version.as_deref(), Some(NEW));
    assert!(env.did("build Proxy") && env.did("load_proxy"));
    assert!(
        !env.did("install") && !env.did("restart"),
        "the proxy hands over in place, nothing restarts"
    );
}

#[test]
fn a_refused_proxy_change_waits_for_an_empty_server() {
    let mut env = Fake::new(&["crates/hot-proxy/src/upgrade.rs"]);
    env.players = 2;
    env.proxy_refuses = true;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::Staged);
    assert!(u.bad.is_empty(), "a refusal is not a broken version");
    assert!(env.did("load_proxy") && env.did("build Full") && !env.did("install"));
    env.players = 0;
    u.step(&mut env, DAY + 600, DAY);
    assert_eq!(u.phase, Phase::Done);
    assert!(env.did("install") && env.did("restart"));
}

#[test]
fn a_proxy_that_dies_after_the_takeover_is_skipped_for_good() {
    let mut env = Fake::new(&["crates/hot-proxy/src/session.rs"]);
    env.proxy_dies = true;
    let mut u = Upgrade::default();
    u.step(&mut env, DAY, DAY);
    assert_eq!(u.phase, Phase::RolledBack);
    assert_eq!(u.code.as_deref(), Some("HOT-E0708"));
    assert_eq!(env.version.as_deref(), Some(OLD));
    assert_eq!(u.bad, vec![NEW.to_owned()]);
    assert!(
        !env.did("build Full"),
        "a broken proxy is not deployed later"
    );
}
