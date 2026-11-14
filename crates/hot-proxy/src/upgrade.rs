//! PL: Podmiana programu proxy przy graczach (M13.1, ADR 0033). `hot-proxy upgrade PLIK`:
//! 1. `PLIK --check-resume 1` potwierdza, że zna format stanu; inaczej `HOT-E0609` i nic się nie
//!    zmienia;
//! 2. proxy przestaje przyjmować połączenia (czekają w kolejce jądra) i czeka, aż trwające
//!    logowania się skończą;
//! 3. odczyt od każdego gracza staje między pakietami, a zapis do graczy zostaje wstrzymany;
//! 4. stan graczy idzie do pliku 0600 w `hot-state`, usuniętego od razu po otwarciu;
//! 5. gniazda graczy, nasłuchu, gniazdo sterujące i plik stanu tracą `FD_CLOEXEC`, a proces
//!    wykonuje `execve(PLIK, te same argumenty + --resume-fd N)`: ten sam PID, więc systemd
//!    i kontener niczego nie zauważają;
//! 6. nowy program odtwarza szyfrowanie graczy i loguje ich do silnika od nowa (jak po awarii
//!    silnika), a odpowiedź na `upgrade` pisze już on.
//!
//! Gdy `execve` się nie uda, wszystko rusza dalej w starym programie. Połączeń z silnikiem nie
//! przekazujemy: `execve` je zamyka, a silnik widzi zwykłe wyjście gracza.
//!
//! EN: Swapping the proxy program with players on (M13.1, ADR 0033). `hot-proxy upgrade FILE`:
//! 1. `FILE --check-resume 1` confirms it knows the state format; otherwise `HOT-E0609` and nothing
//!    changes;
//! 2. the proxy stops accepting connections (they wait in the kernel queue) and waits for logins
//!    in progress to finish;
//! 3. reading from every player stops between packets, and writing to the players is held;
//! 4. the players' state goes to a 0600 file in `hot-state`, removed right after it is opened;
//! 5. the player sockets, the listener, the control socket and the state file lose `FD_CLOEXEC`,
//!    and the process runs `execve(FILE, the same arguments + --resume-fd N)`: the same PID, so
//!    systemd and the container notice nothing;
//! 6. the new program rebuilds the players' encryption and logs them in to the engine anew (as
//!    after an engine crash); the new program writes the answer to `upgrade`.
//!
//! When `execve` fails, everything goes on in the old program. Engine connections are not handed
//! over: `execve` closes them, and the engine sees an ordinary player exit.

use crate::codes;
use crate::link::{Link, Parked};
use crate::session::Proxy;
use hot_diag::DiagError;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// PL: Wersja formatu stanu przekazywanego nowemu programowi. EN: The version of the state format
/// handed to the new program.
pub const FORMAT: u32 = 1;

/// PL: Tyle `upgrade` czeka na koniec trwających logowań. EN: How long `upgrade` waits for logins
/// in progress to finish.
const SETTLE: Duration = Duration::from_secs(10);

/// PL: Tyle `upgrade` czeka, aż odczyt każdego gracza stanie. EN: How long `upgrade` waits for
/// every player reader to stop.
const FREEZE: Duration = Duration::from_secs(2);

/// PL: Tyle nowy plik ma na odpowiedź na `--check-resume`. EN: How long the new file has to answer
/// `--check-resume`.
const CHECK: Duration = Duration::from_secs(10);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// PL: Bramka przyjmowania graczy i licznik logowań w toku. EN: The gate for accepting players and
/// the count of logins in progress.
#[derive(Default)]
pub(crate) struct Gate {
    closed: Mutex<bool>,
    joining: Arc<AtomicUsize>,
}

/// PL: Logowanie w toku; liczy się, dopóki proxy nie zna gracza (`Link::run`) albo połączenie
/// się nie skończy. EN: A login in progress; it counts until the proxy knows the player
/// (`Link::run`) or the connection ends.
pub(crate) struct Joining(Arc<AtomicUsize>);

impl Drop for Joining {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) enum Accept {
    Got(TcpStream, Joining),
    /// PL: Nikt nie czeka. EN: Nobody is waiting.
    Nothing,
    /// PL: Trwa `upgrade`. EN: An `upgrade` is running.
    Paused,
    Failed(std::io::Error),
}

impl Gate {
    /// PL: Jedno przyjęcie z gniazda bez czekania (gniazdo nieblokujące). EN: One accept without
    /// waiting (a non-blocking listener).
    pub(crate) fn accept(&self, listener: &TcpListener) -> Accept {
        let closed = lock(&self.closed);
        if *closed {
            return Accept::Paused;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                self.joining.fetch_add(1, Ordering::SeqCst);
                Accept::Got(stream, Joining(Arc::clone(&self.joining)))
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Accept::Nothing,
            Err(e) => Accept::Failed(e),
        }
    }

    fn close(&self) -> Reopen<'_> {
        *lock(&self.closed) = true;
        Reopen(self)
    }
}

/// PL: Otwiera bramkę z powrotem, gdy podmiana się nie udała. EN: Opens the gate again when the
/// upgrade failed.
struct Reopen<'a>(&'a Gate);

impl Drop for Reopen<'_> {
    fn drop(&mut self) {
        *lock(&self.0.closed) = false;
    }
}

/// PL: Czeka do `timeout`, aż na gnieździe `fd` będzie coś do przyjęcia.
/// EN: Waits up to `timeout` for something to accept on the socket `fd`.
pub(crate) fn wait_readable(fd: RawFd, timeout: Duration) {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: jeden poprawny `pollfd` na stosie, długość 1; poll niczego nie zachowuje.
    // EN: one valid `pollfd` on the stack, length 1; poll keeps nothing.
    unsafe { libc::poll(&mut p, 1, ms) };
}

/// PL: Włącza albo wyłącza `FD_CLOEXEC` na deskryptorze. EN: Turns `FD_CLOEXEC` on or off.
pub(crate) fn set_cloexec(fd: RawFd, on: bool) -> std::io::Result<()> {
    // SAFETY: F_GETFD i F_SETFD czytają i ustawiają tylko flagi deskryptora; pamięć nietknięta.
    // EN: F_GETFD and F_SETFD only read and set the descriptor's flags; no memory is touched.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let flags = if on {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: jak wyżej. EN: as above.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// PL: Podmiana programu proxy na `binary`. Po udanej ta funkcja nie wraca (odpowiada nowy
/// program przez `reply`); wraca tylko z odmową `HOT-E0609`, a gracze grają dalej.
/// EN: Swaps the proxy program for `binary`. After a success this function never returns (the new
/// program answers through `reply`); it returns only with a `HOT-E0609` refusal, and the players
/// play on.
pub(crate) fn upgrade(proxy: &Arc<Proxy>, home: &Path, binary: &Path, reply: RawFd) -> Value {
    let (Some(&listener), Some(&control)) = (proxy.listener_fd.get(), proxy.control_fd.get())
    else {
        return refuse(proxy, binary, "the proxy is not serving players yet");
    };
    if let Err(why) = check(binary) {
        return refuse(proxy, binary, &why);
    }
    let _reopen = proxy.gate.close();
    let settle = Instant::now() + SETTLE;
    let links = loop {
        let links: Vec<Arc<Link>> = proxy.live().into_iter().filter(|l| !l.ended()).collect();
        if proxy.gate.joining.load(Ordering::SeqCst) == 0 && links.iter().all(|l| l.in_game()) {
            break links;
        }
        if Instant::now() >= settle {
            return refuse(
                proxy,
                binary,
                "a player was still logging in after 10 s; run upgrade again",
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
    let frozen_at = SystemTime::now();
    for l in &links {
        l.request_freeze();
    }
    let deadline = Instant::now() + FREEZE;
    let parked: Vec<Option<Parked>> = links.iter().map(|l| l.frozen(deadline)).collect();
    if !parked.iter().all(|p| p.as_ref().is_some_and(|p| !p.config)) {
        for (l, p) in links.iter().zip(parked) {
            l.thaw(p);
        }
        return refuse(
            proxy,
            binary,
            "a player did not stop within 2 s or is in configuration; run upgrade again",
        );
    }
    let parked: Vec<Parked> = parked.into_iter().flatten().collect();
    let writers: Vec<_> = links.iter().map(|l| l.writer()).collect();
    let players: Vec<Value> = links
        .iter()
        .zip(&parked)
        .zip(&writers)
        .map(|((l, p), w)| l.snapshot(p, w))
        .collect();
    let mut fds = vec![listener, control, reply];
    fds.extend(parked.iter().map(|p| p.reader.stream().as_raw_fd()));
    let state = json!({
        "format": FORMAT,
        "engine": proxy.engine().to_string(),
        "listener": listener,
        "control": control,
        "reply": reply,
        "frozen_at_ms": unix_ms(frozen_at),
        "players": players,
    });
    proxy.log.info(
        "hot-proxy hands the players over to a new program",
        json!({ "binary": binary.display().to_string(), "players": links.len() }),
    );
    let error = hand_over(home, binary, &state, &fds);
    drop(writers);
    for (l, p) in links.iter().zip(parked) {
        l.thaw(Some(p));
    }
    refuse(proxy, binary, &format!("execve failed: {error}"))
}

/// PL: Nowy plik musi znać format stanu. EN: The new file must know the state format.
fn check(binary: &Path) -> Result<(), String> {
    let mut child = Command::new(binary)
        .args(["--check-resume", &FORMAT.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{} cannot start: {e}", binary.display()))?;
    let deadline = Instant::now() + CHECK;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "{} does not take over state format {FORMAT} ({status}); it is older than this proxy or not hot-proxy",
                    binary.display()
                ))
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not answer --check-resume within 10 s",
                    binary.display()
                ));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// PL: Zapisuje stan i wykonuje `execve`; wraca tylko z błędem. EN: Writes the state and runs
/// `execve`; returns only with an error.
fn hand_over(home: &Path, binary: &Path, state: &Value, fds: &[RawFd]) -> std::io::Error {
    let file = match state_file(home, state) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let mut all = fds.to_vec();
    all.push(file.as_raw_fd());
    let mut args: Vec<OsString> = Vec::new();
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        if a == "--resume-fd" {
            it.next();
        } else {
            args.push(a);
        }
    }
    let error = match all.iter().try_for_each(|fd| set_cloexec(*fd, false)) {
        Ok(()) => Command::new(binary)
            .args(&args)
            .arg("--resume-fd")
            .arg(file.as_raw_fd().to_string())
            .exec(),
        Err(e) => e,
    };
    for fd in &all {
        let _ = set_cloexec(*fd, true);
    }
    error
}

/// PL: Plik stanu: tylko właściciel, bez nazwy na dysku już przed `execve`.
/// EN: The state file: owner only, with no name on disk even before `execve`.
fn state_file(home: &Path, state: &Value) -> std::io::Result<File> {
    let dir = home.join("hot-state");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("hot-proxy-upgrade-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    file.write_all(state.to_string().as_bytes())?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

fn refuse(proxy: &Proxy, binary: &Path, why: &str) -> Value {
    let e = DiagError::new(
        codes::UPGRADE,
        format!("hot-proxy cannot hand the players over to {}: {why}", binary.display()),
        "Read the reason: build the file from the same horizon-hot (or newer) and run hot-proxy upgrade again; until then the running proxy keeps the players",
    )
    .context("binary", binary.display().to_string())
    .state("Nothing changed: the players play on with the running proxy");
    proxy.log.error(&e);
    json!({ "error": e.to_string() })
}

fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// PL: Stan przejęty od starego programu. EN: The state taken over from the old program.
pub struct Resumed {
    pub listener: TcpListener,
    pub control: UnixListener,
    /// PL: Aktywny silnik starego programu. EN: The old program's active engine.
    pub engine: Option<SocketAddr>,
    pub players: Players,
}

/// PL: Gracze do odtworzenia i odpowiedź dla `upgrade`. EN: The players to rebuild and the answer
/// for `upgrade`.
pub struct Players {
    reply: UnixStream,
    list: Vec<Value>,
    frozen_at_ms: u64,
}

/// PL: Czyta stan z `--resume-fd`. EN: Reads the state from `--resume-fd`.
///
/// # Errors
/// PL: Brak pliku albo zły format; gracze są wtedy straceni, a proxy nie startuje.
/// EN: No file or a bad format; the players are lost then, and the proxy does not start.
pub fn read_state(fd: RawFd) -> Result<Resumed, String> {
    if fd <= 2 {
        return Err(format!("--resume-fd {fd} is not a state file"));
    }
    // SAFETY: stary program otworzył ten plik dla nas i podał jego numer w `--resume-fd`; nic
    // innego w tym procesie go nie posiada. EN: the old program opened this file for us and named
    // it in `--resume-fd`; nothing else in this process owns it.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("cannot read the state: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("bad state: {e}"))?;
    if v["format"].as_u64() != Some(u64::from(FORMAT)) {
        return Err(format!("state format {} is not {FORMAT}", v["format"]));
    }
    let fd_of = |key: &str| -> Result<RawFd, String> {
        let fd = v[key]
            .as_i64()
            .and_then(|n| RawFd::try_from(n).ok())
            .filter(|fd| *fd > 2)
            .ok_or_else(|| format!("the state has no {key}"))?;
        set_cloexec(fd, true).map_err(|e| format!("{key}: {e}"))?;
        Ok(fd)
    };
    let (listener, control, reply) = (fd_of("listener")?, fd_of("control")?, fd_of("reply")?);
    // SAFETY: każdy numer to gniazdo, które stary program zostawił otwarte dla nas.
    // EN: each number is a socket the old program left open for us.
    let (listener, control, reply) = unsafe {
        (
            TcpListener::from_raw_fd(listener),
            UnixListener::from_raw_fd(control),
            UnixStream::from_raw_fd(reply),
        )
    };
    Ok(Resumed {
        listener,
        control,
        engine: v["engine"].as_str().and_then(|a| a.parse().ok()),
        players: Players {
            reply,
            list: v["players"].as_array().cloned().unwrap_or_default(),
            frozen_at_ms: v["frozen_at_ms"].as_u64().unwrap_or(0),
        },
    })
}

/// PL: Odtwarza graczy i odpowiada na `upgrade`. EN: Rebuilds the players and answers `upgrade`.
pub fn finish(proxy: &Arc<Proxy>, players: Players) {
    let Players {
        mut reply,
        list,
        frozen_at_ms,
    } = players;
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    for p in &list {
        match Link::restore(proxy, p) {
            Ok(name) => moved.push(name),
            Err(why) => failed.push(json!({ "player": p["name"], "reason": why })),
        }
    }
    let handover_ms = unix_ms(SystemTime::now()).saturating_sub(frozen_at_ms);
    proxy.log.info(
        "hot-proxy took the players over from the previous program",
        json!({ "players": moved, "handover_ms": handover_ms, "pid": std::process::id() }),
    );
    if !failed.is_empty() {
        proxy.log.error(
            &DiagError::new(
                codes::UPGRADE,
                format!("hot-proxy could not take over {} players", failed.len()),
                "The state from the previous program did not fit; these players join again. Report it with hot-diag report",
            )
            .context("failed", Value::Array(failed.clone()).to_string())
            .state("The other players play on"),
        );
    }
    let answer = json!({
        "upgraded": true,
        "pid": std::process::id(),
        "version": crate::VERSION,
        "players": moved.len(),
        "failed": failed,
        "handover_ms": handover_ms,
    });
    let _ = writeln!(reply, "{answer}");
}
