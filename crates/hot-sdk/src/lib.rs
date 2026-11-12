//! PL: Biblioteka dla autorów modów gorących. Mod to typ z cechą [`HotMod`] i jedno wywołanie
//! [`hot_mod!`], które eksportuje tabelę C ABI z `hot-abi`. Każdy eksport łapie panikę: panika
//! w modzie nie przechodzi przez granicę i nie zabija silnika, tylko wyłącza tę generację (D4).
//! [`Host`] żyje tylko w trakcie wywołania, więc mod nie może go zachować na później.
//! Używają: mody gorące (`examples/`, repo serwera). Nie używa: `hot-core`, `hot-host`.
//!
//! EN: The library for hot mod authors. A mod is a type with the [`HotMod`] trait and one
//! [`hot_mod!`] call, which exports the C ABI table from `hot-abi`. Every export catches panics:
//! a panic in a mod does not cross the boundary or kill the engine, it disables that generation
//! (D4). [`Host`] lives only for the call, so a mod cannot keep it for later.
//!
//! ```ignore
//! struct Licznik { ticks: u64 }
//! impl hot_sdk::HotMod for Licznik {
//!     fn init(_host: &mut hot_sdk::Host<'_>, state: &[u8]) -> Self {
//!         Licznik { ticks: hot_sdk::state::u64_from(state) }
//!     }
//!     fn save(&mut self, _host: &mut hot_sdk::Host<'_>) -> Vec<u8> {
//!         self.ticks.to_le_bytes().to_vec()
//!     }
//! }
//! hot_sdk::hot_mod!(Licznik, id = "licznik", version = "0.1.0");
//! ```

use std::ffi::c_void;
use std::fmt;
use std::path::PathBuf;

pub use hot_abi;
use hot_abi::{HostApi, PlayerInfo, Str};

/// PL: Wynik obsługi zdarzenia. EN: The outcome of handling an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// PL: Dalej, jak bez moda. EN: Carry on as without the mod.
    Continue,
    /// PL: Mod przejął zdarzenie (komenda obsłużona, akcja anulowana).
    /// EN: The mod took the event (command handled, action cancelled).
    Cancel,
}

/// PL: Mod gorący. `init` dostaje stan poprzedniej generacji (pusty przy pierwszym starcie),
/// `save` oddaje stan przy wymianie na nową generację i musi zatrzymać własne wątki.
/// EN: A hot mod. `init` gets the previous generation's state (empty on first start), `save`
/// hands the state over when a new generation replaces it and must stop the mod's own threads.
pub trait HotMod: Sized + 'static {
    /// PL: Start generacji. EN: Starts a generation.
    fn init(host: &mut Host<'_>, state: &[u8]) -> Self;

    /// PL: Jedno zdarzenie z silnika. EN: One event from the engine.
    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        let _ = (host, event);
        Flow::Continue
    }

    /// PL: Koniec generacji: zatrzymaj wątki i timery, oddaj stan dla następnej.
    /// EN: End of the generation: stop threads and timers, return the state for the next one.
    fn save(&mut self, host: &mut Host<'_>) -> Vec<u8> {
        let _ = host;
        Vec::new()
    }

    /// PL: Zdarzenie w postaci z ABI. Domyślnie zamienia je i woła [`HotMod::on_event`].
    /// Nadpisuje tylko `hot-core`, który przekazuje zdarzenie modom bez zmian.
    /// EN: The event in its ABI form. By default converts it and calls [`HotMod::on_event`].
    /// Only `hot-core` overrides it, to pass the event to mods unchanged.
    #[doc(hidden)]
    fn on_raw_event(&mut self, host: &mut Host<'_>, event: &hot_abi::Event) -> Flow {
        // SAFETY: napisy zdarzenia są ważne przez całe wywołanie (kontrakt ABI).
        // EN: the event's strings are valid for the whole call (ABI contract).
        let event = unsafe { Event::from_abi(event) };
        self.on_event(host, &event)
    }
}

/// PL: UUID gracza. EN: A player's UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub [u8; 16]);

impl PlayerId {
    /// PL: Czy to „brak gracza” (same zera). EN: Whether this is "no player" (all zeros).
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.0 == [0; 16]
    }
}

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// PL: Gracz na serwerze. EN: A player on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    /// PL: Np. `minecraft:overworld`. EN: E.g. `minecraft:overworld`.
    pub dimension: String,
    pub position: [f64; 3],
    /// PL: Poziom operatora 0–4. EN: Operator level 0–4.
    pub permission: u8,
    /// PL: 0 przetrwanie, 1 kreatywny, 2 przygoda, 3 obserwator. EN: Game mode as in vanilla.
    pub game_mode: i32,
}

/// PL: Zdarzenie z silnika (D5). Nowe rodzaje trafiają do `Unknown`, więc stary mod działa dalej.
/// EN: An event from the engine (D5). New kinds land in `Unknown`, so an old mod keeps working.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Event<'a> {
    /// PL: Koniec ticku serwera; `tick` rośnie o 1. EN: End of a server tick; `tick` grows by 1.
    ServerTick {
        tick: u64,
    },
    PlayerJoined {
        player: PlayerId,
        name: &'a str,
    },
    PlayerLeft {
        player: PlayerId,
        name: &'a str,
    },
    /// PL: Odpowiedź przez [`Host::reply`]. EN: Answer through [`Host::reply`].
    Chat {
        player: PlayerId,
        name: &'a str,
        text: &'a str,
    },
    /// PL: Komenda gracza bez ukośnika, np. `sklep kup 3`. Odpowiedź przez [`Host::reply`].
    /// EN: A player's command without the slash, e.g. `sklep kup 3`. Answer via [`Host::reply`].
    Command {
        player: PlayerId,
        name: &'a str,
        command: &'a str,
    },
    /// PL: Komenda konsoli serwera bez ukośnika. EN: A server console command without the slash.
    ConsoleCommand {
        command: &'a str,
    },
    UseItem {
        player: PlayerId,
        item: &'a str,
    },
    UseItemOn {
        player: PlayerId,
        item: &'a str,
        pos: [i32; 3],
        face: [i32; 3],
    },
    RightClickBlock {
        player: PlayerId,
        pos: [i32; 3],
        face: [i32; 3],
    },
    LeftClickBlock {
        player: PlayerId,
        pos: [i32; 3],
        face: [i32; 3],
        block: &'a str,
    },
    PlaceBlock {
        player: PlayerId,
        item: &'a str,
        pos: [i32; 3],
        block: &'a str,
    },
    DestroyBlock {
        player: PlayerId,
        pos: [i32; 3],
        block: &'a str,
    },
    RandomTick {
        dimension: &'a str,
        pos: [i32; 3],
        block: &'a str,
    },
    /// PL: Dane serwera wczytane (`reload` po komendzie `/reload`). EN: Server data loaded.
    DataLoaded {
        reload: bool,
    },
    /// PL: Rodzaj nieznany tej wersji `hot-sdk`. EN: A kind unknown to this `hot-sdk` version.
    Unknown {
        kind: u32,
    },
}

impl<'a> Event<'a> {
    /// PL: Zamiana zdarzenia z ABI. EN: Converts an ABI event.
    ///
    /// # Safety
    /// PL: Napisy w `e` są ważne przez `'a`. EN: The strings in `e` are valid for `'a`.
    #[must_use]
    pub unsafe fn from_abi(e: &hot_abi::Event) -> Event<'a> {
        use hot_abi::kind;
        // SAFETY: wymaganie funkcji. EN: the function's contract.
        let s = |x: Str| -> &'a str { unsafe { x.as_str() }.unwrap_or("") };
        let player = PlayerId(e.player);
        match e.kind {
            kind::SERVER_TICK => Event::ServerTick { tick: e.number },
            kind::PLAYER_JOINED => Event::PlayerJoined {
                player,
                name: s(e.name),
            },
            kind::PLAYER_LEFT => Event::PlayerLeft {
                player,
                name: s(e.name),
            },
            kind::PLAYER_CHAT => Event::Chat {
                player,
                name: s(e.name),
                text: s(e.text),
            },
            kind::PLAYER_COMMAND => Event::Command {
                player,
                name: s(e.name),
                command: s(e.text),
            },
            kind::CONSOLE_COMMAND => Event::ConsoleCommand { command: s(e.text) },
            kind::USE_ITEM => Event::UseItem {
                player,
                item: s(e.item),
            },
            kind::USE_ITEM_ON => Event::UseItemOn {
                player,
                item: s(e.item),
                pos: e.pos,
                face: e.face,
            },
            kind::RIGHT_CLICK_BLOCK => Event::RightClickBlock {
                player,
                pos: e.pos,
                face: e.face,
            },
            kind::LEFT_CLICK_BLOCK => Event::LeftClickBlock {
                player,
                pos: e.pos,
                face: e.face,
                block: s(e.block),
            },
            kind::PLACE_BLOCK => Event::PlaceBlock {
                player,
                item: s(e.item),
                pos: e.pos,
                block: s(e.block),
            },
            kind::DESTROY_BLOCK => Event::DestroyBlock {
                player,
                pos: e.pos,
                block: s(e.block),
            },
            kind::RANDOM_TICK => Event::RandomTick {
                dimension: s(e.text),
                pos: e.pos,
                block: s(e.block),
            },
            kind::DATA_LOADED => Event::DataLoaded {
                reload: e.number == 1,
            },
            other => Event::Unknown { kind: other },
        }
    }
}

/// PL: Funkcje gospodarza na czas jednego wywołania (`HostCtx<'call>` z D4).
/// EN: Host functions for one call (`HostCtx<'call>` from D4).
pub struct Host<'call> {
    api: &'call HostApi,
}

impl<'call> Host<'call> {
    /// PL: Opakowuje tabelę gospodarza. EN: Wraps the host table.
    #[must_use]
    pub fn new(api: &'call HostApi) -> Self {
        Host { api }
    }

    /// PL: Surowa tabela gospodarza; potrzebna tylko `hot-core`, który buduje z niej tabele modów.
    /// EN: The raw host table; only `hot-core` needs it, to build the mods' tables from it.
    #[doc(hidden)]
    #[must_use]
    pub fn raw(&self) -> &'call HostApi {
        self.api
    }

    /// PL: Wpis informacyjny w logu silnika. EN: An info line in the engine's log.
    pub fn info(&mut self, message: &str) {
        (self.api.log)(self.api.ctx, hot_abi::level::INFO, Str::from_str(message));
    }

    /// PL: Ostrzeżenie w logu silnika. EN: A warning in the engine's log.
    pub fn warn(&mut self, message: &str) {
        (self.api.log)(self.api.ctx, hot_abi::level::WARN, Str::from_str(message));
    }

    /// PL: Błąd w logu silnika. EN: An error in the engine's log.
    pub fn error(&mut self, message: &str) {
        (self.api.log)(self.api.ctx, hot_abi::level::ERROR, Str::from_str(message));
    }

    /// PL: Linia na czacie gracza; `false`, gdy go nie ma. EN: A chat line to a player.
    pub fn tell(&mut self, player: PlayerId, message: &str) -> bool {
        (self.api.tell)(self.api.ctx, &player.0, Str::from_str(message))
    }

    /// PL: Linia na czacie wszystkich graczy. EN: A chat line to every player.
    pub fn broadcast(&mut self, message: &str) {
        (self.api.broadcast)(self.api.ctx, Str::from_str(message));
    }

    /// PL: Odpowiedź na bieżącą komendę albo wiadomość. EN: The answer to the current command.
    pub fn reply(&mut self, message: &str) {
        (self.api.reply)(self.api.ctx, Str::from_str(message));
    }

    /// PL: Komenda jak z konsoli (bez ukośnika). Nie działa w zdarzeniach gracza.
    /// EN: A command as from the console (no slash). Not available in player events.
    pub fn run_command(&mut self, line: &str) -> bool {
        (self.api.run_command)(self.api.ctx, Str::from_str(line))
    }

    /// PL: Gracze na serwerze. EN: The players on the server.
    #[must_use]
    pub fn players(&mut self) -> Vec<Player> {
        extern "C" fn each(user: *mut c_void, p: &PlayerInfo) {
            // SAFETY: `user` to `&mut Vec<Player>` z wywołania niżej; napisy ważne w wywołaniu.
            // EN: `user` is the `&mut Vec<Player>` from the call below; strings valid in the call.
            let out = unsafe { &mut *user.cast::<Vec<Player>>() };
            let text = |x: Str| String::from_utf8_lossy(unsafe { x.bytes() }).into_owned();
            out.push(Player {
                id: PlayerId(p.uuid),
                name: text(p.name),
                dimension: text(p.dimension),
                position: p.position,
                permission: p.permission,
                game_mode: p.game_mode,
            });
        }
        let mut out: Vec<Player> = Vec::new();
        (self.api.players)(self.api.ctx, each, (&raw mut out).cast());
        out
    }

    /// PL: Blok w danym miejscu, np. `minecraft:stone`. EN: The block at a position.
    #[must_use]
    pub fn block(&mut self, pos: [i32; 3]) -> String {
        let mut out = String::new();
        (self.api.block)(self.api.ctx, &pos, write_string, (&raw mut out).cast());
        out
    }

    /// PL: Katalog trwałych danych tego moda (`hot-data/<mod>/`). EN: This mod's data folder.
    #[must_use]
    pub fn data_dir(&mut self) -> PathBuf {
        let mut out = String::new();
        (self.api.data_dir)(self.api.ctx, write_string, (&raw mut out).cast());
        PathBuf::from(out)
    }
}

extern "C" fn write_string(user: *mut c_void, text: Str) {
    // SAFETY: `user` to `&mut String` z wywołującego; napis ważny w wywołaniu.
    // EN: `user` is the caller's `&mut String`; the string is valid in the call.
    let out = unsafe { &mut *user.cast::<String>() };
    *out = String::from_utf8_lossy(unsafe { text.bytes() }).into_owned();
}

/// PL: Pomocnicze odczyty stanu. EN: State reading helpers.
pub mod state {
    /// PL: `u64` zapisany jako 8 bajtów little-endian; brak albo zła długość daje 0.
    /// EN: A `u64` saved as 8 little-endian bytes; missing or wrong length gives 0.
    #[must_use]
    pub fn u64_from(state: &[u8]) -> u64 {
        state.try_into().map(u64::from_le_bytes).unwrap_or(0)
    }
}

/// PL: Części dla makra [`hot_mod!`]; nie do bezpośredniego użycia.
/// EN: Parts for the [`hot_mod!`] macro; not for direct use.
#[doc(hidden)]
pub mod __rt {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::panic::{self, catch_unwind, AssertUnwindSafe};
    use std::sync::Once;

    use hot_abi::{flow, level, Buf, HostApi, Str};

    use crate::{Flow, Host, HotMod};

    thread_local! {
        static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// PL: Hak paniki w kopii biblioteki standardowej tego moda (każdy `cdylib` ma własną, więc
    /// hak nie dotyka silnika). Zapamiętuje miejsce i treść paniki, żeby gospodarz dostał je w
    /// logu: bez tego wpis o panice nie miałby pliku i linii (D27). Poprzedni hak dalej pisze na
    /// stderr.
    ///
    /// EN: A panic hook in this mod's own copy of the standard library (every `cdylib` has one,
    /// so the hook does not touch the engine). Remembers the panic's place and message so the host
    /// gets them in the log: without it the panic entry would lack file and line (D27). The
    /// previous hook still prints to stderr.
    fn install_hook() {
        static HOOK: Once = Once::new();
        HOOK.call_once(|| {
            let previous = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                let payload = info.payload();
                let message = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-text panic payload".to_owned());
                let place = info
                    .location()
                    .map_or_else(|| "unknown place".to_owned(), ToString::to_string);
                let backtrace = std::backtrace::Backtrace::force_capture();
                let text = format!("panicked at {place}: {message}\n{backtrace}");
                LAST_PANIC.with(|p| *p.borrow_mut() = Some(text));
                previous(info);
            }));
        });
    }

    /// PL: Oddaje gospodarzowi opis ostatniej paniki jako błąd w logu. Pierwsza linia zaczyna się
    /// od `panicked at`, dalej backtrace.
    /// EN: Hands the last panic's description to the host as an error log line. The first line
    /// starts with `panicked at`, the backtrace follows.
    fn report_panic(api: &HostApi) {
        let text = LAST_PANIC
            .with(|p| p.borrow_mut().take())
            .unwrap_or_else(|| "panicked at unknown place: no panic hook message".to_owned());
        (api.log)(api.ctx, level::ERROR, Str::from_str(&text));
    }

    pub extern "C" fn init<T: HotMod>(api: &HostApi, prev: Str) -> *mut c_void {
        install_hook();
        catch_unwind(AssertUnwindSafe(|| {
            let mut host = Host::new(api);
            // SAFETY: stan pożyczony na czas wywołania (kontrakt ABI). EN: borrowed for the call.
            let state = unsafe { prev.bytes() };
            Box::into_raw(Box::new(T::init(&mut host, state))).cast::<c_void>()
        }))
        .unwrap_or_else(|_| {
            report_panic(api);
            std::ptr::null_mut()
        })
    }

    pub extern "C" fn on_event<T: HotMod>(
        instance: *mut c_void,
        api: &HostApi,
        event: &hot_abi::Event,
    ) -> u32 {
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: `instance` pochodzi z `init::<T>` i gospodarz woła mod z jednego wątku.
            // EN: `instance` comes from `init::<T>` and the host calls the mod from one thread.
            let this = unsafe { &mut *instance.cast::<T>() };
            let mut host = Host::new(api);
            match this.on_raw_event(&mut host, event) {
                Flow::Continue => flow::CONTINUE,
                Flow::Cancel => flow::CANCEL,
            }
        }))
        .unwrap_or_else(|_| {
            report_panic(api);
            flow::PANICKED
        })
    }

    pub extern "C" fn deactivate<T: HotMod>(instance: *mut c_void, api: &HostApi) -> Buf {
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: jak w `on_event`; po tym wywołaniu gospodarz nie używa `instance`.
            // EN: as in `on_event`; the host never uses `instance` after this call.
            let mut this = unsafe { Box::from_raw(instance.cast::<T>()) };
            let mut host = Host::new(api);
            let mut state = std::mem::ManuallyDrop::new(this.save(&mut host));
            drop(this);
            Buf {
                ptr: state.as_mut_ptr(),
                len: state.len(),
                cap: state.capacity(),
            }
        }))
        .unwrap_or_else(|_| {
            report_panic(api);
            Buf::EMPTY
        })
    }

    pub extern "C" fn free_buf(buf: Buf) {
        if buf.ptr.is_null() {
            return;
        }
        // SAFETY: bufor powstał w `deactivate` z `Vec<u8>` tego moda. EN: made by `deactivate`.
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

/// PL: Eksportuje mod: `hot_mod!(Typ, id = "nazwa", version = "0.1.0")`. Jedno wywołanie na crate.
/// EN: Exports a mod: `hot_mod!(Type, id = "name", version = "0.1.0")`. One call per crate.
#[macro_export]
macro_rules! hot_mod {
    ($ty:ty, id = $id:expr, version = $version:expr) => {
        /// PL: Tabela moda dla gospodarza. EN: The mod's table for the host.
        #[no_mangle]
        pub extern "C" fn hot_mod_entry() -> *const $crate::hot_abi::ModVTable {
            static VTABLE: $crate::hot_abi::ModVTable = $crate::hot_abi::ModVTable {
                abi: $crate::hot_abi::ABI_MAJOR,
                id: $crate::hot_abi::Str::from_str($id),
                version: $crate::hot_abi::Str::from_str($version),
                init: $crate::__rt::init::<$ty>,
                on_event: $crate::__rt::on_event::<$ty>,
                deactivate: $crate::__rt::deactivate::<$ty>,
                free_buf: $crate::__rt::free_buf,
            };
            &VTABLE
        }
    };
}
