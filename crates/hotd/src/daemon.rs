//! PL: Jeden krok `hotd` (pętla w `main` woła go co 10 s, D17):
//! 1. sprawdzenie, czy silnik nie padł zaraz po ostatnim commicie (D13);
//! 2. `git fetch`;
//! 3. klasyfikacja zmian od ostatniego **zastosowanego** commita (D24);
//! 4. pobranie z `hot-builds` i sprawdzenie podpisu artefaktów;
//! 5. `hot unload` / `hot load` w silniku i potwierdzenie z `hot-state/status.json`.
//!
//! Commit, który się nie udał, zostaje zapamiętany i nie jest ponawiany; następny commit liczy
//! zmiany od ostatniego udanego, więc poprawka niczego nie gubi.
//!
//! EN: One `hotd` step (the loop in `main` calls it every 10 s, D17): check whether the engine
//! died right after the last commit (D13), `git fetch`, classify changes since the last **applied**
//! commit (D24), fetch artifacts from `hot-builds` and check their signatures, `hot unload` /
//! `hot load` in the engine and confirmation from `hot-state/status.json`. A failed commit is
//! remembered and not retried; the next commit counts changes from the last applied one, so a
//! fix loses nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hot_diag::{DiagError, LogConfig, Logger, Redactor};
use serde_json::{json, Value};

use crate::classify::{classify, code_mods, plan, valid_id};
use crate::engine::Engine;
use crate::git::{Mirror, BUILDS_BRANCH};
use crate::state::{write_atomic, State};
use crate::{codes, sign, VERSION};

/// PL: Ustawienia `hotd`. EN: `hotd` settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// PL: `HOT_HOME` silnika: tu są `hot-state/` i `logs/`. EN: The engine's `HOT_HOME`.
    pub home: PathBuf,
    pub repo: String,
    pub branch: String,
    pub allowed_signers: PathBuf,
    /// PL: Ile czekać na potwierdzenie z `hot-core`. EN: How long to wait for `hot-core`.
    pub confirm_timeout: Duration,
    /// PL: Awaria silnika w tym czasie po zastosowaniu = zły commit (D13: 60 s).
    /// EN: An engine crash within this time after applying = a bad commit (D13: 60 s).
    pub crash_window: Duration,
}

impl Config {
    #[must_use]
    pub fn new(home: PathBuf, repo: &str, branch: &str) -> Self {
        let allowed_signers = home.join("hot-keys").join("allowed_signers");
        Self {
            home,
            repo: repo.to_owned(),
            branch: branch.to_owned(),
            allowed_signers,
            confirm_timeout: Duration::from_secs(60),
            crash_window: Duration::from_secs(60),
        }
    }

    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.home.join("hot-state").join("hotd.json")
    }

    #[must_use]
    pub fn core_status(&self) -> PathBuf {
        self.home.join("hot-state").join("status.json")
    }
}

/// PL: Nazwa platformy w `hot-builds`, np. `x86_64-linux`. EN: Platform name in `hot-builds`.
#[must_use]
pub fn target() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// PL: Ile katalogów commitów trzymać w `hot-state/artifacts/`. EN: Artifact folders kept.
const KEEP_ARTIFACTS: usize = 5;

/// PL: Demon z silnikiem `E`. EN: The daemon with engine `E`.
pub struct Daemon<E: Engine> {
    cfg: Config,
    mirror: Mirror,
    engine: E,
    log: Arc<Logger>,
    state: State,
    /// PL: Ostatni błąd pobierania; ten sam nie trafia do logu co 10 s.
    /// EN: The last fetch error; the same one is not logged every 10 s.
    last_fetch_error: Option<String>,
}

impl<E: Engine> Daemon<E> {
    /// PL: Otwiera log i stan. EN: Opens the log and the state.
    ///
    /// # Errors
    /// PL: Log albo plik stanu nie działa. EN: The log or the state file does not work.
    pub fn open(cfg: Config, engine: E) -> Result<Self, DiagError> {
        let mut redactor = Redactor::new();
        if let Some(secret) = url_userinfo(&cfg.repo) {
            redactor.add(secret);
        }
        let log = Arc::new(Logger::open(
            LogConfig::new("hotd", VERSION, cfg.home.join("logs")),
            redactor,
        )?);
        let state = State::load(&cfg.state_file())?;
        let mirror = Mirror::new(
            cfg.home.join("hot-state").join("repo.git"),
            &cfg.repo,
            &cfg.branch,
        );
        Ok(Self {
            cfg,
            mirror,
            engine,
            log,
            state,
            last_fetch_error: None,
        })
    }

    /// PL: Logger dla haka paniki. EN: The logger for the panic hook.
    #[must_use]
    pub fn logger(&self) -> Arc<Logger> {
        Arc::clone(&self.log)
    }

    #[must_use]
    pub fn state(&self) -> &State {
        &self.state
    }

    /// PL: Jeden krok; błędy idą do logu i do `hotd.json`. EN: One step; errors go to the log
    /// and to `hotd.json`.
    pub fn step(&mut self) {
        self.check_crash();
        if let Err(e) = self.mirror.fetch() {
            let key = e.fields()["chain"].to_string();
            if self.last_fetch_error.as_ref() != Some(&key) {
                self.error(&e);
                self.last_fetch_error = Some(key);
            }
            self.save();
            return;
        }
        if self.last_fetch_error.take().is_some() {
            self.log.info(
                "hotd fetches the server repository again",
                json!({ "repo": self.cfg.repo }),
            );
        }
        let Some(head) = self.mirror.head() else {
            self.state.waiting = Some(format!(
                "w repo nie ma gałęzi {} — wypchnij ją albo popraw --branch",
                self.cfg.branch
            ));
            self.save();
            return;
        };
        self.state.head = Some(head.clone());
        if !self.state.settled(&head) {
            if let Err((e, reason)) = self.apply(&head) {
                self.error(&e);
                self.state.fail(&head, &reason);
                self.state.waiting = None;
            }
        }
        self.save();
    }

    fn apply(&mut self, head: &str) -> Result<(), (DiagError, String)> {
        let from = self
            .state
            .applied
            .as_deref()
            .filter(|c| self.mirror.rev(c).is_some());
        let fetch = |e: DiagError| (e, "git nie odczytał commita".to_owned());
        let changed = self.mirror.changed(from, head).map_err(fetch)?;
        let files = self.mirror.files(head).map_err(fetch)?;
        let in_repo = code_mods(&files);
        let core = CoreStatus::read(&self.cfg.core_status());
        let active: BTreeSet<String> = core.mods.keys().cloned().collect();
        let p = plan(&classify(&changed), &in_repo, &active);
        self.state.deferred = p.deferred;

        let mut loads = BTreeMap::new();
        if !p.load.is_empty() {
            let Some(build) = self.build_info(head)? else {
                let text = format!("czekam na CI dla {} (gałąź {BUILDS_BRANCH})", short(head));
                if self.state.waiting.as_deref() != Some(&text) {
                    self.log.info(
                        "hotd waits for the CI build of the commit",
                        json!({ "commit": head }),
                    );
                }
                self.state.waiting = Some(text);
                return Ok(());
            };
            for id in &p.load {
                loads.insert(id.clone(), self.fetch_artifact(head, &build, id)?);
            }
        }
        self.state.waiting = None;
        if p.load.is_empty() && p.unload.is_empty() {
            self.mark_applied(head, false);
            return Ok(());
        }
        let started = Instant::now();
        for id in &p.unload {
            self.engine
                .command(&format!("hot unload {id}"))
                .map_err(|e| (e, format!("silnik nie przyjął hot unload {id}")))?;
        }
        for path in loads.values() {
            self.engine
                .command(&format!("hot load {}", path.display()))
                .map_err(|e| (e, "silnik nie przyjął hot load".to_owned()))?;
        }
        self.confirm(head, &loads, &p.unload)?;
        self.log.info(
            "hotd applied a commit of the server repository",
            json!({
                "commit": head,
                "loaded": loads.keys().collect::<Vec<_>>(),
                "unloaded": p.unload,
                "ms": started.elapsed().as_millis(),
            }),
        );
        self.mark_applied(head, true);
        prune_artifacts(&self.cfg.home.join("hot-state").join("artifacts"), head);
        Ok(())
    }

    fn mark_applied(&mut self, head: &str, engine_changed: bool) {
        if self.state.applied.as_deref() != Some(head) {
            self.state.previous = self.state.applied.take();
        }
        self.state.applied = Some(head.to_owned());
        // PL: Okno D13 tylko wtedy, gdy silnik coś dostał. EN: The D13 window only when the
        // engine got something.
        (self.state.applied_at_ms, self.state.applied_pid) = if engine_changed {
            (now_ms(), self.engine.pid())
        } else {
            (0, None)
        };
    }

    /// PL: `build.json` commita: `None`, gdy CI jeszcze nie skończyło.
    /// EN: The commit's `build.json`: `None` while CI has not finished.
    fn build_info(&self, head: &str) -> Result<Option<Value>, (DiagError, String)> {
        let Some(bytes) = self.mirror.build_file(&format!("{head}/build.json")) else {
            return Ok(None);
        };
        let build: Value = serde_json::from_slice(&bytes).map_err(|e| {
            (
                missing(head, "build.json is not valid JSON").caused_by(&e),
                "uszkodzony build.json z CI".to_owned(),
            )
        })?;
        if build["ok"] == true {
            return Ok(Some(build));
        }
        let failed = build["failed"].as_str().unwrap_or("?");
        let log = build["log"].as_str().unwrap_or("");
        // PL: Pierwsza linia `error…` to diagnoza kompilatora; ostatnia to tylko „could not compile”.
        // EN: The first `error…` line is the compiler's diagnosis; the last only says "could not compile".
        let last = log.lines().find(|l| l.starts_with("error")).unwrap_or("");
        let e = DiagError::new(
            codes::CI_BUILD_FAILED,
            format!(
                "CI could not build hot mod \"{failed}\" in commit {}",
                short(head)
            ),
            "Fix the compile error shown in chain in mods/<mod>/ of the server repository and push; the server keeps the previous mods meanwhile",
        )
        .expected("Every mod in mods/ compiles in CI")
        .cause(last.to_owned())
        .context("commit", head.to_owned())
        .context("mod", failed.to_owned())
        .chain_line(log.to_owned())
        .state("Nothing changed on the server; hotd waits for the next commit");
        Err((
            e,
            format!("CI nie zbudowało moda {failed}: {}", last.trim()),
        ))
    }

    fn fetch_artifact(
        &self,
        head: &str,
        build: &Value,
        id: &str,
    ) -> Result<PathBuf, (DiagError, String)> {
        let target = target();
        let Some(name) = build["targets"][&target][id].as_str() else {
            let e = missing(
                head,
                &format!("build.json has no file for mod \"{id}\" on platform {target}"),
            )
            .context("mod", id.to_owned());
            return Err((e, format!("CI nie zbudowało {id} dla {target}")));
        };
        if !valid_file_name(name) {
            let e = missing(head, &format!("build.json names an unsafe file {name:?}"));
            return Err((e, format!("niedozwolona nazwa pliku {name}")));
        }
        let dir = self.cfg.home.join("hot-state").join("artifacts").join(head);
        let file = dir.join(name);
        let sig = dir.join(format!("{name}.sig"));
        for (path, inner) in [(&file, name.to_owned()), (&sig, format!("{name}.sig"))] {
            let Some(bytes) = self.mirror.build_file(&format!("{head}/{target}/{inner}")) else {
                let e = missing(head, &format!("{BUILDS_BRANCH} has no {target}/{inner}"))
                    .context("mod", id.to_owned());
                return Err((e, format!("brak pliku {inner} w {BUILDS_BRANCH}")));
            };
            write_atomic(path, &bytes).map_err(|e| (e, "nie da się zapisać artefaktu".into()))?;
        }
        sign::verify(&self.cfg.allowed_signers, &file, &sig).map_err(|e| {
            // PL: Pliku z błędnym podpisem nie zostawiamy na dysku. EN: Not kept on disk.
            let _ = fs::remove_file(&file);
            (e, format!("zły podpis artefaktu {name}"))
        })?;
        Ok(file)
    }

    /// PL: Czeka, aż `hot-core` pokaże nowe mody i zniknięcie wyłączonych. `hot status` wysyłany
    /// w pętli przełącza też serwer w pauzie (ADR 0007).
    /// EN: Waits until `hot-core` shows the new mods and the unloaded ones gone. `hot status`
    /// sent in the loop also switches a paused server (ADR 0007).
    fn confirm(
        &mut self,
        head: &str,
        loads: &BTreeMap<String, PathBuf>,
        unloads: &BTreeSet<String>,
    ) -> Result<(), (DiagError, String)> {
        let deadline = Instant::now() + self.cfg.confirm_timeout;
        loop {
            self.engine
                .command("hot status")
                .map_err(|e| (e, "silnik nie przyjął hot status".to_owned()))?;
            let core = CoreStatus::read(&self.cfg.core_status());
            let mut done = unloads.iter().all(|id| !core.mods.contains_key(id));
            for (id, path) in loads {
                let source = path.display().to_string();
                if let Some((code, what)) = core.error_for(&source) {
                    return Err(apply_error(
                        head,
                        &format!("hot-core refused the file of mod \"{id}\": {code} {what}"),
                        &format!("hot-core odrzucił {id}: {code} (opis w docs/bledy/{code}.md)"),
                    ));
                }
                match core.by_source.get(&source) {
                    Some(loaded) if loaded == id => {}
                    Some(other) => {
                        return Err(apply_error(
                            head,
                            &format!("the library in mods/{id}/ declares the mod id \"{other}\""),
                            &format!("id w hot_mod! ({other}) różni się od katalogu mods/{id}"),
                        ));
                    }
                    None => done = false,
                }
            }
            if done {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(apply_error(
                    head,
                    &format!(
                        "hot-core did not confirm the change within {} s",
                        self.cfg.confirm_timeout.as_secs()
                    ),
                    "hot-core nie potwierdził zmian na czas",
                ));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// PL: D13: silnik padł (inny PID albo nie działa) w oknie po zastosowaniu → zły commit.
    /// EN: D13: the engine died (another PID or not running) within the window → a bad commit.
    fn check_crash(&mut self) {
        let Some(pid) = self.state.applied_pid else {
            return;
        };
        let window = u64::try_from(self.cfg.crash_window.as_millis()).unwrap_or(u64::MAX);
        if now_ms().saturating_sub(self.state.applied_at_ms) > window {
            self.state.applied_pid = None;
            return;
        }
        let now = self.engine.pid();
        if now == Some(pid) {
            return;
        }
        let Some(commit) = self.state.applied.clone() else {
            return;
        };
        let e = DiagError::new(
            codes::BAD_COMMIT,
            format!(
                "The engine stopped within {} s after commit {} was applied; the commit is marked bad",
                self.cfg.crash_window.as_secs(),
                short(&commit)
            ),
            "Read the engine log (logs/latest.log) and logs/hot-core.jsonl around that time, fix the mod changed in this commit and push a new commit",
        )
        .expected("The engine keeps the same PID after hot load")
        .context("commit", commit.clone())
        .context("pid_before", pid)
        .context("pid_now", now.map_or(Value::Null, Value::from))
        .state("hotd will not apply this commit again; after a restart hot-core loads the last good generations");
        self.error(&e);
        self.state.mark_bad(&commit);
        self.state.applied = self.state.previous.take();
        self.state.applied_pid = None;
        self.save();
    }

    fn error(&mut self, e: &DiagError) {
        self.log.error(e);
        self.state
            .record(e, &hot_diag::utc_timestamp(SystemTime::now()));
    }

    fn save(&mut self) {
        let v = self
            .state
            .to_json(&hot_diag::utc_timestamp(SystemTime::now()));
        if let Err(e) = write_atomic(&self.cfg.state_file(), format!("{v:#}\n").as_bytes()) {
            self.log.error(&e);
        }
    }
}

/// PL: Potrzebne kawałki `hot-state/status.json` z `hot-core`. EN: The needed parts of
/// `hot-core`'s `hot-state/status.json`.
#[derive(Debug, Default)]
struct CoreStatus {
    /// PL: id → plik źródłowy. EN: id → source file.
    mods: BTreeMap<String, String>,
    /// PL: plik źródłowy → id. EN: source file → id.
    by_source: BTreeMap<String, String>,
    errors: Vec<Value>,
}

impl CoreStatus {
    /// PL: Brak albo zły plik = brak aktywnych modów. EN: Missing or bad file = no active mods.
    fn read(path: &Path) -> Self {
        let v: Value = fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let mut s = Self::default();
        for m in v["mods"].as_array().into_iter().flatten() {
            let (Some(id), Some(source)) = (m["id"].as_str(), m["source"].as_str()) else {
                continue;
            };
            s.mods.insert(id.to_owned(), source.to_owned());
            s.by_source.insert(source.to_owned(), id.to_owned());
        }
        s.errors = v["recent_errors"].as_array().cloned().unwrap_or_default();
        s
    }

    fn error_for(&self, source: &str) -> Option<(String, String)> {
        self.errors
            .iter()
            .rev()
            .find(|e| e["context"]["source"] == source)
            .map(|e| {
                (
                    e["code"].as_str().unwrap_or("").to_owned(),
                    e["what"].as_str().unwrap_or("").to_owned(),
                )
            })
    }
}

fn apply_error(head: &str, what: &str, reason: &str) -> (DiagError, String) {
    let e = DiagError::new(
        codes::APPLY_FAILED,
        format!("Commit {} was not applied: {what}", short(head)),
        "Read the hot-core entry with the code named in cause (logs/hot-core.jsonl, docs/bledy/), fix the mod in the server repository and push a new commit",
    )
    .expected("hot-core lists every new mod with its file from hot-state/artifacts and no unloaded mod")
    .cause(what.to_owned())
    .context("commit", head.to_owned())
    .state("hotd stops at this commit; mods already switched stay, the rest keep their previous generation");
    (e, reason.to_owned())
}

fn missing(head: &str, what: &str) -> DiagError {
    DiagError::new(
        codes::ARTIFACT_MISSING,
        format!("The CI result for commit {} is incomplete: {what}", short(head)),
        "Open the hot.yml run for this commit in the repository's Actions tab; check that it builds for this server's platform (context.target) and pushes to the hot-builds branch, then push again",
    )
    .context("commit", head.to_owned())
    .context("target", target())
    .state("Nothing changed on the server; hotd waits for the next commit")
}

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

fn valid_file_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("");
    !name.contains('/') && !name.contains("..") && valid_id(stem.trim_start_matches("lib"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// PL: `user:token` z adresu `https://user:token@host/…`. EN: The userinfo of an URL.
fn url_userinfo(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host_part = rest.split('/').next()?;
    host_part
        .rsplit_once('@')
        .map(|(info, _)| info.to_owned())
        .filter(|s| !s.is_empty())
}

/// PL: Zostawia `KEEP_ARTIFACTS` najnowszych katalogów i zawsze bieżący.
/// EN: Keeps the `KEEP_ARTIFACTS` newest folders and always the current one.
fn prune_artifacts(dir: &Path, current: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name() != current)
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dirs.sort();
    let extra = dirs.len().saturating_sub(KEEP_ARTIFACTS - 1);
    for (_, path) in dirs.into_iter().take(extra) {
        // PL: Stare kopie; `hot-core` ładuje z własnych kopii w `hot-state/gen/`.
        // EN: Old copies; `hot-core` loads from its own copies in `hot-state/gen/`.
        let _ = fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userinfo_is_found_for_masking() {
        assert_eq!(
            url_userinfo("https://x:tok@github.com/a/b").as_deref(),
            Some("x:tok")
        );
        assert_eq!(url_userinfo("git@github.com:a/b.git"), None);
        assert_eq!(url_userinfo("https://github.com/a@b"), None);
    }

    #[test]
    fn artifact_names_cannot_escape_the_folder() {
        assert!(valid_file_name("libsklep.so"));
        assert!(valid_file_name("sklep.dylib"));
        assert!(!valid_file_name("../x.so"));
        assert!(!valid_file_name("a/b.so"));
    }
}
