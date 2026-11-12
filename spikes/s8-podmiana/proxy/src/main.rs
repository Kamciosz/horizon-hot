//! S8 (M0.9): prototyp proxy, które podmienia silnik za plecami gracza bez
//! ekranu ładowania. Jeden gracz, tryb offline, bez kompresji do klienta.
//! Opis i wyniki: docs/spike/s8-*.md.
//!
//! EN: S8 spike. A proxy that swaps the engine behind a connected player with
//! no loading screen: it logs into the new engine itself, replays the
//! client's configuration, swallows the new `Login` and swaps the player's
//! entity id (client id <-> new server id) in entity packets.
//!
//! s8-proxy --listen 127.0.0.1:25700 --server-bin <horizon-server>
//!          --a 25613:<dir A> --b 25566:<dir B> --swap-file <path>
//!
//! Creating `<swap-file>` stops the active engine, starts the other one and
//! moves the player over; the file is removed when the swap starts.
//!
//! S6 (M0.7): with `--pack-file <path>`, a file holding `<url> <sha1>` makes the
//! proxy push that resource pack to the client in play; the client's answers
//! are logged as `PACK RESULT` and not passed to the engine.

use horizon_protocol::{
    buf::write_varint,
    connection::{ConnReader, ConnWriter, Connection},
    ids,
    packets::{Empty, configuration as cfg, handshake, login, play},
};
use std::{
    collections::HashSet,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

type R<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static START: OnceLock<Instant> = OnceLock::new();

fn ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

macro_rules! log {
    ($($t:tt)*) => { eprintln!("[{:>10.1} ms] {}", ms(), format!($($t)*)) };
}

#[derive(Clone)]
struct Backend {
    port: u16,
    dir: PathBuf,
}

struct Args {
    listen: String,
    server_bin: PathBuf,
    backends: [Backend; 2],
    swap_file: PathBuf,
    pack_file: Option<PathBuf>,
}

fn parse_args() -> R<Args> {
    let mut a = std::env::args().skip(1);
    let (mut listen, mut bin, mut ba, mut bb, mut swap, mut pack) =
        (None, None, None, None, None, None);
    let backend = |s: String| -> R<Backend> {
        let (port, dir) = s.split_once(':').ok_or("backend must be PORT:DIR")?;
        Ok(Backend {
            port: port.parse()?,
            dir: dir.into(),
        })
    };
    while let Some(k) = a.next() {
        let v = a.next().ok_or(format!("{k} needs a value"))?;
        match k.as_str() {
            "--listen" => listen = Some(v),
            "--server-bin" => bin = Some(PathBuf::from(v)),
            "--a" => ba = Some(backend(v)?),
            "--b" => bb = Some(backend(v)?),
            "--swap-file" => swap = Some(PathBuf::from(v)),
            "--pack-file" => pack = Some(PathBuf::from(v)),
            _ => return Err(format!("unknown argument {k}").into()),
        }
    }
    Ok(Args {
        listen: listen.ok_or("--listen missing")?,
        server_bin: bin.ok_or("--server-bin missing")?,
        backends: [ba.ok_or("--a missing")?, bb.ok_or("--b missing")?],
        swap_file: swap.ok_or("--swap-file missing")?,
        pack_file: pack,
    })
}

/// Co klient wysłał w logowaniu i konfiguracji; proxy powtarza to nowemu silnikowi.
#[derive(Default, Clone)]
struct Recorded {
    hello: Vec<u8>,
    client_information: Option<Vec<u8>>,
    payloads: Vec<(String, Vec<u8>)>,
    known_packs: Option<Vec<u8>>,
    pack_responses: Vec<(u128, Vec<u8>)>,
    cookies: Vec<(String, Vec<u8>)>,
    accepted_conduct: bool,
}

/// Id gracza u klienta (z pierwszego `Login`) i u aktywnego silnika.
#[derive(Clone, Copy, Default)]
struct PlayerIds {
    client: Option<i32>,
    server: Option<i32>,
}

impl PlayerIds {
    /// Zamiana jest inwolucją, więc działa tak samo w obie strony.
    fn swap_fn(self) -> Option<impl Fn(i32) -> i32> {
        // Próba kontrolna: S8_NO_REMAP=1 wyłącza zamianę.
        if std::env::var_os("S8_NO_REMAP").is_some() {
            return None;
        }
        match (self.client, self.server) {
            (Some(c), Some(s)) if c != s => Some(move |id| {
                if id == s {
                    c
                } else if id == c {
                    s
                } else {
                    id
                }
            }),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Timeline {
    last_from_backend: f64,
    swap_at: Option<f64>,
    last_old_before_swap: f64,
    first_from_new: Option<f64>,
    position_from_new: Option<f64>,
    teleport_ack: Option<f64>,
    dropped_serverbound: u64,
}

struct Shared {
    client: Mutex<ConnWriter>,
    backend: Mutex<Option<ConnWriter>>,
    generation: AtomicU64,
    ids: Mutex<PlayerIds>,
    entities: Mutex<HashSet<i32>>,
    recorded: Mutex<Recorded>,
    timeline: Mutex<Timeline>,
    /// S6: id paczki wysłanej przez proxy i czas wysłania (ms).
    pack_push: Mutex<Option<(u128, f64)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(payload.len() + 3);
    write_varint(&mut v, id);
    v.extend_from_slice(payload);
    v
}

fn name(play_state: bool, clientbound: bool, id: i32) -> &'static str {
    let names: &[&str] = match (play_state, clientbound) {
        (true, true) => ids::play::clientbound::NAMES,
        (true, false) => ids::play::serverbound::NAMES,
        (false, true) => ids::configuration::clientbound::NAMES,
        (false, false) => ids::configuration::serverbound::NAMES,
    };
    usize::try_from(id)
        .ok()
        .and_then(|i| names.get(i))
        .copied()
        .unwrap_or("?")
}

// ---- Zamiana id encji na surowych bajtach (bez przekodowania reszty pakietu) ----

fn read_varint(b: &[u8]) -> Option<(i32, usize)> {
    let mut v: u32 = 0;
    for i in 0..5 {
        let byte = *b.get(i)?;
        v |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((v as i32, i + 1));
        }
    }
    None
}

/// Zamienia VarInt od `at`; zwraca pozycję za nim.
fn remap_varint(p: &mut Vec<u8>, at: usize, f: &impl Fn(i32) -> i32) -> Option<usize> {
    let (v, n) = read_varint(p.get(at..)?)?;
    let nv = f(v);
    if nv == v {
        return Some(at + n);
    }
    let mut enc = Vec::new();
    write_varint(&mut enc, nv);
    let len = enc.len();
    p.splice(at..at + n, enc);
    Some(at + len)
}

fn remap_int(p: &mut [u8], at: usize, f: &impl Fn(i32) -> i32) -> Option<usize> {
    let raw: [u8; 4] = p.get(at..at + 4)?.try_into().ok()?;
    let nv = f(i32::from_be_bytes(raw));
    p[at..at + 4].copy_from_slice(&nv.to_be_bytes());
    Some(at + 4)
}

fn remap_varint_list(p: &mut Vec<u8>, at: usize, f: &impl Fn(i32) -> i32) -> Option<usize> {
    let (count, n) = read_varint(p.get(at..)?)?;
    let mut pos = at + n;
    for _ in 0..count {
        pos = remap_varint(p, pos, f)?;
    }
    Some(pos)
}

fn remap_clientbound(id: i32, p: &mut Vec<u8>, f: &impl Fn(i32) -> i32) -> Option<()> {
    use ids::play::clientbound as cb;
    match id {
        cb::ADD_ENTITY
        | cb::ANIMATE
        | cb::DAMAGE_EVENT
        | cb::ENTITY_POSITION_SYNC
        | cb::HURT_ANIMATION
        | cb::MOVE_ENTITY_POS
        | cb::MOVE_ENTITY_POS_ROT
        | cb::MOVE_ENTITY_ROT
        | cb::REMOVE_MOB_EFFECT
        | cb::ROTATE_HEAD
        | cb::SET_CAMERA
        | cb::SET_ENTITY_DATA
        | cb::SET_ENTITY_MOTION
        | cb::SET_EQUIPMENT
        | cb::TELEPORT_ENTITY
        | cb::UPDATE_ATTRIBUTES
        | cb::UPDATE_MOB_EFFECT => {
            remap_varint(p, 0, f)?;
        }
        cb::ENTITY_EVENT => {
            remap_int(p, 0, f)?;
        }
        cb::SET_ENTITY_LINK => {
            remap_int(p, 0, f)?;
            remap_int(p, 4, f)?;
        }
        cb::TAKE_ITEM_ENTITY => {
            let at = remap_varint(p, 0, f)?;
            remap_varint(p, at, f)?;
        }
        cb::SET_PASSENGERS => {
            let at = remap_varint(p, 0, f)?;
            remap_varint_list(p, at, f)?;
        }
        cb::REMOVE_ENTITIES => {
            remap_varint_list(p, 0, f)?;
        }
        _ => {}
    }
    Some(())
}

fn remap_serverbound(id: i32, p: &mut Vec<u8>, f: &impl Fn(i32) -> i32) -> Option<()> {
    use ids::play::serverbound as sb;
    if id == sb::INTERACT || id == sb::PLAYER_COMMAND {
        remap_varint(p, 0, f)?;
    }
    Some(())
}

fn track_entities(sh: &Shared, id: i32, p: &[u8]) {
    use ids::play::clientbound as cb;
    if id == cb::ADD_ENTITY {
        if let Some((e, _)) = read_varint(p) {
            lock(&sh.entities).insert(e);
        }
    } else if id == cb::REMOVE_ENTITIES
        && let Some((count, mut pos)) = read_varint(p)
    {
        let mut set = lock(&sh.entities);
        for _ in 0..count {
            let Some((e, n)) = p.get(pos..).and_then(read_varint) else {
                break;
            };
            set.remove(&e);
            pos += n;
        }
    }
}

// ---- Wątki przekazujące ----

fn client_loop(mut r: ConnReader, sh: Arc<Shared>) {
    let mut config = true;
    let mut seen = HashSet::new();
    loop {
        let (id, mut p) = match r.recv() {
            Ok(x) => x,
            Err(e) => {
                log!("client: connection ended: {e}");
                return;
            }
        };
        if seen.insert((config, id)) {
            log!(
                "client -> server [{}] {}",
                if config { "config" } else { "play" },
                name(!config, false, id)
            );
        }
        if config {
            record(&sh, id, &p);
            if id == ids::configuration::serverbound::FINISH_CONFIGURATION {
                config = false;
            }
        } else {
            let ids_now = *lock(&sh.ids);
            if let Some(f) = ids_now.swap_fn()
                && remap_serverbound(id, &mut p, &f).is_none()
            {
                log!("client: could not remap {}", name(true, false, id));
            }
            if id == ids::play::serverbound::RESOURCE_PACK && pack_answer(&sh, &p) {
                continue;
            }
            if id == ids::play::serverbound::ACCEPT_TELEPORTATION {
                let mut t = lock(&sh.timeline);
                if let (Some(swap), Some(_), None) =
                    (t.swap_at, t.position_from_new, t.teleport_ack)
                {
                    let now = ms();
                    t.teleport_ack = Some(now);
                    log!(
                        "SWAP RESULT: swap start -> client accepted new position: {:.0} ms; \
                         last packet from old engine -> first from new: {:.0} ms; \
                         client packets dropped meanwhile: {}",
                        now - swap,
                        t.first_from_new.unwrap_or(f64::NAN) - t.last_old_before_swap,
                        t.dropped_serverbound
                    );
                }
            }
        }
        let mut backend = lock(&sh.backend);
        match backend.as_mut() {
            Some(w) => {
                if let Err(e) = w.send(&body(id, &p)) {
                    log!("client -> server: send failed: {e}");
                }
            }
            None => lock(&sh.timeline).dropped_serverbound += 1,
        }
    }
}

/// S6: wysyła klientowi w grze paczkę zasobów `<url> <sha1>` (wymaganą, bez pytania).
fn push_pack(sh: &Shared, line: &str) -> R<()> {
    let mut parts = line.split_whitespace();
    let (url, hash) = (
        parts.next().ok_or("no url")?,
        parts.next().ok_or("no sha1")?,
    );
    // Każde wysłanie ma nowe id, żeby klient traktował je jak nową paczkę.
    let id = 0x5106_u128 << 64 | ms() as u128;
    let push = play::Clientbound::ResourcePackPush(cfg::ResourcePackPush {
        id,
        url: url.to_owned(),
        hash: hash.to_owned(),
        required: true,
        prompt: None,
    });
    *lock(&sh.pack_push) = Some((id, ms()));
    lock(&sh.client).send(&push.encode())?;
    log!("pack push: {url} sha1 {hash}");
    Ok(())
}

/// S6: odpowiedź klienta na paczkę wysłaną przez proxy. Zwraca `true`, gdy to ta paczka
/// (wtedy silnik jej nie dostaje, bo o niej nie wie).
fn pack_answer(sh: &Shared, p: &[u8]) -> bool {
    let Some((pushed, at)) = *lock(&sh.pack_push) else {
        return false;
    };
    let Ok(play::Serverbound::ResourcePack(r)) =
        play::Serverbound::decode(ids::play::serverbound::RESOURCE_PACK, p)
    else {
        log!("client: could not decode resource_pack answer");
        return false;
    };
    if r.id != pushed {
        return false;
    }
    log!("PACK RESULT: action {} after {:.0} ms", r.action, ms() - at);
    true
}

fn record(sh: &Shared, id: i32, p: &[u8]) {
    let b = body(id, p);
    let mut rec = lock(&sh.recorded);
    match cfg::Serverbound::decode(id, p) {
        Ok(cfg::Serverbound::ClientInformation(_)) => rec.client_information = Some(b),
        Ok(cfg::Serverbound::CustomPayload(c)) => rec.payloads.push((c.channel, b)),
        Ok(cfg::Serverbound::SelectKnownPacks(_)) => rec.known_packs = Some(b),
        Ok(cfg::Serverbound::ResourcePack(r)) => rec.pack_responses.push((r.id, b)),
        Ok(cfg::Serverbound::CookieResponse(c)) => rec.cookies.push((c.key, b)),
        Ok(cfg::Serverbound::AcceptCodeOfConduct(_)) => rec.accepted_conduct = true,
        Ok(_) => {}
        Err(e) => log!(
            "client: config packet {} did not decode: {e}",
            name(false, false, id)
        ),
    }
}

fn backend_loop(mut r: ConnReader, sh: Arc<Shared>, generation: u64, mut config: bool) {
    let tag = format!("engine#{generation}");
    let mut seen = HashSet::new();
    loop {
        let (id, mut p) = match r.recv() {
            Ok(x) => x,
            Err(e) => {
                if sh.generation.load(Ordering::SeqCst) == generation {
                    log!("{tag}: connection ended while active: {e}");
                } else {
                    log!("{tag}: closed after swap ({e})");
                }
                return;
            }
        };
        if sh.generation.load(Ordering::SeqCst) != generation {
            continue;
        }
        if seen.insert((config, id)) {
            log!(
                "{tag} -> client [{}] {}",
                if config { "config" } else { "play" },
                name(!config, true, id)
            );
        }
        if config {
            if id == ids::configuration::clientbound::FINISH_CONFIGURATION {
                config = false;
            }
        } else {
            use ids::play::clientbound as cb;
            if generation == 0
                && id == cb::LOGIN
                && let Some(raw) = p.get(0..4)
            {
                let pid = i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
                lock(&sh.ids).client = Some(pid);
                log!("{tag}: player entity id on the client = {pid}");
            }
            if generation > 0 && drop_after_swap(id, &p) {
                log!("{tag}: dropped {} after swap", name(true, true, id));
                continue;
            }
            let ids_now = *lock(&sh.ids);
            if let Some(f) = ids_now.swap_fn()
                && remap_clientbound(id, &mut p, &f).is_none()
            {
                log!("{tag}: could not remap {}", name(true, true, id));
            }
            track_entities(&sh, id, &p);
            let mut t = lock(&sh.timeline);
            let now = ms();
            t.last_from_backend = now;
            if generation > 0 {
                t.first_from_new.get_or_insert(now);
                if id == cb::PLAYER_POSITION {
                    t.position_from_new.get_or_insert(now);
                }
            }
        }
        let mut cw = lock(&sh.client);
        if sh.generation.load(Ordering::SeqCst) != generation {
            continue;
        }
        if let Err(e) = cw.send(&body(id, &p)) {
            log!("{tag} -> client: send failed: {e}");
            return;
        }
    }
}

/// Pakiety nowego silnika, których klient nie może zobaczyć po podmianie.
fn drop_after_swap(id: i32, p: &[u8]) -> bool {
    use ids::play::clientbound as cb;
    // GameEvent 13 = LEVEL_CHUNKS_LOAD_START (remote.rs:4479).
    (id == cb::GAME_EVENT && p.first() == Some(&13))
        || (id == cb::SYSTEM_CHAT && p.windows(25).any(|w| w == b"multiplayer.player.joined"))
}

// ---- Logowanie do silnika i konfiguracja w imieniu klienta ----

fn backend_login(port: u16, hello: &[u8]) -> R<Connection> {
    let stream = TcpStream::connect(("127.0.0.1", port))?;
    let mut c = Connection::from_stream(stream)?;
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: horizon_protocol::PROTOCOL_VERSION,
            host: "127.0.0.1".into(),
            port,
            intent: handshake::LOGIN,
        })
        .encode(),
    )?;
    c.send(hello)?;
    loop {
        let (id, p) = c.recv()?;
        match login::Clientbound::decode(id, &p)? {
            login::Clientbound::Compression(x) => c.set_compression(x.threshold),
            login::Clientbound::LoginFinished(_) => {
                c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())?;
                return Ok(c);
            }
            login::Clientbound::Disconnect(d) => {
                return Err(format!("login refused: {}", d.reason_json).into());
            }
            other => return Err(format!("unexpected login packet: {other:?}").into()),
        }
    }
}

fn configure(c: &mut Connection, rec: &Recorded) -> R<()> {
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
                let answers: Vec<_> = rec
                    .pack_responses
                    .iter()
                    .filter(|(pid, _)| *pid == x.id)
                    .collect();
                if answers.is_empty() {
                    log!(
                        "swap: new engine pushed a resource pack the client never answered: {}",
                        x.url
                    );
                }
                for (_, b) in answers {
                    c.send(b)?;
                }
            }
            Ok(cfg::Clientbound::CookieRequest(x)) => {
                for (key, b) in &rec.cookies {
                    if *key == x.key {
                        c.send(b)?;
                    }
                }
            }
            Ok(cfg::Clientbound::CodeOfConduct(_)) if rec.accepted_conduct => {
                c.send(&cfg::Serverbound::AcceptCodeOfConduct(Empty).encode())?;
            }
            Ok(cfg::Clientbound::Disconnect(d)) => {
                return Err(format!("configuration refused: {:?}", d.reason).into());
            }
            Ok(cfg::Clientbound::FinishConfiguration(_)) => {
                c.send(&cfg::Serverbound::FinishConfiguration(Empty).encode())?;
                return Ok(());
            }
            Ok(_) => {}
            Err(e) => log!(
                "swap: config packet {} did not decode: {e}",
                name(false, true, id)
            ),
        }
    }
}

fn swap(sh: &Arc<Shared>, args: &Args, active: &mut usize) -> R<()> {
    let (from, to) = (*active, 1 - *active);
    {
        let _client = lock(&sh.client);
        sh.generation.fetch_add(1, Ordering::SeqCst);
    }
    let generation = sh.generation.load(Ordering::SeqCst);
    *lock(&sh.backend) = None;
    {
        let mut t = lock(&sh.timeline);
        let last = t.last_from_backend;
        *t = Timeline {
            swap_at: Some(ms()),
            last_old_before_swap: last,
            last_from_backend: last,
            ..Timeline::default()
        };
    }
    log!("swap: engine {from} -> {to} (generation {generation})");
    // `stop` zgłasza koniec co ok. 0,2 s; świat jest wolny, gdy znika proces silnika
    // (`horizon-client --control-socket <dir>/.horizon/engine.json`).
    let t = ms();
    let mut stop = Command::new(&args.server_bin)
        .arg("--dir")
        .arg(&args.backends[from].dir)
        .arg("stop")
        .spawn()?;
    let pattern = format!(
        "control-socket {}/.horizon/engine.json",
        args.backends[from].dir.display()
    );
    let stop_deadline = Instant::now() + Duration::from_secs(30);
    while Command::new("pgrep")
        .arg("-f")
        .arg(&pattern)
        .output()?
        .status
        .success()
    {
        if Instant::now() > stop_deadline {
            return Err("old engine still running 30 s after stop".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    log!("swap: old engine process gone after {:.0} ms", ms() - t);
    thread::spawn(move || {
        let status = stop.wait();
        log!(
            "swap: horizon-server stop returned {status:?} after {:.0} ms",
            ms() - t
        );
    });
    // `start` zwraca wynik co ok. 0,5 s; proxy loguje się, gdy tylko port przyjmie połączenie.
    let t = ms();
    let mut child = Command::new(&args.server_bin)
        .arg("--dir")
        .arg(&args.backends[to].dir)
        .arg("start")
        .spawn()?;
    thread::spawn(move || {
        let status = child.wait();
        log!(
            "swap: horizon-server start returned {status:?} after {:.0} ms",
            ms() - t
        );
    });
    let rec = lock(&sh.recorded).clone();
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut conn = loop {
        match backend_login(args.backends[to].port, &rec.hello) {
            Ok(c) => break c,
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                return Err(format!("new engine did not accept the login in 25 s: {e}").into());
            }
        }
    };
    log!("swap: logged into engine {to}");
    configure(&mut conn, &rec)?;
    log!("swap: configuration with engine {to} finished");
    let server_id = loop {
        let (id, p) = conn.recv()?;
        if id == ids::play::clientbound::LOGIN {
            let raw = p.get(0..4).ok_or("short Login")?;
            break i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
        }
        log!("swap: dropped {} before Login", name(true, true, id));
    };
    conn.send(&play::Serverbound::PlayerLoaded(Empty).encode())?;
    let client_id = {
        let mut ids_now = lock(&sh.ids);
        ids_now.server = Some(server_id);
        ids_now.client
    };
    log!(
        "swap: Login from engine {to} swallowed, player id there = {server_id}, on the client = {client_id:?}"
    );
    let old: Vec<i32> = lock(&sh.entities)
        .drain()
        .filter(|e| Some(*e) != client_id)
        .collect();
    log!(
        "swap: removing {} entities of the old engine on the client",
        old.len()
    );
    lock(&sh.client)
        .send(&play::Clientbound::RemoveEntities(play::RemoveEntities { ids: old }).encode())?;
    let (reader, writer) = conn.split();
    *lock(&sh.backend) = Some(writer);
    let sh2 = Arc::clone(sh);
    thread::spawn(move || backend_loop(reader, sh2, generation, false));
    *active = to;
    Ok(())
}

// ---- Sesja jednego gracza ----

fn relay_status(mut client: Connection, port: u16) -> R<()> {
    let mut s = Connection::from_stream(TcpStream::connect(("127.0.0.1", port))?)?;
    s.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: horizon_protocol::PROTOCOL_VERSION,
            host: "127.0.0.1".into(),
            port,
            intent: handshake::STATUS,
        })
        .encode(),
    )?;
    for _ in 0..2 {
        let (id, p) = client.recv()?;
        s.send(&body(id, &p))?;
        let (id, p) = s.recv()?;
        client.send(&body(id, &p))?;
    }
    Ok(())
}

fn session(stream: TcpStream, args: &Args, active: &mut usize) -> R<()> {
    let mut client = Connection::from_stream(stream)?;
    let (id, p) = client.recv()?;
    let handshake::Serverbound::Intention(intent) = handshake::Serverbound::decode(id, &p)? else {
        return Err("expected a handshake".into());
    };
    if intent.intent == handshake::STATUS {
        return relay_status(client, args.backends[*active].port);
    }
    let (id, p) = client.recv()?;
    let login::Serverbound::Hello(hello) = login::Serverbound::decode(id, &p)? else {
        return Err("expected login Hello".into());
    };
    log!("session: {} logs in through engine {}", hello.name, *active);
    let hello_body = body(id, &p);
    let backend = backend_login(args.backends[*active].port, &hello_body)?;
    client.send(
        &login::Clientbound::LoginFinished(login::LoginFinished {
            profile: login::GameProfile {
                uuid: hello.uuid,
                name: hello.name.clone(),
                properties: vec![],
            },
            session: 0,
        })
        .encode(),
    )?;
    let (id, _) = client.recv()?;
    if id != ids::login::serverbound::LOGIN_ACKNOWLEDGED {
        return Err(format!("expected LoginAcknowledged, got id {id}").into());
    }
    let (client_r, client_w) = client.split();
    let (backend_r, backend_w) = backend.split();
    let sh = Arc::new(Shared {
        client: Mutex::new(client_w),
        backend: Mutex::new(Some(backend_w)),
        generation: AtomicU64::new(0),
        ids: Mutex::new(PlayerIds::default()),
        entities: Mutex::new(HashSet::new()),
        recorded: Mutex::new(Recorded {
            hello: hello_body,
            ..Recorded::default()
        }),
        timeline: Mutex::new(Timeline::default()),
        pack_push: Mutex::new(None),
    });
    let sh_b = Arc::clone(&sh);
    thread::spawn(move || backend_loop(backend_r, sh_b, 0, true));
    let sh_c = Arc::clone(&sh);
    let client_thread = thread::spawn(move || client_loop(client_r, sh_c));
    while !client_thread.is_finished() {
        if let Some(f) = args.pack_file.as_ref().filter(|f| f.exists()) {
            let line = std::fs::read_to_string(f).unwrap_or_default();
            let _ = std::fs::remove_file(f);
            if let Err(e) = push_pack(&sh, &line) {
                log!("pack push FAILED: {e}");
            }
        }
        if args.swap_file.exists() {
            let _ = std::fs::remove_file(&args.swap_file);
            if let Err(e) = swap(&sh, args, active) {
                log!("swap FAILED: {e}");
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    log!("session: ended");
    Ok(())
}

fn main() {
    let _ = ms();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            log!("arguments: {e}");
            std::process::exit(2);
        }
    };
    let listener = match TcpListener::bind(&args.listen) {
        Ok(l) => l,
        Err(e) => {
            log!("cannot listen on {}: {e}", args.listen);
            std::process::exit(1);
        }
    };
    log!(
        "listening on {}, engine 0 on port {}",
        args.listen,
        args.backends[0].port
    );
    let mut active = 0;
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if let Err(e) = session(s, &args, &mut active) {
                    log!("session error: {e}");
                }
            }
            Err(e) => log!("accept failed: {e}"),
        }
    }
}
