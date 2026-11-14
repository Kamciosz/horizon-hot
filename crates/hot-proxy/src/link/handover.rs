//! PL: Gracz przy podmianie programu proxy (M13.1, ADR 0033). Stary program zatrzymuje odczyt od
//! gracza między pakietami (`freeze`), zapisuje stan połączenia (`snapshot`), a nowy odtwarza go
//! z tego samego gniazda (`restore`). Szyfrowanie idzie dalej od zapisanych rejestrów, a do silnika
//! gracz loguje się od nowa, tą samą drogą co po awarii silnika. Całość prowadzi `crate::upgrade`.
//!
//! EN: A player during a proxy program upgrade (M13.1, ADR 0033). The old program stops reading from
//! the player between packets (`freeze`), saves the connection state (`snapshot`), and the new one
//! rebuilds it from the same socket (`restore`). Encryption goes on from the saved registers, and
//! the player logs in to the engine anew, the same way as after an engine crash. `crate::upgrade`
//! runs the whole thing.

use super::{lock, Link, Recorded, Rejoin, State};
use crate::client_io::{ClientConn, ClientReader, ClientWriter, Handover};
use crate::session::{Profile, Proxy, ENGINE_LOST_WAIT};
use horizon_protocol::crypto::uuid_string;
use horizon_protocol::packets::{handshake::Intention, login::Property};
use serde_json::{json, Value};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

/// PL: Co tyle odczyt od gracza sprawdza, czy ma stanąć. EN: How often the player reader checks
/// whether to stop.
pub(super) const POLL: Duration = Duration::from_millis(50);

/// PL: Zatrzymany odczyt od gracza. EN: A stopped player reader.
pub(crate) struct Parked {
    pub(crate) reader: ClientReader,
    /// PL: Gracz był w konfiguracji. EN: The player was in configuration.
    pub(crate) config: bool,
}

/// PL: Prośba o postój i miejsce na zatrzymany odczyt. EN: The stop request and the place for the
/// stopped reader.
#[derive(Default)]
pub(super) struct Freeze {
    wanted: AtomicBool,
    parked: Mutex<Option<Parked>>,
    changed: Condvar,
}

impl Freeze {
    pub(super) fn wanted(&self) -> bool {
        self.wanted.load(Ordering::SeqCst)
    }

    /// PL: Oddaje odczyt i czeka, aż wróci (po nieudanej podmianie). Po udanej proces znika.
    /// EN: Hands the reader over and waits for it to come back (after a failed upgrade). After a
    /// successful one the process is gone.
    pub(super) fn park(&self, parked: Parked) -> Parked {
        let mut slot = lock(&self.parked);
        *slot = Some(parked);
        self.changed.notify_all();
        loop {
            if !self.wanted() {
                if let Some(p) = slot.take() {
                    return p;
                }
            }
            slot = self
                .changed
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Link {
    pub(crate) fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// PL: Prosi odczyt od gracza o postój. EN: Asks the player reader to stop.
    pub(crate) fn request_freeze(&self) {
        self.freeze.wanted.store(true, Ordering::SeqCst);
    }

    /// PL: Zatrzymany odczyt albo `None`, gdy nie stanął do `deadline`.
    /// EN: The stopped reader, or `None` when it did not stop by `deadline`.
    pub(crate) fn frozen(&self, deadline: Instant) -> Option<Parked> {
        let mut slot = lock(&self.freeze.parked);
        loop {
            if let Some(p) = slot.take() {
                return Some(p);
            }
            if self.ended() {
                return None;
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            slot = match self.freeze.changed.wait_timeout(slot, left) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            };
        }
    }

    /// PL: Oddaje odczyt i puszcza gracza dalej. EN: Gives the reader back and lets the player go on.
    pub(crate) fn thaw(&self, parked: Option<Parked>) {
        let mut slot = lock(&self.freeze.parked);
        if parked.is_some() {
            *slot = parked;
        }
        self.freeze.wanted.store(false, Ordering::SeqCst);
        self.freeze.changed.notify_all();
    }

    /// PL: Zapis do gracza; dopóki trzymany, nic do gracza nie idzie. `None` po `deadline`: zapis
    /// może stać w `write` do gracza, który przestał czytać.
    /// EN: Writing to the player; while held, nothing goes to the player. `None` after `deadline`:
    /// the writer may hang in `write` to a player who stopped reading.
    pub(crate) fn writer_until(&self, deadline: Instant) -> Option<MutexGuard<'_, ClientWriter>> {
        loop {
            match self.client.try_lock() {
                Ok(held) => return Some(held),
                Err(TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(TryLockError::WouldBlock) => return None,
            }
        }
    }

    /// PL: Stan gracza dla nowego programu. Wołać z zatrzymanym odczytem i trzymanym zapisem.
    /// EN: The player's state for the new program. Call with the reader stopped and the writer held.
    pub(crate) fn snapshot(&self, parked: &Parked, writer: &ClientWriter) -> Value {
        let (secret, read_register, pending, threshold) = parked.reader.handover();
        let st = lock(&self.state);
        let r = &st.recorded;
        let pairs = |list: &[(String, Vec<u8>)]| -> Vec<Value> {
            list.iter().map(|(k, b)| json!([k, hex(b)])).collect()
        };
        json!({
            "fd": parked.reader.stream().as_raw_fd(),
            "name": self.profile.name,
            "uuid": format!("{:032x}", self.profile.uuid),
            "properties": self.profile.properties.iter()
                .map(|p| json!({ "name": p.name, "value": p.value, "signature": p.signature }))
                .collect::<Vec<_>>(),
            "intent": {
                "protocol": self.intent.protocol,
                "host": self.intent.host,
                "port": self.intent.port,
                "intent": self.intent.intent,
            },
            "address": self.address.to_string(),
            "secret": secret.map(|s| hex(&s)),
            "read_register": hex(&read_register),
            "write_register": hex(&writer.register()),
            "pending": hex(&pending),
            "threshold": threshold,
            "client_id": st.client_id,
            "entities": st.entities.iter().collect::<Vec<_>>(),
            "chat_next": st.chat_next,
            "bundle_open": st.bundle_open,
            "own_keep_alive": st.own_keep_alive,
            "recorded": {
                "client_information": r.client_information.as_deref().map(hex),
                "payloads": pairs(&r.payloads),
                "known_packs": r.known_packs.as_deref().map(hex),
                "pack_responses": r.pack_responses.iter()
                    .map(|(id, b)| json!([format!("{id:032x}"), hex(b)]))
                    .collect::<Vec<_>>(),
                "cookies": pairs(&r.cookies),
                "accepted_conduct": r.accepted_conduct,
            },
        })
    }

    /// PL: Odtwarza gracza z `snapshot` w nowym programie: gracz czeka w przerwie, a do silnika
    /// loguje się od nowa, jak po awarii silnika. Zwraca nazwę gracza.
    /// EN: Rebuilds a player from `snapshot` in the new program: the player waits in a gap and
    /// logs in to the engine anew, as after an engine crash. Returns the player's name.
    ///
    /// # Errors
    /// PL: Stan nie pasuje do tego formatu; gniazdo gracza zostaje zamknięte.
    /// EN: The state does not fit this format; the player's socket is closed.
    pub(crate) fn restore(proxy: &Arc<Proxy>, v: &Value) -> Result<String, String> {
        let f = Fields(v);
        let fd = RawFd::try_from(f.int("fd")?).map_err(|e| e.to_string())?;
        // SAFETY: stary program otworzył to gniazdo dla nas i podał jego numer w stanie; nic
        // innego w tym procesie go nie posiada. EN: the old program left this socket open for us
        // and named it in the state; nothing else in this process owns it.
        let stream = unsafe { TcpStream::from_raw_fd(fd) };
        crate::upgrade::set_cloexec(fd, true).map_err(|e| e.to_string())?;
        let (link, reader) = Self::rebuild(stream, &f)?;
        let name = link.name.clone();
        let uuid = uuid_string(link.profile.uuid);
        link.begin_gap();
        proxy.register(&link);
        let (l, p) = (Arc::clone(&link), Arc::clone(proxy));
        thread::spawn(move || {
            let why = l.client_loop(reader, false);
            l.end(why);
            let why = l.why.get().cloned().unwrap_or_default();
            p.log.info(
                "A player left the proxy",
                json!({ "player": l.name, "uuid": uuid, "reason": why }),
            );
        });
        let (l, p) = (link, Arc::clone(proxy));
        thread::spawn(move || {
            if let Rejoin::Failed(why) = l.rejoin(&p, ENGINE_LOST_WAIT) {
                p.lost(&l, &why, p.engine());
            }
        });
        Ok(name)
    }

    fn rebuild(stream: TcpStream, f: &Fields) -> Result<(Arc<Self>, ClientReader), String> {
        let secret = match f.opt_bytes("secret")? {
            Some(s) => Some(sixteen(&s)?),
            None => None,
        };
        let handover = Handover {
            secret,
            read_register: sixteen(&f.bytes("read_register")?)?,
            write_register: sixteen(&f.bytes("write_register")?)?,
            pending: f.bytes("pending")?,
            threshold: f
                .0
                .get("threshold")
                .and_then(Value::as_u64)
                .and_then(|t| usize::try_from(t).ok()),
        };
        let (reader, writer) = ClientConn::resume(stream, handover)
            .map_err(|e| e.to_string())?
            .split();
        let client_closer = reader.closer().map_err(|e| e.to_string())?;
        reader
            .set_read_timeout(Some(POLL))
            .map_err(|e| e.to_string())?;
        reader
            .set_write_timeout(Some(super::CLIENT_WRITE))
            .map_err(|e| e.to_string())?;
        let mut properties = Vec::new();
        for p in f.list("properties")? {
            let p = Fields(p);
            properties.push(Property {
                name: p.str("name")?,
                value: p.str("value")?,
                signature: p.0.get("signature").and_then(Value::as_str).map(Into::into),
            });
        }
        let intent = Fields(&f.0["intent"]);
        let rec = Fields(&f.0["recorded"]);
        let pairs = |key: &str| -> Result<Vec<(String, Vec<u8>)>, String> {
            rec.list(key)?
                .iter()
                .map(|kv| Ok((Fields(kv).at_str(0)?, Fields(kv).at_bytes(1)?)))
                .collect()
        };
        let state = State {
            client_id: f.opt_i32("client_id")?,
            entities: f
                .list("entities")?
                .iter()
                .filter_map(|e| e.as_i64().and_then(|e| i32::try_from(e).ok()))
                .collect(),
            recorded: Recorded {
                client_information: rec.opt_bytes("client_information")?,
                payloads: pairs("payloads")?,
                known_packs: rec.opt_bytes("known_packs")?,
                pack_responses: rec
                    .list("pack_responses")?
                    .iter()
                    .map(|kv| {
                        let id = u128::from_str_radix(&Fields(kv).at_str(0)?, 16)
                            .map_err(|e| e.to_string())?;
                        Ok((id, Fields(kv).at_bytes(1)?))
                    })
                    .collect::<Result<_, String>>()?,
                cookies: pairs("cookies")?,
                accepted_conduct: rec.0["accepted_conduct"].as_bool() == Some(true),
            },
            chat_next: f.opt_i32("chat_next")?.unwrap_or(0),
            bundle_open: f.0["bundle_open"].as_bool() == Some(true),
            own_keep_alive: f.int("own_keep_alive")?,
            ..State::default()
        };
        let profile = Profile {
            uuid: u128::from_str_radix(&f.str("uuid")?, 16).map_err(|e| e.to_string())?,
            name: f.str("name")?,
            properties,
        };
        let link = Arc::new(Self {
            name: profile.name.clone(),
            profile,
            intent: Intention {
                protocol: i32::try_from(intent.int("protocol")?).map_err(|e| e.to_string())?,
                host: intent.str("host")?,
                port: u16::try_from(intent.int("port")?).map_err(|e| e.to_string())?,
                intent: i32::try_from(intent.int("intent")?).map_err(|e| e.to_string())?,
            },
            address: f.str("address")?.parse().map_err(|e| format!("{e}"))?,
            client: Mutex::new(writer),
            client_closer,
            engine: Mutex::new(None),
            generation: AtomicU64::new(1),
            state: Mutex::new(state),
            ended: AtomicBool::new(false),
            why: OnceLock::new(),
            freeze: Freeze::default(),
        });
        Ok((link, reader))
    }
}

/// PL: Pola stanu z czytelnym błędem. EN: State fields with a readable error.
struct Fields<'a>(&'a Value);

impl Fields<'_> {
    fn get(&self, key: &str) -> Result<&Value, String> {
        self.0
            .get(key)
            .ok_or_else(|| format!("the state has no {key}"))
    }
    fn str(&self, key: &str) -> Result<String, String> {
        self.get(key)?
            .as_str()
            .map(Into::into)
            .ok_or_else(|| format!("{key} is not text"))
    }
    fn int(&self, key: &str) -> Result<i64, String> {
        self.get(key)?
            .as_i64()
            .ok_or_else(|| format!("{key} is not a number"))
    }
    fn opt_i32(&self, key: &str) -> Result<Option<i32>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| format!("{key} is not a 32-bit number")),
        }
    }
    fn bytes(&self, key: &str) -> Result<Vec<u8>, String> {
        unhex(&self.str(key)?).ok_or_else(|| format!("{key} is not hex"))
    }
    fn opt_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.bytes(key).map(Some),
        }
    }
    fn list(&self, key: &str) -> Result<&Vec<Value>, String> {
        self.get(key)?
            .as_array()
            .ok_or_else(|| format!("{key} is not a list"))
    }
    fn at_str(&self, i: usize) -> Result<String, String> {
        self.0
            .get(i)
            .and_then(Value::as_str)
            .map(Into::into)
            .ok_or_else(|| format!("item {i} is not text"))
    }
    fn at_bytes(&self, i: usize) -> Result<Vec<u8>, String> {
        unhex(&self.at_str(i)?).ok_or_else(|| format!("item {i} is not hex"))
    }
}

fn sixteen(b: &[u8]) -> Result<[u8; 16], String> {
    b.try_into()
        .map_err(|_| format!("a key or register has {} bytes, not 16", b.len()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&bytes), "0001abff");
        assert_eq!(unhex("0001abff").as_deref(), Some(&bytes[..]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
