//! PL: Połączenie jednego gracza w grze: klient po jednej stronie i wymienny silnik po drugiej.
//! Po pierwszym wejściu proxy nigdy nie wysyła klientowi `Login`, `Respawn` ani
//! `StartConfiguration` od nowego silnika, bo każdy z nich pokazuje ekran ładowania. Zamiast tego
//! samo loguje się do nowego silnika, odtwarza konfigurację klienta, połyka `Login`, usuwa
//! u klienta encje starego silnika i zamienia id gracza (`remap`). W przerwie trzyma klienta
//! własnym keep-alive. Zasady: `docs/podmiana.md`; wzór: próba S8 (`spikes/s8-podmiana/proxy`).
//!
//! Kolejność blokad: `client` → `state` → `engine`. Nikt nie bierze wcześniejszej, trzymając
//! późniejszą.
//!
//! EN: One player's connection in game: the client on one side and a replaceable engine on the
//! other. After the first join the proxy never sends the client a `Login`, `Respawn` or
//! `StartConfiguration` from a new engine, as each shows a loading screen. It logs in to the new
//! engine itself, replays the client's configuration, swallows `Login`, removes the old engine's
//! entities on the client and swaps the player's id (`remap`). During a gap it keeps the client
//! with its own keep-alives. Rules: `docs/podmiana.md`; model: spike S8.
//!
//! Lock order: `client` → `state` → `engine`. Nobody takes an earlier lock while holding a later one.

mod handover;

use crate::client_io::{ClientCloser, ClientConn, ClientReader, ClientWriter};
use crate::remap::{self, Swap};
use crate::session::{EngineLogin, Profile, Proxy};
use crate::upgrade::Joining;
pub(crate) use handover::Parked;
use handover::{Freeze, POLL};
use horizon_protocol::{
    buf::write_varint,
    connection::{Closer, ConnReader, ConnWriter, Connection},
    ids,
    packets::{configuration as cfg, handshake::Intention, play, Empty},
    text,
};
use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// PL: Klient rozłącza się po 30 s ciszy; proxy wysyła swój keep-alive co 10 s przerwy, pierwszy po
/// 5 s. EN: The client leaves after 30 s of silence; the proxy sends its keep-alive every 10 s of a
/// gap, the first after 5 s.
const GAP_FIRST_KEEP_ALIVE: Duration = Duration::from_secs(5);
const GAP_KEEP_ALIVE: Duration = Duration::from_secs(10);

/// PL: Termin jednego zapisu do gracza. Gracz, który przestał odbierać dane, jest rozłączany po
/// 15–30 s: zapis, który zdążył wysłać część, kończy się po terminie bez błędu, a dopiero następny
/// czeka cały termin i zwraca błąd. Bez tego zawieszony klient trzymałby pisarza w zapisie bez
/// końca, a każde `upgrade` by odmawiało. Silnik wyrzuca gracza bez odpowiedzi na keep-alive też po
/// 15–30 s.
/// EN: The limit for one write to the player. A player who stopped taking data is dropped after
/// 15–30 s: a write that sent a part ends at the limit without an error, and only the next one waits
/// the whole limit and fails. Without it a hung client would hold the writer in a write forever and
/// every `upgrade` would refuse. The engine drops a player who does not answer keep-alives after
/// 15–30 s too.
const CLIENT_WRITE: Duration = Duration::from_secs(15);

/// PL: Przez tyle po wejściu na nowy silnik proxy ukrywa „X dołączył/opuścił grę” i `game_event` 13:
/// to skutki podmiany, nie prawdziwe wejścia. EN: For this long after joining a new engine the proxy
/// hides "X joined/left the game" and `game_event` 13: effects of the swap, not real joins.
const SWAP_FILTER: Duration = Duration::from_secs(5);

/// PL: Znacznik w id keep-alive wysłanych przez proxy (silnik daje milisekundy od startu).
/// EN: The tag in the ids of the proxy's keep-alives (the engine uses milliseconds since start).
const OWN_KEEP_ALIVE: i64 = 0x686f_7400_0000_0000;
const OWN_KEEP_ALIVE_MASK: i64 = 0x7fff_ff00_0000_0000;

/// PL: `game_event` „początek ładowania poziomu”. EN: The "level chunks load start" game event.
const LEVEL_CHUNKS_LOAD_START: u8 = 13;

/// PL: Co klient wysłał w konfiguracji; proxy powtarza to nowemu silnikowi.
/// EN: What the client sent in configuration; the proxy replays it to a new engine.
#[derive(Default, Clone)]
struct Recorded {
    client_information: Option<Vec<u8>>,
    payloads: Vec<(String, Vec<u8>)>,
    known_packs: Option<Vec<u8>>,
    pack_responses: Vec<(u128, Vec<u8>)>,
    cookies: Vec<(String, Vec<u8>)>,
    accepted_conduct: bool,
}

/// PL: Przerwa: brak silnika, klient trzymany przez proxy. EN: A gap: no engine, the proxy holds the client.
struct Gap {
    since: Instant,
    keep_alive_at: Instant,
    dropped: u64,
}

#[derive(Default)]
struct State {
    /// PL: Id gracza z pierwszego `Login`; `Some` = klient jest w grze. EN: From the first `Login`.
    client_id: Option<i32>,
    engine_id: Option<i32>,
    /// PL: Encje pokazane klientowi (id u klienta). EN: Entities shown to the client (client ids).
    entities: HashSet<i32>,
    recorded: Recorded,
    /// PL: Następny numer `PlayerChat` u klienta i przesunięcie dla aktywnego silnika.
    /// EN: The client's next `PlayerChat` number and the shift for the active engine.
    chat_next: i32,
    chat_base: i32,
    bundle_open: bool,
    /// PL: Keep-alive wysłane przez aktywny silnik, jeszcze bez odpowiedzi.
    /// EN: Keep-alives the active engine sent, not answered yet.
    engine_keep_alives: VecDeque<i64>,
    filter_until: Option<Instant>,
    gap: Option<Gap>,
    own_keep_alive: i64,
}

impl State {
    fn swap(&self) -> Option<Swap> {
        match (self.client_id, self.engine_id) {
            (Some(client), Some(engine)) if client != engine => Some(Swap { client, engine }),
            _ => None,
        }
    }
}

struct Engine {
    writer: ConnWriter,
    closer: Closer,
}

/// PL: Wynik ponownego wejścia na silnik. EN: The outcome of joining an engine again.
pub enum Rejoin {
    /// PL: Gracz gra na nowym silniku; czas przerwy u niego. EN: Playing on the new engine; the pause.
    Moved(Duration),
    /// PL: Inna podmiana albo koniec połączenia zajęły jego miejsce. EN: Another swap or the end took over.
    Superseded,
    /// PL: Nie udało się; powód do logu. EN: Failed; the reason for the log.
    Failed(String),
}

/// PL: Gracz w grze. EN: A player in game.
pub struct Link {
    pub name: String,
    profile: Profile,
    intent: Intention,
    address: IpAddr,
    client: Mutex<ClientWriter>,
    client_closer: ClientCloser,
    engine: Mutex<Option<Engine>>,
    generation: AtomicU64,
    state: Mutex<State>,
    ended: AtomicBool,
    /// PL: Kto pierwszy zakończył połączenie i dlaczego. EN: Who ended the connection first and why.
    why: OnceLock<String>,
    /// PL: Postój odczytu od gracza przy podmianie programu proxy (`upgrade`).
    /// EN: The player reader's stop during a proxy program upgrade (`upgrade`).
    freeze: Freeze,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    write_varint(&mut out, id);
    out.extend_from_slice(payload);
    out
}

impl Link {
    /// PL: Startuje przekazywanie po logowaniu (obie strony w konfiguracji). Wraca, gdy gracz
    /// wyjdzie; zwraca powód. `joining` liczy logowanie w toku do chwili, gdy proxy zna gracza.
    /// EN: Starts relaying after login (both sides in configuration). Returns when the player
    /// leaves, with the reason. `joining` counts a login in progress until the proxy knows the player.
    pub(crate) fn run(
        proxy: &Arc<Proxy>,
        client: ClientConn,
        engine: Connection,
        (profile, intent, address): (Profile, Intention, IpAddr),
        joining: Joining,
    ) -> String {
        let (client_r, client_w) = client.split();
        let (engine_r, engine_w) = engine.split();
        let (Ok(client_closer), Ok(engine_closer)) = (client_r.closer(), engine_r.closer()) else {
            return "the connection could not be set up".into();
        };
        // PL: Limit z logowania nie dotyczy gry (menu pauzy, pobieranie paczki). Odczyt od gracza
        // budzi się co `POLL`, żeby zauważyć prośbę o postój.
        // EN: The login limit does not apply in game (pause menu, pack download). The player
        // reader wakes every `POLL` to notice a stop request.
        let _ = client_r.set_read_timeout(Some(POLL));
        let _ = client_r.set_write_timeout(Some(CLIENT_WRITE));
        let _ = engine_r.set_read_timeout(None);
        let link = Arc::new(Self {
            name: profile.name.clone(),
            profile,
            intent,
            address,
            client: Mutex::new(client_w),
            client_closer,
            engine: Mutex::new(Some(Engine {
                writer: engine_w,
                closer: engine_closer,
            })),
            generation: AtomicU64::new(0),
            state: Mutex::new(State::default()),
            ended: AtomicBool::new(false),
            why: OnceLock::new(),
            freeze: Freeze::default(),
        });
        proxy.register(&link);
        drop(joining);
        let (l, p) = (Arc::clone(&link), Arc::clone(proxy));
        thread::spawn(move || l.engine_loop(&p, engine_r, 0, true));
        let why = link.client_loop(client_r, true);
        link.end(why);
        link.why.get().cloned().unwrap_or_default()
    }

    /// PL: Klient jest w grze (dostał `Login`). EN: The client is in game (got `Login`).
    pub fn in_game(&self) -> bool {
        lock(&self.state).client_id.is_some() && !self.ended.load(Ordering::SeqCst)
    }

    fn end(&self, why: String) {
        let _ = self.why.set(why);
        self.ended.store(true, Ordering::SeqCst);
        self.close_engine();
        self.client_closer.close();
    }

    /// PL: Zamyka połączenie z silnikiem; pakiety, które jeszcze przyjdą, nie dojdą do klienta.
    /// EN: Closes the engine connection; packets still arriving never reach the client.
    fn close_engine(&self) {
        {
            let _client = lock(&self.client);
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(e) = lock(&self.engine).take() {
            e.writer.shutdown();
            e.closer.close();
        }
    }

    /// PL: Początek przerwy: od teraz nic od starego silnika nie dochodzi do klienta, a pakiety
    /// klienta przepadają. EN: The gap starts: nothing from the old engine reaches the client from
    /// now on, and the client's packets are dropped.
    pub fn begin_gap(&self) {
        self.close_engine();
        let now = Instant::now();
        lock(&self.state).gap.get_or_insert(Gap {
            since: now,
            keep_alive_at: now + GAP_FIRST_KEEP_ALIVE,
            dropped: 0,
        });
    }

    /// PL: Keep-alive od proxy, gdy przerwa trwa. Wołane co ok. 250 ms.
    /// EN: The proxy's keep-alive while a gap lasts. Called about every 250 ms.
    pub fn tick(&self) {
        let id = {
            let mut st = lock(&self.state);
            let now = Instant::now();
            let Some(gap) = st.gap.as_mut().filter(|g| now >= g.keep_alive_at) else {
                return;
            };
            gap.keep_alive_at = now + GAP_KEEP_ALIVE;
            st.own_keep_alive += 1;
            OWN_KEEP_ALIVE | st.own_keep_alive
        };
        let packet = play::Clientbound::KeepAlive(cfg::KeepAlive { id }).encode();
        let sent = lock(&self.client).send(&packet);
        if let Err(e) = sent {
            self.end(format!("the player connection ended: {e}"));
        }
    }

    /// PL: Kończy połączenie z powodem widocznym dla gracza. EN: Ends the connection with a reason for the player.
    pub fn give_up(&self, reason: &str) {
        let packet = play::Clientbound::Disconnect(cfg::Disconnect {
            reason: text::literal(reason),
        })
        .encode();
        let _ = lock(&self.client).send(&packet);
        self.end(format!("the proxy disconnected the player: {reason}"));
    }

    /// PL: Pakiety gracza do silnika. `config`: gracz jest w konfiguracji. Między pakietami
    /// staje na prośbę `upgrade`. EN: The player's packets to the engine. `config`: the player is
    /// in configuration. Stops between packets when `upgrade` asks.
    fn client_loop(&self, mut r: ClientReader, mut config: bool) -> String {
        loop {
            if self.freeze.wanted() {
                Parked { reader: r, config } = self.freeze.park(Parked { reader: r, config });
            }
            let (id, mut p) = match r.poll() {
                Ok(Some(x)) => x,
                Ok(None) => continue,
                Err(e) => return format!("the player connection ended: {e}"),
            };
            if config {
                self.record(id, &p);
                config = id != ids::configuration::serverbound::FINISH_CONFIGURATION;
            } else {
                use ids::play::serverbound as sb;
                if id == sb::CONFIGURATION_ACKNOWLEDGED {
                    config = true;
                }
                let mut st = lock(&self.state);
                if id == sb::KEEP_ALIVE && !keep_alive_answer(&mut st, &p) {
                    continue;
                }
                if let Some(s) = st.swap() {
                    let _ = remap::serverbound(id, &mut p, s);
                }
            }
            let sent = lock(&self.engine)
                .as_mut()
                .map(|e| e.writer.send(&body(id, &p)));
            if sent.is_none() {
                if let Some(g) = lock(&self.state).gap.as_mut() {
                    g.dropped += 1;
                }
            }
        }
    }

    fn record(&self, id: i32, p: &[u8]) {
        let b = body(id, p);
        let rec = &mut lock(&self.state).recorded;
        match cfg::Serverbound::decode(id, p) {
            Ok(cfg::Serverbound::ClientInformation(_)) => rec.client_information = Some(b),
            Ok(cfg::Serverbound::CustomPayload(c)) => {
                rec.payloads.retain(|(ch, _)| *ch != c.channel);
                rec.payloads.push((c.channel, b));
            }
            Ok(cfg::Serverbound::SelectKnownPacks(_)) => rec.known_packs = Some(b),
            Ok(cfg::Serverbound::ResourcePack(r)) => rec.pack_responses.push((r.id, b)),
            Ok(cfg::Serverbound::CookieResponse(c)) => rec.cookies.push((c.key, b)),
            Ok(cfg::Serverbound::AcceptCodeOfConduct(_)) => rec.accepted_conduct = true,
            _ => {}
        }
    }

    fn engine_loop(
        self: Arc<Self>,
        proxy: &Arc<Proxy>,
        mut r: ConnReader,
        generation: u64,
        mut config: bool,
    ) {
        let current = |l: &Self| l.generation.load(Ordering::SeqCst) == generation;
        let mut kicked = false;
        let why = loop {
            let (id, mut p) = match r.recv() {
                Ok(x) => x,
                Err(e) => break format!("the engine connection ended: {e}"),
            };
            if !current(&self) {
                continue;
            }
            if config {
                use ids::configuration::clientbound as ccb;
                config = id != ccb::FINISH_CONFIGURATION;
                kicked = id == ccb::DISCONNECT;
            } else {
                use ids::play::clientbound as cb;
                if !self.engine_packet(id, &mut p, generation) {
                    continue;
                }
                config = id == cb::START_CONFIGURATION;
                kicked = id == cb::DISCONNECT;
            }
            let mut client = lock(&self.client);
            if !current(&self) {
                continue;
            }
            if let Err(e) = client.send(&body(id, &p)) {
                // PL: Zapis mógł zejść tylko w części, więc szyfrowany strumień jest zepsuty.
                // EN: The write may have gone out only in part, so the encrypted stream is broken.
                drop(client);
                self.end(format!("the player connection ended: {e}"));
                return;
            }
            drop(client);
            if kicked {
                break "the engine disconnected the player".into();
            }
        };
        if !current(&self) || self.ended.load(Ordering::SeqCst) {
            return;
        }
        if kicked || !self.in_game() {
            self.end(why);
            return;
        }
        // PL: Silnik zniknął bez `Disconnect`: awaria albo restart. Gracz czeka na nowy (D13).
        // EN: The engine vanished without `Disconnect`: a crash or a restart. The player waits (D13).
        proxy.engine_lost(&self, &why);
    }

    /// PL: Pakiet od aktywnego silnika w fazie gry. `false`: nie dla klienta.
    /// EN: A packet from the active engine in play. `false`: not for the client.
    fn engine_packet(&self, id: i32, p: &mut Vec<u8>, generation: u64) -> bool {
        use ids::play::clientbound as cb;
        let mut st = lock(&self.state);
        match id {
            cb::LOGIN if generation == 0 => {
                let player = p
                    .get(0..4)
                    .and_then(|b| b.try_into().ok())
                    .map(i32::from_be_bytes);
                st.client_id = player;
                st.engine_id = player;
            }
            cb::KEEP_ALIVE => {
                if let Some(raw) = p.get(0..8).and_then(|b| b.try_into().ok()) {
                    if st.engine_keep_alives.len() >= 8 {
                        st.engine_keep_alives.pop_front();
                    }
                    st.engine_keep_alives.push_back(i64::from_be_bytes(raw));
                }
            }
            cb::BUNDLE_DELIMITER => st.bundle_open = !st.bundle_open,
            cb::PLAYER_CHAT => {
                if let Some(n) = remap::shift_chat_index(p, st.chat_base) {
                    st.chat_next = n.wrapping_add(1);
                }
            }
            _ => {}
        }
        if st.filter_until.is_some_and(|t| Instant::now() < t) && hidden_after_swap(id, p) {
            return false;
        }
        if let Some(s) = st.swap() {
            let _ = remap::clientbound(id, p, s);
        }
        if id == cb::ADD_ENTITY {
            if let Some((e, _)) = remap::read_varint(p) {
                st.entities.insert(e);
            }
        } else if id == cb::REMOVE_ENTITIES {
            for e in remap::removed_entities(p) {
                st.entities.remove(&e);
            }
        }
        true
    }

    /// PL: Wejście na aktywny silnik proxy w miejsce starego; wołać po `begin_gap`. Powtarza
    /// logowanie co 20 ms do `wait` (nowy silnik przyjmuje graczy dopiero ze światem), za każdym
    /// razem pod bieżący adres, więc `set_engine` w trakcie kieruje gracza gdzie indziej.
    /// EN: Joins the proxy's active engine in place of the old one; call after `begin_gap`. Retries
    /// the login every 20 ms until `wait` (a new engine takes players only once it has the world),
    /// each time at the current address, so `set_engine` meanwhile sends the player elsewhere.
    pub fn rejoin(self: &Arc<Self>, proxy: &Arc<Proxy>, wait: Duration) -> Rejoin {
        let generation = self.generation.load(Ordering::SeqCst);
        let superseded = || {
            self.generation.load(Ordering::SeqCst) != generation
                || self.ended.load(Ordering::SeqCst)
        };
        let deadline = Instant::now() + wait;
        let (mut conn, target) = loop {
            if superseded() {
                return Rejoin::Superseded;
            }
            let target = proxy.engine();
            let last = match proxy.engine_login(target, &self.intent, &self.profile, self.address) {
                EngineLogin::Joined(c, _) => break (*c, target),
                EngineLogin::Refused(reason) => format!("the engine refused the player: {reason}"),
                EngineLogin::Failed(e) => e.to_string(),
            };
            if Instant::now() >= deadline {
                return Rejoin::Failed(last);
            }
            thread::sleep(Duration::from_millis(20));
        };
        let recorded = lock(&self.state).recorded.clone();
        let engine_id = match configure(&mut conn, &recorded).and_then(|()| await_login(&mut conn))
        {
            Ok(id) => id,
            Err(e) => return Rejoin::Failed(format!("the new engine broke off: {e}")),
        };
        if let Err(e) = conn.send(&play::Serverbound::PlayerLoaded(Empty).encode()) {
            return Rejoin::Failed(format!("the new engine broke off: {e}"));
        }
        let (reader, writer) = conn.split();
        let Ok(closer) = reader.closer() else {
            return Rejoin::Failed("the new engine connection could not be set up".into());
        };
        let _ = reader.set_read_timeout(None);
        let pause = {
            let mut client = lock(&self.client);
            if superseded() {
                closer.close();
                return Rejoin::Superseded;
            }
            let mut st = lock(&self.state);
            let mut out = Vec::new();
            if st.bundle_open {
                out.push(play::Clientbound::BundleDelimiter(Empty).encode());
                st.bundle_open = false;
            }
            let player = st.client_id;
            let old: Vec<i32> = st.entities.drain().filter(|e| Some(*e) != player).collect();
            let removed = old.len();
            out.push(play::Clientbound::RemoveEntities(play::RemoveEntities { ids: old }).encode());
            for packet in out {
                if let Err(e) = client.send(&packet) {
                    drop(st);
                    drop(client);
                    closer.close();
                    self.end(format!("the player connection ended: {e}"));
                    return Rejoin::Superseded;
                }
            }
            st.engine_id = Some(engine_id);
            st.chat_base = st.chat_next;
            st.engine_keep_alives.clear();
            st.filter_until = Some(Instant::now() + SWAP_FILTER);
            let gap = st.gap.take();
            *lock(&self.engine) = Some(Engine { writer, closer });
            let pause = gap.as_ref().map(|g| g.since.elapsed()).unwrap_or_default();
            proxy.log.info(
                "A player moved to another engine",
                json!({
                    "player": self.name,
                    "engine": target.to_string(),
                    "pause_ms": pause.as_millis(),
                    "removed_entities": removed,
                    "dropped_packets": gap.map_or(0, |g| g.dropped),
                }),
            );
            pause
        };
        let (l, p) = (Arc::clone(self), Arc::clone(proxy));
        thread::spawn(move || l.engine_loop(&p, reader, generation, false));
        Rejoin::Moved(pause)
    }
}

/// PL: Odpowiedź klienta na keep-alive. Przechodzi tylko odpowiedź na keep-alive aktywnego
/// silnika; odpowiedź na keep-alive proxy albo starego silnika przepada.
/// EN: The client's keep-alive answer. Only an answer to the active engine's keep-alive goes
/// through; an answer to the proxy's or the old engine's is dropped.
fn keep_alive_answer(st: &mut State, p: &[u8]) -> bool {
    let Some(id) = p
        .get(0..8)
        .and_then(|b| b.try_into().ok())
        .map(i64::from_be_bytes)
    else {
        return true;
    };
    if id & OWN_KEEP_ALIVE_MASK == OWN_KEEP_ALIVE {
        return false;
    }
    match st.engine_keep_alives.iter().position(|k| *k == id) {
        Some(i) => {
            st.engine_keep_alives.drain(..=i);
            true
        }
        None => false,
    }
}

/// PL: Pakiety nowego silnika, które są skutkiem podmiany. EN: New-engine packets caused by the swap.
fn hidden_after_swap(id: i32, p: &[u8]) -> bool {
    use ids::play::clientbound as cb;
    let has = |needle: &[u8]| p.windows(needle.len()).any(|w| w == needle);
    (id == cb::GAME_EVENT && p.first() == Some(&LEVEL_CHUNKS_LOAD_START))
        || (id == cb::SYSTEM_CHAT
            && (has(b"multiplayer.player.joined") || has(b"multiplayer.player.left")))
}

/// PL: Konfiguracja z nowym silnikiem w imieniu klienta: to samo, co klient wysłał przy wejściu.
/// EN: Configuration with a new engine on the client's behalf: what the client sent when joining.
fn configure(c: &mut Connection, rec: &Recorded) -> horizon_protocol::Result<()> {
    if let Some(b) = &rec.client_information {
        c.send(b)?;
    }
    for (ch, b) in &rec.payloads {
        if ch == "minecraft:brand" {
            c.send(b)?;
        }
    }
    loop {
        let (id, p) = c.recv()?;
        match cfg::Clientbound::decode(id, &p) {
            Ok(cfg::Clientbound::SelectKnownPacks(_)) => match &rec.known_packs {
                Some(b) => c.send(b)?,
                None => c.send(
                    &cfg::Serverbound::SelectKnownPacks(cfg::SelectKnownPacks { packs: vec![] })
                        .encode(),
                )?,
            },
            Ok(cfg::Clientbound::CustomPayload(x)) => {
                for (ch, b) in &rec.payloads {
                    if *ch == x.channel && ch != "minecraft:brand" {
                        c.send(b)?;
                    }
                }
            }
            Ok(cfg::Clientbound::KeepAlive(k)) => {
                c.send(&cfg::Serverbound::KeepAlive(k).encode())?
            }
            Ok(cfg::Clientbound::Ping(x)) => c.send(&cfg::Serverbound::Pong(x).encode())?,
            Ok(cfg::Clientbound::ResourcePackPush(x)) => {
                for (_, b) in rec.pack_responses.iter().filter(|(id, _)| *id == x.id) {
                    c.send(b)?;
                }
            }
            Ok(cfg::Clientbound::CookieRequest(x)) => {
                for (_, b) in rec.cookies.iter().filter(|(key, _)| *key == x.key) {
                    c.send(b)?;
                }
            }
            Ok(cfg::Clientbound::CodeOfConduct(_)) if rec.accepted_conduct => {
                c.send(&cfg::Serverbound::AcceptCodeOfConduct(Empty).encode())?;
            }
            Ok(cfg::Clientbound::Disconnect(d)) => {
                return Err(horizon_protocol::Error::new(format!(
                    "configuration refused: {:?}",
                    d.reason
                )));
            }
            Ok(cfg::Clientbound::FinishConfiguration(_)) => {
                return c.send(&cfg::Serverbound::FinishConfiguration(Empty).encode());
            }
            _ => {}
        }
    }
}

/// PL: Czeka na `Login` nowego silnika i go połyka; zwraca id gracza tam. Pakiety przed nim
/// przepadają (S8). EN: Waits for the new engine's `Login` and swallows it; returns the player's id
/// there. Packets before it are dropped (S8).
fn await_login(c: &mut Connection) -> horizon_protocol::Result<i32> {
    loop {
        let (id, p) = c.recv()?;
        if id == ids::play::clientbound::LOGIN {
            return p
                .get(0..4)
                .and_then(|b| b.try_into().ok())
                .map(i32::from_be_bytes)
                .ok_or_else(|| horizon_protocol::Error::new("a short Login"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_answers_to_the_active_engine_reach_it() {
        let mut st = State::default();
        st.engine_keep_alives.extend([10, 20]);
        let answer = |id: i64| id.to_be_bytes().to_vec();
        assert!(!keep_alive_answer(&mut st, &answer(OWN_KEEP_ALIVE | 3)));
        assert!(
            !keep_alive_answer(&mut st, &answer(999)),
            "stale: the old engine's id"
        );
        assert!(keep_alive_answer(&mut st, &answer(20)));
        assert!(
            st.engine_keep_alives.is_empty(),
            "older ids are answered too"
        );
        assert!(
            !keep_alive_answer(&mut st, &answer(20)),
            "one answer per keep-alive"
        );
    }

    #[test]
    fn the_proxy_tag_never_matches_an_engine_id() {
        // PL: Silnik daje milisekundy od startu: nawet 100 lat to mniej niż 2^42.
        // EN: The engine uses milliseconds since start: even 100 years is below 2^42.
        let century: i64 = 100 * 365 * 24 * 3600 * 1000;
        assert_ne!(century & OWN_KEEP_ALIVE_MASK, OWN_KEEP_ALIVE);
        assert_eq!((OWN_KEEP_ALIVE | 77) & OWN_KEEP_ALIVE_MASK, OWN_KEEP_ALIVE);
    }

    #[test]
    fn swap_side_effects_are_hidden() {
        use ids::play::clientbound as cb;
        assert!(hidden_after_swap(cb::GAME_EVENT, &[13, 0, 0, 0, 0]));
        assert!(!hidden_after_swap(cb::GAME_EVENT, &[3, 0, 0, 0, 0]));
        assert!(hidden_after_swap(
            cb::SYSTEM_CHAT,
            b"..multiplayer.player.joined.."
        ));
        assert!(hidden_after_swap(
            cb::SYSTEM_CHAT,
            b"..multiplayer.player.left.."
        ));
        assert!(!hidden_after_swap(cb::SYSTEM_CHAT, b"..chat.type.text.."));
    }
}
