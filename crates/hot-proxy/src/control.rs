//! PL: Gniazdo sterujące `<home>/hot-proxy.sock` (Unix, tylko właściciel, 0600). Jedna komenda na
//! połączenie, odpowiedź to linie JSON-a, ostatnia jest wynikiem:
//! - `status` — aktywny silnik i gracze w grze;
//! - `switch [ADRES]` — przenosi graczy na silnik pod `ADRES` (bez adresu: na ten sam). Najpierw
//!   linia `{"held":N}`: od niej stary silnik nic już graczom nie wyśle, więc wolno go zatrzymać;
//! - `engine ADRES` — tylko zmienia adres; gracze w przerwie logują się już pod nowy.
//!
//! Woła je `hot-proxy switch|status` i `hotd` przy podmianie silnika (D11).
//!
//! EN: The control socket `<home>/hot-proxy.sock` (Unix, owner only, 0600). One command per
//! connection; the reply is JSON lines, the last one is the result:
//! - `status` — the active engine and the players in game;
//! - `switch [ADDR]` — moves the players to the engine at `ADDR` (no address: the same one).
//!   First a `{"held":N}` line: from then on the old engine sends the players nothing, so it may
//!   stop;
//! - `engine ADDR` — only changes the address; players in a gap log in at the new one.
//!
//! `hot-proxy switch|status` and `hotd` (when it swaps the engine, D11) call it.

use crate::session::{Proxy, SWITCH_WAIT};
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
                proxy.switch_reporting(target, SWITCH_WAIT, |players| {
                    let _ = writeln!(stream, "{}", json!({ "held": players }));
                })
            }
            Err(_) => json!({ "error": "switch needs an address like 127.0.0.1:25566" }),
        },
        (Some("engine"), Some(target), None) => match target.parse::<SocketAddr>() {
            Ok(target) => {
                proxy.set_engine(target);
                json!({ "engine": target.to_string() })
            }
            Err(_) => json!({ "error": "engine needs an address like 127.0.0.1:25566" }),
        },
        _ => {
            json!({ "error": format!("unknown command {:?}; use status, switch [ADDR] or engine ADDR", line.trim()) })
        }
    };
    let _ = writeln!(stream, "{reply}");
}

/// PL: Adres aktywnego silnika z `<home>/hot-state/engine.json` (zapisuje go `hotd` po podmianie
/// silnika); `None`, gdy pliku nie ma albo nie ma w nim adresu.
/// EN: The active engine address from `<home>/hot-state/engine.json` (`hotd` writes it after an
/// engine swap); `None` when the file or the address is missing.
#[must_use]
pub fn active_engine(home: &Path) -> Option<SocketAddr> {
    let text = std::fs::read_to_string(home.join("hot-state").join("engine.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["address"].as_str()?.parse().ok()
}

/// PL: Wysyła komendę do działającego proxy i zwraca ostatnią linię odpowiedzi (wynik). Podmiana
/// może czekać na silnik do 25 s, więc limit odczytu jest długi.
/// EN: Sends a command to the running proxy and returns the last reply line (the result). A switch
/// may wait up to 25 s for the engine, so the read limit is long.
///
/// # Errors
/// PL: Proxy nie działa albo nie odpowiedziało. EN: The proxy is not running or did not answer.
pub fn request(home: &Path, command: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket_path(home))?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    writeln!(stream, "{command}")?;
    let mut last = None;
    for line in BufReader::new(&stream).lines() {
        last = Some(line?);
    }
    last.map(|l| l.trim_end().to_owned()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "hot-proxy closed the control connection without an answer",
        )
    })
}
