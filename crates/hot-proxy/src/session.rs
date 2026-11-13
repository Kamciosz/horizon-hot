//! PL: Jedno połączenie gracza: powitanie, logowanie online, logowanie do silnika z podpisanymi
//! danymi gracza, a potem przesyłanie pakietów w obie strony. Kod logowania wzorowany na serwerze
//! Horizona (`horizon-client/src/lan/conn.rs`, `authenticate`) i na próbie S8
//! (`spikes/s8-podmiana/proxy`). Strona gracza i strona silnika mają osobne połączenia
//! (własna kompresja, szyfrowanie tylko u gracza), więc później proxy podmieni silnik pod graczem.
//!
//! EN: One player connection: handshake, online login, login to the engine with the player's signed
//! data, then relaying packets both ways. The login code follows Horizon's server
//! (`horizon-client/src/lan/conn.rs`, `authenticate`) and spike S8 (`spikes/s8-podmiana/proxy`).
//! The player side and the engine side are separate connections (own compression, encryption only
//! towards the player), so the proxy can later swap the engine under the player.

use crate::auth::SessionService;
use crate::config::{Config, COMPRESSION};
use crate::{codes, forwarding};
use horizon_protocol::{
    buf::write_varint,
    connection::{ConnReader, ConnWriter, Connection},
    crypto::{offline_uuid, random_bytes, server_hash, uuid_string},
    ids,
    packets::{
        handshake::{self, Intention},
        login::{self, Property},
        Empty,
    },
    rsa::RsaPrivateKey,
};
use hot_diag::{DiagError, Logger};
use serde_json::json;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// PL: Tyle czeka proxy na gracza albo silnik w czasie logowania.
/// EN: How long the proxy waits for the player or the engine during login.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// PL: Tyle czeka proxy na połączenie z silnikiem. EN: How long the proxy waits to reach the engine.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// PL: Dlaczego połączenie się skończyło. EN: Why a connection ended.
enum Failure {
    /// PL: Błąd do logu z kodem; gracz dostał już powód. EN: A coded error; the player got a reason.
    Diag(DiagError),
    /// PL: Gracz się rozłączył albo wysłał coś złego. EN: The player left or sent something wrong.
    Gone(String),
}

impl From<DiagError> for Failure {
    fn from(e: DiagError) -> Self {
        Self::Diag(e)
    }
}

fn gone(e: impl std::fmt::Display) -> Failure {
    Failure::Gone(e.to_string())
}

/// PL: Kim jest gracz po logowaniu. EN: Who the player is after login.
struct Profile {
    uuid: u128,
    name: String,
    properties: Vec<Property>,
}

/// PL: Proxy: ustawienia, klucz RSA na czas działania, usługa sesji i log.
/// EN: The proxy: settings, an RSA key for its lifetime, the session service and the log.
pub struct Proxy {
    config: Config,
    key: RsaPrivateKey,
    public_der: Vec<u8>,
    sessions: SessionService,
    log: Arc<Logger>,
}

impl Proxy {
    /// PL: Sprawdza ustawienia i robi klucz RSA (1024 bity, jak serwer gry).
    /// EN: Checks the settings and makes the RSA key (1024 bits, as the game server does).
    ///
    /// # Errors
    /// PL: Złe ustawienia albo brak losowości w systemie. EN: Bad settings or no OS randomness.
    pub fn new(config: Config, log: Arc<Logger>) -> Result<Self, DiagError> {
        config.check()?;
        let key = RsaPrivateKey::generate(1024).map_err(|e| {
            DiagError::new(
                codes::CONFIG,
                format!("hot-proxy cannot make its RSA key: {e}"),
                "Check that the system gives random numbers (/dev/urandom) and start the proxy again",
            )
            .state("The proxy did not start")
        })?;
        let public_der = key.public_der();
        let sessions = SessionService::new(&config.discovery);
        Ok(Self {
            config,
            key,
            public_der,
            sessions,
            log,
        })
    }

    /// PL: Przyjmuje graczy bez końca, każdego w osobnym wątku. EN: Accepts players forever, a thread each.
    pub fn serve(self: &Arc<Self>, listener: &TcpListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let proxy = Arc::clone(self);
                    thread::spawn(move || proxy.connection(stream));
                }
                Err(e) => self.log.warn(
                    "hot-proxy could not accept a connection",
                    json!({ "error": e.to_string() }),
                ),
            }
        }
    }

    fn connection(&self, stream: TcpStream) {
        let Ok(address) = stream.peer_addr() else {
            return;
        };
        match self.login(stream, address) {
            Ok(Some((client, engine, profile))) => {
                let who = json!({ "player": profile.name, "uuid": uuid_string(profile.uuid) });
                self.log
                    .info("A player joined through the proxy", who.clone());
                let started = Instant::now();
                let why = relay(client, engine);
                let mut left = who;
                left["reason"] = why.into();
                left["seconds"] = started.elapsed().as_secs().into();
                self.log.info("A player left the proxy", left);
            }
            Ok(None) => {}
            Err(Failure::Diag(e)) => self.log.error(&e),
            Err(Failure::Gone(reason)) => self.log.warn(
                "A connection ended during login",
                json!({ "reason": reason }),
            ),
        }
    }

    /// PL: Logowanie gracza. `None`: zapytanie o status serwera, już obsłużone.
    /// EN: The player's login. `None`: a server status query, already answered.
    fn login(
        &self,
        stream: TcpStream,
        address: SocketAddr,
    ) -> Result<Option<(Connection, Connection, Profile)>, Failure> {
        stream.set_read_timeout(Some(LOGIN_TIMEOUT)).map_err(gone)?;
        let mut client = Connection::from_stream(stream).map_err(gone)?;
        let (id, p) = client.recv().map_err(gone)?;
        let handshake::Serverbound::Intention(intent) =
            handshake::Serverbound::decode(id, &p).map_err(gone)?
        else {
            return Err(gone(format!(
                "the client did not start with a handshake (packet {id})"
            )));
        };
        if intent.intent == handshake::STATUS {
            self.status(client, &intent);
            return Ok(None);
        }
        let (id, p) = client.recv().map_err(gone)?;
        let login::Serverbound::Hello(hello) = login::Serverbound::decode(id, &p).map_err(gone)?
        else {
            return Err(gone("the client did not start the login with Hello"));
        };
        let profile = if self.config.online {
            self.authenticate(&mut client, &hello.name)?
        } else {
            Profile {
                uuid: offline_uuid(&hello.name),
                name: hello.name,
                properties: Vec::new(),
            }
        };
        let (engine, finished) = match self.engine_login(&intent, &profile, address.ip()) {
            Ok(Ok(done)) => done,
            Ok(Err(reason_json)) => {
                disconnect(&mut client, &reason_json);
                return Err(engine_error(
                    format!("The engine refused {}: {reason_json}", profile.name),
                    "If the reason mentions the proxy or forwarding, give the engine and the proxy the same forwarding.secret; otherwise it is the engine's own refusal (whitelist, ban, full server)",
                    &self.config,
                )
                .into());
            }
            Err(e) => {
                disconnect(
                    &mut client,
                    &translate("multiplayer.disconnect.server_shutdown"),
                );
                return Err(e.into());
            }
        };
        client
            .send(
                &login::Clientbound::Compression(login::Compression {
                    threshold: COMPRESSION,
                })
                .encode(),
            )
            .map_err(gone)?;
        client.set_compression(COMPRESSION);
        client
            .send(&login::Clientbound::LoginFinished(finished).encode())
            .map_err(gone)?;
        let (id, _) = client.recv().map_err(gone)?;
        if id != ids::login::serverbound::LOGIN_ACKNOWLEDGED {
            return Err(gone(format!("expected LoginAcknowledged, got packet {id}")));
        }
        Ok(Some((client, engine, profile)))
    }

    /// PL: Szyfrowanie i `hasJoined` jak w serwerze gry. EN: Encryption and `hasJoined`, as the game server.
    fn authenticate(&self, client: &mut Connection, name: &str) -> Result<Profile, Failure> {
        let mut challenge = [0u8; 4];
        random_bytes(&mut challenge).map_err(gone)?;
        client
            .send(
                &login::Clientbound::Hello(login::Hello {
                    server_id: String::new(),
                    public_key: self.public_der.clone(),
                    challenge: challenge.to_vec(),
                    should_authenticate: true,
                })
                .encode(),
            )
            .map_err(gone)?;
        let (id, p) = client.recv().map_err(gone)?;
        let login::Serverbound::Key(key) = login::Serverbound::decode(id, &p).map_err(gone)? else {
            return Err(gone("the client did not answer the encryption request"));
        };
        if self.key.decrypt(&key.challenge).ok().as_deref() != Some(&challenge[..]) {
            return Err(gone(
                "the client's encryption answer does not match the challenge",
            ));
        }
        let secret: [u8; 16] = self
            .key
            .decrypt(&key.key)
            .ok()
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| gone("the client's shared secret is not 16 bytes"))?;
        let digest = server_hash("", &secret, &self.public_der);
        client.enable_encryption(&secret);
        match self.sessions.has_joined(name, &digest) {
            Ok(Some(joined)) => Ok(Profile {
                uuid: joined.uuid,
                name: joined.name.unwrap_or_else(|| name.to_owned()),
                properties: joined.properties,
            }),
            Ok(None) => {
                disconnect(
                    client,
                    &translate("multiplayer.disconnect.unverified_username"),
                );
                Err(DiagError::new(
                    codes::UNVERIFIED,
                    format!("The session service did not confirm that {name} joined this server"),
                    "Nothing to fix on the server when the player has no bought copy or no Microsoft sign-in; a player who owns the game signs in again in the launcher and checks the computer clock",
                )
                .expected("hasJoined returns the player's profile")
                .context("player", name)
                .state("The player was refused; nobody else is affected")
                .into())
            }
            Err(why) => {
                disconnect(
                    client,
                    &translate("multiplayer.disconnect.authservers_down"),
                );
                Err(DiagError::new(
                    codes::SESSION_SERVICE_DOWN,
                    format!("hot-proxy cannot ask the session service about {name}: {why}"),
                    "Check the server's internet connection and DNS (curl -sI https://discovery.minecraftservices.com); when Mojang's services are down, wait",
                )
                .context("player", name)
                .context("discovery", self.sessions.discovery_url())
                .state("The player was refused; players already in the game play on")
                .into())
            }
        }
    }

    /// PL: Logowanie do silnika z danymi gracza podpisanymi sekretem. `Ok(Err)`: silnik odmówił
    /// (powód w JSON-ie dla gracza).
    /// EN: Logs in to the engine with the player's data signed with the secret. `Ok(Err)`: the engine
    /// refused (the reason as JSON for the player).
    fn engine_login(
        &self,
        intent: &Intention,
        profile: &Profile,
        address: IpAddr,
    ) -> Result<Result<(Connection, login::LoginFinished), String>, DiagError> {
        let config = &self.config;
        let fail = |what: String| {
            engine_error(
                what,
                "Start the engine (systemctl status horizon-hot-server) and check that it listens on the address given to --engine",
                config,
            )
        };
        let stream = TcpStream::connect_timeout(&config.engine, CONNECT_TIMEOUT).map_err(|e| {
            fail(format!(
                "hot-proxy cannot reach the engine at {}: {e}",
                config.engine
            ))
        })?;
        stream.set_read_timeout(Some(LOGIN_TIMEOUT)).map_err(|e| {
            fail(format!(
                "hot-proxy cannot set up the engine connection: {e}"
            ))
        })?;
        let mut c = Connection::from_stream(stream).map_err(|e| {
            fail(format!(
                "hot-proxy cannot set up the engine connection: {e}"
            ))
        })?;
        let lost = |e: horizon_protocol::Error| {
            fail(format!("The engine connection broke during login: {e}"))
        };
        c.send(
            &handshake::Serverbound::Intention(Intention {
                protocol: intent.protocol,
                host: intent.host.clone(),
                port: intent.port,
                intent: handshake::LOGIN,
            })
            .encode(),
        )
        .map_err(lost)?;
        c.send(
            &login::Serverbound::Hello(login::ServerHello {
                name: profile.name.clone(),
                uuid: profile.uuid,
            })
            .encode(),
        )
        .map_err(lost)?;
        let mut forwarded = false;
        loop {
            let (id, p) = c.recv().map_err(lost)?;
            match login::Clientbound::decode(id, &p).map_err(lost)? {
                login::Clientbound::Compression(x) => c.set_compression(x.threshold),
                login::Clientbound::CustomQuery(q) if q.channel == forwarding::CHANNEL => {
                    let player = forwarding::Player {
                        address,
                        uuid: profile.uuid,
                        name: &profile.name,
                        properties: &profile.properties,
                    };
                    let payload = forwarding::answer(&config.secret, &q.data, &player)
                        .map_err(|e| fail(format!("hot-proxy cannot answer the engine: {e}")))?;
                    c.send(&query_answer(q.transaction, Some(payload))).map_err(lost)?;
                    forwarded = true;
                }
                login::Clientbound::CustomQuery(q) => {
                    c.send(&query_answer(q.transaction, None)).map_err(lost)?;
                }
                login::Clientbound::CookieRequest(r) => {
                    c.send(
                        &login::Serverbound::CookieResponse(login::CookieResponse {
                            key: r.key,
                            payload: None,
                        })
                        .encode(),
                    )
                    .map_err(lost)?;
                }
                login::Clientbound::Hello(_) => {
                    return Err(engine_error(
                        "The engine asked the proxy for encryption: it runs in online mode".into(),
                        "Set online_mode = false and add forwarding.secret in the engine's folder (the installer does this); the proxy checks the purchase instead",
                        config,
                    ))
                }
                login::Clientbound::Disconnect(d) => return Ok(Err(d.reason_json)),
                login::Clientbound::Unknown(id, _) => {
                    return Err(engine_error(
                        format!("The engine sent login packet {id}, which this proxy does not know"),
                        "Build the proxy at the engine's Horizon revision (horizon-protocol in the workspace Cargo.toml, ADR 0010)",
                        config,
                    ))
                }
                login::Clientbound::LoginFinished(finished) => {
                    if !forwarded {
                        return Err(engine_error(
                            "The engine let the player in without asking for forwarded data: it has no forwarding.secret".into(),
                            "Put the proxy's secret into forwarding.secret next to the engine's server.toml and restart the engine",
                            config,
                        ));
                    }
                    c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())
                        .map_err(lost)?;
                    return Ok(Ok((c, finished)));
                }
            }
        }
    }

    /// PL: Zapytanie o status (lista serwerów) idzie do silnika bez zmian. Błąd tylko kończy
    /// połączenie: lista serwerów odpytuje często, a logowanie zgłosi problem z kodem.
    /// EN: A status query (server list) goes to the engine unchanged. A failure only ends the
    /// connection: the server list asks often, and a login reports the problem with a code.
    fn status(&self, mut client: Connection, intent: &Intention) {
        let Ok(stream) = TcpStream::connect_timeout(&self.config.engine, CONNECT_TIMEOUT) else {
            return;
        };
        let _ = stream.set_read_timeout(Some(LOGIN_TIMEOUT));
        let Ok(mut engine) = Connection::from_stream(stream) else {
            return;
        };
        let mut relay = || -> horizon_protocol::Result<()> {
            engine.send(
                &handshake::Serverbound::Intention(Intention {
                    protocol: intent.protocol,
                    host: intent.host.clone(),
                    port: intent.port,
                    intent: handshake::STATUS,
                })
                .encode(),
            )?;
            // PL: Prośba o status i ping, każde z odpowiedzią. EN: Status request and ping, each answered.
            for _ in 0..2 {
                let (id, p) = client.recv()?;
                engine.send(&body(id, &p))?;
                let (id, p) = engine.recv()?;
                client.send(&body(id, &p))?;
            }
            Ok(())
        };
        let _ = relay();
    }
}

/// PL: Przesyła pakiety w obie strony, aż jedna strona się rozłączy; wtedy zamyka obie.
/// EN: Relays packets both ways until one side disconnects, then closes both.
fn relay(client: Connection, engine: Connection) -> String {
    let (client_r, client_w) = client.split();
    let (engine_r, engine_w) = engine.split();
    // PL: Limit z logowania nie dotyczy gry: gracz może długo nic nie wysyłać (pauza, pobieranie
    // paczki zasobów). EN: The login limit does not apply in game: a player may send nothing for a
    // long time (pause menu, resource pack download).
    let _ = client_r.set_read_timeout(None);
    let _ = engine_r.set_read_timeout(None);
    let first = Arc::new(OnceLock::new());
    let first_to_client = Arc::clone(&first);
    let to_client = thread::spawn(move || {
        pump(
            engine_r,
            client_w,
            ["the engine", "the player"],
            &first_to_client,
        );
    });
    pump(client_r, engine_w, ["the player", "the engine"], &first);
    let _ = to_client.join();
    first.get().cloned().unwrap_or_default()
}

/// PL: Przesyła pakiety z `from` do `to`. Pierwszy kończący zapisuje w `first`, która strona
/// zerwała połączenie i dlaczego; potem zamyka obie, więc druga pętla też się kończy.
/// EN: Relays packets from `from` to `to`. The first to end writes into `first` which side broke
/// off and why, then closes both, so the other loop ends too.
fn pump(mut from: ConnReader, mut to: ConnWriter, sides: [&str; 2], first: &OnceLock<String>) {
    let mut packet = Vec::new();
    let why = loop {
        let (id, payload) = match from.recv() {
            Ok(p) => p,
            Err(e) => break format!("{} connection ended: {e}", sides[0]),
        };
        packet.clear();
        write_varint(&mut packet, id);
        packet.extend_from_slice(&payload);
        if let Err(e) = to.send(&packet) {
            break format!("{} connection ended: {e}", sides[1]);
        }
    };
    let _ = first.set(why);
    to.shutdown();
    if let Ok(closer) = from.closer() {
        closer.close();
    }
}

fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    write_varint(&mut out, id);
    out.extend_from_slice(payload);
    out
}

fn query_answer(transaction: i32, payload: Option<Vec<u8>>) -> Vec<u8> {
    login::Serverbound::CustomQueryAnswer(login::CustomQueryAnswer {
        transaction,
        payload,
    })
    .encode()
}

fn translate(key: &str) -> String {
    json!({ "translate": key }).to_string()
}

/// PL: Odmowa w czasie logowania; błąd zapisu nic nie zmienia, bo i tak się rozłączamy.
/// EN: A refusal during login; a failed write changes nothing, as we disconnect anyway.
fn disconnect(client: &mut Connection, reason_json: &str) {
    let _ = client.send(
        &login::Clientbound::Disconnect(login::Disconnect {
            reason_json: reason_json.to_owned(),
        })
        .encode(),
    );
}

fn engine_error(what: String, fix: &str, config: &Config) -> DiagError {
    DiagError::new(codes::ENGINE_LOGIN, what, fix)
        .context("engine", config.engine.to_string())
        .state("The player was refused; players already in the game play on")
}
