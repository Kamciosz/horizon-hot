//! PL: Dwie strony silnika i podmiana na rozgrzany silnik (D11, D12). Strona `a` to katalog
//! serwera (`HOT_HOME`), strona `b` to `<home>/engine-b`: kopia `server.toml` strony `a` z portem
//! o jeden wyżej i dowiązaniami do świata i plików wspólnych. Aktywną stronę zapisuje
//! `hot-state/engine.json`; czytają go `hotd` (komendy do silnika) i `hot-proxy` (po restarcie).
//!
//! Kolejność podmiany (D11, S13): nowy silnik startuje w tle i czeka przed światem (mixin
//! `wait_for_world` w `hot-host`), stary dostaje `save-all flush`, proxy przestaje przekazywać
//! jego pakiety („held”), stary dostaje `stop`, nowy zajmuje świat, proxy loguje graczy. Błąd przed
//! zamrożeniem: gracze zostają na starym. Błąd po zamrożeniu: proxy trzyma graczy, a stary silnik
//! wraca tą samą drogą.
//!
//! EN: The two engine sides and the swap to a warm engine (D11, D12). Side `a` is the server
//! folder (`HOT_HOME`), side `b` is `<home>/engine-b`: a copy of side `a`'s `server.toml` with the
//! port one higher and links to the world and the shared files. `hot-state/engine.json` records
//! the active side; `hotd` (engine commands) and `hot-proxy` (after a restart) read it.
//!
//! The swap order (D11, S13): the new engine starts in the background and waits before the world
//! (the `wait_for_world` mixin in `hot-host`), the old one gets `save-all flush`, the proxy stops
//! passing its packets on ("held"), the old one gets `stop`, the new one takes the world, the proxy
//! logs the players in. A failure before the freeze: the players stay on the old engine. A failure
//! after it: the proxy holds the players and the old engine comes back the same way.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::SocketAddr;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hot_diag::{DiagError, Logger};
use serde_json::{json, Value};

use crate::codes;
use crate::state::write_atomic;

/// PL: Linia mixina `wait_for_world` w `logs/latest.log`: nowy silnik jest gotowy do podmiany.
/// EN: The `wait_for_world` mixin's line in `logs/latest.log`: the new engine is ready to swap.
pub const WAITING_LINE: &str = "horizon-hot: waiting for the world";

/// PL: Pliki strony `a`, które strona `b` dzieli przez dowiązanie (świat dochodzi osobno).
/// EN: Side `a` files that side `b` shares through a link (the world is added separately).
const SHARED: [&str; 12] = [
    "backups",
    "horizon-mods",
    "horizon-mods.txt",
    "ops.json",
    "whitelist.json",
    "banned-players.json",
    "banned-ips.json",
    "usercache.json",
    "forwarding.secret",
    "eula.txt",
    "config",
    "resources",
];

/// PL: Strona silnika. EN: An engine side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

impl Side {
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Side::A => "a",
            Side::B => "b",
        }
    }
}

/// PL: Układ stron w katalogu serwera. EN: The side layout in the server folder.
#[derive(Debug, Clone)]
pub struct Layout {
    /// PL: Katalog strony `a` i `HOT_HOME`. EN: Side `a`'s folder and `HOT_HOME`.
    pub home: PathBuf,
}

impl Layout {
    #[must_use]
    pub fn dir(&self, side: Side) -> PathBuf {
        match side {
            Side::A => self.home.clone(),
            Side::B => self.home.join("engine-b"),
        }
    }

    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.home.join("hot-state").join("engine.json")
    }

    /// PL: Aktywna strona; `a`, gdy brak zapisu albo strona `b` nie istnieje.
    /// EN: The active side; `a` when nothing is recorded or side `b` does not exist.
    #[must_use]
    pub fn active(&self) -> Side {
        let side = fs::read_to_string(self.state_file())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["side"].as_str().map(str::to_owned));
        if side.as_deref() == Some("b") && self.dir(Side::B).join("server.toml").is_file() {
            Side::B
        } else {
            Side::A
        }
    }

    /// PL: Czas ostatniej udanej podmiany (ms od epoki), 0 bez podmiany.
    /// EN: The time of the last successful swap (ms since the epoch), 0 without one.
    #[must_use]
    pub fn swapped_at_ms(&self) -> u64 {
        fs::read_to_string(self.state_file())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["swapped_at_ms"].as_u64())
            .unwrap_or(0)
    }

    /// PL: Katalog aktywnej strony. EN: The active side's folder.
    #[must_use]
    pub fn active_dir(&self) -> PathBuf {
        self.dir(self.active())
    }

    /// PL: Adres silnika strony z jej `server.toml`. EN: The side's engine address from its `server.toml`.
    ///
    /// # Errors
    /// PL: Brak pliku albo portu. EN: The file or the port is missing.
    pub fn address(&self, side: Side) -> Result<SocketAddr, String> {
        let file = self.dir(side).join("server.toml");
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let port: u16 = toml_value(&text, "server", "port")
            .and_then(|p| p.parse().ok())
            .ok_or_else(|| format!("{} has no [server] port", file.display()))?;
        let bind = toml_value(&text, "server", "bind").filter(|b| !b.is_empty());
        format!("{}:{port}", bind.as_deref().unwrap_or("127.0.0.1"))
            .parse()
            .map_err(|e| format!("{}: [server] bind and port: {e}", file.display()))
    }

    fn record(&self, side: Side, address: SocketAddr, engine: &str) -> Result<(), DiagError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let v = json!({
            "side": side.name(),
            "dir": self.dir(side).display().to_string(),
            "address": address.to_string(),
            "engine": engine,
            "swapped_at_ms": now,
        });
        write_atomic(&self.state_file(), format!("{v:#}\n").as_bytes())
    }

    /// PL: Odświeża stronę `b` ze strony `a`: `server.toml`, dowiązania, port o jeden wyżej, bez
    /// panelu i ze stałym API na wolnym porcie. EN: Refreshes side `b` from side `a`: `server.toml`,
    /// links, the port one higher, no panel and the API on a free port.
    fn prepare_b(&self, engines: &mut dyn Engines, engine: &str) -> Result<(), String> {
        let (a, b) = (self.dir(Side::A), self.dir(Side::B));
        fs::create_dir_all(&b).map_err(|e| format!("{}: {e}", b.display()))?;
        fs::copy(a.join("server.toml"), b.join("server.toml"))
            .map_err(|e| format!("server.toml cannot be copied to {}: {e}", b.display()))?;
        let world = crate::data::world_dir(&a);
        let world_name = world.file_name().map(PathBuf::from).unwrap_or_default();
        let shared = SHARED.iter().map(PathBuf::from).chain([world_name]);
        for name in shared {
            let (target, link) = (a.join(&name), b.join(&name));
            if !target.exists() {
                continue;
            }
            if link.is_symlink() || link.is_file() {
                fs::remove_file(&link).map_err(|e| format!("{}: {e}", link.display()))?;
            } else if link.is_dir() {
                fs::remove_dir_all(&link).map_err(|e| format!("{}: {e}", link.display()))?;
            }
            std::os::unix::fs::symlink(&target, &link).map_err(|e| {
                format!(
                    "{} cannot link to {}: {e}",
                    link.display(),
                    target.display()
                )
            })?;
        }
        let port = self.address(Side::A)?.port() + 1;
        engines.configure(
            &b,
            &[
                format!("server.port={port}"),
                "panel.bind=".into(),
                "api.bind=".into(),
                format!("engine.path={engine}"),
            ],
        )
    }
}

/// PL: Wartość klucza z sekcji `server.toml` (bez cudzysłowów i komentarza).
/// EN: A key's value from a `server.toml` section (no quotes, no comment).
fn toml_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line.split('#').next().map(str::trim) == Some(&format!("[{section}]"));
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if !inside || k.trim() != key {
            continue;
        }
        let v = v.trim();
        return Some(match v.strip_prefix('"') {
            Some(rest) => rest.split('"').next().unwrap_or_default().to_owned(),
            None => v.split('#').next().unwrap_or_default().trim().to_owned(),
        });
    }
    None
}

/// PL: Co podmiana potrzebuje od `horizon-server`. EN: What the swap needs from `horizon-server`.
pub trait Engines {
    /// PL: `start` w tle; wraca od razu. EN: `start` in the background; returns at once.
    ///
    /// # Errors
    /// PL: Programu nie da się uruchomić. EN: The program cannot be run.
    fn start(&mut self, dir: &Path) -> Result<Box<dyn Started>, String>;
    /// PL: `stop` z czekaniem. EN: `stop`, waiting for it.
    ///
    /// # Errors
    /// PL: Wyjście `stop`. EN: The `stop` output.
    fn stop(&mut self, dir: &Path) -> Result<(), String>;
    /// PL: Zatrzymuje silnik czekający na świat: `stop` w tle, potem jeden SIGTERM do `engine_pid`
    /// strony (D11). EN: Stops an engine waiting for the world: `stop` in the background, then one
    /// SIGTERM to the side's `engine_pid` (D11).
    fn halt_waiting(&mut self, dir: &Path);
    /// PL: Komenda konsoli. EN: A console command.
    ///
    /// # Errors
    /// PL: Wyjście `cmd`. EN: The `cmd` output.
    fn command(&mut self, dir: &Path, line: &str) -> Result<String, String>;
    /// PL: `config set` z listą `klucz=wartość`. EN: `config set` with `key=value` items.
    ///
    /// # Errors
    /// PL: Wyjście `config set`. EN: The `config set` output.
    fn configure(&mut self, dir: &Path, settings: &[String]) -> Result<(), String>;
    /// PL: Faza z `status --json` (`stopped`, gdy brak). EN: The `status --json` phase.
    fn phase(&mut self, dir: &Path) -> String;
}

/// PL: Uruchomione w tle `start`. EN: A `start` running in the background.
pub trait Started {
    /// PL: Wynik `start`, gdy już się skończył. EN: The `start` result once it ended.
    fn exited(&mut self) -> Option<Result<(), String>>;
}

/// PL: Co podmiana potrzebuje od `hot-proxy`. EN: What the swap needs from `hot-proxy`.
pub trait Switcher {
    /// PL: Przenosi graczy na `to`; `held` woła, gdy stary silnik nic już im nie wysyła.
    /// EN: Moves the players to `to`; calls `held` once the old engine sends them nothing.
    ///
    /// # Errors
    /// PL: Proxy nie odpowiedziało. EN: The proxy did not answer.
    fn switch(&self, to: SocketAddr, held: &mut dyn FnMut()) -> Result<Value, String>;
    /// PL: Nowy adres bez przenoszenia. EN: A new address without moving.
    ///
    /// # Errors
    /// PL: Proxy nie odpowiedziało. EN: The proxy did not answer.
    fn set_engine(&self, to: SocketAddr) -> Result<(), String>;
}

/// PL: Limity czasu podmiany. EN: The swap time limits.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// PL: Na linię „waiting for the world” (mixin czeka najwyżej 120 s). EN: For the waiting line.
    pub warm: Duration,
    /// PL: Od zatrzymania starego do fazy `running` nowego. EN: From the old stop to the new `running`.
    pub take_over: Duration,
    /// PL: Na powrót starego silnika. EN: For the old engine to come back.
    pub back: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            warm: Duration::from_secs(120),
            take_over: Duration::from_secs(10),
            back: Duration::from_secs(12),
        }
    }
}

fn wait_started(started: &mut dyn Started, limit: Duration) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(result) = started.exited() {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "horizon-server start did not reach the running phase in {} s",
                limit.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// PL: Wynik etapu po zamrożeniu. EN: The result of the stage after the freeze.
#[derive(Default)]
struct Frozen {
    held: bool,
    stop_ms: u128,
    stop_error: Option<String>,
    take_over: Option<Result<(), String>>,
    back: Option<Result<(), String>>,
}

fn ms(since: Instant) -> u128 {
    since.elapsed().as_millis()
}

/// PL: Podmienia aktywny silnik na rozgrzany silnik drugiej strony (D11). `engine` to nowy plik
/// silnika (każdy pod własną ścieżką, nigdy nadpisany); `None` = ten sam co teraz.
/// EN: Swaps the active engine for a warm engine on the other side (D11). `engine` is the new
/// engine file (each under its own path, never overwritten); `None` = the current one.
///
/// # Errors
/// PL: `HOT-E0606` (gracze zostali na starym silniku) albo `HOT-E0607` (nowy padł po zamrożeniu;
/// stary wrócił albo nie). EN: `HOT-E0606` (the players stayed on the old engine) or `HOT-E0607`
/// (the new one failed after the freeze; the old one came back or not).
#[allow(clippy::too_many_lines)] // PL: kroki D11 po kolei. EN: the D11 steps in order.
pub fn swap(
    layout: &Layout,
    engines: &mut dyn Engines,
    proxy: &dyn Switcher,
    engine: Option<&Path>,
    limits: Limits,
    log: &Logger,
) -> Result<Value, DiagError> {
    let from = layout.active();
    let to = from.other();
    let (from_dir, to_dir) = (layout.dir(from), layout.dir(to));
    let engine = match engine {
        Some(p) => p.display().to_string(),
        None => fs::read_to_string(from_dir.join("server.toml"))
            .ok()
            .and_then(|t| toml_value(&t, "engine", "path"))
            .unwrap_or_default(),
    };
    let before = |what: String| {
        DiagError::new(
            codes::SWAP_NOT_STARTED,
            format!("The new engine did not get ready: {what}"),
            "Read the new side's logs/latest.log and logs/supervisor.log; a new engine file must be built with hot-host (the waiting line comes from its wait_for_world mixin)",
        )
        .expected(format!("the {} side starts and logs \"{WAITING_LINE}\"", to.name()))
        .context("from", from.name())
        .context("to", to.name())
        .context("dir", to_dir.display().to_string())
        .context("engine", engine.clone())
        .state("The players stay on the old engine; nothing was frozen")
    };
    let setup = if to == Side::B {
        layout.prepare_b(engines, &engine)
    } else {
        engines.configure(&to_dir, &[format!("engine.path={engine}")])
    };
    setup.map_err(before)?;
    let (from_addr, to_addr) = (
        layout.address(from).map_err(before)?,
        layout.address(to).map_err(before)?,
    );
    if engines.phase(&to_dir) != "stopped" {
        engines.halt_waiting(&to_dir);
    }
    let latest = to_dir.join("logs").join("latest.log");
    let _ = fs::remove_file(&latest);
    let t0 = Instant::now();
    let mut started = engines.start(&to_dir).map_err(before)?;
    // PL: Gotowość: linia mixina albo silnik od razu ze światem (stary go nie trzymał).
    // EN: Ready: the mixin's line, or the engine went straight to the world (the old one held none).
    let ready = loop {
        let text = fs::read_to_string(&latest).unwrap_or_default();
        if text.contains(WAITING_LINE) || text.contains("Done (") {
            break Ok(());
        }
        match started.exited() {
            Some(Ok(())) => break Ok(()),
            Some(Err(e)) => break Err(format!("horizon-server start failed: {e}")),
            None => {}
        }
        if t0.elapsed() >= limits.warm {
            break Err(format!(
                "no \"{WAITING_LINE}\" line in {} within {} s",
                latest.display(),
                limits.warm.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    if let Err(why) = ready {
        engines.halt_waiting(&to_dir);
        let e = before(why);
        log.error(&e);
        return Err(e);
    }
    let warm_ms = ms(t0);
    if let Err(e) = engines.command(&from_dir, "save-all flush") {
        log.warn(
            "save-all flush before the swap failed; stop still saves the world",
            json!({ "dir": from_dir.display().to_string(), "error": e }),
        );
    }
    let mut frozen = Frozen::default();
    let mut freeze_at = None;
    let reply = proxy.switch(to_addr, &mut || {
        if frozen.held {
            return;
        }
        frozen.held = true;
        let t = Instant::now();
        freeze_at = Some(t);
        frozen.stop_error = engines.stop(&from_dir).err();
        frozen.stop_ms = ms(t);
        let took = wait_started(started.as_mut(), limits.take_over);
        if took.is_err() {
            // PL: D11 krok 5: gracze w przerwie idą z powrotem na stary silnik.
            // EN: D11 step 5: the players in the gap go back to the old engine.
            engines.halt_waiting(&to_dir);
            let back = proxy
                .set_engine(from_addr)
                .and_then(|()| engines.start(&from_dir))
                .and_then(|mut s| wait_started(s.as_mut(), limits.back));
            frozen.back = Some(back);
        }
        frozen.take_over = Some(took);
    });
    if !frozen.held {
        // PL: Proxy odmówiło przed zamrożeniem. EN: The proxy refused before the freeze.
        engines.halt_waiting(&to_dir);
        let e = before(format!(
            "hot-proxy did not start the switch: {}",
            reply.as_ref().map_or_else(Clone::clone, Value::to_string)
        ));
        log.error(&e);
        return Err(e);
    }
    let freeze_ms = freeze_at.map_or(0, ms);
    let reply = reply.unwrap_or_else(|e| json!({ "error": e }));
    let report = json!({
        "from": from.name(),
        "to": to.name(),
        "engine": engine,
        "address": to_addr.to_string(),
        "warm_ms": warm_ms,
        "stop_ms": frozen.stop_ms,
        "freeze_ms": freeze_ms,
        "proxy": reply,
    });
    match frozen.take_over {
        Some(Ok(())) => {
            layout.record(to, to_addr, &engine)?;
            if let Some(e) = &frozen.stop_error {
                log.warn(
                    "The old engine's stop reported an error",
                    json!({ "error": e }),
                );
            }
            log.info("Engine swap finished", report.clone());
            Ok(report)
        }
        took => {
            let why = took.and_then(Result::err).unwrap_or_default();
            let back = frozen.back.unwrap_or_else(|| Err("not tried".into()));
            let state = match &back {
                Ok(()) => "The old engine runs again and the players went back to it",
                Err(_) => "The old engine did not come back; hot-proxy holds the players up to 180 s, then asks them to join again",
            };
            let mut e = DiagError::new(
                codes::SWAP_FAILED,
                format!("The new engine did not take the world: {why}"),
                "Read the new side's logs/latest.log and crash-reports/; the swap ran on the old engine's world, so look for a world load error there first",
            )
            .expected("the new engine reaches the running phase within seconds of the old one's stop")
            .context("from", from.name())
            .context("to", to.name())
            .context("engine", engine.clone())
            .context("report", report)
            .state(state);
            if let Err(b) = back {
                e = e.chain_line(format!("the old engine's start: {b}"));
            }
            log.error(&e);
            Err(e)
        }
    }
}

/// PL: Prawdziwy `horizon-server`. EN: The real `horizon-server`.
pub struct Server {
    pub program: PathBuf,
}

impl Server {
    fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let out = Command::new(&self.program)
            .arg("--dir")
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{} cannot be started: {e}", self.program.display()))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            Ok(stdout)
        } else {
            Err(format!(
                "{} {}: exit {}: {} {}",
                self.program.display(),
                args.join(" "),
                out.status,
                stdout.trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// PL: Pid procesu silnika strony, gdy działa. EN: The side's engine process pid when running.
    #[must_use]
    pub fn engine_pid(&self, dir: &Path) -> Option<u64> {
        let status = self.status(dir);
        if status["phase"] != "running" {
            return None;
        }
        status["engine_pid"].as_u64()
    }

    fn status(&self, dir: &Path) -> Value {
        self.run(dir, &["status", "--json"])
            .ok()
            .and_then(|o| serde_json::from_str(&o).ok())
            .unwrap_or(Value::Null)
    }
}

struct Background(Child, Option<Result<(), String>>);

impl Started for Background {
    fn exited(&mut self) -> Option<Result<(), String>> {
        if self.1.is_none() {
            let status = match self.0.try_wait() {
                Ok(Some(s)) => s,
                Ok(None) => return None,
                Err(e) => {
                    self.1 = Some(Err(e.to_string()));
                    return self.1.clone();
                }
            };
            let mut err = String::new();
            if let Some(mut s) = self.0.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            self.1 = Some(if status.success() {
                Ok(())
            } else {
                Err(format!("exit {status}: {}", err.trim()))
            });
        }
        self.1.clone()
    }
}

impl Engines for Server {
    fn start(&mut self, dir: &Path) -> Result<Box<dyn Started>, String> {
        let child = Command::new(&self.program)
            .arg("--dir")
            .arg(dir)
            .arg("start")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{} cannot be started: {e}", self.program.display()))?;
        Ok(Box::new(Background(child, None)))
    }

    fn stop(&mut self, dir: &Path) -> Result<(), String> {
        self.run(dir, &["stop"]).map(drop)
    }

    fn halt_waiting(&mut self, dir: &Path) {
        let pid = self.status(dir)["engine_pid"].as_u64();
        let stopper = Command::new(&self.program)
            .arg("--dir")
            .arg(dir)
            .arg("stop")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        thread::sleep(Duration::from_millis(500));
        // PL: Czekający silnik nie czyta konsoli; jeden SIGTERM kończy czekanie (M0.20). Pid tylko
        // ze `status` tej strony: przy tym samym pliku obie strony mają jeden program.
        // EN: A waiting engine does not read its console; one SIGTERM ends the wait (M0.20). The pid
        // only from this side's `status`: with the same file both sides run one program.
        if let Some(pid) = pid
            .and_then(|p| libc::pid_t::try_from(p).ok())
            .filter(|p| *p > 1)
        {
            // SAFETY: kill z dodatnim pidem tylko wysyła sygnał. EN: kill with a positive pid only signals.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        if let Ok(mut child) = stopper {
            let deadline = Instant::now() + Duration::from_secs(20);
            while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn command(&mut self, dir: &Path, line: &str) -> Result<String, String> {
        self.run(dir, &["cmd", line])
    }

    fn configure(&mut self, dir: &Path, settings: &[String]) -> Result<(), String> {
        let mut args = vec!["config", "set"];
        args.extend(settings.iter().map(String::as_str));
        self.run(dir, &args).map(drop)
    }

    fn phase(&mut self, dir: &Path) -> String {
        self.status(dir)["phase"]
            .as_str()
            .unwrap_or("stopped")
            .to_owned()
    }
}

/// PL: `hot-proxy` przez jego gniazdo `<home>/hot-proxy.sock`. EN: `hot-proxy` through its socket.
pub struct ProxySocket {
    pub home: PathBuf,
}

impl ProxySocket {
    fn connect(&self, command: &str) -> std::io::Result<UnixStream> {
        let mut s = UnixStream::connect(self.home.join("hot-proxy.sock"))?;
        s.set_read_timeout(Some(Duration::from_secs(90)))?;
        writeln!(s, "{command}")?;
        Ok(s)
    }
}

impl Switcher for ProxySocket {
    fn switch(&self, to: SocketAddr, held: &mut dyn FnMut()) -> Result<Value, String> {
        let stream = match self.connect(&format!("switch {to}")) {
            Ok(s) => s,
            // PL: Proxy nie działa: nikogo nie trzeba przenosić, a po starcie przeczyta
            // `engine.json`. EN: The proxy is down: nobody to move, and it reads `engine.json`
            // when it starts.
            Err(e) => {
                held();
                return Ok(
                    json!({ "players": 0, "note": format!("hot-proxy is not running: {e}") }),
                );
            }
        };
        let mut last = Value::Null;
        for line in BufReader::new(stream).lines() {
            let line = line.map_err(|e| format!("hot-proxy control socket: {e}"))?;
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| format!("hot-proxy answered {line:?}: {e}"))?;
            if v.get("held").is_some() {
                held();
            } else {
                last = v;
            }
        }
        if last.get("error").is_some() || last.is_null() {
            return Err(format!("hot-proxy answered {last}"));
        }
        Ok(last)
    }

    fn set_engine(&self, to: SocketAddr) -> Result<(), String> {
        match self.connect(&format!("engine {to}")) {
            Ok(s) => {
                let mut line = String::new();
                BufReader::new(s)
                    .read_line(&mut line)
                    .map_err(|e| format!("hot-proxy control socket: {e}"))?;
                if line.contains("\"error\"") {
                    return Err(format!("hot-proxy answered {}", line.trim()));
                }
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }
}
