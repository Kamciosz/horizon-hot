//! PL: `hotd engine` — główny proces usługi silnika (zamiast `horizon-server run`). Uruchamia
//! silnik aktywnej strony (`horizon-server start`), pilnuje, żeby druga strona nie czekała bez
//! celu, i na żądanie z gniazda `<home>/hot-engine.sock` robi podmianę silnika (D11, `swap.rs`).
//! Podmiana idzie z tej usługi, nie z `hotd run`: tamta ma najniższy priorytet (D16), a liczy się
//! każda milisekunda pauzy. SIGTERM (systemd, `docker stop`): zatrzymuje silnik, który zapisuje
//! świat, potem kończy się.
//!
//! Gniazdo (0600), jedna komenda na połączenie, jedna linia JSON-a w odpowiedzi:
//! - `status` — aktywna strona, jej katalog, adres i faza;
//! - `swap [PLIK]` — podmiana na nowy plik silnika (bez pliku: na ten sam).
//!
//! Co minutę sprawdza też pamięć starych generacji modów i po przekroczeniu progu sam robi
//! podmianę na ten sam plik (D4, `reclaim.rs`).
//!
//! EN: `hotd engine`, the main process of the engine service (instead of `horizon-server run`). It
//! starts the active side's engine (`horizon-server start`), keeps the other side from waiting for
//! nothing, and swaps the engine (D11, `swap.rs`) on request from the `<home>/hot-engine.sock`
//! socket. The swap runs from this service, not from `hotd run`: that one has the lowest priority
//! (D16), and every millisecond of the pause counts. SIGTERM (systemd, `docker stop`): stops the
//! engine, which saves the world, then exits.
//!
//! The socket (0600), one command per connection, one JSON line in reply:
//! - `status` — the active side, its folder, address and phase;
//! - `swap [FILE]` — a swap to a new engine file (no file: the same one).
//!
//! Every minute it also checks the memory of old mod generations and past the limit swaps to the
//! same file by itself (D4, `reclaim.rs`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use hot_diag::{DiagError, Logger};
use serde_json::{json, Value};

use crate::codes;
use crate::reclaim::{self, Reclaim};
use crate::swap::{self, Engines, Layout, Limits, ProxySocket, Server};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// PL: Ścieżka gniazda. EN: The socket path.
#[must_use]
pub fn socket_path(home: &Path) -> PathBuf {
    home.join("hot-engine.sock")
}

/// PL: Jak skończył się `hotd engine`. EN: How `hotd engine` ended.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// PL: SIGTERM albo silnik zatrzymany z zewnątrz. EN: SIGTERM, or the engine stopped from outside.
    Stopped,
    /// PL: Supervisor Horizona przestał wznawiać silnik po awariach. EN: Horizon's supervisor gave up.
    GaveUp,
}

/// PL: Główna pętla `hotd engine`. EN: The `hotd engine` main loop.
///
/// # Errors
/// PL: Silnik nie wystartował albo gniazda nie da się otworzyć. EN: The engine did not start, or
/// the socket cannot be opened.
pub fn run(
    home: &Path,
    program: &Path,
    mut reclaim: Reclaim,
    log: &Arc<Logger>,
) -> Result<End, DiagError> {
    // SAFETY: obsługa tylko zapisuje atomową flagę. EN: the handler only stores an atomic flag.
    unsafe {
        libc::signal(libc::SIGTERM, on_stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_stop as *const () as libc::sighandler_t);
    }
    let layout = Layout {
        home: home.to_owned(),
    };
    let mut server = Server {
        program: program.to_owned(),
    };
    let active = layout.active();
    let (dir, other) = (layout.dir(active), layout.dir(active.other()));
    // PL: Po przerwanej podmianie druga strona może czekać na świat; po 120 s wpadłaby w pętlę.
    // EN: After an interrupted swap the other side may wait for the world; after 120 s it loops.
    if other.join("server.toml").is_file() && server.phase(&other) != "stopped" {
        log.warn(
            "The inactive side was running; hotd stops it",
            json!({ "dir": other.display().to_string() }),
        );
        server.halt_waiting(&other);
    }
    if server.phase(&dir) == "stopped" {
        let started = server
            .start(&dir)
            .and_then(|mut s| wait(s.as_mut(), Duration::from_secs(900)));
        if let Err(why) = started {
            return Err(DiagError::new(
                codes::ENGINE_START,
                format!("The engine of side {} does not start: {why}", active.name()),
                "Read logs/supervisor.log and logs/latest.log in that folder; fix the cause, then restart the service",
            )
            .expected("horizon-server start reaches the running phase")
            .context("dir", dir.display().to_string())
            .state("The server is down; hot-proxy holds players for up to 180 s"));
        }
    }
    log.info(
        "hotd engine runs the engine",
        json!({ "side": active.name(), "dir": dir.display().to_string() }),
    );
    let busy = Arc::new(Mutex::new(()));
    serve(&layout, program, log, &busy).map_err(|e| {
        DiagError::new(
            codes::ENGINE_START,
            format!("hotd engine cannot open its socket: {e}"),
            "Another hotd engine with the same --home runs (stop it), or HOT_HOME is not writable, or its path is too long for a Unix socket (keep it under 104 bytes)",
        )
        .context("socket", socket_path(home).display().to_string())
        .state("The engine runs, but engine swaps are not possible until hotd engine restarts")
    })?;
    let mut checked = Instant::now();
    let mut memory_checked = Instant::now();
    loop {
        if STOP.load(Ordering::SeqCst) {
            let _one = busy.lock().unwrap_or_else(PoisonError::into_inner);
            let dir = layout.active_dir();
            log.info(
                "hotd engine stops the engine",
                json!({ "dir": dir.display().to_string() }),
            );
            let _ = server.stop(&dir);
            let _ = std::fs::remove_file(socket_path(home));
            return Ok(End::Stopped);
        }
        if checked.elapsed() >= Duration::from_secs(5) {
            checked = Instant::now();
            if let Ok(_one) = busy.try_lock() {
                let dir = layout.active_dir();
                if server.phase(&dir) == "stopped" {
                    let _ = std::fs::remove_file(socket_path(home));
                    let gave_up = std::fs::read_to_string(dir.join("logs").join("supervisor.log"))
                        .is_ok_and(|t| {
                            t.lines()
                                .rev()
                                .take(20)
                                .any(|l| l.contains("not restarting it"))
                        });
                    log.warn(
                        "The engine stopped; hotd engine ends",
                        json!({ "dir": dir.display().to_string(), "gave_up": gave_up }),
                    );
                    return Ok(if gave_up { End::GaveUp } else { End::Stopped });
                }
            }
        }
        if memory_checked.elapsed() >= reclaim::EVERY {
            memory_checked = Instant::now();
            if let Ok(_one) = busy.try_lock() {
                reclaim_memory(&layout, &mut server, &mut reclaim, log);
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// PL: Podmiana na ten sam plik silnika, gdy stare generacje modów zajmują za dużo (D4).
/// EN: A swap to the same engine file when old mod generations take too much (D4).
fn reclaim_memory(layout: &Layout, server: &mut Server, reclaim: &mut Reclaim, log: &Logger) {
    let status = std::fs::read(layout.home.join("hot-state").join("status.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or(Value::Null);
    let now = Instant::now();
    let Some(stale) = reclaim.due(&status, server.engine_pid(&layout.active_dir()), now) else {
        return;
    };
    log.info(
        "hotd engine swaps the engine to free the memory of old mod generations (D4)",
        json!({
            "stale_bytes": stale,
            "libraries": status["libraries"]["count"],
            "limit_bytes": reclaim.limit,
        }),
    );
    let proxy = ProxySocket {
        home: layout.home.clone(),
    };
    match swap::swap(layout, server, &proxy, None, Limits::default(), log) {
        Ok(report) => {
            log.info("Memory swap done", json!({ "report": report }));
            reclaim.done(None, Instant::now());
        }
        // PL: `swap` sam zapisał błąd w logu. EN: `swap` has logged the error itself.
        Err(e) => reclaim.done(Some(e.code()), Instant::now()),
    }
}

fn wait(started: &mut dyn swap::Started, limit: Duration) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(r) = started.exited() {
            return r;
        }
        if Instant::now() >= deadline {
            return Err(format!("no running phase in {} s", limit.as_secs()));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn serve(
    layout: &Layout,
    program: &Path,
    log: &Arc<Logger>,
    busy: &Arc<Mutex<()>>,
) -> std::io::Result<()> {
    let path = socket_path(&layout.home);
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("another hotd engine answers on {}", path.display()),
            ));
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let (layout, program, log, busy) = (
        layout.clone(),
        program.to_owned(),
        Arc::clone(log),
        Arc::clone(busy),
    );
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (layout, program, log, busy) = (
                layout.clone(),
                program.clone(),
                Arc::clone(&log),
                Arc::clone(&busy),
            );
            thread::spawn(move || answer(&layout, &program, &log, &busy, stream));
        }
    });
    Ok(())
}

fn answer(layout: &Layout, program: &Path, log: &Logger, busy: &Mutex<()>, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let mut server = Server {
        program: program.to_owned(),
    };
    let mut words = line.split_whitespace();
    let reply = match (words.next(), words.next(), words.next()) {
        (Some("status"), None, _) => {
            let side = layout.active();
            let dir = layout.dir(side);
            json!({
                "side": side.name(),
                "dir": dir.display().to_string(),
                "address": layout.address(side).map(|a| a.to_string()).unwrap_or_default(),
                "phase": server.phase(&dir),
            })
        }
        (Some("swap"), engine, None) => {
            let _one = busy.lock().unwrap_or_else(PoisonError::into_inner);
            let proxy = ProxySocket {
                home: layout.home.clone(),
            };
            match swap::swap(
                layout,
                &mut server,
                &proxy,
                engine.map(Path::new),
                Limits::default(),
                log,
            ) {
                Ok(report) => json!({ "ok": true, "report": report }),
                Err(e) => json!({ "ok": false, "error": Value::Object(e.fields()) }),
            }
        }
        _ => {
            json!({ "ok": false, "error": format!("unknown command {:?}; use status or swap [FILE]", line.trim()) })
        }
    };
    let _ = writeln!(stream, "{reply}");
}

/// PL: Komenda do działającego `hotd engine`; zwraca linię odpowiedzi.
/// EN: A command to the running `hotd engine`; returns the reply line.
///
/// # Errors
/// PL: `hotd engine` nie działa albo nie odpowiedział. EN: `hotd engine` is down or did not answer.
pub fn request(home: &Path, command: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(socket_path(home))?;
    // PL: Podmiana: do 120 s rozgrzewania i ok. 25 s na graczy. EN: Up to 120 s warm-up, ~25 s players.
    s.set_read_timeout(Some(Duration::from_secs(300)))?;
    writeln!(s, "{command}")?;
    let mut reply = String::new();
    BufReader::new(&s).read_line(&mut reply)?;
    if reply.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "hotd engine closed the connection without an answer",
        ));
    }
    Ok(reply.trim_end().to_owned())
}
