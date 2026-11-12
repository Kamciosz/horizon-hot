//! PL: Stabilna granica C ABI między `hot-host` (mod natywny w silniku), `hot-core` i modami
//! gorącymi (`.so` na serwerze, `.dylib` w testach na macOS). Strony budują się osobno, każda z
//! własną biblioteką standardową i stertą, więc łączy je tylko to, co tu zdefiniowane (D3).
//! Ten sam kontrakt obowiązuje na obu piętrach: `hot-host` ładuje `hot-core` jak mod, a `hot-core`
//! ładuje mody gorące. Zasady: napisy pożyczane na czas wywołania (`Str`), bufor zwraca ta strona,
//! która go przydzieliła (`Buf` + `free_buf`), `HostApi` ważne tylko w trakcie wywołania.
//! Crate bez zależności: trafia też do paczki `hot-host` jako `[[crate]]` (S1).
//!
//! EN: Stable C ABI boundary between `hot-host` (the native mod inside the engine), `hot-core` and
//! hot mods (`.so` on the server, `.dylib` in macOS tests). The sides are built separately, each
//! with its own standard library and heap, so only what is defined here connects them (D3). The
//! same contract holds on both levels: `hot-host` loads `hot-core` as a mod, and `hot-core` loads
//! hot mods. Rules: strings are borrowed for the call (`Str`), a buffer goes back to the side that
//! allocated it (`Buf` + `free_buf`), `HostApi` is valid only during the call.
//! No dependencies: this crate is also copied into the `hot-host` bundle as a `[[crate]]` (S1).

#![no_std]

use core::ffi::c_void;

/// PL: Wersja główna ABI. Gospodarz odrzuca mod, który zgłasza inną wartość.
/// Zmiana niezgodna wstecz w tym crate = podbicie tej liczby.
///
/// EN: Major ABI version. The host rejects a mod that reports a different value.
/// Any backwards-incompatible change in this crate bumps this number.
pub const ABI_MAJOR: u32 = 1;

/// PL: Nazwa eksportu, który zwraca `*const ModVTable`. EN: Export name returning `*const ModVTable`.
pub const ENTRY_SYMBOL: &[u8] = b"hot_mod_entry\0";

/// PL: Pożyczony napis UTF-8 (wskaźnik + długość), ważny tylko w trakcie wywołania.
/// EN: A borrowed UTF-8 string (pointer + length), valid only during the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    /// PL: Pusty napis. EN: An empty string.
    pub const EMPTY: Str = Str {
        ptr: core::ptr::null(),
        len: 0,
    };

    /// PL: Pożycza `&str` na czas wywołania. EN: Borrows a `&str` for the call.
    #[must_use]
    pub const fn from_str(s: &str) -> Self {
        Str {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// PL: Pożycza bajty (np. stan moda) na czas wywołania. EN: Borrows bytes for the call.
    #[must_use]
    pub const fn from_bytes(b: &[u8]) -> Self {
        Str {
            ptr: b.as_ptr(),
            len: b.len(),
        }
    }

    /// PL: Bajty napisu.
    /// EN: The string's bytes.
    ///
    /// # Safety
    /// PL: `ptr` wskazuje `len` ważnych bajtów przez cały czas życia `'a` (albo `len == 0`).
    /// EN: `ptr` points to `len` valid bytes for the whole of `'a` (or `len == 0`).
    #[must_use]
    pub unsafe fn bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 || self.ptr.is_null() {
            return &[];
        }
        // SAFETY: wymaganie funkcji. EN: the function's contract.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// PL: Napis jako `&str`; zły UTF-8 daje `None`.
    /// EN: The string as `&str`; invalid UTF-8 gives `None`.
    ///
    /// # Safety
    /// PL: jak [`Str::bytes`]. EN: as [`Str::bytes`].
    #[must_use]
    pub unsafe fn as_str<'a>(self) -> Option<&'a str> {
        // SAFETY: wymaganie funkcji. EN: the function's contract.
        core::str::from_utf8(unsafe { self.bytes() }).ok()
    }
}

/// PL: Bufor przydzielony przez stronę, która go zwraca. Wraca do niej przez jej `free_buf`.
/// EN: A buffer allocated by the side that returns it. It goes back through that side's `free_buf`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Buf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl Buf {
    /// PL: Pusty bufor (nic do zwolnienia). EN: An empty buffer (nothing to free).
    pub const EMPTY: Buf = Buf {
        ptr: core::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
}

/// PL: UUID gracza jako 16 bajtów big-endian. EN: A player's UUID as 16 big-endian bytes.
pub type Uuid = [u8; 16];

/// PL: Wynik zdarzenia. EN: An event's outcome.
pub mod flow {
    /// PL: Dalej, jak bez moda. EN: Carry on as without the mod.
    pub const CONTINUE: u32 = 0;
    /// PL: Mod przejął zdarzenie (komenda obsłużona, akcja anulowana).
    /// EN: The mod took the event (command handled, action cancelled).
    pub const CANCEL: u32 = 1;
    /// PL: Mod spanikował; `hot-sdk` złapał panikę. Gospodarz wyłącza tę generację (D4).
    /// EN: The mod panicked; `hot-sdk` caught it. The host disables this generation (D4).
    pub const PANICKED: u32 = 0xFFFF;
}

/// PL: Rodzaj zdarzenia (D5). Nieznany rodzaj mod pomija i zwraca `CONTINUE`, więc nowe rodzaje
/// nie łamią ABI.
/// EN: The event kind (D5). A mod skips an unknown kind and returns `CONTINUE`, so new kinds do
/// not break the ABI.
pub mod kind {
    /// PL: Koniec ticku serwera, świat działa. EN: End of a server tick, world running.
    pub const SERVER_TICK: u32 = 1;
    /// PL: Gracz wszedł (`player`, `name`). EN: A player joined (`player`, `name`).
    pub const PLAYER_JOINED: u32 = 2;
    /// PL: Gracz wyszedł (`player`, `name`). EN: A player left (`player`, `name`).
    pub const PLAYER_LEFT: u32 = 3;
    /// PL: Wiadomość na czacie (`player`, `name`, `text`). EN: A chat message.
    pub const PLAYER_CHAT: u32 = 4;
    /// PL: Komenda gracza bez ukośnika (`player`, `name`, `text`); odpowiedź przez `HostApi::reply`.
    /// EN: A player's command without the slash; answer through `HostApi::reply`.
    pub const PLAYER_COMMAND: u32 = 5;
    /// PL: Komenda z konsoli serwera bez ukośnika (`text`); odpowiedź przez `HostApi::reply`.
    /// EN: A server console command without the slash (`text`); answer through `HostApi::reply`.
    pub const CONSOLE_COMMAND: u32 = 6;
    /// PL: Użycie przedmiotu (`item`). EN: Item use (`item`).
    pub const USE_ITEM: u32 = 7;
    /// PL: Użycie przedmiotu na bloku (`item`, `pos`, `face`). EN: Item used on a block.
    pub const USE_ITEM_ON: u32 = 8;
    /// PL: Prawy klik na bloku (`pos`, `face`). EN: Right click on a block.
    pub const RIGHT_CLICK_BLOCK: u32 = 9;
    /// PL: Lewy klik na bloku (`pos`, `face`, `block`). EN: Left click on a block.
    pub const LEFT_CLICK_BLOCK: u32 = 10;
    /// PL: Postawienie bloku (`item`, `pos`, `block`). EN: A block placed.
    pub const PLACE_BLOCK: u32 = 11;
    /// PL: Zniszczenie bloku (`pos`, `block`). EN: A block destroyed.
    pub const DESTROY_BLOCK: u32 = 12;
    /// PL: Losowy tick bloku (`pos`, `block`, `text` = wymiar). EN: A random block tick.
    pub const RANDOM_TICK: u32 = 13;
    /// PL: Dane serwera wczytane lub przeładowane (`number` = 1 przy `reload`).
    /// EN: Server data loaded or reloaded (`number` = 1 on `reload`).
    pub const DATA_LOADED: u32 = 14;
}

/// PL: Zdarzenie. Pola, których rodzaj nie używa, są puste. Napisy pożyczone na czas wywołania.
/// EN: An event. Fields its kind does not use are empty. Strings are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub kind: u32,
    /// PL: Gracz, którego dotyczy zdarzenie (zera: brak). EN: The player concerned (zeros: none).
    pub player: Uuid,
    pub name: Str,
    /// PL: Treść czatu, komendy albo wymiar. EN: Chat text, command, or dimension.
    pub text: Str,
    /// PL: Przedmiot, np. `minecraft:stick`. EN: An item, e.g. `minecraft:stick`.
    pub item: Str,
    /// PL: Blok, np. `minecraft:stone`. EN: A block, e.g. `minecraft:stone`.
    pub block: Str,
    pub pos: [i32; 3],
    pub face: [i32; 3],
    /// PL: Liczba zależna od rodzaju (np. numer ticku). EN: A kind-specific number (e.g. tick).
    pub number: u64,
}

impl Event {
    /// PL: Zdarzenie danego rodzaju z pustymi polami. EN: An event of a kind with empty fields.
    #[must_use]
    pub const fn new(kind: u32) -> Self {
        Event {
            kind,
            player: [0; 16],
            name: Str::EMPTY,
            text: Str::EMPTY,
            item: Str::EMPTY,
            block: Str::EMPTY,
            pos: [0; 3],
            face: [0; 3],
            number: 0,
        }
    }
}

/// PL: Dane gracza przekazywane przez `HostApi::players`. EN: Player data from `HostApi::players`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PlayerInfo {
    pub uuid: Uuid,
    pub name: Str,
    pub dimension: Str,
    pub position: [f64; 3],
    /// PL: Poziom operatora 0–4. EN: Operator level 0–4.
    pub permission: u8,
    /// PL: 0 przetrwanie, 1 kreatywny, 2 przygoda, 3 obserwator. EN: Game mode as in vanilla.
    pub game_mode: i32,
}

/// PL: Poziomy logu w `HostApi::log`. EN: Log levels for `HostApi::log`.
pub mod level {
    pub const INFO: u32 = 0;
    pub const WARN: u32 = 1;
    pub const ERROR: u32 = 2;
}

/// PL: Funkcje gospodarza (D6) ważne tylko w trakcie wywołania moda. `ctx` przekazuje się
/// z powrotem do każdej funkcji. Wartość `false` znaczy: nie zadziałało albo nie wolno tutaj.
/// EN: Host functions (D6), valid only during the mod call. `ctx` goes back into every
/// function. `false` means: it did not work or is not allowed here.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostApi {
    pub ctx: *mut c_void,
    /// PL: Wpis do logu silnika. EN: A line in the engine's log.
    pub log: extern "C" fn(ctx: *mut c_void, level: u32, message: Str),
    /// PL: Linia na czacie jednego gracza. Tablice idą przez wskaźnik: w C nie da się ich
    /// przekazać przez wartość. EN: A chat line to one player. Arrays go by pointer: C cannot
    /// pass them by value.
    pub tell: extern "C" fn(ctx: *mut c_void, player: &Uuid, message: Str) -> bool,
    /// PL: Linia na czacie wszystkich. EN: A chat line to everyone.
    pub broadcast: extern "C" fn(ctx: *mut c_void, message: Str),
    /// PL: Odpowiedź na komendę gracza albo konsoli. EN: The answer to a player's or console command.
    pub reply: extern "C" fn(ctx: *mut c_void, message: Str),
    /// PL: Komenda jak z konsoli, bez ukośnika; nie działa w hakach gracza (S4).
    /// EN: A command as from the console, no slash; not available in player hooks (S4).
    pub run_command: extern "C" fn(ctx: *mut c_void, line: Str) -> bool,
    /// PL: Woła `each(user, &info)` dla każdego gracza na serwerze.
    /// EN: Calls `each(user, &info)` for every player on the server.
    pub players: extern "C" fn(
        ctx: *mut c_void,
        each: extern "C" fn(user: *mut c_void, player: &PlayerInfo),
        user: *mut c_void,
    ),
    /// PL: Blok w danym miejscu (`minecraft:air`, gdy nic nie ma); zapis do `out(user, napis)`.
    /// EN: The block at a position (`minecraft:air` when none); written to `out(user, text)`.
    pub block: extern "C" fn(
        ctx: *mut c_void,
        pos: &[i32; 3],
        out: extern "C" fn(user: *mut c_void, block: Str),
        user: *mut c_void,
    ),
    /// PL: Katalog danych tego moda (`hot-data/<mod>/`); zapis do `out(user, ścieżka)`.
    /// EN: This mod's data folder (`hot-data/<mod>/`); written to `out(user, path)`.
    pub data_dir: extern "C" fn(
        ctx: *mut c_void,
        out: extern "C" fn(user: *mut c_void, path: Str),
        user: *mut c_void,
    ),
}

/// PL: Tabela eksportowana przez mod (`hot_mod_entry`). `instance` to wskaźnik moda, którego
/// gospodarz nie dotyka.
/// EN: The table a mod exports (`hot_mod_entry`). `instance` is the mod's pointer, opaque to
/// the host.
#[repr(C)]
pub struct ModVTable {
    /// PL: Musi równać się `ABI_MAJOR` gospodarza. EN: Must equal the host's `ABI_MAJOR`.
    pub abi: u32,
    /// PL: Identyfikator moda, np. `licznik`. EN: The mod's id, e.g. `licznik`.
    pub id: Str,
    /// PL: Wersja moda. EN: The mod's version.
    pub version: Str,
    /// PL: Start generacji ze stanem poprzedniej (pusty przy pierwszym starcie). Null = błąd.
    /// EN: Starts a generation with the previous one's state (empty on first start). Null = error.
    pub init: extern "C" fn(host: &HostApi, prev_state: Str) -> *mut c_void,
    /// PL: Jedno zdarzenie; zwraca wartość z [`flow`].
    /// EN: One event; returns a value from [`flow`].
    pub on_event: extern "C" fn(instance: *mut c_void, host: &HostApi, event: &Event) -> u32,
    /// PL: Koniec generacji: zatrzymuje wątki i timery, zwalnia instancję, oddaje stan.
    /// EN: Ends the generation: stops threads and timers, frees the instance, returns its state.
    pub deactivate: extern "C" fn(instance: *mut c_void, host: &HostApi) -> Buf,
    /// PL: Zwalnia bufor zwrócony przez ten mod. EN: Frees a buffer this mod returned.
    pub free_buf: extern "C" fn(buf: Buf),
}

// PL: Tabela jest stałą w pamięci moda i tylko się ją czyta, a napisy wskazują dane `'static`,
// więc współdzielenie między wątkami jest bezpieczne. Bez tego mod nie zrobi z niej `static`.
// EN: The table is a constant in the mod's memory and is only read, and its strings point to
// `'static` data, so sharing it across threads is safe. Without this a mod cannot make it `static`.
unsafe impl Sync for ModVTable {}

/// PL: Typ eksportu `hot_mod_entry`. EN: The type of the `hot_mod_entry` export.
pub type EntryFn = unsafe extern "C" fn() -> *const ModVTable;
