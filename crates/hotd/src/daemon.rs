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
use crate::data::{self, Staged};
use crate::disk;
use crate::engine::Engine;
use crate::git::{Mirror, BUILDS_BRANCH};
use crate::pack::{self, Look};
use crate::slots::{self, Slots};
use crate::state::{write_atomic, State};
use crate::world;
use crate::{codes, sign, VERSION};

/// PL: Ustawienia `hotd`. EN: `hotd` settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// PL: `HOT_HOME` silnika: tu są `hot-state/` i `logs/`. EN: The engine's `HOT_HOME`.
    pub home: PathBuf,
    pub repo: String,
    pub branch: String,
    pub allowed_signers: PathBuf,
    /// PL: Katalog świata serwera; tu trafiają paczki danych modów (M3).
    /// EN: The server's world folder; mod data packs go here (M3).
    pub world: PathBuf,
    /// PL: Ile czekać na potwierdzenie z `hot-core`. EN: How long to wait for `hot-core`.
    pub confirm_timeout: Duration,
    /// PL: Awaria silnika w tym czasie po zastosowaniu = zły commit (D13: 60 s).
    /// EN: An engine crash within this time after applying = a bad commit (D13: 60 s).
    pub crash_window: Duration,
    /// PL: Poniżej tylu wolnych bajtów `hotd` czeka (M7.2). EN: Below this many free bytes `hotd`
    /// waits (M7.2).
    pub min_free_bytes: u64,
    /// PL: `--dir` serwera Horizona (`crash-reports/`, `backups/`) i program `horizon-server`, do
    /// polecenia przywrócenia (M7.3). EN: The Horizon server's `--dir` (`crash-reports/`,
    /// `backups/`) and the `horizon-server` program, for the restore command (M7.3).
    pub engine_dir: PathBuf,
    pub engine_program: PathBuf,
}

impl Config {
    #[must_use]
    pub fn new(home: PathBuf, repo: &str, branch: &str) -> Self {
        let allowed_signers = home.join("hot-keys").join("allowed_signers");
        let world = home.join("world");
        Self {
            world,
            engine_dir: home.clone(),
            engine_program: PathBuf::from("horizon-server"),
            home,
            repo: repo.to_owned(),
            branch: branch.to_owned(),
            allowed_signers,
            confirm_timeout: Duration::from_secs(60),
            crash_window: Duration::from_secs(60),
            min_free_bytes: disk::MIN_FREE_BYTES,
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
    /// PL: Za mało miejsca na dysku od poprzedniego kroku; błąd idzie do logu raz.
    /// EN: Low disk space since an earlier step; the error is logged once.
    disk_low: bool,
    /// PL: Bieżący `apply` wysłał już coś do silnika; po tym błąd nie jest ponawiany.
    /// EN: The current `apply` already sent something to the engine; after that an error is not
    /// retried.
    engine_touched: bool,
    /// PL: Kiedy `hotd` ostatnio widział działający silnik; starsze raporty awarii nie dotyczą
    /// obecnego postoju. EN: When `hotd` last saw the engine running; older crash reports are not
    /// about the current outage.
    seen_running: Option<SystemTime>,
    /// PL: Raport, z którego `HOT-E0212` już trafił do logu. EN: The report already logged.
    world_report: Option<String>,
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
            disk_low: false,
            engine_touched: false,
            seen_running: None,
            world_report: None,
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
        self.state.brake = crate::brake::note();
        self.check_crash();
        self.check_world();
        if self.check_disk() {
            self.save();
            return;
        }
        if let Err(e) = self.mirror.fetch() {
            let f = e.fields();
            self.state.fetch_error = Some(f["what"].as_str().unwrap_or_default().to_owned());
            let key = f["chain"].to_string();
            if self.last_fetch_error.as_ref() != Some(&key) {
                self.error(&e);
                self.last_fetch_error = Some(key);
            }
            self.save();
            return;
        }
        self.state.fetch_error = None;
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
            self.engine_touched = false;
            if let Err((e, reason)) = self.apply(&head) {
                if !self.engine_touched && out_of_space(&e) {
                    self.wait_for_space(&head, &e);
                } else {
                    self.error(&e);
                    self.state.fail(&head, &reason);
                    self.state.waiting = None;
                }
            }
        } else if !pack::dir(&self.cfg.home).join("pack.zip").exists() {
            // PL: Pierwszy start po aktualizacji `hotd` albo skasowana paczka: odbudowa bez czekania
            // na nowy commit. EN: First start after a `hotd` update or a deleted pack: rebuild
            // without waiting for a new commit.
            if let Err((e, _)) = self.refresh_pack(&head) {
                self.error(&e);
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
        let data_diff = self.data_diff(head, &files)?;
        let pack = self.pack_update(head, &files)?;

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
        if p.load.is_empty() && p.unload.is_empty() && data_diff.is_empty() {
            // PL: Sam wygląd nie potrzebuje silnika: `hot-core` przeczyta sloty przy starcie.
            // EN: A look alone needs no engine: `hot-core` reads the slots when it starts.
            if let Some(u) = &pack {
                let running = self.engine.pid().is_some();
                self.install_pack(head, u, running)?;
            }
            self.mark_applied(head, false);
            return Ok(());
        }
        // PL: Silnik startuje albo wstaje po awarii: commit czeka, a nie przepada jako nieudany.
        // EN: The engine is starting or restarting after a crash: the commit waits instead of
        // being recorded as failed.
        if self.engine.pid().is_none() {
            let text = format!(
                "czekam na silnik dla {} (horizon-server nie jest w fazie running)",
                short(head)
            );
            if self.state.waiting.as_deref() != Some(&text) {
                self.log.info(
                    "hotd waits for the engine to run before applying the commit",
                    json!({ "commit": head }),
                );
            }
            self.state.waiting = Some(text);
            return Ok(());
        }
        // PL: Paczki sprawdzone, zanim silnik dostanie cokolwiek: zły plik nic nie zmienia.
        // EN: Packs are checked before the engine gets anything: a bad file changes nothing.
        let staged = if data_diff.is_empty() {
            None
        } else {
            let mirror = &self.mirror;
            let staged = Staged::prepare(&self.cfg.world, data_diff, |tree, into| {
                mirror
                    .extract(tree, into)
                    .map_err(|e| format!("{}: {}", e.command, e.output))
            })
            .map_err(|e| {
                (
                    e,
                    "paczka danych z błędem — szczegóły w hot status".to_owned(),
                )
            })?;
            Some(staged)
        };
        let started = Instant::now();
        // PL: Sloty przed `hot load`, żeby nowy mod od razu znał swoje bloki.
        // EN: Slots before `hot load`, so a new mod knows its blocks at once.
        if let Some(u) = &pack {
            self.install_pack(head, u, true)?;
        }
        self.engine_touched = true;
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
        if !loads.is_empty() || !p.unload.is_empty() {
            self.confirm(head, &loads, &p.unload)?;
        }
        let (mut written, mut removed, mut reload_ms) = (Vec::new(), Vec::new(), None);
        if let Some(staged) = staged {
            let reload = Instant::now();
            self.reload_data(head, &staged)?;
            reload_ms = Some(reload.elapsed().as_millis());
            written = staged.diff().write.keys().cloned().collect();
            removed = staged.diff().remove.iter().cloned().collect();
            staged.finish();
        }
        self.log.info(
            "hotd applied a commit of the server repository",
            json!({
                "commit": head,
                "loaded": loads.keys().collect::<Vec<_>>(),
                "unloaded": p.unload,
                "data_written": written,
                "data_removed": removed,
                "reload_ms": reload_ms,
                "pack_sha1": pack.as_ref().map(|u| u.sha1.clone()),
                "ms": started.elapsed().as_millis(),
            }),
        );
        self.mark_applied(head, true);
        prune_artifacts(
            &self.cfg.home.join("hot-state").join("artifacts"),
            head,
            KEEP_ARTIFACTS,
        );
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

    /// PL: Przydział slotów i paczka zasobów dla commita; `None`, gdy oba są już takie same.
    /// Wszystko sprawdzone, zanim cokolwiek zostanie zapisane.
    /// EN: The slot assignment and resource pack for a commit; `None` when both are already the
    /// same. Everything is checked before anything is written.
    fn pack_update(
        &self,
        head: &str,
        files: &[String],
    ) -> Result<Option<PackUpdate>, (DiagError, String)> {
        let file = self.slots_file();
        let current =
            Slots::read(&file).map_err(|e| (e, "hot-state/slots.json nieczytelny".to_owned()))?;
        let declared = slots::declared_blocks(files);
        let mut next = current.clone();
        next.assign(&declared)
            .map_err(|e| (e, "pula slotów pełna — szczegóły w hot status".to_owned()))?;
        let mut langs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut looks = BTreeMap::new();
        for block in &declared {
            let (id, name) = block.split_once(':').unwrap_or((block, ""));
            let path = format!("mods/{id}/assets/blocks/{name}.png");
            let texture = self.mirror.blob(head, &path).unwrap_or_default();
            pack::check_texture(&path, &texture)
                .map_err(|e| (e, format!("{path} nie jest obrazem PNG")))?;
            if !langs.contains_key(id) {
                let lang_path = format!("mods/{id}/assets/lang.json");
                let lang = match self.mirror.blob(head, &lang_path) {
                    Some(bytes) => pack::parse_lang(id, &bytes)
                        .map_err(|e| (e, format!("{lang_path} z błędem")))?,
                    None => BTreeMap::new(),
                };
                langs.insert(id.to_owned(), lang);
            }
            let shown = langs.get(id).and_then(|l| l.get(name)).cloned();
            looks.insert(
                block.clone(),
                Look {
                    texture,
                    name: shown,
                },
            );
        }
        let zip = pack::zip(&pack::files(&next, &looks));
        let sha1 = pack::sha1_hex(&zip);
        let installed =
            fs::read_to_string(pack::dir(&self.cfg.home).join("pack.sha1")).unwrap_or_default();
        if next == current && installed.trim() == sha1 {
            return Ok(None);
        }
        Ok(Some(PackUpdate {
            slots_changed: next != current,
            slots: next,
            zip,
            sha1,
        }))
    }

    /// PL: Zapisuje sloty, potem paczkę; `hot slots` mówi `hot-core`, że przydział się zmienił.
    /// EN: Writes the slots, then the pack; `hot slots` tells `hot-core` the assignment changed.
    fn install_pack(
        &mut self,
        head: &str,
        u: &PackUpdate,
        engine: bool,
    ) -> Result<(), (DiagError, String)> {
        // PL: Paczka przed slotami: gdy zapis paczki się nie uda, `slots.json` zostaje stary, więc
        // ponowiona próba znów widzi zmianę slotów i wyśle `hot slots`.
        // EN: The pack before the slots: when the pack write fails, `slots.json` stays old, so the
        // retry sees the slot change again and sends `hot slots`.
        pack::install(&self.cfg.home, &u.zip)
            .map_err(|e| (e, "nie zapisano paczki zasobów".to_owned()))?;
        if u.slots_changed {
            u.slots
                .write(&self.slots_file())
                .map_err(|e| (e, "nie zapisano hot-state/slots.json".to_owned()))?;
        }
        if u.slots_changed && engine {
            self.engine_touched = true;
            let reply = self
                .engine
                .command("hot slots")
                .map_err(|e| (e, "silnik nie przyjął hot slots".to_owned()))?;
            // PL: `hot-core` sprzed M5 odpowiada samą instrukcją; mody nie znałyby swoich bloków.
            // EN: A `hot-core` older than M5 answers with its usage only; mods would not know their
            // blocks.
            if !reply.contains("Bloki modów") {
                return Err(apply_error(
                    head,
                    &format!("hot-core did not confirm hot slots, it answered: {reply}"),
                    "hot-core nie zna hot slots — zaktualizuj hot-core",
                ));
            }
        }
        self.log.info(
            "hotd installed the resource pack; players get it at their next join",
            json!({ "sha1": u.sha1, "bytes": u.zip.len(), "blocks": u.slots.blocks.len() }),
        );
        Ok(())
    }

    fn refresh_pack(&mut self, head: &str) -> Result<(), (DiagError, String)> {
        let files = self
            .mirror
            .files(head)
            .map_err(|e| (e, "git nie odczytał commita".to_owned()))?;
        if let Some(u) = self.pack_update(head, &files)? {
            let running = self.engine.pid().is_some();
            self.install_pack(head, &u, running)?;
        }
        Ok(())
    }

    fn slots_file(&self) -> PathBuf {
        self.cfg.home.join("hot-state").join("slots.json")
    }

    /// PL: Które paczki danych zapisać i usunąć, żeby świat pasował do commita.
    /// EN: Which data packs to write and remove so the world matches the commit.
    fn data_diff(&self, head: &str, files: &[String]) -> Result<data::Diff, (DiagError, String)> {
        let mut wanted = BTreeMap::new();
        for id in data::data_mods(files) {
            let Some(tree) = self.mirror.tree(head, &format!("mods/{id}/data")) else {
                let e = DiagError::new(
                    codes::DATA_PACK_FAILED,
                    format!("git cannot find mods/{id}/data in commit {}", short(head)),
                    "Run git fsck in hot-state/repo.git; if it reports damage, move the folder aside and hotd fetches the repository again",
                )
                .context("commit", head.to_owned())
                .state("Nothing changed on the server");
                return Err((e, format!("git nie widzi mods/{id}/data")));
            };
            wanted.insert(id, tree);
        }
        Ok(data::diff(&wanted, &data::installed(&self.cfg.world)))
    }

    /// PL: Wstawia paczki, wysyła jedno `reload` i sprawdza `datapack list`. Gdy silnik nie
    /// pokazuje zmian, przywraca stare paczki i przeładowuje je jeszcze raz.
    /// EN: Puts the packs in place, sends one `reload` and checks `datapack list`. When the engine
    /// does not show the change, restores the old packs and reloads them again.
    fn reload_data(&mut self, head: &str, staged: &Staged) -> Result<(), (DiagError, String)> {
        let reason = "silnik nie włączył paczek danych".to_owned();
        let log_file = self.engine.log_file();
        let offset = data::log_offset(&log_file);
        if let Err(e) = staged.swap_in() {
            staged.restore();
            return Err((e, "nie da się zapisać paczek danych".to_owned()));
        }
        let answer = self
            .engine
            .command("reload")
            .and_then(|_| self.engine.command("datapack list"));
        let answer = match answer {
            Ok(a) => a,
            Err(e) => {
                staged.restore();
                return Err((e, reason));
            }
        };
        let enabled = data::enabled_in(&answer);
        let engine_log = data::read_reload_log(&data::log_since(&log_file, offset));
        let diff = staged.diff();
        let missing: Vec<&String> = diff
            .write
            .keys()
            .filter(|id| !enabled.contains(*id))
            .collect();
        let still: Vec<&String> = diff
            .remove
            .iter()
            .filter(|id| enabled.contains(*id))
            .collect();
        if missing.is_empty() && still.is_empty() && engine_log.failures.is_empty() {
            if !engine_log.warnings.is_empty() {
                self.log.warn(
                    "the engine skipped files of the data packs; fix them in the server repository",
                    json!({
                        "commit": head,
                        "engine_log": engine_log.warnings,
                        "log_file": log_file.display().to_string(),
                    }),
                );
            }
            self.state.data_warnings = engine_log.warnings;
            return Ok(());
        }
        staged.restore();
        let _ = self.engine.command("reload");
        let mut e = DiagError::new(
            codes::DATA_PACK_FAILED,
            format!(
                "After reload the engine does not use the data packs of commit {}",
                short(head)
            ),
            "If chain has a line \"... are not applied: ...\", fix the file it names in mods/<id>/data/ and push a new commit. Otherwise check <world>/datapacks.json: a pack listed under \"disabled\" stays off; remove it from there (or run /datapack enable \"file/hot-<id>\") and push a new commit",
        )
        .expected("datapack list shows every file/hot-<id> pack of the commit as enabled, none of the removed ones, and the engine log has no \"are not applied\" line")
        .context("commit", head.to_owned())
        .context("not_enabled", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        .context("not_removed", still.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        .context("log_file", log_file.display().to_string());
        for line in engine_log.failures {
            e = e.chain_line(line);
        }
        let e = e.chain_line(answer).state(
            "The previous data packs are back in place and reloaded; mods switched in this commit stay",
        );
        Err((e, reason))
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

    /// PL: M7.2 (D20): za mało miejsca → sprzątanie własnych plików; gdy nadal za mało, błąd
    /// (raz) i `true`: krok nie pobiera i nie stosuje commitów, bo `git fetch` i artefakty piszą
    /// na dysk. Bez pomiaru (`df` nie działa) krok idzie normalnie.
    /// EN: M7.2 (D20): low space → clean up our own files; still low → an error (once) and `true`:
    /// the step neither fetches nor applies, as `git fetch` and artifacts write to disk. Without a
    /// measurement (`df` fails) the step runs as usual.
    /// PL: Commit nie wszedł, bo zabrakło miejsca, zanim silnik cokolwiek dostał. To nie wina
    /// commita: nie trafia na listę nieudanych, a następny krok spróbuje znowu. Błąd idzie do
    /// logu raz na commit.
    /// EN: The commit did not apply because space ran out before the engine got anything. Not the
    /// commit's fault: it is not recorded as failed and the next step tries again. The error is
    /// logged once per commit.
    fn wait_for_space(&mut self, head: &str, e: &DiagError) {
        let text = format!(
            "brak miejsca na dysku przy stosowaniu {} — ponowię, gdy się zwolni (HOT-E0211)",
            short(head)
        );
        if self.state.waiting.as_deref() == Some(text.as_str()) {
            return;
        }
        let f = e.fields();
        let mut err = DiagError::new(
            codes::DISK_LOW,
            format!(
                "The disk holding {} ran out of space while hotd applied commit {}",
                self.cfg.home.display(),
                short(head)
            ),
            "Free disk space (df -h; old world backups, journalctl --vacuum-size=200M, sudo apt clean); hotd applies the commit again by itself at the next step",
        )
        .expected("free space for the artifacts, data packs and the resource pack of the commit")
        .context("commit", head)
        .context("failed_code", f["code"].clone())
        .context("failed_what", f["what"].clone())
        .state("the commit is not marked as failed; the engine got nothing from it; hotd retries every 10 s");
        for line in error_texts(&f) {
            err = err.chain_line(line);
        }
        self.error(&err);
        self.state.waiting = Some(text);
    }

    /// PL: Silnik nie działa, a najnowszy raport awarii mówi, że świat się nie wczytał: przyczyna
    /// i polecenie przywrócenia w `hot status`, `HOT-E0212` raz na raport. Bez przywracania (D20).
    /// EN: The engine is down and the newest crash report says the world did not load: the reason
    /// and the restore command in `hot status`, `HOT-E0212` once per report. No restore (D20).
    fn check_world(&mut self) {
        if self.engine.pid().is_some() {
            self.seen_running = Some(SystemTime::now());
            if self.state.world_damage.take().is_some() {
                self.log.info(
                    "the engine runs again; the world message is cleared",
                    json!({ "report": self.world_report }),
                );
            }
            return;
        }
        let dir = self.cfg.engine_dir.clone();
        let Some(failure) = world::load_failure(&dir, self.seen_running) else {
            return;
        };
        let backup = world::newest_backup(&dir);
        let restore = backup.as_ref().map(|b| {
            format!(
                "{} --dir {} restore {b} --yes",
                self.cfg.engine_program.display(),
                dir.display()
            )
        });
        self.state.world_damage = Some(match &restore {
            Some(cmd) => format!(
                "silnik nie wczytuje świata: {} (crash-reports/{}). hotd niczego nie przywraca sam. Sprawdź kopię i przywróć ją jako użytkownik serwera: {cmd} (obecny świat trafi do backups/replaced-…; HOT-E0212)",
                failure.reason, failure.report
            ),
            None => format!(
                "silnik nie wczytuje świata: {} (crash-reports/{}), a w backups/ nie ma kopii (HOT-E0212)",
                failure.reason, failure.report
            ),
        });
        if self.world_report.as_deref() == Some(failure.report.as_str()) {
            return;
        }
        let e = DiagError::new(
            codes::WORLD_DAMAGED,
            format!("The engine cannot load the world: {}", failure.reason),
            match &restore {
                Some(cmd) => format!(
                    "Check the backup, then restore it as the server user (it replaces the world; the current one is kept in backups/replaced-*): {cmd}"
                ),
                None => "No backup in backups/: copy the world folder aside, then repair level.dat with an NBT editor or start a new world".to_owned(),
            },
        )
        .expected("the engine loads the world named in server.toml")
        .context("report", failure.report.clone())
        .context("backup", backup.map_or(Value::Null, Value::from))
        .context("dir", dir.display().to_string())
        .chain_line(failure.line)
        .state("hotd restores nothing by itself (D20); the server stays down and commits wait");
        self.error(&e);
        self.world_report = Some(failure.report);
    }

    fn check_disk(&mut self) -> bool {
        let min = self.cfg.min_free_bytes;
        let Some(before) = disk::free_bytes(&self.cfg.home) else {
            return false;
        };
        if before >= min {
            if std::mem::take(&mut self.disk_low) {
                self.state.waiting = None;
                self.log.info(
                    "hotd has enough disk space again and resumes",
                    json!({ "free_bytes": before, "min_free_bytes": min }),
                );
            }
            return false;
        }
        let state = self.cfg.home.join("hot-state");
        let applied = self.state.applied.clone().unwrap_or_default();
        prune_artifacts(&state.join("artifacts"), &applied, 1);
        let cores = disk::prune_core_copies(&state.join("core"));
        let after = disk::free_bytes(&self.cfg.home).unwrap_or(before);
        if after >= min {
            self.log.info(
                "hotd freed disk space by removing its old artifacts and hot-core copies",
                json!({ "free_before": before, "free_after": after, "hot_core_copies_bytes": cores }),
            );
            return false;
        }
        let mib = |b: u64| b / (1024 * 1024);
        self.state.waiting = Some(format!(
            "mało miejsca na dysku: wolne {} MB, potrzeba {} MB — hotd nie pobiera i nie stosuje commitów (HOT-E0211)",
            mib(after),
            mib(min)
        ));
        if !self.disk_low {
            self.disk_low = true;
            let usage: serde_json::Map<String, Value> =
                disk::usage(&self.cfg.home, &["world", "hot-state", "logs", "backups"])
                    .into_iter()
                    .map(|(d, kib)| (d, Value::from(kib)))
                    .collect();
            let e = DiagError::new(
                codes::DISK_LOW,
                format!(
                    "Only {} MB free on the disk holding {}, hotd needs {} MB; its own cleanup freed {} MB",
                    mib(after),
                    self.cfg.home.display(),
                    mib(min),
                    mib(after.saturating_sub(before))
                ),
                "Free disk space: check df -h and the sizes in context.usage_kib; old world backups (horizon-server backups), journalctl --vacuum-size=200M, sudo apt clean. hotd resumes by itself within 10 s once enough is free",
            )
            .expected("free space on the server disk stays above min_free_bytes")
            .context("free_bytes", after)
            .context("min_free_bytes", min)
            .context("usage_kib", Value::Object(usage))
            .state("hotd neither fetches nor applies commits until there is space; the server and hot mods keep running");
            self.error(&e);
        }
        true
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
        .state("hotd will not apply this commit again; after a restart hot-core loads the last good generations and hotd puts back the data packs of the last good commit");
        self.error(&e);
        self.state.mark_bad(&commit);
        self.state.applied = self.state.previous.take();
        self.state.applied_pid = None;
        self.restore_data();
        self.save();
    }

    /// PL: Po złym commicie paczki danych wracają do zastosowanego commita (bez commita: żadnych
    /// naszych paczek), żeby silnik nie wstawał w kółko na paczce, która go położyła.
    /// EN: After a bad commit the data packs go back to the applied commit (none of ours without
    /// one), so the engine does not keep restarting on the pack that brought it down.
    fn restore_data(&mut self) {
        let target = self.state.applied.clone();
        let files = match &target {
            Some(c) => self.mirror.files(c),
            None => Ok(Vec::new()),
        };
        let diff = files
            .map_err(|e| (e, String::new()))
            .and_then(|files| self.data_diff(target.as_deref().unwrap_or_default(), &files));
        let result = diff.and_then(|diff| {
            if diff.is_empty() {
                return Ok(None);
            }
            let mirror = &self.mirror;
            let staged = Staged::prepare(&self.cfg.world, diff, |tree, into| {
                mirror
                    .extract(tree, into)
                    .map_err(|e| format!("{}: {}", e.command, e.output))
            })
            .and_then(|s| match s.swap_in() {
                Ok(()) => Ok(s),
                Err(e) => {
                    s.restore();
                    Err(e)
                }
            })
            .map_err(|e| (e, String::new()))?;
            Ok(Some(staged))
        });
        match result {
            Ok(None) => {}
            Ok(Some(staged)) => {
                let restored: Vec<String> = staged.diff().write.keys().cloned().collect();
                let removed: Vec<String> = staged.diff().remove.iter().cloned().collect();
                staged.finish();
                // PL: Silnik, który już wstał, mógł wczytać złe paczki. EN: An engine already up
                // may have loaded the bad packs.
                if self.engine.pid().is_some() {
                    let _ = self.engine.command("reload");
                }
                self.log.info(
                    "hotd put back the data packs of the last good commit",
                    json!({ "commit": target, "data_written": restored, "data_removed": removed }),
                );
            }
            Err((e, _)) => self.error(&e),
        }
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

/// PL: Nowy przydział slotów i paczka do zapisania. EN: A new slot assignment and pack to write.
struct PackUpdate {
    slots: Slots,
    slots_changed: bool,
    zip: Vec<u8>,
    sha1: String,
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

/// PL: Zostawia `current` i najnowsze z reszty, razem `keep` katalogów.
/// EN: Keeps `current` and the newest of the rest, `keep` folders in all.
fn prune_artifacts(dir: &Path, current: &str, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name() != current)
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dirs.sort();
    let extra = dirs.len().saturating_sub(keep.saturating_sub(1));
    for (_, path) in dirs.into_iter().take(extra) {
        // PL: Stare kopie; `hot-core` ładuje z własnych kopii w `hot-state/gen/`.
        // EN: Old copies; `hot-core` loads from its own copies in `hot-state/gen/`.
        let _ = fs::remove_dir_all(path);
    }
}

/// PL: Teksty przyczyn błędu (`cause` i `chain`), bez `fix` i `what`, w których słowo „dysk”
/// może paść bez związku z przyczyną. EN: The error's cause texts (`cause` and `chain`), not `fix`
/// or `what`, which may mention the disk unrelated to the cause.
fn error_texts(f: &serde_json::Map<String, Value>) -> Vec<String> {
    let chain = f.get("chain").and_then(Value::as_array);
    f.get("cause")
        .into_iter()
        .chain(chain.into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// PL: Przyczyną jest pełny dysk: `(os error 28)` (ENOSPC na Linuksie i macOS, w każdym języku
/// systemu) albo komunikat gita, który `hotd` uruchamia z `LC_ALL=C`.
/// EN: The cause is a full disk: `(os error 28)` (ENOSPC on Linux and macOS, in any system
/// language) or a git message; `hotd` runs git with `LC_ALL=C`.
fn out_of_space(e: &DiagError) -> bool {
    error_texts(&e.fields())
        .iter()
        .any(|t| t.contains("os error 28") || t.contains("No space left on device"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_cause_says_whether_the_disk_is_full() {
        let full = DiagError::new(codes::PACK_FAILED, "x", "y")
            .caused_by(&std::io::Error::from_raw_os_error(28));
        assert!(out_of_space(&full));
        let git = DiagError::new(codes::DATA_PACK_FAILED, "x", "y")
            .chain_line("exit 128: fatal: write error: No space left on device");
        assert!(out_of_space(&git));
        let other = DiagError::new(
            codes::STATE_FILE,
            "x",
            "check that the disk is not full (No space left on device)",
        )
        .caused_by(&std::io::Error::from_raw_os_error(13));
        assert!(!out_of_space(&other));
    }

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
