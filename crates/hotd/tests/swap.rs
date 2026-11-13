//! PL: Podmiana silnika (D11) na udawanym `horizon-server` i udawanym proxy. Każdy test sprawdza
//! kolejność kroków i to, która strona zostaje aktywna. Pauzę na prawdziwym silniku mierzy test
//! ręczny (`docs/podmiana.md`).
//!
//! EN: The engine swap (D11) on a fake `horizon-server` and a fake proxy. Each test checks the step
//! order and which side stays active. The pause on a real engine is measured by the manual test
//! (`docs/podmiana.md`).

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use std::cell::{Cell, RefCell};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use hot_diag::{LogConfig, Logger, Redactor};
use hotd::swap::{self, Engines, Layout, Limits, Started, Switcher, WAITING_LINE};
use serde_json::{json, Value};

type Events = Rc<RefCell<Vec<String>>>;

/// PL: Zachowanie udawanego silnika. EN: How the fake engine behaves.
#[derive(Clone, Copy)]
struct Plan {
    /// PL: `start` nowej strony kończy się błędem. EN: The new side's `start` fails.
    start_fails: bool,
    /// PL: Nowa strona pisze linię czekania. EN: The new side writes the waiting line.
    ready: bool,
    /// PL: Nowa strona zajmuje świat po zatrzymaniu starej. EN: The new side takes the world.
    takes_over: bool,
    /// PL: Stara strona wraca po nieudanej podmianie. EN: The old side comes back.
    old_comes_back: bool,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            start_fails: false,
            ready: true,
            takes_over: true,
            old_comes_back: true,
        }
    }
}

struct FakeEngines {
    home: PathBuf,
    events: Events,
    plan: Plan,
    /// PL: Liczba `stop` (nowa strona startuje dopiero po zwolnieniu świata).
    /// EN: The number of `stop`s (the new side starts only once the world is free).
    stops: Rc<Cell<u32>>,
}

impl FakeEngines {
    fn side(&self, dir: &Path) -> &'static str {
        if dir == self.home {
            "a"
        } else {
            "b"
        }
    }
}

struct FakeStarted {
    result: Option<Result<(), String>>,
    after_stop: Option<(Rc<Cell<u32>>, u32)>,
}

impl Started for FakeStarted {
    fn exited(&mut self) -> Option<Result<(), String>> {
        match &self.after_stop {
            Some((stops, before)) if stops.get() > *before => Some(Ok(())),
            Some(_) => None,
            None => self.result.clone(),
        }
    }
}

impl Engines for FakeEngines {
    fn start(&mut self, dir: &Path) -> Result<Box<dyn Started>, String> {
        let side = self.side(dir);
        self.events.borrow_mut().push(format!("start {side}"));
        // PL: Pierwszy `start` to nowa strona; kolejny to powrót starej.
        // EN: The first `start` is the new side; a later one is the old side coming back.
        let first = self
            .events
            .borrow()
            .iter()
            .filter(|e| e.starts_with("start"))
            .count()
            == 1;
        if !first {
            let result = if self.plan.old_comes_back {
                Ok(())
            } else {
                Err("exit status: 1".to_owned())
            };
            return Ok(Box::new(FakeStarted {
                result: Some(result),
                after_stop: None,
            }));
        }
        if self.plan.start_fails {
            return Ok(Box::new(FakeStarted {
                result: Some(Err("exit status: 1".into())),
                after_stop: None,
            }));
        }
        if self.plan.ready {
            fs::create_dir_all(dir.join("logs")).unwrap();
            fs::write(
                dir.join("logs/latest.log"),
                format!("[INFO] {WAITING_LINE} (session.lock)\n"),
            )
            .unwrap();
        }
        Ok(Box::new(FakeStarted {
            result: None,
            after_stop: self
                .plan
                .takes_over
                .then(|| (Rc::clone(&self.stops), self.stops.get())),
        }))
    }

    fn stop(&mut self, dir: &Path) -> Result<(), String> {
        let side = self.side(dir);
        self.events.borrow_mut().push(format!("stop {side}"));
        self.stops.set(self.stops.get() + 1);
        Ok(())
    }

    fn halt_waiting(&mut self, dir: &Path) {
        let side = self.side(dir);
        self.events.borrow_mut().push(format!("halt {side}"));
    }

    fn command(&mut self, dir: &Path, line: &str) -> Result<String, String> {
        let side = self.side(dir);
        self.events.borrow_mut().push(format!("cmd {side} {line}"));
        Ok(String::new())
    }

    fn configure(&mut self, dir: &Path, settings: &[String]) -> Result<(), String> {
        let side = self.side(dir);
        self.events
            .borrow_mut()
            .push(format!("configure {side} {}", settings.join(" ")));
        let file = dir.join("server.toml");
        let mut text = fs::read_to_string(&file).unwrap();
        for s in settings {
            if let Some(port) = s.strip_prefix("server.port=") {
                text = text.replace("port = 25570", &format!("port = {port}"));
            }
            if let Some(path) = s.strip_prefix("engine.path=") {
                text = text
                    .lines()
                    .map(|l| {
                        if l.starts_with("path = ") {
                            format!("path = \"{path}\"")
                        } else {
                            l.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        fs::write(file, text).unwrap();
        Ok(())
    }

    fn phase(&mut self, _dir: &Path) -> String {
        "stopped".into()
    }
}

struct FakeProxy {
    events: Events,
    refuses: bool,
}

impl Switcher for FakeProxy {
    fn switch(&self, to: SocketAddr, held: &mut dyn FnMut()) -> Result<Value, String> {
        self.events.borrow_mut().push(format!("switch {to}"));
        if self.refuses {
            return Err("hot-proxy: connection refused".into());
        }
        self.events.borrow_mut().push("held".into());
        held();
        Ok(json!({ "engine": to.to_string(), "moved": 1, "failed": 0 }))
    }

    fn set_engine(&self, to: SocketAddr) -> Result<(), String> {
        self.events.borrow_mut().push(format!("engine {to}"));
        Ok(())
    }
}

struct Server {
    home: PathBuf,
    events: Events,
    log: Logger,
}

impl Server {
    fn new(name: &str) -> Self {
        let home = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("swap-{name}"));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("world")).unwrap();
        fs::create_dir_all(home.join("horizon-mods")).unwrap();
        fs::write(home.join("world/level.dat"), "x").unwrap();
        fs::write(home.join("ops.json"), "[]").unwrap();
        fs::write(
            home.join("server.toml"),
            "[server]\nport = 25570\nbind = \"127.0.0.1\"\n\n[engine]\npath = \"/eng/old\"\n",
        )
        .unwrap();
        let log = Logger::open(
            LogConfig::new("hotd-engine", "test", home.join("logs")),
            Redactor::new(),
        )
        .unwrap();
        Self {
            home,
            events: Rc::default(),
            log,
        }
    }

    fn layout(&self) -> Layout {
        Layout {
            home: self.home.clone(),
        }
    }

    fn swap(&self, plan: Plan, refuses: bool, engine: Option<&str>) -> Result<Value, Value> {
        self.events.borrow_mut().clear();
        let mut engines = FakeEngines {
            home: self.home.clone(),
            events: Rc::clone(&self.events),
            plan,
            stops: Rc::default(),
        };
        let proxy = FakeProxy {
            events: Rc::clone(&self.events),
            refuses,
        };
        let limits = Limits {
            warm: Duration::from_millis(300),
            take_over: Duration::from_millis(300),
            back: Duration::from_millis(300),
        };
        swap::swap(
            &self.layout(),
            &mut engines,
            &proxy,
            engine.map(Path::new),
            limits,
            &self.log,
        )
        .map_err(|e| Value::Object(e.fields()))
    }

    fn events(&self) -> Vec<String> {
        self.events.borrow().clone()
    }

    fn recorded(&self) -> Option<Value> {
        let text = fs::read_to_string(self.home.join("hot-state/engine.json")).ok()?;
        Some(serde_json::from_str(&text).unwrap())
    }
}

#[test]
fn a_swap_warms_the_new_side_and_stops_the_old_one_only_after_the_hold() {
    let s = Server::new("ok");
    let report = s.swap(Plan::default(), false, Some("/eng/new")).unwrap();
    assert_eq!(
        s.events(),
        [
            "configure b server.port=25571 panel.bind= api.bind= engine.path=/eng/new",
            "start b",
            "cmd a save-all flush",
            "switch 127.0.0.1:25571",
            "held",
            "stop a",
        ]
    );
    assert_eq!(report["to"], "b");
    assert_eq!(report["proxy"]["moved"], 1);
    let rec = s.recorded().unwrap();
    assert_eq!(rec["side"], "b");
    assert_eq!(rec["address"], "127.0.0.1:25571");
    assert_eq!(rec["engine"], "/eng/new");
    assert_eq!(s.layout().active_dir(), s.home.join("engine-b"));
    // PL: Strona `b` dzieli świat i pliki wspólne, ale ma własny `server.toml`.
    // EN: Side `b` shares the world and common files but has its own `server.toml`.
    let b = s.home.join("engine-b");
    for shared in ["world", "ops.json", "horizon-mods"] {
        assert_eq!(fs::read_link(b.join(shared)).unwrap(), s.home.join(shared));
    }
    assert!(!b.join("server.toml").is_symlink());
    assert!(fs::read_to_string(s.home.join("server.toml"))
        .unwrap()
        .contains("port = 25570"));
}

#[test]
fn the_next_swap_goes_back_to_side_a_with_the_engine_in_use() {
    let s = Server::new("back-to-a");
    s.swap(Plan::default(), false, Some("/eng/new")).unwrap();
    s.swap(Plan::default(), false, None).unwrap();
    assert_eq!(
        s.events(),
        [
            "configure a engine.path=/eng/new",
            "start a",
            "cmd b save-all flush",
            "switch 127.0.0.1:25570",
            "held",
            "stop b",
        ]
    );
    let rec = s.recorded().unwrap();
    assert_eq!(rec["side"], "a");
    assert_eq!(rec["address"], "127.0.0.1:25570");
}

fn assert_stayed_on_a(s: &Server, err: &Value) {
    assert_eq!(err["code"], "HOT-E0606", "{err:#}");
    assert!(s.recorded().is_none(), "engine.json written");
    assert!(
        s.events().contains(&"halt b".to_owned()),
        "{:?}",
        s.events()
    );
    assert!(
        !s.events().iter().any(|e| e == "stop a"),
        "{:?}",
        s.events()
    );
}

#[test]
fn a_new_engine_that_fails_to_start_leaves_the_players_on_the_old_one() {
    let s = Server::new("start-fails");
    let plan = Plan {
        start_fails: true,
        ..Plan::default()
    };
    let err = s.swap(plan, false, Some("/eng/broken")).unwrap_err();
    assert_stayed_on_a(&s, &err);
    assert!(!s.events().iter().any(|e| e.starts_with("switch")));
}

#[test]
fn a_new_engine_without_the_waiting_line_is_halted_before_the_freeze() {
    let s = Server::new("never-ready");
    let plan = Plan {
        ready: false,
        takes_over: false,
        ..Plan::default()
    };
    let err = s.swap(plan, false, Some("/eng/no-hot-host")).unwrap_err();
    assert_stayed_on_a(&s, &err);
    assert!(err["what"].as_str().unwrap().contains(WAITING_LINE));
    assert!(!s.events().iter().any(|e| e.starts_with("cmd a")));
}

#[test]
fn a_proxy_that_refuses_the_switch_leaves_the_old_engine_running() {
    let s = Server::new("proxy-refuses");
    let err = s.swap(Plan::default(), true, Some("/eng/new")).unwrap_err();
    assert_stayed_on_a(&s, &err);
}

#[test]
fn a_new_engine_that_does_not_take_the_world_brings_the_old_one_back() {
    let s = Server::new("no-take-over");
    let plan = Plan {
        takes_over: false,
        ..Plan::default()
    };
    let err = s.swap(plan, false, Some("/eng/new")).unwrap_err();
    assert_eq!(err["code"], "HOT-E0607", "{err:#}");
    assert_eq!(
        s.events()[4..],
        [
            "held",
            "stop a",
            "halt b",
            "engine 127.0.0.1:25570",
            "start a"
        ]
    );
    assert!(s.recorded().is_none());
    assert!(err["state"].as_str().unwrap().contains("runs again"));
}

#[test]
fn a_failed_return_is_reported_with_the_old_engine_error() {
    let s = Server::new("no-way-back");
    let plan = Plan {
        takes_over: false,
        old_comes_back: false,
        ..Plan::default()
    };
    let err = s.swap(plan, false, Some("/eng/new")).unwrap_err();
    assert_eq!(err["code"], "HOT-E0607");
    assert!(err["state"].as_str().unwrap().contains("did not come back"));
    assert!(err
        .to_string()
        .contains("the old engine's start: exit status: 1"));
}
