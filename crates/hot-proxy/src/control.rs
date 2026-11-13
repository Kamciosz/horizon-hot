//! PL: Gniazdo sterujące `<home>/hot-proxy.sock` (Unix, tylko właściciel, 0600). Jedna komenda na
//! połączenie, jedna linia odpowiedzi w JSON-ie:
//! - `status` — aktywny silnik i gracze w grze;
//! - `switch [ADRES]` — przenosi graczy na silnik pod `ADRES` (bez adresu: na ten sam).
//!
//! Woła je `hot-proxy switch|status`, a w M8.3 `hotd` przy podmianie silnika.
//!
//! EN: The control socket `<home>/hot-proxy.sock` (Unix, owner only, 0600). One command per
//! connection, one JSON line in reply:
//! - `status` — the active engine and the players in game;
//! - `switch [ADDR]` — moves the players to the engine at `ADDR` (no address: the same one).
//!
//! `hot-proxy switch|status` calls it, and `hotd` will in M8.3 when it swaps the engine.

use crate::session::Proxy;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

/// PL: Ścieżka gniazda w HOT_HOME. EN: The socket path in HOT_HOME.
#[must_use]
pub fn socket_path(home: &Path) -> PathBuf {
    home.join("hot-proxy.sock")
}

/// PL: Otwiera gniazdo i obsługuje je w tle. Stare gniazdo po zabitym proxy usuwa; gdy inne proxy
/// jeszcze na nim słucha, odmawia.
/// EN: Opens the socket and serves it in the background. Removes a stale socket left by a killed
/// proxy; refuses when another proxy still listens on it.
///
/// # Errors
/// PL: Inne proxy działa albo gniazda nie da się utworzyć. EN: Another proxy runs, or the socket
/// cannot be made.
pub fn serve(proxy: &Arc<Proxy>, home: &Path) -> std::io::Result<()> {
    let path = socket_path(home);
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("another hot-proxy answers on {}", path.display()),
            ));
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let proxy = Arc::clone(proxy);
    // PL: Jedna podmiana naraz; `status` odpowiada także w jej trakcie.
    // EN: One switch at a time; `status` answers during one too.
    let switching = Arc::new(Mutex::new(()));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let proxy = Arc::clone(&proxy);
            let switching = Arc::clone(&switching);
            thread::spawn(move || answer(&proxy, &switching, stream));
        }
    });
    Ok(())
}

fn answer(proxy: &Arc<Proxy>, switching: &Mutex<()>, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let mut words = line.split_whitespace();
    let reply = match (words.next(), words.next(), words.next()) {
        (Some("status"), None, _) => proxy.status_json(),
        (Some("switch"), target, None) => match target.map(str::parse::<SocketAddr>).transpose() {
            Ok(target) => {
                let _one = switching.lock().unwrap_or_else(PoisonError::into_inner);
                proxy.switch(target)
            }
            Err(_) => json!({ "error": "switch needs an address like 127.0.0.1:25566" }),
        },
        _ => {
            json!({ "error": format!("unknown command {:?}; use status or switch [ADDR]", line.trim()) })
        }
    };
    let _ = writeln!(stream, "{reply}");
}

/// PL: Wysyła komendę do działającego proxy i zwraca linię odpowiedzi. Podmiana może czekać na
/// silnik do 25 s, więc limit odczytu jest długi.
/// EN: Sends a command to the running proxy and returns the reply line. A switch may wait up to
/// 25 s for the engine, so the read limit is long.
///
/// # Errors
/// PL: Proxy nie działa albo nie odpowiedziało. EN: The proxy is not running or did not answer.
pub fn request(home: &Path, command: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket_path(home))?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    writeln!(stream, "{command}")?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    if reply.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "hot-proxy closed the control connection without an answer",
        ));
    }
    Ok(reply.trim_end().to_owned())
}
