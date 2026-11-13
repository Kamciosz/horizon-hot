//! PL: Prawdziwy świat samoaktualizacji ([`crate::upgrade::Env`]). Git i cargo w klonie `--src`,
//! z tymi samymi pakietami co krok 5 instalatora. Jako root polecenia idą przez
//! `runuser -u horizon` z tym samym `HOME` i `PATH` co `as_horizon` w instalatorze, więc w klonie
//! i w `target/` nie powstają pliki roota. Rdzeń przez `horizon-server cmd "hot core load …"`,
//! gracze z gniazda `hot-proxy`, restart przez `systemctl`.
//! EN: The self-update's real world ([`crate::upgrade::Env`]). git and cargo in the `--src` clone,
//! with the same packages as installer step 5. As root, commands go through `runuser -u horizon`
//! with the same `HOME` and `PATH` as `as_horizon` in the installer, so no root-owned files appear
//! in the clone or in `target/`. The core through `horizon-server cmd "hot core load …"`, players
//! from the `hot-proxy` socket, restarts through `systemctl`.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hot_diag::{DiagError, Logger};
use serde_json::{json, Value};

use crate::codes;
use crate::swap::Layout;
use crate::upgrade::{Env, Mode};

/// PL: Pakiety z kroku 5 instalatora. EN: The packages of installer step 5.
pub const PACKAGES: [&str; 4] = ["hotd", "hot-core", "hot-diag", "hot-proxy"];
/// PL: Programy w `bin/`. EN: The programs in `bin/`.
pub const PROGRAMS: [&str; 3] = ["hotd", "hot-diag", "hot-proxy"];
/// PL: Folder odłożonych programów w `bin/`. EN: The staged programs' folder in `bin/`.
const NEXT: &str = ".next";
/// PL: Usługa silnika (`hotd engine`). EN: The engine's service (`hotd engine`).
const ENGINE_UNIT: &str = "horizon-hot-server";
/// PL: Czas na wstanie usług po restarcie. EN: The time for the services to come up.
const COME_UP: Duration = Duration::from_secs(180);

/// PL: Ustawienia i narzędzia prawdziwego świata. EN: The real world's settings and tools.
#[derive(Clone)]
pub struct Real {
    /// PL: Folder serwera (`HOT_HOME`). EN: The server folder (`HOT_HOME`).
    pub home: PathBuf,
    /// PL: `/srv/horizon`: `hot-version`, `HOME` użytkownika `horizon`. EN: `/srv/horizon`:
    /// `hot-version`, the `horizon` user's `HOME`.
    pub root: PathBuf,
    pub src: PathBuf,
    pub bin: PathBuf,
    pub source: String,
    pub git_ref: String,
    pub server: PathBuf,
    /// PL: Użytkownik dla git, cargo i `horizon-server`, gdy działamy jako root. EN: The user for
    /// git, cargo and `horizon-server` when we run as root.
    pub user: Option<String>,
    /// PL: Usługi do restartu w kolejności. EN: The services to restart, in order.
    pub units: Vec<String>,
    pub log: Arc<Logger>,
}

fn core_file() -> String {
    format!(
        "{}hot_core{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn check_error(what: impl Into<String>, cause: String) -> DiagError {
    DiagError::new(
        codes::UPGRADE_CHECK_FAILED,
        what,
        "Read chain; hotd upgrade tries again after a day, or run it now: systemctl start horizon-hot-upgrade",
    )
    .chain_line(cause)
    .state("The server stays on its horizon-hot version")
}

fn rename(from: &Path, to: &Path) -> Result<(), DiagError> {
    fs::rename(from, to).map_err(|e| {
        check_error(
            format!("{} cannot be moved to {}", from.display(), to.display()),
            e.to_string(),
        )
    })
}

impl Real {
    /// PL: Polecenie jako `horizon`, gdy działamy jako root. EN: A command as `horizon` when we run
    /// as root.
    fn cmd(&self, program: &Path) -> Command {
        match &self.user {
            Some(user) => {
                let mut c = Command::new("runuser");
                c.args(["-u", user, "--", "env"])
                    .arg(format!("HOME={}", self.root.display()))
                    .arg(format!(
                        "PATH={}/.cargo/bin:{}:/usr/local/bin:/usr/bin:/bin",
                        self.root.display(),
                        self.bin.display()
                    ))
                    .arg(program);
                c
            }
            None => Command::new(program),
        }
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        let mut c = self.cmd(Path::new(program));
        c.args(args).current_dir(&self.src);
        let out = crate::brake::output(&mut c)
            .map_err(|e| format!("{program} cannot be started: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!(
                "{program} {}: {} {}",
                args.first().copied().unwrap_or_default(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    fn server_cmd(&self, args: &[&str]) -> Result<String, String> {
        let active = Layout {
            home: self.home.clone(),
        }
        .active_dir();
        let out = self
            .cmd(&self.server)
            .arg("--dir")
            .arg(active)
            .args(args)
            .output()
            .map_err(|e| format!("{} cannot be started: {e}", self.server.display()))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            Ok(text)
        } else {
            Err(format!("{}: {}", out.status, text.trim()))
        }
    }

    /// PL: Odczyt `status --json` silnika jako `horizon` dla wątku hamulca. EN: The engine's
    /// `status --json` read as `horizon`, for the brake thread.
    pub fn status_probe(&self) -> impl FnMut() -> Option<String> + Send + 'static {
        let me = self.clone();
        move || me.server_cmd(&["status", "--json"]).ok()
    }

    fn core_dir(&self) -> PathBuf {
        self.home.join("hot-core")
    }

    /// PL: Pary (odłożony, obecny) do podmiany w trybie `full`. EN: The (staged, live) pairs
    /// swapped in `full` mode.
    fn pairs(&self) -> Vec<(PathBuf, PathBuf)> {
        let mut pairs: Vec<_> = PROGRAMS
            .iter()
            .map(|p| (self.bin.join(NEXT).join(p), self.bin.join(p)))
            .collect();
        let live = self.core_dir().join(core_file());
        pairs.push((with_suffix(&live, ".next"), live));
        pairs
    }

    /// PL: Nowy program musi się uruchomić i znać swoje polecenia. Działa jako `horizon`, nigdy
    /// jako root. EN: A new program must start and know its commands. Runs as `horizon`, never as
    /// root.
    fn smoke(&self, program: &Path) -> Result<(), DiagError> {
        match self.cmd(program).arg("--help").output() {
            Ok(o) if String::from_utf8_lossy(&o.stderr).contains("Usage") => Ok(()),
            Ok(o) => Err(check_error(
                format!("the new {} does not run", program.display()),
                format!(
                    "{}: {}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
            )),
            Err(e) => Err(check_error(
                format!("the new {} cannot be started", program.display()),
                e.to_string(),
            )),
        }
    }

    /// PL: Kopia przez `install` jako `horizon`, więc plik ma tego samego właściciela co pliki z
    /// instalatora. EN: A copy through `install` as `horizon`, so the file has the same owner as
    /// the installer's files.
    fn copy(&self, from: &Path, to: &Path, mode: &str) -> Result<(), DiagError> {
        let dir = to.parent().unwrap_or(Path::new("."));
        let path = |p: &Path| p.to_string_lossy().into_owned();
        self.run("install", &["-d", "-m", "0755", &path(dir)])
            .and_then(|_| self.run("install", &["-m", mode, &path(from), &path(to)]))
            .map(|_| ())
            .map_err(|e| {
                check_error(
                    format!("{} cannot be copied to {}", from.display(), to.display()),
                    e,
                )
            })
    }

    fn active_units(&self) -> Result<(), String> {
        for unit in &self.units {
            let ok = Command::new("systemctl")
                .args(["is-active", "-q", unit])
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                return Err(format!("{unit} is not active"));
            }
        }
        if !self.units.iter().any(|u| u.starts_with(ENGINE_UNIT)) {
            return Ok(());
        }
        let status = self.server_cmd(&["status", "--json"]).unwrap_or_default();
        let v: Value = serde_json::from_str(&status).unwrap_or(Value::Null);
        if v["phase"] == "running" {
            Ok(())
        } else {
            Err(format!("the engine is not running: {}", status.trim()))
        }
    }
}

impl Env for Real {
    fn current(&mut self) -> Option<String> {
        fs::read_to_string(self.root.join("hot-version"))
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    fn fetch(&mut self) -> Result<Option<String>, DiagError> {
        self.run(
            "git",
            &[
                "fetch",
                "-q",
                "--tags",
                &self.source,
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        )
        .map_err(|e| {
            check_error(
                format!("horizon-hot cannot be fetched from {}", self.source),
                e,
            )
        })?;
        let branch = format!("refs/remotes/origin/{}^{{commit}}", self.git_ref);
        Ok(self
            .run("git", &["rev-parse", "-q", "--verify", &branch])
            .ok()
            .map(|s| s.trim().to_owned()))
    }

    fn changed(&mut self, from: &str, to: &str) -> Result<Vec<String>, DiagError> {
        let out = self
            .run("git", &["diff", "--name-only", from, to])
            .map_err(|e| check_error("the versions cannot be compared", e))?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    fn build(&mut self, rev: &str, mode: Mode) -> Result<(), DiagError> {
        self.run("git", &["checkout", "-q", "--detach", rev])
            .map_err(|e| check_error(format!("horizon-hot {rev} cannot be checked out"), e))?;
        let mut args = vec!["build", "--release", "-q"];
        let packages: &[&str] = match mode {
            Mode::Core => &["hot-core"],
            Mode::Full => &PACKAGES,
        };
        for p in packages {
            args.extend(["-p", p]);
        }
        self.run("rustup", &["toolchain", "install"])
            .and_then(|_| self.run("cargo", &args))
            .map_err(|e| {
                check_error(
                    format!("horizon-hot {rev} does not compile on this server"),
                    e,
                )
            })?;
        let release = self.src.join("target").join("release");
        let live_core = self.core_dir().join(core_file());
        self.copy(
            &release.join(core_file()),
            &with_suffix(&live_core, ".next"),
            "0644",
        )?;
        if mode == Mode::Full {
            for p in PROGRAMS {
                let staged = self.bin.join(NEXT).join(p);
                self.copy(&release.join(p), &staged, "0755")?;
                self.smoke(&staged)?;
            }
        }
        self.log.info(
            "hotd upgrade built a version",
            json!({ "rev": rev, "mode": format!("{mode:?}") }),
        );
        Ok(())
    }

    fn load_core(&mut self) -> Result<(), DiagError> {
        let live = self.core_dir().join(core_file());
        let next = with_suffix(&live, ".next");
        let reply = self
            .server_cmd(&["cmd", &format!("hot core load {}", next.display())])
            .unwrap_or_else(|e| e);
        if reply.contains("Nie wczytam rdzenia") || reply.contains("nie wystartował") {
            let _ = fs::remove_file(&next);
            return Err(DiagError::new(
                codes::UPGRADE_CORE_FAILED,
                "the new hot-core did not load in the engine",
                "Read the engine's logs/latest.log (HOT-E0406) and logs/hot-core.jsonl; fix hot-core and push a new commit",
            )
            .context("reply", reply.trim())
            .state("The previous hot-core runs; this version is skipped"));
        }
        if !reply.contains("Rdzeń podmieniony") {
            self.log.warn(
                "hotd upgrade: the engine did not confirm hot core load; the next engine start loads the new core",
                json!({ "reply": reply.trim() }),
            );
        }
        rename(&next, &live)
    }

    fn players(&mut self) -> usize {
        let from_proxy = UnixStream::connect(self.home.join("hot-proxy.sock")).and_then(|mut s| {
            s.set_read_timeout(Some(Duration::from_secs(10)))?;
            writeln!(s, "status")?;
            let mut line = String::new();
            BufReader::new(s).read_line(&mut line)?;
            Ok(line)
        });
        if let Ok(line) = from_proxy {
            let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
            if let Some(p) = v["players"].as_array() {
                return p.len();
            }
        }
        let status = self.server_cmd(&["status", "--json"]).unwrap_or_default();
        crate::update_env::health_from(&status)
            .players
            .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
    }

    fn install(&mut self) -> Result<(), DiagError> {
        for (staged, live) in self.pairs() {
            if live.exists() {
                rename(&live, &with_suffix(&live, ".previous"))?;
            }
            rename(&staged, &live)?;
        }
        let _ = fs::remove_dir(self.bin.join(NEXT));
        Ok(())
    }

    fn restart(&mut self) -> Result<(), DiagError> {
        let fail = |what: String| {
            DiagError::new(
                codes::UPGRADE_ROLLED_BACK,
                "the services did not come up after the update",
                "Read journalctl -u <unit> and logs/; hotd upgrade puts the previous files back",
            )
            .chain_line(what)
        };
        for unit in &self.units {
            let out = Command::new("systemctl")
                .args(["restart", unit])
                .output()
                .map_err(|e| fail(format!("systemctl cannot be started: {e}")))?;
            if !out.status.success() {
                return Err(fail(format!(
                    "systemctl restart {unit}: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
        }
        let start = Instant::now();
        loop {
            match self.active_units() {
                Ok(()) => return Ok(()),
                Err(why) if start.elapsed() >= COME_UP => return Err(fail(why)),
                Err(_) => std::thread::sleep(Duration::from_secs(3)),
            }
        }
    }

    fn restore(&mut self) {
        for (_, live) in self.pairs() {
            let previous = with_suffix(&live, ".previous");
            if previous.exists() {
                if let Err(e) = rename(&previous, &live) {
                    self.log.error(&e);
                }
            }
        }
    }

    fn discard(&mut self) {
        let _ = fs::remove_dir_all(self.bin.join(NEXT));
        let _ = fs::remove_file(with_suffix(&self.core_dir().join(core_file()), ".next"));
    }

    fn set_version(&mut self, rev: &str) -> Result<(), DiagError> {
        crate::state::write_atomic(
            &self.root.join("hot-version"),
            format!("{rev}\n").as_bytes(),
        )
    }

    fn info(&mut self, what: &str, context: Value) {
        self.log.info(what, context);
    }

    fn error(&mut self, error: &DiagError) {
        self.log.error(error);
    }
}
