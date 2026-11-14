//! PL: Jedno połączenie gracza: powitanie, logowanie online, logowanie do silnika z podpisanymi
//! danymi gracza, a potem przesyłanie pakietów w obie strony. Kod logowania wzorowany na serwerze
//! Horizona (`horizon-client/src/lan/conn.rs`, `authenticate`) i na próbie S8
//! (`spikes/s8-podmiana/proxy`). Strona gracza i strona silnika mają osobne połączenia
//! (własna kompresja, szyfrowanie tylko u gracza), więc proxy podmienia silnik pod graczem (`link`).
//!
//! EN: One player connection: handshake, online login, login to the engine with the player's signed
//! data, then relaying packets both ways. The login code follows Horizon's server
//! (`horizon-client/src/lan/conn.rs`, `authenticate`) and spike S8 (`spikes/s8-podmiana/proxy`).
//! The player side and the engine side are separate connections (own compression, encryption only
//! towards the player), so the proxy can swap the engine under the player (`link`).

use crate::auth::SessionService;
use crate::client_io::ClientConn;
use crate::config::{Config, COMPRESSION};
use crate::link::{Link, Rejoin};
use crate::upgrade::{self, Accept, Gate, Joining};
use crate::{codes, forwarding};
use horizon_protocol::{
    buf::write_varint,
    connection::Connection,
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
use std::os::fd::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

/// PL: Tyle czeka proxy na gracza albo silnik w czasie logowania.
/// EN: How long the proxy waits for the player or the engine during login.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// PL: Tyle czeka proxy na połączenie z silnikiem. EN: How long the proxy waits to reach the engine.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// PL: Tyle gracz czeka na nowy silnik przy podmianie. EN: How long a player waits for a new engine in a swap.
pub(crate) const SWITCH_WAIT: Duration = Duration::from_secs(25);

/// PL: Tyle gracz czeka, gdy silnik padł: supervisor wznawia go po 5–40 s, a start trwa kilka sekund.
/// EN: How long a player waits after an engine crash: the supervisor restarts it after 5–40 s, and a
/// start takes a few seconds.
pub(crate) const ENGINE_LOST_WAIT: Duration = Duration::from_secs(180);

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
pub(crate) struct Profile {
    pub(crate) uuid: u128,
    pub(crate) name: String,
    pub(crate) properties: Vec<Property>,
}

/// PL: Wynik logowania do silnika. EN: The outcome of a login to the engine.
pub(crate) enum EngineLogin {
    /// PL: Gracz wpuszczony; połączenie w konfiguracji. EN: Let in; the connection is in configuration.
    Joined(Box<Connection>, login::LoginFinished),
    /// PL: Silnik odmówił; powód w JSON-ie dla gracza. EN: The engine refused; the reason as JSON.
    Refused(String),
    /// PL: Błąd po stronie serwera. EN: A server-side failure.
    Failed(DiagError),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// PL: Proxy: ustawienia, klucz RSA na czas działania, usługa sesji i log.
/// EN: The proxy: settings, an RSA key for its lifetime, the session service and the log.
pub struct Proxy {
    config: Config,
    /// PL: Aktywny silnik; zmienia go `switch`. EN: The active engine; `switch` changes it.
    engine: Mutex<SocketAddr>,
    links: Mutex<Vec<Weak<Link>>>,
    key: RsaPrivateKey,
    public_der: Vec<u8>,
    sessions: SessionService,
    pub(crate) log: Arc<Logger>,
    /// PL: Przyjmowanie graczy; zamyka je `upgrade`. EN: Accepting players; `upgrade` closes it.
    pub(crate) gate: Gate,
    /// PL: Gniazda dla `upgrade`: nasłuch graczy i gniazdo sterujące.
    /// EN: Sockets for `upgrade`: the players' listener and the control socket.
    pub(crate) listener_fd: OnceLock<RawFd>,
    pub(crate) control_fd: OnceLock<RawFd>,
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
            engine: Mutex::new(config.engine),
            links: Mutex::new(Vec::new()),
            config,
            key,
            public_der,
            sessions,
            log,
            gate: Gate::default(),
            listener_fd: OnceLock::new(),
            control_fd: OnceLock::new(),
        })
    }

    /// PL: Przyjmuje graczy bez końca, każdego w osobnym wątku. EN: Accepts players forever, a thread each.
    pub fn serve(self: &Arc<Self>, listener: &TcpListener) {
        // PL: Keep-alive od proxy dla graczy czekających na silnik. EN: Proxy keep-alives for waiting players.
        let ticker = Arc::downgrade(self);
        thread::spawn(move || {
            while let Some(proxy) = ticker.upgrade() {
                for link in proxy.live() {
                    link.tick();
                }
                drop(proxy);
                thread::sleep(Duration::from_millis(250));
            }
        });
        // PL: Gniazdo nieblokujące i `poll`: `upgrade` może wstrzymać przyjmowanie między
        // dwoma `accept`. EN: A non-blocking listener and `poll`: `upgrade` can hold accepting
        // between two `accept` calls.
        let fd = listener.as_raw_fd();
        let _ = self.listener_fd.set(fd);
        if let Err(e) = listener.set_nonblocking(true) {
            self.log.warn(
                "hot-proxy cannot make its listener non-blocking",
                json!({ "error": e.to_string() }),
            );
        }
        loop {
            match self.gate.accept(listener) {
                Accept::Got(stream, joining) => {
                    // PL: Na macOS przyjęte gniazdo dziedziczy brak blokowania.
                    // EN: On macOS the accepted socket inherits non-blocking mode.
                    let _ = stream.set_nonblocking(false);
                    let proxy = Arc::clone(self);
                    thread::spawn(move || proxy.connection(stream, joining));
                }
                Accept::Nothing => upgrade::wait_readable(fd, Duration::from_millis(100)),
                Accept::Paused => thread::sleep(Duration::from_millis(20)),
                Accept::Failed(e) => {
                    self.log.warn(
                        "hot-proxy could not accept a connection",
                        json!({ "error": e.to_string() }),
                    );
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    /// PL: Aktywny silnik. EN: The active engine.
    pub fn engine(&self) -> SocketAddr {
        *lock(&self.engine)
    }

    pub(crate) fn register(&self, link: &Arc<Link>) {
        let mut links = lock(&self.links);
        links.retain(|l| l.strong_count() > 0);
        links.push(Arc::downgrade(link));
    }

    pub(crate) fn live(&self) -> Vec<Arc<Link>> {
        lock(&self.links).iter().filter_map(Weak::upgrade).collect()
    }

    /// PL: Przenosi wszystkich graczy w grze na silnik `target` (domyślnie ten sam) bez ekranu
    /// ładowania. Najpierw wszyscy przestają dostawać pakiety starego silnika, potem każdy loguje się
    /// do nowego równolegle, więc pauza nie rośnie z liczbą graczy. Zwraca podsumowanie w JSON-ie.
    /// EN: Moves every player in game to the engine `target` (the same one by default) with no
    /// loading screen. First everybody stops getting the old engine's packets, then each logs in to
    /// the new one in parallel, so the pause does not grow with the player count. Returns a JSON
    /// summary.
    pub fn switch(self: &Arc<Self>, target: Option<SocketAddr>) -> serde_json::Value {
        self.switch_within(target, SWITCH_WAIT)
    }

    /// PL: `switch` z własnym limitem czekania na silnik. EN: `switch` with its own wait for the engine.
    pub fn switch_within(
        self: &Arc<Self>,
        target: Option<SocketAddr>,
        wait: Duration,
    ) -> serde_json::Value {
        self.switch_reporting(target, wait, |_| {})
    }

    /// PL: Ustawia adres silnika bez przenoszenia graczy. Gracze w przerwie (podmiana albo awaria)
    /// logują się już pod nowy adres; tak `hotd` wraca na stary silnik, gdy nowy padł po zamrożeniu.
    /// EN: Sets the engine address without moving anybody. Players in a gap (a switch or a crash)
    /// log in at the new address from now on; this is how `hotd` goes back to the old engine when
    /// the new one failed after the freeze.
    pub fn set_engine(&self, target: SocketAddr) {
        *lock(&self.engine) = target;
        self.log.info(
            "Engine address set",
            json!({ "engine": target.to_string() }),
        );
    }

    /// PL: `switch`, który woła `held(liczba graczy)`, gdy już nikt nie dostaje pakietów starego
    /// silnika. Dopiero wtedy wolno go zatrzymać: jego `Disconnect` nie dojdzie do graczy.
    /// EN: A `switch` that calls `held(player count)` once nobody gets the old engine's packets.
    /// Only then may the old engine stop: its `Disconnect` never reaches the players.
    pub fn switch_reporting(
        self: &Arc<Self>,
        target: Option<SocketAddr>,
        wait: Duration,
        held: impl FnOnce(usize),
    ) -> serde_json::Value {
        {
            let mut engine = lock(&self.engine);
            if let Some(t) = target {
                *engine = t;
            }
        }
        let links: Vec<Arc<Link>> = self.live().into_iter().filter(|l| l.in_game()).collect();
        for link in &links {
            link.begin_gap();
        }
        held(links.len());
        let results: Vec<Rejoin> = thread::scope(|s| {
            let handles: Vec<_> = links
                .iter()
                .map(|l| s.spawn(move || l.rejoin(self, wait)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Rejoin::Failed("the switch thread panicked".into()))
                })
                .collect()
        });
        let mut pause = Duration::ZERO;
        let mut failed = Vec::new();
        for (result, link) in results.into_iter().zip(&links) {
            match result {
                Rejoin::Moved(p) => pause = pause.max(p),
                Rejoin::Superseded => {}
                Rejoin::Failed(why) => {
                    self.lost(link, &why, self.engine());
                    failed.push(json!({ "player": link.name, "reason": why }));
                }
            }
        }
        let summary = json!({
            "engine": self.engine().to_string(),
            "players": links.len(),
            "moved": links.len() - failed.len(),
            "failed": failed,
            "pause_ms": pause.as_millis(),
        });
        self.log.info("Engine switch finished", summary.clone());
        summary
    }

    /// PL: Stan dla `hot-proxy status`. EN: The state for `hot-proxy status`.
    pub fn status_json(&self) -> serde_json::Value {
        let players: Vec<_> = self
            .live()
            .into_iter()
            .filter(|l| l.in_game())
            .map(|l| l.name.clone())
            .collect();
        json!({ "engine": self.engine().to_string(), "players": players })
    }

    /// PL: Silnik zerwał połączenie bez `Disconnect` (awaria, restart): gracz czeka na silnik pod
    /// aktywnym adresem (D13). EN: The engine broke off without `Disconnect` (a crash, a restart):
    /// the player waits for the engine at the active address (D13).
    pub(crate) fn engine_lost(self: &Arc<Self>, link: &Arc<Link>, why: &str) {
        let target = self.engine();
        self.log.warn(
            "The engine connection ended; the player waits for the engine",
            json!({ "player": link.name, "engine": target.to_string(), "reason": why }),
        );
        link.begin_gap();
        if let Rejoin::Failed(why) = link.rejoin(self, ENGINE_LOST_WAIT) {
            self.lost(link, &why, self.engine());
        }
    }

    pub(crate) fn lost(&self, link: &Link, why: &str, target: SocketAddr) {
        link.give_up("The server is restarting. Join again in a minute.");
        self.log.error(
            &DiagError::new(
                codes::REJOIN,
                format!("{} could not move to the engine at {target}: {why}", link.name),
                "Check that the engine runs and takes players (logs/latest.log in the engine folder, hot-proxy status); the player can join again",
            )
            .context("player", link.name.as_str())
            .context("engine", target.to_string())
            .state("This player was disconnected with a message; players who moved play on"),
        );
    }

    fn connection(self: &Arc<Self>, stream: TcpStream, joining: Joining) {
        let Ok(address) = stream.peer_addr() else {
            return;
        };
        match self.login(stream, address) {
            Ok(Some((client, engine, profile, intent))) => {
                let who = json!({ "player": profile.name, "uuid": uuid_string(profile.uuid) });
                self.log
                    .info("A player joined through the proxy", who.clone());
                let started = Instant::now();
                let player = (profile, intent, address.ip());
                let why = Link::run(self, client, engine, player, joining);
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
    ) -> Result<Option<(ClientConn, Connection, Profile, Intention)>, Failure> {
        stream.set_read_timeout(Some(LOGIN_TIMEOUT)).map_err(gone)?;
        let mut client = ClientConn::new(stream).map_err(gone)?;
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
        let target = self.engine();
        let (engine, finished) = match self.engine_login(target, &intent, &profile, address.ip()) {
            EngineLogin::Joined(c, finished) => (*c, finished),
            EngineLogin::Refused(reason_json) => {
                disconnect(&mut client, &reason_json);
                return Err(engine_error(
                    format!("The engine refused {}: {reason_json}", profile.name),
                    "If the reason mentions the proxy or forwarding, give the engine and the proxy the same forwarding.secret; otherwise it is the engine's own refusal (whitelist, ban, full server)",
                    target,
                )
                .into());
            }
            EngineLogin::Failed(e) => {
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
        Ok(Some((client, engine, profile, intent)))
    }

    /// PL: Szyfrowanie i `hasJoined` jak w serwerze gry. EN: Encryption and `hasJoined`, as the game server.
    fn authenticate(&self, client: &mut ClientConn, name: &str) -> Result<Profile, Failure> {
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

    /// PL: Logowanie do silnika `target` z danymi gracza podpisanymi sekretem.
    /// EN: Logs in to the engine `target` with the player's data signed with the secret.
    pub(crate) fn engine_login(
        &self,
        target: SocketAddr,
        intent: &Intention,
        profile: &Profile,
        address: IpAddr,
    ) -> EngineLogin {
        match self.try_engine_login(target, intent, profile, address) {
            Ok(Ok((c, finished))) => EngineLogin::Joined(Box::new(c), finished),
            Ok(Err(reason)) => EngineLogin::Refused(reason),
            Err(e) => EngineLogin::Failed(e),
        }
    }

    fn try_engine_login(
        &self,
        target: SocketAddr,
        intent: &Intention,
        profile: &Profile,
        address: IpAddr,
    ) -> Result<Result<(Connection, login::LoginFinished), String>, DiagError> {
        let config = &self.config;
        let fail = |what: String| {
            engine_error(
                what,
                "Start the engine (systemctl status horizon-hot-server) and check that it listens on the address given to --engine",
                target,
            )
        };
        let stream = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT).map_err(|e| {
            fail(format!(
                "hot-proxy cannot reach the engine at {target}: {e}"
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
                        target,
                    ))
                }
                login::Clientbound::Disconnect(d) => return Ok(Err(d.reason_json)),
                login::Clientbound::Unknown(id, _) => {
                    return Err(engine_error(
                        format!("The engine sent login packet {id}, which this proxy does not know"),
                        "Build the proxy at the engine's Horizon revision (horizon-protocol in the workspace Cargo.toml, ADR 0010)",
                        target,
                    ))
                }
                login::Clientbound::LoginFinished(finished) => {
                    if !forwarded {
                        return Err(engine_error(
                            "The engine let the player in without asking for forwarded data: it has no forwarding.secret".into(),
                            "Put the proxy's secret into forwarding.secret next to the engine's server.toml and restart the engine",
                            target,
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
    fn status(&self, mut client: ClientConn, intent: &Intention) {
        let Ok(stream) = TcpStream::connect_timeout(&self.engine(), CONNECT_TIMEOUT) else {
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
fn disconnect(client: &mut ClientConn, reason_json: &str) {
    let _ = client.send(
        &login::Clientbound::Disconnect(login::Disconnect {
            reason_json: reason_json.to_owned(),
        })
        .encode(),
    );
}

fn engine_error(what: String, fix: &str, engine: SocketAddr) -> DiagError {
    DiagError::new(codes::ENGINE_LOGIN, what, fix)
        .context("engine", engine.to_string())
        .state("The player was refused; players already in the game play on")
}
