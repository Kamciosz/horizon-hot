//! PL: Podmiana programu proxy przy graczu (M13.1, ADR 0033) na prawdziwym pliku `hot-proxy`:
//! gracz z szyfrowaniem (atrapa usługi sesji), atrapa silnika przyjmująca logowania bez końca,
//! pół ramki w locie w chwili podmiany. Sprawdza ten sam PID, brak rozłączenia i odmowę
//! `HOT-E0609` dla pliku, który stanu nie przejmie. Bez Mojanga.
//! EN: Swapping the proxy program with a player on (M13.1, ADR 0033) on the real `hot-proxy` file:
//! an encrypted player (a stand-in session service), a stand-in engine taking logins forever, half
//! a frame in flight at the moment of the swap. Checks the same PID, no disconnect, and the
//! `HOT-E0609` refusal for a file that cannot take the state over. No Mojang.

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use horizon_protocol::{
    buf::write_varint,
    connection::Connection,
    crypto::generate_secret,
    frame,
    ids::{configuration as cids, play as pids},
    packets::{configuration as cfg, handshake, login, Empty},
    rsa::RsaPublicKey,
};
use hot_proxy::forwarding::CHANNEL;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hot-proxy");
/// PL: Id gracza i keep-alive w n-tym logowaniu do atrapy. EN: The player id and keep-alive in the
/// stand-in's n-th login.
const PLAYER: i32 = 100;
const KEEP_ALIVE: i64 = 1000;
const TESTER: u128 = 0x069a_79f4_44e9_4726_a5be_fca9_0e38_aaf5;

fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, id);
    out.extend_from_slice(payload);
    out
}

/// PL: `PlayerCommand` gracza `player` (id encji na początku). EN: A `PlayerCommand` of `player`
/// (the entity id first).
fn command(player: i32) -> Vec<u8> {
    action(player, 1)
}

fn action(player: i32, action: u8) -> Vec<u8> {
    let mut p = Vec::new();
    write_varint(&mut p, player);
    p.extend_from_slice(&[action, 0]);
    body(pids::serverbound::PLAYER_COMMAND, &p)
}

/// PL: Atrapa usługi sesji, która zna tylko gracza „Tester”. EN: A stand-in session service that
/// knows only "Tester".
fn session_service() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let path = String::from_utf8_lossy(&request)
                .split(' ')
                .nth(1)
                .unwrap_or("")
                .to_owned();
            let text = if path.starts_with("/discovery") {
                format!(
                    r#"{{"discovery":{{"session":{{"endpoints":{{"verify":{{"uri":"http://127.0.0.1:{port}/verify"}}}}}}}}}}"#
                )
            } else {
                format!(r#"{{"id":"{TESTER:032x}","name":"Tester","properties":[]}}"#)
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}/discovery")
}

/// PL: Pakiet, który dostała atrapa: numer logowania, id, treść.
/// EN: A packet the stand-in got: the login number, the id, the body.
type Got = (usize, i32, Vec<u8>);

/// PL: Atrapa silnika: n-te logowanie ma gracza `PLAYER + n` i keep-alive `KEEP_ALIVE + n`.
/// `PlayerCommand` z akcją [`FLOOD`] zalewa gracza danymi bez kompresji i po każdym pakiecie
/// zgłasza id [`FLOODED`].
/// EN: A stand-in engine: login n has player `PLAYER + n` and keep-alive `KEEP_ALIVE + n`.
/// A `PlayerCommand` with action [`FLOOD`] floods the player with data that does not compress and
/// reports id [`FLOODED`] after each packet.
fn fake_engine() -> (SocketAddr, Receiver<Got>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, got) = mpsc::channel();
    thread::spawn(move || {
        for (n, stream) in listener.incoming().flatten().enumerate() {
            let tx = tx.clone();
            thread::spawn(move || {
                let _ = serve_login(n, stream, &tx);
            });
        }
    });
    (address, got)
}

const FLOOD: u8 = 9;
const FLOODED: i32 = -1;

fn flood(n: usize, c: &mut Connection, tx: &Sender<Got>) -> horizon_protocol::Result<()> {
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    let noise: Vec<u8> = (0..64 * 1024)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x.to_le_bytes()[0]
        })
        .collect();
    for _ in 0..1024 {
        c.send(&body(pids::clientbound::SET_TIME, &noise))?;
        tx.send((n, FLOODED, Vec::new())).ok();
    }
    Ok(())
}

fn serve_login(n: usize, stream: TcpStream, tx: &Sender<Got>) -> horizon_protocol::Result<()> {
    let n32 = i32::try_from(n).unwrap();
    let mut c = Connection::from_stream(stream)?;
    c.recv()?; // PL: powitanie. EN: handshake.
    c.recv()?; // PL: Hello.
    c.send(
        &login::Clientbound::CustomQuery(login::CustomQuery {
            transaction: 1,
            channel: CHANNEL.into(),
            data: vec![1],
        })
        .encode(),
    )?;
    c.recv()?;
    c.send(
        &login::Clientbound::LoginFinished(login::LoginFinished {
            profile: login::GameProfile {
                uuid: TESTER,
                name: "Tester".into(),
                properties: vec![],
            },
            session: 0,
        })
        .encode(),
    )?;
    c.recv()?; // PL: LoginAcknowledged.
    c.send(&cfg::Clientbound::FinishConfiguration(Empty).encode())?;
    loop {
        let (id, p) = c.recv()?;
        tx.send((n, id, p)).ok();
        if id == cids::serverbound::FINISH_CONFIGURATION {
            break;
        }
    }
    let mut login = (PLAYER + n32).to_be_bytes().to_vec();
    login.extend_from_slice(b"rest of Login");
    c.send(&body(pids::clientbound::LOGIN, &login))?;
    c.send(&body(
        pids::clientbound::KEEP_ALIVE,
        &(KEEP_ALIVE + i64::from(n32)).to_be_bytes(),
    ))?;
    loop {
        let (id, p) = c.recv()?;
        if id == pids::serverbound::PLAYER_COMMAND && p.ends_with(&[FLOOD, 0]) {
            flood(n, &mut c, tx)?;
        }
        tx.send((n, id, p)).ok();
    }
}

/// PL: Działający `hot-proxy`; test go zabija na końcu. EN: A running `hot-proxy`; killed at the end.
struct Running {
    child: Child,
    home: PathBuf,
    listen: SocketAddr,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(engine: SocketAddr, discovery: &str, name: &str) -> Running {
    let home = std::env::temp_dir().join(format!("hot-up-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let secret = home.join("forwarding.secret");
    std::fs::write(&secret, "test-secret").unwrap();
    let listen = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let child = Command::new(BIN)
        .arg("--engine")
        .arg(engine.to_string())
        .arg("--secret")
        .arg(&secret)
        .arg("--home")
        .arg(&home)
        .arg("--listen")
        .arg(listen.to_string())
        .arg("--discovery")
        .arg(discovery)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let running = Running {
        child,
        home,
        listen,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while hot_proxy::control::request(&running.home, "status").is_err() {
        assert!(Instant::now() < deadline, "hot-proxy did not start");
        thread::sleep(Duration::from_millis(20));
    }
    running
}

/// PL: Gracz „Tester” z szyfrowaniem aż do gry. Zwraca połączenie, surowe gniazdo (do pół ramki)
/// i id gracza z `Login`. EN: The player "Tester" with encryption up to play. Returns the
/// connection, the raw socket (for half a frame) and the player id from `Login`.
fn join(proxy: SocketAddr) -> (Connection, TcpStream, i32) {
    let stream = TcpStream::connect(proxy).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let raw = stream.try_clone().unwrap();
    let mut c = Connection::from_stream(stream).unwrap();
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: horizon_protocol::PROTOCOL_VERSION,
            host: "localhost".into(),
            port: proxy.port(),
            intent: handshake::LOGIN,
        })
        .encode(),
    )
    .unwrap();
    c.send(
        &login::Serverbound::Hello(login::ServerHello {
            name: "Tester".into(),
            uuid: 0,
        })
        .encode(),
    )
    .unwrap();
    loop {
        let (id, p) = c.recv().unwrap();
        match login::Clientbound::decode(id, &p).unwrap() {
            login::Clientbound::Hello(hello) => {
                let key = RsaPublicKey::from_der(&hello.public_key).unwrap();
                let secret = generate_secret().unwrap();
                c.send(
                    &login::Serverbound::Key(login::Key {
                        key: key.encrypt(&secret).unwrap(),
                        challenge: key.encrypt(&hello.challenge).unwrap(),
                    })
                    .encode(),
                )
                .unwrap();
                c.enable_encryption(&secret);
            }
            login::Clientbound::Compression(x) => c.set_compression(x.threshold),
            login::Clientbound::LoginFinished(_) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())
        .unwrap();
    c.send(
        &cfg::Serverbound::CustomPayload(cfg::CustomPayload {
            channel: "minecraft:brand".into(),
            data: b"\x04test".to_vec(),
        })
        .encode(),
    )
    .unwrap();
    loop {
        let (id, _) = c.recv().unwrap();
        if id == cids::clientbound::FINISH_CONFIGURATION {
            break;
        }
    }
    c.send(&cfg::Serverbound::FinishConfiguration(Empty).encode())
        .unwrap();
    let (id, p) = c.recv().unwrap();
    assert_eq!(id, pids::clientbound::LOGIN);
    (c, raw, i32::from_be_bytes(p[0..4].try_into().unwrap()))
}

/// PL: Pakiety do gracza aż do keep-alive `id`. EN: The player's packets up to keep-alive `id`.
fn until_keep_alive(c: &mut Connection, id: i64) -> Vec<i32> {
    let mut seen = Vec::new();
    loop {
        let (got, p) = c.recv().unwrap();
        seen.push(got);
        if got == pids::clientbound::KEEP_ALIVE && p == id.to_be_bytes() {
            return seen;
        }
    }
}

/// PL: Pakiety od atrapy z logowania n, aż przyjdzie `want`. EN: The stand-in's packets from login
/// n until `want`.
fn engine_until(got: &Receiver<Got>, n: usize, want: i32) -> Vec<u8> {
    loop {
        let (from, id, p) = got
            .recv_timeout(Duration::from_secs(20))
            .expect("the engine got it");
        if from == n && id == want {
            return p;
        }
    }
}

fn upgrade(home: &Path, file: &str) -> (bool, serde_json::Value) {
    let out = Command::new(BIN)
        .arg("upgrade")
        .arg(file)
        .arg("--home")
        .arg(home)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let answer = serde_json::from_str(text.trim()).unwrap_or(serde_json::Value::String(text));
    (out.status.success(), answer)
}

fn log(home: &Path) -> String {
    std::fs::read_to_string(home.join("logs/hot-proxy.jsonl")).unwrap_or_default()
}

#[test]
fn an_upgrade_keeps_the_encrypted_player_and_the_pid() {
    let discovery = session_service();
    let (engine, got) = fake_engine();
    let mut proxy = start(engine, &discovery, "keep");
    let (mut c, mut raw, player) = join(proxy.listen);
    assert_eq!(player, PLAYER);
    until_keep_alive(&mut c, KEEP_ALIVE);
    c.send(&command(PLAYER)).unwrap();
    engine_until(&got, 0, pids::serverbound::PLAYER_COMMAND);

    // PL: Pół ramki leci do starego programu; reszta dopiero do nowego.
    // EN: Half a frame goes to the old program; the rest only to the new one.
    let threshold = usize::try_from(hot_proxy::config::COMPRESSION).unwrap();
    let contents = frame::contents(&command(PLAYER), Some(threshold)).unwrap();
    let mut framed = Vec::new();
    c.writer.frame_into(&contents, &mut framed);
    let half = framed.len() / 2;
    raw.write_all(&framed[..half]).unwrap();
    thread::sleep(Duration::from_millis(100));

    let (ok, answer) = upgrade(&proxy.home, BIN);
    assert!(ok, "{answer}");
    assert_eq!(answer["upgraded"], true, "{answer}");
    assert_eq!(answer["players"], 1, "{answer}");
    assert_eq!(
        answer["pid"],
        proxy.child.id(),
        "the same process: {answer}"
    );
    assert!(proxy.child.try_wait().unwrap().is_none(), "still running");

    // PL: Nowy program loguje gracza do silnika od nowa; gracz dostaje jego keep-alive bez
    // ponownego `Login` i bez konfiguracji. EN: The new program logs the player in to the engine
    // anew; the player gets its keep-alive with no new `Login` and no configuration.
    let seen = until_keep_alive(&mut c, KEEP_ALIVE + 1);
    assert!(!seen.contains(&pids::clientbound::LOGIN), "{seen:?}");
    assert!(
        !seen.contains(&pids::clientbound::START_CONFIGURATION),
        "{seen:?}"
    );
    assert!(!seen.contains(&pids::clientbound::DISCONNECT), "{seen:?}");

    raw.write_all(&framed[half..]).unwrap();
    let p = engine_until(&got, 1, pids::serverbound::PLAYER_COMMAND);
    assert_eq!(
        hot_proxy::remap::read_varint(&p).unwrap().0,
        PLAYER + 1,
        "the half frame arrived whole, with the new engine's player id"
    );
    c.send(&command(PLAYER)).unwrap();
    engine_until(&got, 1, pids::serverbound::PLAYER_COMMAND);

    let text = log(&proxy.home);
    assert!(text.contains("took the players over"), "{text}");
    assert!(text.contains("handover_ms"), "{text}");
    assert!(!text.contains("HOT-E06"), "{text}");
}

#[test]
fn a_file_that_cannot_take_over_is_refused_and_the_player_stays() {
    let discovery = session_service();
    let (engine, got) = fake_engine();
    let mut proxy = start(engine, &discovery, "refuse");
    let (mut c, _raw, _) = join(proxy.listen);
    until_keep_alive(&mut c, KEEP_ALIVE);

    let (ok, answer) = upgrade(&proxy.home, "/usr/bin/false");
    assert!(!ok, "{answer}");
    assert!(
        answer["error"].to_string().contains("HOT-E0609"),
        "{answer}"
    );
    assert!(log(&proxy.home).contains("HOT-E0609"));
    assert!(proxy.child.try_wait().unwrap().is_none(), "still running");

    // PL: Gracz gra dalej na tym samym połączeniu z silnikiem, a nowi gracze wchodzą.
    // EN: The player plays on over the same engine connection, and new players get in.
    c.send(&command(PLAYER)).unwrap();
    engine_until(&got, 0, pids::serverbound::PLAYER_COMMAND);
    let (mut second, _raw, player) = join(proxy.listen);
    assert_eq!(player, PLAYER + 1);
    second.send(&command(player)).unwrap();
    engine_until(&got, 1, pids::serverbound::PLAYER_COMMAND);
}

#[test]
fn a_player_who_stopped_reading_pauses_the_others_under_a_second_then_is_dropped() {
    let discovery = session_service();
    let (engine, got) = fake_engine();
    let mut proxy = start(engine, &discovery, "stuck");
    let (mut stuck, _raw, player) = join(proxy.listen);
    until_keep_alive(&mut stuck, KEEP_ALIVE);
    let (mut other, _raw2, other_id) = join(proxy.listen);
    until_keep_alive(&mut other, KEEP_ALIVE + 1);

    // PL: Gracz przestaje czytać, a silnik go zalewa; czekamy, aż zapis proxy do niego stanie.
    // EN: The player stops reading and the engine floods him; wait until the proxy's write to him
    // hangs.
    stuck.send(&action(player, FLOOD)).unwrap();
    let mut sent = 0;
    loop {
        match got.recv_timeout(Duration::from_secs(1)) {
            Ok((0, FLOODED, _)) => sent += 1,
            Ok(_) => {}
            Err(_) if sent > 0 => break,
            Err(e) => panic!("the flood did not start: {e}"),
        }
    }
    assert!(sent < 1024, "the flood never hung ({sent} packets)");

    // PL: Drugi gracz wysyła polecenie co 20 ms, a test mierzy najdłuższą przerwę między nimi po
    // stronie silnika: to czas, przez który podmiana go trzymała.
    // EN: The other player sends a command every 20 ms and the test measures the longest gap
    // between them at the engine: the time the upgrade held him.
    let stop = Arc::new(AtomicBool::new(false));
    let sender = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                other.send(&command(other_id)).unwrap();
                thread::sleep(Duration::from_millis(20));
            }
            other
        })
    };
    let watcher = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let (mut last, mut longest) = (None::<Instant>, Duration::ZERO);
            loop {
                match got.recv_timeout(Duration::from_millis(100)) {
                    Ok((1, id, _)) if id == pids::serverbound::PLAYER_COMMAND => {
                        let now = Instant::now();
                        if let Some(last) = last {
                            longest = longest.max(now - last);
                        }
                        last = Some(now);
                    }
                    Ok(_) => {}
                    Err(_) if stop.load(Ordering::SeqCst) => return (got, longest),
                    Err(_) => {}
                }
            }
        })
    };
    thread::sleep(Duration::from_millis(300));
    let (ok, answer) = upgrade(&proxy.home, BIN);
    thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::SeqCst);
    let mut other = sender.join().unwrap();
    let (got, longest) = watcher.join().unwrap();
    assert!(!ok, "{answer}");
    assert!(
        answer["error"].to_string().contains("did not take writes"),
        "{answer}"
    );
    assert!(longest < Duration::from_secs(1), "paused {longest:?}");
    assert!(proxy.child.try_wait().unwrap().is_none(), "still running");

    // PL: Po terminie zapisu (15–30 s bez postępu) proxy rozłącza zawieszonego gracza i podmiana
    // przechodzi. EN: After the write limit (15–30 s without progress) the proxy drops the hung
    // player and the upgrade goes through.
    let deadline = Instant::now() + Duration::from_secs(60);
    while players(&proxy.home) != 1 {
        assert!(
            Instant::now() < deadline,
            "the hung player was never dropped"
        );
        thread::sleep(Duration::from_millis(200));
    }
    let (ok, answer) = upgrade(&proxy.home, BIN);
    assert!(ok, "{answer}");
    assert_eq!(answer["players"], 1, "{answer}");
    // PL: Nowy program loguje gracza do silnika od nowa: to trzecie logowanie atrapy. Pakiety
    // gracza z przerwy przepadają (D13), więc polecenie idzie po keep-alive nowego silnika.
    // EN: The new program logs the player into the engine again: the stand-in's third login. The
    // player's packets from the gap are dropped (D13), so the command goes after the new engine's
    // keep-alive.
    until_keep_alive(&mut other, KEEP_ALIVE + 2);
    other.send(&command(other_id)).unwrap();
    engine_until(&got, 2, pids::serverbound::PLAYER_COMMAND);
}

fn players(home: &Path) -> usize {
    let status = hot_proxy::control::request(home, "status").unwrap();
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    status["players"].as_array().map_or(0, Vec::len)
}
