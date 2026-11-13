//! PL: Podmiana silnika pod graczem (M8.2–M8.4) na jednej maszynie: atrapa silnika daje każdemu
//! logowaniu inne id gracza, encję, keep-alive i wiadomość czatu numer 0, a klient z
//! `horizon-protocol` sprawdza, co do niego dochodzi. Bez Mojanga.
//! EN: Swapping the engine under a player (M8.2–M8.4) on one machine: the stand-in engine gives
//! every login another player id, an entity, a keep-alive and chat message number 0, and a client
//! built on `horizon-protocol` checks what reaches it. No Mojang.

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use horizon_protocol::{
    buf::write_varint,
    connection::Connection,
    ids::{configuration as cids, play as pids},
    packets::{configuration as cfg, handshake, login, Empty},
};
use hot_proxy::config::Config;
use hot_proxy::forwarding::CHANNEL;
use hot_proxy::session::Proxy;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SECRET: &[u8] = b"test-secret";
/// PL: Id gracza w n-tym logowaniu do atrapy. EN: The player's id in the stand-in's n-th login.
const PLAYER: i32 = 100;
const ENTITY: i32 = 500;
const KEEP_ALIVE: i64 = 1000;
const BRAND: &[u8] = b"\x04test";

fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, id);
    out.extend_from_slice(payload);
    out
}

fn varint(v: i32) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, v);
    out
}

fn first_varint(p: &[u8]) -> i32 {
    hot_proxy::remap::read_varint(p).unwrap().0
}

/// PL: Pakiet, który dostała atrapa: numer logowania, id, treść.
/// EN: A packet the stand-in got: the login number, the id, the body.
type Got = (usize, i32, Vec<u8>);

struct FakeEngine {
    address: SocketAddr,
    got: Receiver<Got>,
    /// PL: Połączenia z proxy, żeby test mógł je zerwać jak awaria. EN: To break them like a crash.
    streams: Arc<Mutex<Vec<TcpStream>>>,
    /// PL: Opóźnienie przed obsługą logowania numer n. EN: A delay before serving login n.
    delays: Arc<Mutex<Vec<(usize, Duration)>>>,
}

/// PL: Atrapa silnika: przyjmuje logowania bez końca; n-te logowanie ma gracza `PLAYER + n`,
/// encję `ENTITY + n`, keep-alive `KEEP_ALIVE + n` i czat numer 0.
/// EN: A stand-in engine taking logins forever; login n has player `PLAYER + n`, entity
/// `ENTITY + n`, keep-alive `KEEP_ALIVE + n` and chat number 0.
fn fake_engine() -> FakeEngine {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, got) = mpsc::channel();
    let streams = Arc::new(Mutex::new(Vec::new()));
    let delays: Arc<Mutex<Vec<(usize, Duration)>>> = Arc::new(Mutex::new(Vec::new()));
    let (s, d) = (Arc::clone(&streams), Arc::clone(&delays));
    thread::spawn(move || {
        for (n, stream) in listener.incoming().flatten().enumerate() {
            s.lock().unwrap().push(stream.try_clone().unwrap());
            let delay = d
                .lock()
                .unwrap()
                .iter()
                .find(|(i, _)| *i == n)
                .map(|(_, t)| *t);
            let tx = tx.clone();
            thread::spawn(move || {
                if let Some(t) = delay {
                    thread::sleep(t);
                }
                let _ = serve_login(n, stream, &tx);
            });
        }
    });
    FakeEngine {
        address,
        got,
        streams,
        delays,
    }
}

fn serve_login(
    n: usize,
    stream: TcpStream,
    tx: &mpsc::Sender<Got>,
) -> horizon_protocol::Result<()> {
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
                uuid: 1,
                name: "Dev".into(),
                properties: vec![],
            },
            session: 0,
        })
        .encode(),
    )?;
    c.recv()?; // PL: LoginAcknowledged.
               // PL: Konfiguracja: atrapa prosi o markę i kończy. EN: Configuration: asks for the brand and ends.
    c.send(
        &cfg::Clientbound::CustomPayload(cfg::CustomPayload {
            channel: "minecraft:brand".into(),
            data: b"\x04fake".to_vec(),
        })
        .encode(),
    )?;
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
    let mut add = varint(ENTITY + n32);
    add.extend_from_slice(&[0; 16]);
    c.send(&body(pids::clientbound::ADD_ENTITY, &add))?;
    let mut event = (PLAYER + n32).to_be_bytes().to_vec();
    event.push(9);
    c.send(&body(pids::clientbound::ENTITY_EVENT, &event))?;
    c.send(&body(
        pids::clientbound::KEEP_ALIVE,
        &(KEEP_ALIVE + i64::from(n32)).to_be_bytes(),
    ))?;
    // PL: Skutki wejścia na silnik, które proxy ukrywa po podmianie. EN: Join effects hidden after a swap.
    c.send(&body(
        pids::clientbound::SYSTEM_CHAT,
        b"{\"translate\":\"multiplayer.player.joined\"}",
    ))?;
    c.send(&body(pids::clientbound::GAME_EVENT, &[13, 0, 0, 0, 0]))?;
    let mut chat = varint(0);
    chat.extend_from_slice(b"chat");
    c.send(&body(pids::clientbound::PLAYER_CHAT, &chat))?;
    loop {
        let (id, p) = c.recv()?;
        tx.send((n, id, p)).ok();
    }
}

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hot-switch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start_proxy(engine: SocketAddr, home: &std::path::Path) -> (SocketAddr, Arc<Proxy>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = listener.local_addr().unwrap();
    let log = Arc::new(
        hot_diag::Logger::open(
            hot_diag::LogConfig::new("hot-proxy", hot_proxy::VERSION, home.join("logs")),
            hot_diag::Redactor::new(),
        )
        .unwrap(),
    );
    let proxy = Arc::new(
        Proxy::new(
            Config {
                listen,
                engine,
                secret: SECRET.to_vec(),
                online: false,
                discovery: "http://127.0.0.1:9/unused".into(),
            },
            log,
        )
        .unwrap(),
    );
    let p = Arc::clone(&proxy);
    thread::spawn(move || p.serve(&listener));
    (listen, proxy)
}

/// PL: Klient offline aż do gry; zwraca połączenie i swoje id gracza z `Login`.
/// EN: An offline client up to play; returns the connection and its player id from `Login`.
fn join(proxy: SocketAddr) -> (Connection, i32) {
    let stream = TcpStream::connect(proxy).unwrap();
    // PL: Zły proxy nie może zawiesić testu. EN: A broken proxy must not hang the test.
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
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
            name: "Dev".into(),
            uuid: 0,
        })
        .encode(),
    )
    .unwrap();
    loop {
        let (id, p) = c.recv().unwrap();
        match login::Clientbound::decode(id, &p).unwrap() {
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
            data: BRAND.to_vec(),
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
    let player = i32::from_be_bytes(p[0..4].try_into().unwrap());
    (c, player)
}

/// PL: Czyta pakiety klienta do `want` albo do limitu czasu. Zwraca wszystko, co przyszło.
/// EN: Reads the client's packets until `want` or the time limit. Returns everything received.
fn read_until(c: &mut Connection, want: impl Fn(i32, &[u8]) -> bool) -> Vec<(i32, Vec<u8>)> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        let (id, p) = c.recv().unwrap();
        let done = want(id, &p);
        seen.push((id, p));
        if done {
            return seen;
        }
    }
    panic!(
        "timed out; got {:?}",
        seen.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );
}

/// PL: Pakiety od atrapy z logowania n, aż przyjdzie `want`. EN: The stand-in's packets from login n until `want`.
fn engine_until(
    e: &FakeEngine,
    n: usize,
    want: impl Fn(i32, &[u8]) -> bool,
) -> Vec<(i32, Vec<u8>)> {
    let mut seen = Vec::new();
    loop {
        let (from, id, p) = e
            .got
            .recv_timeout(Duration::from_secs(20))
            .expect("the engine got it");
        if from != n {
            continue;
        }
        let done = want(id, &p);
        seen.push((id, p));
        if done {
            return seen;
        }
    }
}

/// PL: Pierwsze wejście: encja, zdarzenie gracza, keep-alive i czat numer 0 od logowania 0.
/// EN: The first join: the entity, the player event, the keep-alive and chat 0 from login 0.
fn first_join(e: &FakeEngine, proxy: SocketAddr) -> Connection {
    let (mut c, player) = join(proxy);
    assert_eq!(player, PLAYER);
    let seen = read_until(&mut c, |id, _| id == pids::clientbound::PLAYER_CHAT);
    assert!(seen.contains(&(
        pids::clientbound::KEEP_ALIVE,
        KEEP_ALIVE.to_be_bytes().to_vec()
    )));
    // PL: Keep-alive zostaje bez odpowiedzi: po podmianie będzie nieaktualny.
    // EN: The keep-alive stays unanswered: after a swap it is stale.
    let mut command = varint(PLAYER);
    command.extend_from_slice(&[1, 0]);
    c.send(&body(pids::serverbound::PLAYER_COMMAND, &command))
        .unwrap();
    engine_until(e, 0, |id, _| id == pids::serverbound::PLAYER_COMMAND);
    c
}

#[test]
fn a_switch_moves_the_player_without_a_loading_screen() {
    let e = fake_engine();
    let dir = home("same");
    let (at, proxy) = start_proxy(e.address, &dir);
    let mut c = first_join(&e, at);
    for n in 1..=3usize {
        let n32 = i32::try_from(n).unwrap();
        let p = Arc::clone(&proxy);
        let summary = thread::spawn(move || p.switch(None));
        let seen = read_until(&mut c, |id, _| id == pids::clientbound::PLAYER_CHAT);
        let summary = summary.join().unwrap();
        assert_eq!(summary["moved"], 1, "{summary}");
        assert_eq!(summary["failed"].as_array().unwrap().len(), 0, "{summary}");
        let ids: Vec<i32> = seen.iter().map(|(id, _)| *id).collect();
        for screen in [
            pids::clientbound::LOGIN,
            pids::clientbound::RESPAWN,
            pids::clientbound::START_CONFIGURATION,
            pids::clientbound::SYSTEM_CHAT,
            pids::clientbound::GAME_EVENT,
        ] {
            assert!(
                !ids.contains(&screen),
                "packet {screen} is a loading screen or a join effect: {ids:?}"
            );
        }
        // PL: Encja poprzedniego silnika znika, zanim przyjdzie nowa. EN: The old entity goes first.
        let removed = seen
            .iter()
            .position(|(id, _)| *id == pids::clientbound::REMOVE_ENTITIES)
            .expect("RemoveEntities");
        let removed_ids = hot_proxy::remap::removed_entities(&seen[removed].1);
        assert_eq!(
            removed_ids,
            [ENTITY + n32 - 1],
            "only the old entity, never the player"
        );
        let added = seen
            .iter()
            .position(|(id, _)| *id == pids::clientbound::ADD_ENTITY)
            .expect("AddEntity");
        assert!(removed < added);
        assert_eq!(first_varint(&seen[added].1), ENTITY + n32);
        // PL: Zdarzenie gracza z nowym id silnika dochodzi z id klienta.
        // EN: The player's event with the new engine id arrives with the client's id.
        let (_, event) = seen
            .iter()
            .find(|(id, _)| *id == pids::clientbound::ENTITY_EVENT)
            .unwrap();
        assert_eq!(event[0..4], PLAYER.to_be_bytes());
        // PL: Czat numer 0 nowego silnika to dla klienta kolejny numer. EN: Chat 0 continues the count.
        let (_, chat) = seen.last().unwrap();
        assert_eq!(first_varint(chat), n32);
        // PL: Odpowiedź na stary keep-alive przepada, na nowy dochodzi; komenda gracza dostaje id silnika.
        // EN: The stale keep-alive answer is dropped, the new one gets through; the player command
        // gets the engine's id.
        c.send(&body(
            pids::serverbound::KEEP_ALIVE,
            &(KEEP_ALIVE + i64::from(n32) - 1).to_be_bytes(),
        ))
        .unwrap();
        c.send(&body(
            pids::serverbound::KEEP_ALIVE,
            &(KEEP_ALIVE + i64::from(n32)).to_be_bytes(),
        ))
        .unwrap();
        let mut command = varint(PLAYER);
        command.extend_from_slice(&[1, 0]);
        c.send(&body(pids::serverbound::PLAYER_COMMAND, &command))
            .unwrap();
        let got = engine_until(&e, n, |id, _| id == pids::serverbound::PLAYER_COMMAND);
        let alive: Vec<i64> = got
            .iter()
            .filter(|(id, _)| *id == pids::serverbound::KEEP_ALIVE)
            .map(|(_, p)| i64::from_be_bytes(p[0..8].try_into().unwrap()))
            .collect();
        assert_eq!(alive, [KEEP_ALIVE + i64::from(n32)]);
        assert_eq!(first_varint(&got.last().unwrap().1), PLAYER + n32);
        // PL: Nowy silnik dostał markę klienta z jego konfiguracji. EN: The new engine got the client's brand.
        let config = engine_until_seen(&got);
        assert!(config, "the brand replay");
    }
    let status = proxy.status_json();
    assert_eq!(status["players"], serde_json::json!(["Dev"]));
}

/// PL: Marka klienta jest w pakietach konfiguracji, które atrapa dostała przed grą; `engine_until`
/// zbiera tylko jedno logowanie, więc szukamy w tym samym zbiorze.
/// EN: The client's brand is among the configuration packets the stand-in got before play;
/// `engine_until` collects one login only, so we look in that set.
fn engine_until_seen(got: &[(i32, Vec<u8>)]) -> bool {
    got.iter()
        .any(|(id, p)| *id == cids::serverbound::CUSTOM_PAYLOAD && p.ends_with(BRAND))
}

#[test]
fn a_crashed_engine_holds_the_player_with_proxy_keep_alives() {
    let e = fake_engine();
    // PL: Nowy silnik wstaje dopiero po 6 s. EN: The new engine comes up only after 6 s.
    e.delays.lock().unwrap().push((1, Duration::from_secs(6)));
    let dir = home("crash");
    let (at, _proxy) = start_proxy(e.address, &dir);
    let mut c = first_join(&e, at);
    // PL: Awaria: silnik zrywa połączenie bez `Disconnect`. EN: A crash: no `Disconnect`.
    e.streams.lock().unwrap()[0]
        .shutdown(Shutdown::Both)
        .unwrap();
    let started = Instant::now();
    let seen = read_until(&mut c, |id, p| {
        id == pids::clientbound::KEEP_ALIVE && p[0] == 0x68
    });
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "the first proxy keep-alive comes after ~5 s"
    );
    let (_, own) = seen.last().unwrap();
    c.send(&body(pids::serverbound::KEEP_ALIVE, own)).unwrap();
    let seen = read_until(&mut c, |id, _| id == pids::clientbound::PLAYER_CHAT);
    assert!(!seen.iter().any(|(id, _)| *id == pids::clientbound::LOGIN));
    c.send(&body(
        pids::serverbound::KEEP_ALIVE,
        &(KEEP_ALIVE + 1).to_be_bytes(),
    ))
    .unwrap();
    let got = engine_until(&e, 1, |id, _| id == pids::serverbound::KEEP_ALIVE);
    let (_, answer) = got.last().unwrap();
    assert_eq!(
        answer[..],
        (KEEP_ALIVE + 1).to_be_bytes(),
        "the proxy's own keep-alive answer never reaches the engine"
    );
}

#[test]
fn the_control_socket_switches_and_reports() {
    let e = fake_engine();
    let other = fake_engine();
    let dir = home("control");
    let (at, proxy) = start_proxy(e.address, &dir);
    hot_proxy::control::serve(&proxy, &dir).unwrap();
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(hot_proxy::control::socket_path(&dir))
            .unwrap()
            .permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);
    let mut c = first_join(&e, at);
    let d = dir.clone();
    let target = other.address;
    let reply = thread::spawn(move || hot_proxy::control::request(&d, &format!("switch {target}")));
    let seen = read_until(&mut c, |id, _| id == pids::clientbound::PLAYER_CHAT);
    // PL: Inny silnik, logowanie 0: gracz tam ma id `PLAYER`, jak u klienta.
    // EN: Another engine, login 0: the player has id `PLAYER` there, as on the client.
    assert!(seen
        .iter()
        .any(|(id, p)| *id == pids::clientbound::ADD_ENTITY && first_varint(p) == ENTITY));
    let reply: serde_json::Value = serde_json::from_str(&reply.join().unwrap().unwrap()).unwrap();
    assert_eq!(reply["engine"], target.to_string());
    assert_eq!(reply["moved"], 1);
    let status: serde_json::Value =
        serde_json::from_str(&hot_proxy::control::request(&dir, "status").unwrap()).unwrap();
    assert_eq!(status["engine"], target.to_string());
    let bad: serde_json::Value =
        serde_json::from_str(&hot_proxy::control::request(&dir, "switch nowhere").unwrap())
            .unwrap();
    assert!(bad["error"].is_string());
    // PL: Drugie proxy z tym samym HOT_HOME nie przejmie gniazda. EN: A second proxy cannot take the socket.
    assert!(hot_proxy::control::serve(&proxy, &dir).is_err());
}

#[test]
fn a_switch_to_a_dead_engine_disconnects_with_a_reason_and_e0605() {
    let e = fake_engine();
    let dir = home("dead");
    let (at, proxy) = start_proxy(e.address, &dir);
    let mut c = first_join(&e, at);
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    // PL: Pod tym adresem nikt nie słucha; 2 s zamiast 25 s. EN: Nothing listens there; 2 s, not 25 s.
    let summary = proxy.switch_within(Some(closed), Duration::from_secs(2));
    assert_eq!(summary["moved"], 0, "{summary}");
    let seen = read_until(&mut c, |id, _| id == pids::clientbound::DISCONNECT);
    let (_, reason) = seen.last().unwrap();
    assert!(String::from_utf8_lossy(reason).contains("restarting"));
    let log = std::fs::read_to_string(dir.join("logs/hot-proxy.jsonl")).unwrap();
    assert!(log.contains("HOT-E0605"), "{log}");
}
