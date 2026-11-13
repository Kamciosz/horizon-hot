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
    /// PL: Katalog świata i paczki włączone przy ostatnim `reload`.
    /// EN: The world folder and the packs enabled at the last `reload`.
    world: PathBuf,
    packs: Rc<RefCell<Vec<String>>>,
    /// PL: `reload` bez skutku, jak paczka wyłączona w `datapacks.json`.
    /// EN: `reload` without effect, like a pack disabled in `datapacks.json`.
    reload_ignored: Rc<Cell<bool>>,
}

impl Fake {
    fn new(home: &Path) -> Self {
        Self {
            status: home.join("hot-state/status.json"),
            commands: Rc::default(),
            mods: Rc::default(),
            pid: Rc::new(Cell::new(Some(100))),
            declared_id: Rc::default(),
            world: home.join("world"),
            packs: Rc::default(),
            reload_ignored: Rc::default(),
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
    fn command(&mut self, line: &str) -> Result<String, DiagError> {
        self.commands.borrow_mut().push(line.to_owned());
        if line == "reload" && !self.reload_ignored.get() {
            // PL: Jak Horizon: każdy katalog z `pack.mcmeta` w `datapacks/`. EN: As Horizon does.
            let mut found: Vec<String> = fs::read_dir(self.world.join("datapacks"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().join("pack.mcmeta").exists())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            found.sort();
            // PL: Znaczniki w treści paczki udają ostrzeżenia Horizona w jego logu.
            // EN: Markers in pack content fake Horizon's warnings in its log.
            let mut log = String::new();
            for pack in &found {
                let text = read_all(&self.world.join("datapacks").join(pack));
                if text.contains("ZLY_SCHEMAT") {
                    log.push_str("Horizon: recipe przepisy:stol not loaded: unknown item\n");
                }
                if text.contains("PSUJE_TABELE") {
                    log.push_str("Horizon: the world's data packs are not applied: bad table\n");
                }
            }
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log_file())
                .unwrap();
            std::io::Write::write_all(&mut file, log.as_bytes()).unwrap();
            *self.packs.borrow_mut() = found;
        } else if line == "datapack list" {
            let packs = self.packs.borrow();
            let list: Vec<String> = std::iter::once("[vanilla (built-in)]".to_owned())
                .chain(packs.iter().map(|p| format!("[file/{p} (world)]")))
                .collect();
            return Ok(format!(
                "There are {} data pack(s) enabled: {}\nThere are no more data packs available",
                list.len(),
                list.join(", ")
            ));
        } else if let Some(path) = line.strip_prefix("hot load ") {
            let stem = Path::new(path).file_stem().unwrap().to_string_lossy();
            let id = self
                .declared_id
                .borrow()
                .clone()
                .unwrap_or_else(|| stem.into_owned());
            let mut mods = self.mods.borrow_mut();
            mods.retain(|(m, _)| *m != id);
            mods.push((id, path.to_owned()));
        } else if line == "hot slots" {
            let n = fs::read_to_string(self.status.with_file_name("slots.json"))
                .map(|t| t.matches("\":").count())
                .unwrap_or(0);
            return Ok(format!("Bloki modów: {n} (hot-state/slots.json)."));
        } else if let Some(id) = line.strip_prefix("hot unload ") {
            self.mods.borrow_mut().retain(|(m, _)| m != id);
        }
        self.write();
        Ok(String::new())
    }

    fn pid(&mut self) -> Option<u32> {
        self.pid.get()
    }

    fn log_file(&self) -> PathBuf {
        self.world.with_file_name("engine.log")
    }
}

/// PL: Cała treść plików w katalogu, rekurencyjnie. EN: All file contents in a folder.
fn read_all(dir: &Path) -> String {
    let mut out = String::new();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        if path.is_dir() {
            out.push_str(&read_all(&path));
        } else {
            out.push_str(&fs::read_to_string(&path).unwrap_or_default());
        }
    }
    out
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
        Self::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("hotd-{name}")))
    }

    fn at(root: PathBuf) -> Self {
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
        let bytes: Vec<(&str, &[u8])> = write.iter().map(|(p, t)| (*p, t.as_bytes())).collect();
        self.commit_bytes(&bytes, remove)
    }

    /// PL: Jak `commit`, ale pliki binarne (obrazy PNG). EN: Like `commit`, with binary files.
    fn commit_bytes(&self, write: &[(&str, &[u8])], remove: &[&str]) -> String {
        for (path, content) in write {
            let p = self.work.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
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
    // PL: Stan na teraz w hotd.json i w `hotd status`. EN: The current state, for the installer.
    assert!(d.state().fetch_error.is_some());
    assert!(hotd::status::render(&w.home).contains("repo teraz niedostępne"));

    // PL: Repo wraca → błąd znika ze stanu. EN: The repository is back → the error clears.
    git(&w.root, &["init", "-q", "--bare", "nie-ma"]);
    d.step();
    assert!(d.state().fetch_error.is_none());
}

#[test]
fn commit_waits_while_the_engine_is_not_running() {
    let w = World::new("engine-down");
    let fake = Fake::new(&w.home);
    fake.pid.set(None);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&MOD_FILES, &[]);
    w.publish(&sha, &["licznik"], true, false);

    d.step();
    assert!(
        fake.take_commands().is_empty(),
        "no command to a stopped engine"
    );
    assert!(!d.state().settled(&sha), "the commit is not lost");
    assert!(d
        .state()
        .waiting
        .as_deref()
        .unwrap()
        .contains("czekam na silnik"));

    fake.pid.set(Some(100));
    d.step();
    assert!(fake.take_commands()[0].starts_with("hot load "));
    assert_eq!(d.state().applied.as_deref(), Some(sha.as_str()));
}

const DATA_V1: (&str, &str) = (
    "mods/przepisy/data/przepisy/recipe/stol.json",
    "{\"type\": \"minecraft:crafting_shapeless\", \"v\": 1}\n",
);

fn pack_file(w: &World) -> PathBuf {
    w.home
        .join("world/datapacks/hot-przepisy/data/przepisy/recipe/stol.json")
}

#[test]
fn data_only_commit_copies_the_pack_and_reloads_without_ci() {
    let w = World::new("data");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&[DATA_V1], &[]);

    d.step();
    assert_eq!(fake.take_commands(), ["reload", "datapack list"]);
    assert_eq!(d.state().applied.as_deref(), Some(sha.as_str()));
    assert_eq!(fs::read_to_string(pack_file(&w)).unwrap(), DATA_V1.1);
    let mcmeta =
        fs::read_to_string(w.home.join("world/datapacks/hot-przepisy/pack.mcmeta")).unwrap();
    assert!(mcmeta.contains("min_format"), "{mcmeta}");
    assert!(!w.home.join("world/.hot-staging").exists());

    // PL: Commit bez zmian w data/ nie wysyła `reload`. EN: No data change, no `reload`.
    w.commit(&[("README.md", "x\n")], &[]);
    d.step();
    assert!(fake.take_commands().is_empty());

    // PL: Usunięcie moda usuwa paczkę. EN: Removing the mod removes the pack.
    let gone = w.commit(&[], &["mods/przepisy"]);
    d.step();
    assert_eq!(fake.take_commands(), ["reload", "datapack list"]);
    assert!(!w.home.join("world/datapacks/hot-przepisy").exists());
    assert_eq!(d.state().applied.as_deref(), Some(gone.as_str()));
}

#[test]
fn code_only_commit_sends_no_reload() {
    let w = World::new("code-no-reload");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&MOD_FILES, &[]);
    w.publish(&sha, &["licznik"], true, false);
    d.step();
    let commands = fake.take_commands();
    assert!(commands[0].starts_with("hot load "));
    assert!(!commands.iter().any(|c| c == "reload"), "{commands:?}");
}

#[test]
fn malformed_json_rejects_the_commit_and_writes_nothing() {
    let w = World::new("data-bad");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit(&[DATA_V1], &[]);
    d.step();
    fake.take_commands();

    let bad = w.commit(&[(DATA_V1.0, "{\"type\": zle")], &[]);
    d.step();
    assert!(fake.take_commands().is_empty(), "the engine gets nothing");
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    assert!(d.state().failed.iter().any(|(c, _)| *c == bad));
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0208");
    assert_eq!(fs::read_to_string(pack_file(&w)).unwrap(), DATA_V1.1);
    assert!(w.status_text().contains("paczka danych z błędem"));
}

#[test]
fn hot_folder_without_marker_is_not_touched() {
    let w = World::new("data-foreign");
    let fake = Fake::new(&w.home);
    let foreign = w.home.join("world/datapacks/hot-obcy");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("pack.mcmeta"), "{}").unwrap();
    let mut d = w.daemon(fake.clone());
    w.commit(&[DATA_V1], &[]);
    d.step();
    w.commit(&[], &["mods/przepisy"]);
    d.step();
    assert!(foreign.join("pack.mcmeta").exists());
}

#[test]
fn data_waits_while_the_engine_is_not_running() {
    let w = World::new("data-engine-down");
    let fake = Fake::new(&w.home);
    fake.pid.set(None);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&[DATA_V1], &[]);
    d.step();
    assert!(fake.take_commands().is_empty());
    assert!(
        !pack_file(&w).exists(),
        "nothing written for a stopped engine"
    );
    assert!(!d.state().settled(&sha));

    fake.pid.set(Some(100));
    d.step();
    assert!(pack_file(&w).exists());
    assert_eq!(d.state().applied.as_deref(), Some(sha.as_str()));
}

#[test]
fn pack_the_engine_does_not_enable_is_rolled_back() {
    let w = World::new("data-rollback");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit(&[DATA_V1], &[]);
    d.step();
    fake.take_commands();

    fake.reload_ignored.set(true);
    fake.packs.borrow_mut().clear();
    w.commit(&[(DATA_V1.0, "{\"v\": 2}\n")], &[]);
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0208");
    assert_eq!(
        fs::read_to_string(pack_file(&w)).unwrap(),
        DATA_V1.1,
        "the previous pack is back"
    );
    assert_eq!(
        fake.take_commands(),
        ["reload", "datapack list", "reload"],
        "the old pack is reloaded"
    );
}

#[test]
fn tables_the_engine_does_not_apply_roll_the_packs_back() {
    let w = World::new("data-not-applied");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit(&[DATA_V1], &[]);
    d.step();
    fake.take_commands();

    w.commit(&[(DATA_V1.0, "{\"v\": \"PSUJE_TABELE\"}\n")], &[]);
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0208");
    assert_eq!(fs::read_to_string(pack_file(&w)).unwrap(), DATA_V1.1);
    let log = fs::read_to_string(w.home.join("logs/hotd.jsonl")).unwrap();
    assert!(log.contains("are not applied: bad table"), "{log}");
}

#[test]
fn skipped_files_are_applied_with_a_warning_in_status() {
    let w = World::new("data-skipped");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let sha = w.commit(&[(DATA_V1.0, "{\"v\": \"ZLY_SCHEMAT\"}\n")], &[]);
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(sha.as_str()));
    assert!(w
        .status_text()
        .contains("plik pominięty przez silnik: Horizon: recipe przepisy:stol not loaded"));

    // PL: Poprawka czyści ostrzeżenie. EN: A fix clears the warning.
    w.commit(&[DATA_V1], &[]);
    d.step();
    assert!(d.state().data_warnings.is_empty());
}

#[test]
fn bad_commit_puts_back_the_previous_data_packs() {
    let w = World::new("data-bad-commit");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    w.commit(&[DATA_V1], &[]);
    d.step();
    let v2 = w.commit(
        &[
            (DATA_V1.0, "{\"v\": 2}\n"),
            ("mods/nowy/data/n/tags/block/a.json", "{}"),
        ],
        &[],
    );
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(v2.as_str()));
    assert!(w.home.join("world/datapacks/hot-nowy").exists());
    fake.take_commands();

    fake.pid.set(Some(200));
    d.step();
    assert_eq!(d.state().bad, std::slice::from_ref(&v2));
    assert_eq!(fs::read_to_string(pack_file(&w)).unwrap(), DATA_V1.1);
    assert!(!w.home.join("world/datapacks/hot-nowy").exists());
    assert_eq!(
        fake.take_commands(),
        ["reload"],
        "the restarted engine reloads the good packs"
    );
}

/// PL: Najkrótszy „PNG” dla `hotd`: sprawdza tylko sygnaturę; reszta odróżnia wersje.
/// EN: The shortest "PNG" for `hotd`: only the signature is checked; the rest tells versions apart.
fn png(tag: &str) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    b.extend_from_slice(tag.as_bytes());
    b
}

const LADA: &str = "mods/wyglad/assets/blocks/lada.png";
const LANG: (&str, &str) = (
    "mods/wyglad/assets/lang.json",
    "{\"lada\": \"Lada sklepowa\"}",
);

fn read_slots(w: &World) -> Value {
    serde_json::from_slice(&fs::read(w.home.join("hot-state/slots.json")).unwrap()).unwrap()
}

fn pack_zip(w: &World) -> Vec<u8> {
    fs::read(w.home.join("hot-state/pack/pack.zip")).unwrap()
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_block_texture_gets_a_slot_and_a_pack_without_ci() {
    let w = World::new("look");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit_bytes(&[(LADA, &png("v1")), (LANG.0, LANG.1.as_bytes())], &[]);
    d.step();
    assert_eq!(
        d.state().applied.as_deref(),
        Some(v1.as_str()),
        "no CI needed"
    );
    assert_eq!(fake.take_commands(), ["hot slots"]);
    assert_eq!(read_slots(&w)["blocks"]["wyglad:lada"], 0);
    let zip = pack_zip(&w);
    assert!(has(&zip, b"assets/hot_slots/textures/block/slot_0.png"));
    assert!(has(&zip, &png("v1")));
    assert!(has(&zip, b"Lada sklepowa"));
    let sha1 = fs::read_to_string(w.home.join("hot-state/pack/pack.sha1")).unwrap();

    // PL: Nowy wygląd tego samego bloku: ten sam slot, nowa paczka, silnik nic nie dostaje.
    // EN: A new look for the same block: the same slot, a new pack, nothing for the engine.
    w.commit_bytes(&[(LADA, &png("v2"))], &[]);
    d.step();
    assert!(fake.take_commands().is_empty());
    assert_eq!(read_slots(&w)["blocks"]["wyglad:lada"], 0);
    assert!(has(&pack_zip(&w), &png("v2")));
    assert_ne!(
        fs::read_to_string(w.home.join("hot-state/pack/pack.sha1")).unwrap(),
        sha1
    );

    // PL: Mod usunięty: slot zostaje zajęty, paczka traci teksturę. Nowy blok dostaje kolejny.
    // EN: The mod removed: its slot stays taken, the pack loses the texture. A new block gets the next.
    w.commit(&[], &["mods/wyglad"]);
    d.step();
    assert!(!has(&pack_zip(&w), b"textures/block/slot_0.png"));
    assert_eq!(read_slots(&w)["blocks"]["wyglad:lada"], 0);
    w.commit_bytes(&[("mods/inny/assets/blocks/kamien.png", &png("k"))], &[]);
    d.step();
    assert_eq!(read_slots(&w)["blocks"]["inny:kamien"], 1);
    w.commit_bytes(&[(LADA, &png("v3"))], &[]);
    d.step();
    assert_eq!(
        read_slots(&w)["blocks"]["wyglad:lada"],
        0,
        "the old slot comes back"
    );
    assert!(
        w.status_text().contains("wyglad:lada = hot_slots:slot_0"),
        "{}",
        w.status_text()
    );
}

#[test]
fn a_bad_texture_rejects_the_commit_and_changes_nothing() {
    let w = World::new("look-bad");
    let fake = Fake::new(&w.home);
    let mut d = w.daemon(fake.clone());
    let v1 = w.commit_bytes(&[(LADA, &png("v1"))], &[]);
    d.step();
    fake.take_commands();
    let zip = pack_zip(&w);

    let bad = w.commit_bytes(
        &[
            (LADA, b"GIF89a".as_slice()),
            ("mods/wyglad/assets/blocks/nowy.png", &png("n")),
        ],
        &[],
    );
    d.step();
    assert!(fake.take_commands().is_empty());
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    assert!(d.state().failed.iter().any(|(c, _)| *c == bad));
    assert_eq!(d.state().errors.last().unwrap()["code"], "HOT-E0210");
    assert_eq!(pack_zip(&w), zip);
    assert!(read_slots(&w)["blocks"].get("wyglad:nowy").is_none());
}

/// PL: M7.2: za mało miejsca → `hotd` sprząta swoje stare pliki, nie pobiera repo i nie stosuje
/// commitów, błąd raz; po zwolnieniu miejsca commit wchodzi. EN: M7.2: low disk → `hotd` cleans
/// its old files, fetches and applies nothing, logs once; once space is back the commit applies.
#[test]
fn low_disk_pauses_hotd_and_cleans_its_own_files() {
    let w = World::new("disk");
    let fake = Fake::new(&w.home);
    let state = w.home.join("hot-state");
    for old in ["stary1", "stary2"] {
        fs::create_dir_all(state.join("artifacts").join(old)).unwrap();
    }
    fs::create_dir_all(state.join("core")).unwrap();
    fs::write(state.join("core/hot_core-1.so"), "stary rdzeń").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    fs::write(state.join("core/hot_core-2.so"), "nowy rdzeń").unwrap();
    let sha = w.commit(&MOD_FILES, &[]);
    w.publish(&sha, &["licznik"], true, false);

    let mut cfg = Config::new(w.home.clone(), &w.origin, "main");
    cfg.confirm_timeout = Duration::from_secs(2);
    cfg.min_free_bytes = u64::MAX;
    let mut d = Daemon::open(cfg, fake.clone()).unwrap();
    for _ in 0..3 {
        d.step();
    }
    assert!(fake.take_commands().is_empty());
    assert!(d.state().applied.is_none());
    assert!(
        !state.join("repo.git").exists(),
        "the repository was fetched"
    );
    assert!(d
        .state()
        .waiting
        .as_deref()
        .unwrap()
        .contains("mało miejsca"));
    assert!(w.status_text().contains("mało miejsca na dysku"));
    let log = fs::read_to_string(w.home.join("logs/hotd.jsonl")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.contains("HOT-E0211")).count(),
        1,
        "{log}"
    );
    assert_eq!(fs::read_dir(state.join("artifacts")).unwrap().count(), 0);
    let cores: Vec<_> = fs::read_dir(state.join("core"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(cores, ["hot_core-2.so"]);

    let mut cfg = Config::new(w.home.clone(), &w.origin, "main");
    cfg.confirm_timeout = Duration::from_secs(2);
    cfg.min_free_bytes = 0;
    let mut d = Daemon::open(cfg, fake.clone()).unwrap();
    d.step();
    assert!(fake.take_commands()[0].starts_with("hot load "));
    assert_eq!(d.state().applied.as_deref(), Some(sha.as_str()));
    assert!(d.state().waiting.is_none());
}

/// PL: Bajty, których zip nie ściśnie (prosty generator liniowy). EN: Bytes zip cannot shrink.
fn noise(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (x >> 56) as u8
        })
        .collect()
}

/// PL: Zapisuje `file`, aż system plików odmówi i miejsca przestanie przybywać (APFS oddaje
/// miejsce po usuniętych plikach z opóźnieniem). Przy `slack` > 0 zapisuje go od nowa o tyle
/// krótszy (APFS na pełnym dysku nie skraca pliku, ale usunąć go pozwala).
/// EN: Writes `file` until the file system refuses and no more space appears (APFS frees the
/// space of removed files with a delay). With `slack` > 0 writes it again that much shorter (APFS
/// on a full disk refuses to shorten a file but lets it be removed).
fn fill_disk(file: &Path, slack: u64) {
    use std::io::Write;
    let chunk = vec![0u8; 1 << 20];
    let mut f = fs::File::create(file).unwrap();
    loop {
        let len = f.metadata().unwrap().len();
        while f.write_all(&chunk).is_ok() {}
        while f.write_all(&chunk[..4096]).is_ok() {}
        let _ = f.sync_all();
        std::thread::sleep(Duration::from_millis(500));
        if f.metadata().unwrap().len() == len {
            break;
        }
    }
    let full = f.metadata().unwrap().len();
    if slack == 0 {
        return;
    }
    drop(f);
    fs::remove_file(file).unwrap();
    let mut f = fs::File::create(file).unwrap();
    let mut left = full.saturating_sub(slack);
    while left > 0 {
        let n = left.min(chunk.len() as u64);
        f.write_all(&chunk[..usize::try_from(n).unwrap()]).unwrap();
        left -= n;
    }
    f.sync_all().unwrap();
}

/// PL: M7.2 na prawdziwie pełnym dysku. Działa tylko ze zmienną `HOT_FULL_DISK_DIR`: katalog na
/// małym, osobnym woluminie (np. obraz `hdiutil` 64 MB), nigdy na dysku systemu. Sprawdza, że
/// brak miejsca w połowie pobierania albo budowy paczki nie zostawia uciętych plików stanu ani
/// paczki, nie kończy się paniką, a po zwolnieniu miejsca commit wchodzi.
/// EN: M7.2 on a really full disk. Runs only with `HOT_FULL_DISK_DIR`: a folder on a small, separate
/// volume (e.g. a 64 MB `hdiutil` image), never the system disk. Checks that running out of space
/// mid-fetch or mid-pack leaves no torn state or pack file, does not panic, and that the commit
/// applies once space is back.
#[test]
fn full_disk_leaves_state_and_pack_whole() {
    let Some(dir) = std::env::var_os("HOT_FULL_DISK_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let filler = dir.join("filler");
    let _ = fs::remove_file(&filler);
    let w = World::at(dir.join("w"));
    let fake = Fake::new(&w.home);
    let open = || {
        let mut cfg = Config::new(w.home.clone(), &w.origin, "main");
        cfg.confirm_timeout = Duration::from_secs(2);
        cfg.min_free_bytes = 0;
        Daemon::open(cfg, fake.clone()).unwrap()
    };
    let mut d = open();
    let v1 = w.commit_bytes(&[(LADA, &png("v1")), (LANG.0, LANG.1.as_bytes())], &[]);
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(v1.as_str()));
    let (zip1, slots1) = (pack_zip(&w), read_slots(&w));

    fake.take_commands();
    let mut big = png("v2");
    big.extend(noise(4 << 20));
    // PL: Nowy blok zmienia przydział slotów, więc wznowienie musi wysłać `hot slots`.
    // EN: A new block changes the slot assignment, so the retry must send `hot slots`.
    let nowy = "mods/wyglad/assets/blocks/nowy.png";
    let v2 = w.commit_bytes(&[(LADA, &big), (nowy, &png("n"))], &[]);
    // PL: 0 = pada już pobieranie; 6 MB = pobranie (~4 MB) wchodzi, paczka (~4 MB) już nie.
    // EN: 0 = the fetch fails; 6 MB = the fetch (~4 MB) fits, the pack (~4 MB) does not.
    for slack in [0, 6 << 20] {
        fill_disk(&filler, slack);
        d.step();
        let applied = open().state().applied.clone();
        let errors: Vec<Value> = d.state().errors.iter().map(|e| e["code"].clone()).collect();
        let why = format!("slack {slack}: applied {applied:?}, errors {errors:?}");
        assert_eq!(
            d.state().applied,
            applied,
            "state file differs from memory; {why}"
        );
        assert_eq!(applied.as_deref(), Some(v1.as_str()), "{why}");
        assert_eq!(pack_zip(&w), zip1, "{why}");
        assert_eq!(read_slots(&w), slots1, "{why}");
        assert!(d.state().failed.is_empty(), "{why}: {:?}", d.state().failed);
        assert!(fake.take_commands().is_empty(), "{why}");
        fs::remove_file(&filler).unwrap();
    }
    let codes: Vec<Value> = d.state().errors.iter().map(|e| e["code"].clone()).collect();
    assert!(codes.contains(&json!("HOT-E0211")), "{codes:?}");
    assert!(!codes.contains(&json!("HOT-E0210")), "{codes:?}");
    d.step();
    assert_eq!(d.state().applied.as_deref(), Some(v2.as_str()));
    assert!(d.state().waiting.is_none());
    assert!(has(&pack_zip(&w), &big[..64]));
    assert_eq!(read_slots(&w)["blocks"]["wyglad:nowy"], 1);
    assert_eq!(fake.take_commands(), ["hot slots"]);
}
