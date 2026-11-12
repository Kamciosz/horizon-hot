//! PL: Przepływ `hotd` bez sieci: repo serwera jako lokalne repo gita (`file://`), gałąź
//! `hot-builds` pisana przez test tak jak przez CI, klucz podpisu tworzony na czas testu,
//! a silnik udawany: zapisuje komendy i odpowiada plikiem `status.json` jak `hot-core`.
//! Każdy test to jedno sprawdzenie M2.
//!
//! EN: The `hotd` flow without network: the server repository is a local git repository
//! (`file://`), the test writes the `hot-builds` branch like CI does, the signing key is created
//! for the test, and the engine is a fake that records commands and answers with a `status.json`
//! like `hot-core`. Each test is one M2 check.

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use hot_diag::DiagError;
use hotd::daemon::{target, Config, Daemon};
use hotd::engine::Engine;
use serde_json::{json, Value};

fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    let out = Command::new(program)
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut all = vec![
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
    ];
    all.extend_from_slice(args);
    run(dir, "git", &all)
}

/// PL: Udawany silnik z `hot-core`: `hot load <plik>` ładuje mod o id z nazwy pliku.
/// EN: A fake engine with `hot-core`: `hot load <file>` loads a mod whose id is the file stem.
#[derive(Clone)]
struct Fake {
    status: PathBuf,
    commands: Rc<RefCell<Vec<String>>>,
    mods: Rc<RefCell<Vec<(String, String)>>>,
    pid: Rc<Cell<Option<u32>>>,
    /// PL: Id, które „biblioteka” zgłasza zamiast nazwy pliku. EN: An id the library reports.
    declared_id: Rc<RefCell<Option<String>>>,
}

impl Fake {
    fn new(home: &Path) -> Self {
        Self {
            status: home.join("hot-state/status.json"),
            commands: Rc::default(),
            mods: Rc::default(),
            pid: Rc::new(Cell::new(Some(100))),
            declared_id: Rc::default(),
        }
    }

    fn write(&self) {
        let mods: Vec<Value> = self
            .mods
            .borrow()
            .iter()
            .map(|(id, source)| json!({"id": id, "version": "1", "generation": 1, "good": false, "source": source}))
            .collect();
        fs::create_dir_all(self.status.parent().unwrap()).unwrap();
        fs::write(
            &self.status,
            json!({"version": "t", "tick": 0, "mods": mods, "recent_errors": []}).to_string(),
        )
        .unwrap();
    }

    fn take_commands(&self) -> Vec<String> {
        std::mem::take(&mut *self.commands.borrow_mut())
    }
}

impl Engine for Fake {
    fn command(&mut self, line: &str) -> Result<(), DiagError> {
        self.commands.borrow_mut().push(line.to_owned());
        if let Some(path) = line.strip_prefix("hot load ") {
            let stem = Path::new(path).file_stem().unwrap().to_string_lossy();
            let id = self
                .declared_id
                .borrow()
                .clone()
                .unwrap_or_else(|| stem.into_owned());
            let mut mods = self.mods.borrow_mut();
            mods.retain(|(m, _)| *m != id);
            mods.push((id, path.to_owned()));
        } else if let Some(id) = line.strip_prefix("hot unload ") {
            self.mods.borrow_mut().retain(|(m, _)| m != id);
        }
        self.write();
        Ok(())
    }

    fn pid(&mut self) -> Option<u32> {
        self.pid.get()
    }
}

struct World {
    root: PathBuf,
    work: PathBuf,
    builds: PathBuf,
    home: PathBuf,
    key: PathBuf,
    origin: String,
}

impl World {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("hotd-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let origin_dir = root.join("origin.git");
        git(
            &root,
            &["init", "--quiet", "--bare", "-b", "main", "origin.git"],
        );
        let origin = format!("file://{}", origin_dir.display());
        let work = root.join("work");
        git(&root, &["init", "--quiet", "-b", "main", "work"]);
        git(&work, &["remote", "add", "origin", &origin]);
        let builds = root.join("builds");
        git(&root, &["init", "--quiet", "-b", "hot-builds", "builds"]);
        git(&builds, &["remote", "add", "origin", &origin]);
        let key = root.join("key");
        run(
            &root,
            "ssh-keygen",
            &["-q", "-t", "ed25519", "-N", "", "-C", "hot-ci", "-f", "key"],
        );
        let home = root.join("home");
        let public = fs::read_to_string(root.join("key.pub")).unwrap();
        fs::create_dir_all(home.join("hot-keys")).unwrap();
        fs::write(
            home.join("hot-keys/allowed_signers"),
            format!("hot-ci namespaces=\"horizon-hot-mod\" {public}"),
        )
        .unwrap();
        Self {
            root,
            work,
            builds,
            home,
            key,
            origin,
        }
    }

    fn daemon(&self, engine: Fake) -> Daemon<Fake> {
        let mut cfg = Config::new(self.home.clone(), &self.origin, "main");
        cfg.confirm_timeout = Duration::from_secs(2);
        Daemon::open(cfg, engine).unwrap()
    }

    /// PL: Commit na `main`: zapisuje pliki, usuwa ścieżki, wypycha. EN: A commit on `main`.
    fn commit(&self, write: &[(&str, &str)], remove: &[&str]) -> String {
        for (path, text) in write {
            let p = self.work.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
        for path in remove {
            git(&self.work, &["rm", "-r", "--quiet", path]);
        }
        git(&self.work, &["add", "-A"]);
        git(
            &self.work,
            &["commit", "--quiet", "--allow-empty", "-m", "zmiana"],
        );
        git(&self.work, &["push", "--quiet", "origin", "main"]);
        git(&self.work, &["rev-parse", "HEAD"])
    }

    /// PL: To, co robi CI: `<sha>/build.json` i podpisane pliki. EN: What CI does.
    fn publish(&self, sha: &str, mods: &[&str], ok: bool, tamper: bool) {
        let dir = self.builds.join(sha);
        let bin = dir.join(target());
        fs::create_dir_all(&bin).unwrap();
        let build = if ok {
            let files: serde_json::Map<String, Value> = mods
                .iter()
                .map(|id| ((*id).to_owned(), Value::from(format!("{id}.so"))))
                .collect();
            for id in mods {
                let file = bin.join(format!("{id}.so"));
                fs::write(&file, format!("library {id} {sha}")).unwrap();
                run(
                    &bin,
                    "ssh-keygen",
                    &[
                        "-q",
                        "-Y",
                        "sign",
                        "-f",
                        &self.key.display().to_string(),
                        "-n",
                        "horizon-hot-mod",
                        &file.display().to_string(),
                    ],
                );
                if tamper {
                    fs::write(&file, "podmieniona biblioteka").unwrap();
                }
            }
            json!({"commit": sha, "ok": true, "targets": {target(): files}})
        } else {
            json!({"commit": sha, "ok": false, "failed": mods[0],
                   "log": "   Compiling licznik\nerror[E0425]: cannot find value `x` in this scope\n --> src/lib.rs:3:5"})
        };
        fs::write(dir.join("build.json"), build.to_string()).unwrap();
        git(&self.builds, &["add", "-A"]);
        git(&self.builds, &["commit", "--quiet", "-m", "build"]);
        git(&self.builds, &["push", "--quiet", "origin", "hot-builds"]);
    }

    fn status_text(&self) -> String {
        hotd::status::render(&self.home)
    }
}

const MOD_FILES: [(&str, &str); 2] = [
    ("mods/licznik/Cargo.toml", "[package]\nname = \"licznik\"\n"),
    ("mods/licznik/src/lib.rs", "// v1\n"),
];

#[test]
fn good_commit_waits_for_ci_then_loads_the_signed_library() {
    let w = World::new("good");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&MOD_FILES, &[]);

    d.step();
    assert!(fake.take_commands().is_empty(), "nothing before CI is done");
    assert!(d
        .state()
        .waiting
        .as_deref()
        .unwrap()
        .contains("czekam na CI"));

    w.publish(&sha, &["licznik"], true, false);
    d.step();
    let commands = fake.take_commands();
    assert!(commands[0].starts_with("hot load ") && commands[0].ends_with("licznik.so"));
    assert!(commands[1..].iter().all(|c| c == "hot status"));
    assert_eq!(d.state().applied.as_deref(), Some(sha.as_str()));
    assert!(d.state().waiting.is_none());

    // PL: Ten sam commit drugi raz niczego nie robi. EN: The same commit again does nothing.
    d.step();
    assert!(fake.take_commands().is_empty());

    // PL: README nie wymaga CI ani silnika. EN: A README needs neither CI nor the engine.
    let readme = w.commit(&[("README.md", "serwer")], &[]);
    d.step();
    assert!(fake.take_commands().is_empty());
    assert_eq!(d.state().applied.as_deref(), Some(readme.as_str()));
    assert!(w.status_text().contains("licznik 1"));
}

#[test]
fn repo_without_mods_gives_a_clean_server_without_ci() {
    let w = World::new("clean");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&MOD_FILES, &[]);
    w.publish(&sha, &["licznik"], true, false);
    d.step();
    fake.take_commands();

    let gone = w.commit(&[], &["mods"]);
    d.step();
    assert_eq!(fake.take_commands()[0], "hot unload licznik");
    assert!(fake.mods.borrow().is_empty());
    assert_eq!(d.state().applied.as_deref(), Some(gone.as_str()));
}

#[test]
fn tampered_artifact_is_rejected_and_nothing_loads() {
    let w = World::new("tamper");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&MOD_FILES, &[]);
    w.publish(&sha, &["licznik"], true, true);
    d.step();
    assert!(fake.take_commands().is_empty());
    assert!(d.state().applied.is_none());
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0203");
    let file = w
        .home
        .join("hot-state/artifacts")
        .join(&sha)
        .join("licznik.so");
    assert!(!file.exists(), "a file with a bad signature stays on disk");
    let log = fs::read_to_string(w.home.join("logs/hotd.jsonl")).unwrap();
    assert!(log.contains("HOT-E0203"));
}

#[test]
fn commit_that_does_not_compile_leaves_the_server_and_says_why() {
    let w = World::new("broken");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit(&MOD_FILES, &[]);
    w.publish(&v1, &["licznik"], true, false);
    d.step();
    fake.take_commands();

    let broken = w.commit(&[("mods/licznik/src/lib.rs", "x\n")], &[]);
    w.publish(&broken, &["licznik"], false, false);
    d.step();
    assert!(fake.take_commands().is_empty());
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    let text = w.status_text();
    assert!(
        text.contains("CI nie zbudowało moda licznik") && text.contains("E0425"),
        "{text}"
    );

    // PL: Poprawka (zmiana od ostatniego udanego commita) wchodzi. EN: The fix goes in.
    let fixed = w.commit(&[("mods/licznik/src/lib.rs", "// v2\n")], &[]);
    w.publish(&fixed, &["licznik"], true, false);
    d.step();
    assert!(fake.take_commands()[0].starts_with("hot load "));
    assert_eq!(d.state().applied.as_deref(), Some(fixed.as_str()));
}

#[test]
fn engine_crash_right_after_apply_marks_the_commit_bad() {
    let w = World::new("crash");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit(&MOD_FILES, &[]);
    w.publish(&v1, &["licznik"], true, false);
    d.step();
    let v2 = w.commit(&[("mods/licznik/src/lib.rs", "// v2\n")], &[]);
    w.publish(&v2, &["licznik"], true, false);
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(v2.as_str()));
    fake.take_commands();

    fake.pid.set(Some(200));
    d.step();
    assert_eq!(d.state().bad, std::slice::from_ref(&v2));
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    assert!(
        fake.take_commands().is_empty(),
        "a bad commit is not reapplied"
    );
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0206");
}

#[test]
fn mod_id_different_from_its_folder_is_reported() {
    let w = World::new("id");
    let fake = Fake::new(&w.home);
    *fake.declared_id.borrow_mut() = Some("inny".into());
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&MOD_FILES, &[]);
    w.publish(&sha, &["licznik"], true, false);
    d.step();
    assert!(d.state().applied.is_none());
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0205");
    assert!(w.status_text().contains("różni się od katalogu"));
}

#[test]
fn unreachable_repository_is_logged_once() {
    let w = World::new("offline");
    let fake = Fake::new(&w.home);
    let mut cfg = Config::new(
        w.home.clone(),
        &format!("file://{}", w.root.join("nie-ma").display()),
        "main",
    );
    cfg.confirm_timeout = Duration::from_secs(1);
    let mut d = Daemon::open(cfg, fake).unwrap();
    d.step();
    d.step();
    let log = fs::read_to_string(w.home.join("logs/hotd.jsonl")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.contains("HOT-E0201")).count(),
        1,
        "{log}"
    );
}
