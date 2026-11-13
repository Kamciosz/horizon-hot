//! PL: Logowanie przez proxy na jednej maszynie: atrapa usługi sesji (HTTP), atrapa silnika, która
//! sprawdza podpis jak silnik Horizona, i klient zbudowany na `horizon-protocol`. Bez Mojanga.
//! EN: Login through the proxy on one machine: a stand-in session service (HTTP), a stand-in engine
//! that checks the signature as Horizon's engine does, and a client built on `horizon-protocol`.
//! No Mojang.

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use horizon_protocol::{
    buf::{write_varint, Reader},
    connection::Connection,
    crypto::{generate_secret, offline_uuid, server_hash},
    ids,
    packets::{
        handshake,
        login::{self, Property},
        Empty,
    },
    rsa::RsaPublicKey,
};
use hot_proxy::config::Config;
use hot_proxy::forwarding::{hmac_sha256, CHANNEL};
use hot_proxy::session::Proxy;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

const SECRET: &[u8] = b"test-secret";
const TESTER: u128 = 0x069a_79f4_44e9_4726_a5be_fca9_0e38_aaf5;

fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, id);
    out.extend_from_slice(payload);
    out
}

/// PL: Atrapa usługi sesji: zna tylko gracza „Tester”. Zapisuje każde `serverId`.
/// EN: A stand-in session service that knows only "Tester". Records every `serverId`.
fn session_service() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_by_server = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request).into_owned();
            let path = request.split(' ').nth(1).unwrap_or("").to_owned();
            let (status, text) = if path.starts_with("/discovery") {
                (
                    "200 OK",
                    format!(
                        r#"{{"discovery":{{"session":{{"endpoints":{{"verify":{{"uri":"http://127.0.0.1:{port}/verify"}}}}}}}}}}"#
                    ),
                )
            } else {
                let query = path.split_once('?').map_or("", |(_, q)| q);
                let field = |name: &str| {
                    query
                        .split('&')
                        .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
                        .unwrap_or("")
                        .to_owned()
                };
                seen_by_server.lock().unwrap().push(field("serverId"));
                if field("username") == "Tester" {
                    (
                        "200 OK",
                        format!(
                            r#"{{"id":"{TESTER:032x}","name":"Tester","properties":[{{"name":"textures","value":"skin","signature":"sig"}}]}}"#
                        ),
                    )
                } else {
                    ("204 No Content", String::new())
                }
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (format!("http://127.0.0.1:{port}/discovery"), seen)
}

/// PL: Co atrapa silnika dostała od proxy. EN: What the stand-in engine got from the proxy.
#[derive(Debug)]
struct Forwarded {
    address: String,
    uuid: u128,
    name: String,
    properties: Vec<Property>,
    from_client: (i32, Vec<u8>),
}

/// PL: Atrapa silnika: z sekretem pyta o dane gracza i sprawdza podpis jak Horizon; bez sekretu
/// wpuszcza od razu (silnik bez `forwarding.secret`).
/// EN: A stand-in engine: with a secret it asks for the player's data and checks the signature as
/// Horizon does; without one it lets the player in at once (an engine without `forwarding.secret`).
fn engine(secret: Option<&'static [u8]>) -> (SocketAddr, mpsc::Receiver<Forwarded>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut c = Connection::from_stream(stream).unwrap();
        let (id, p) = c.recv().unwrap();
        let handshake::Serverbound::Intention(intent) =
            handshake::Serverbound::decode(id, &p).unwrap()
        else {
            panic!("no handshake");
        };
        assert_eq!(intent.intent, handshake::LOGIN);
        let (id, p) = c.recv().unwrap();
        let login::Serverbound::Hello(hello) = login::Serverbound::decode(id, &p).unwrap() else {
            panic!("no hello");
        };
        c.send(&login::Clientbound::Compression(login::Compression { threshold: 64 }).encode())
            .unwrap();
        c.set_compression(64);
        let mut got = Forwarded {
            address: String::new(),
            uuid: hello.uuid,
            name: hello.name.clone(),
            properties: Vec::new(),
            from_client: (0, Vec::new()),
        };
        if let Some(secret) = secret {
            c.send(
                &login::Clientbound::CustomQuery(login::CustomQuery {
                    transaction: 5,
                    channel: CHANNEL.into(),
                    data: vec![1],
                })
                .encode(),
            )
            .unwrap();
            let (id, p) = c.recv().unwrap();
            let login::Serverbound::CustomQueryAnswer(answer) =
                login::Serverbound::decode(id, &p).unwrap()
            else {
                panic!("no answer");
            };
            assert_eq!(answer.transaction, 5);
            let payload = answer.payload.unwrap();
            let (signature, data) = payload.split_at(32);
            assert_eq!(signature, hmac_sha256(secret, data), "bad signature");
            let mut r = Reader::new(data);
            assert_eq!(r.varint().unwrap(), 1);
            got.address = r.string(64).unwrap();
            got.uuid = r.uuid().unwrap();
            got.name = r.string(16).unwrap();
            for _ in 0..r.count(64).unwrap() {
                let name = r.string(64).unwrap();
                let value = r.string(32767).unwrap();
                let signature = if r.bool().unwrap() {
                    Some(r.string(1024).unwrap())
                } else {
                    None
                };
                got.properties.push(Property {
                    name,
                    value,
                    signature,
                });
            }
            assert_eq!(r.remaining(), 0);
        }
        c.send(
            &login::Clientbound::LoginFinished(login::LoginFinished {
                profile: login::GameProfile {
                    uuid: got.uuid,
                    name: got.name.clone(),
                    properties: got.properties.clone(),
                },
                session: 9,
            })
            .encode(),
        )
        .unwrap();
        let (id, _) = c.recv().unwrap();
        assert_eq!(id, ids::login::serverbound::LOGIN_ACKNOWLEDGED);
        c.send(&body(3, b"from the engine")).unwrap();
        got.from_client = c.recv().unwrap();
        tx.send(got).unwrap();
    });
    (address, rx)
}

fn log_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hot-proxy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// PL: Startuje proxy na wolnym porcie. EN: Starts the proxy on a free port.
fn proxy(engine: SocketAddr, online: bool, discovery: &str, logs: &Path) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = listener.local_addr().unwrap();
    let log = Arc::new(
        hot_diag::Logger::open(
            hot_diag::LogConfig::new("hot-proxy", hot_proxy::VERSION, logs),
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
                online,
                discovery: discovery.into(),
            },
            log,
        )
        .unwrap(),
    );
    thread::spawn(move || proxy.serve(&listener));
    listen
}

/// PL: Klient: powitanie i logowanie; przy prośbie o szyfrowanie robi to, co gra po zalogowaniu
/// kontem. Zwraca połączenie i `serverId`, który policzył.
/// EN: A client: handshake and login; on an encryption request it does what a signed-in game does.
/// Returns the connection and the `serverId` it computed.
fn client(proxy: SocketAddr, name: &str) -> (Connection, Option<String>) {
    let mut c = Connection::from_stream(TcpStream::connect(proxy).unwrap()).unwrap();
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
            name: name.into(),
            uuid: 0,
        })
        .encode(),
    )
    .unwrap();
    let (id, p) = c.recv().unwrap();
    let mut hash = None;
    match login::Clientbound::decode(id, &p).unwrap() {
        login::Clientbound::Hello(hello) => {
            assert!(hello.should_authenticate);
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
            hash = Some(server_hash(&hello.server_id, &secret, &hello.public_key));
        }
        login::Clientbound::Compression(x) => c.set_compression(x.threshold),
        other => panic!("unexpected {other:?}"),
    }
    (c, hash)
}

/// PL: Reszta logowania po stronie klienta: kompresja i `LoginFinished` albo odmowa.
/// EN: The rest of the client's login: compression and `LoginFinished`, or a refusal.
fn finish(c: &mut Connection) -> Result<login::GameProfile, String> {
    loop {
        let (id, p) = c.recv().map_err(|e| e.to_string())?;
        match login::Clientbound::decode(id, &p).unwrap() {
            login::Clientbound::Compression(x) => c.set_compression(x.threshold),
            login::Clientbound::LoginFinished(f) => {
                c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())
                    .unwrap();
                return Ok(f.profile);
            }
            login::Clientbound::Disconnect(d) => return Err(d.reason_json),
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn log_text(dir: &Path) -> String {
    let mut text = String::new();
    for _ in 0..50 {
        text = std::fs::read_to_string(dir.join("hot-proxy.jsonl")).unwrap_or_default();
        if text.contains("HOT-E06") {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(20));
    }
    text
}

#[test]
fn an_online_player_reaches_the_engine_with_a_signed_profile() {
    let (discovery, seen) = session_service();
    let (engine_at, engine_rx) = engine(Some(SECRET));
    let logs = log_dir("online");
    let at = proxy(engine_at, true, &discovery, &logs);
    let (mut c, hash) = client(at, "Tester");
    let profile = finish(&mut c).unwrap();
    assert_eq!(profile.uuid, TESTER);
    assert_eq!(profile.name, "Tester");
    // PL: Proxy policzyło ten sam `serverId` co klient. EN: The proxy computed the client's `serverId`.
    assert_eq!(seen.lock().unwrap().as_slice(), [hash.unwrap()]);
    assert_eq!(c.recv().unwrap(), (3, b"from the engine".to_vec()));
    c.send(&body(4, b"from the client")).unwrap();
    let got = engine_rx.recv().unwrap();
    assert_eq!(got.address, "127.0.0.1");
    assert_eq!(got.uuid, TESTER);
    assert_eq!(got.name, "Tester");
    assert_eq!(got.properties.len(), 1);
    assert_eq!(got.properties[0].signature.as_deref(), Some("sig"));
    assert_eq!(got.from_client, (4, b"from the client".to_vec()));
}

#[test]
fn a_player_the_session_service_does_not_know_is_refused() {
    let (discovery, _) = session_service();
    let (engine_at, _engine_rx) = engine(Some(SECRET));
    let logs = log_dir("unverified");
    let at = proxy(engine_at, true, &discovery, &logs);
    let (mut c, _) = client(at, "Cracked");
    let reason = finish(&mut c).unwrap_err();
    assert!(reason.contains("multiplayer.disconnect.unverified_username"));
    assert!(log_text(&logs).contains("HOT-E0601"));
}

#[test]
fn no_session_service_means_a_refusal_not_an_offline_login() {
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (engine_at, _engine_rx) = engine(Some(SECRET));
    let logs = log_dir("down");
    let at = proxy(
        engine_at,
        true,
        &format!("http://{closed}/discovery"),
        &logs,
    );
    let (mut c, _) = client(at, "Tester");
    let reason = finish(&mut c).unwrap_err();
    assert!(reason.contains("multiplayer.disconnect.authservers_down"));
    assert!(log_text(&logs).contains("HOT-E0602"));
}

#[test]
fn an_engine_without_a_forwarding_secret_is_refused() {
    let (discovery, _) = session_service();
    let (engine_at, _engine_rx) = engine(None);
    let logs = log_dir("unsigned");
    let at = proxy(engine_at, true, &discovery, &logs);
    let (mut c, _) = client(at, "Tester");
    let reason = finish(&mut c).unwrap_err();
    assert!(reason.contains("multiplayer.disconnect.server_shutdown"));
    assert!(log_text(&logs).contains("HOT-E0603"));
}

#[test]
fn offline_mode_on_loopback_uses_the_offline_uuid() {
    let (engine_at, engine_rx) = engine(Some(SECRET));
    let logs = log_dir("offline");
    let at = proxy(engine_at, false, "http://127.0.0.1:9/unused", &logs);
    let (mut c, hash) = client(at, "Dev");
    assert_eq!(hash, None);
    let profile = finish(&mut c).unwrap();
    assert_eq!(profile.uuid, offline_uuid("Dev"));
    c.recv().unwrap();
    c.send(&body(4, b"x")).unwrap();
    assert_eq!(engine_rx.recv().unwrap().uuid, offline_uuid("Dev"));
}
