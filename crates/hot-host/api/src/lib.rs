//! PL: `hot-host` (D2, M1.3): jedyny mod natywny projektu, wkompilowany w silnik. Ładuje
//! `hot-core` jako zwykły mod gorący (ten sam kontrakt `hot-abi`) i zamienia haki Horizona na
//! zdarzenia ABI. Sam nie ma logiki modów; jego jedyne zadania to:
//! - wczytać rdzeń przy starcie serwera (`server_about_to_start`) i oddać mu stan przy
//!   zatrzymaniu (`server_stopping`);
//! - przekazywać zdarzenia (tick, gracze, czat, komendy, bloki, przedmioty);
//! - podmienić sam rdzeń komendą `hot core load [plik]` bez restartu (D11, to samo PID).
//!
//! Rdzeń szukany jest w `$HOT_CORE`, a bez tej zmiennej w `<home>/hot-core/<lib>hot_core.<so|dylib>`;
//! `<home>` to `$HOT_HOME` albo katalog roboczy serwera. Każde wczytanie idzie z kopii
//! w `hot-state/core/` pod unikalną nazwą, bo `dlopen` tej samej ścieżki oddałby starą bibliotekę.
//! Błędy idą do logu silnika (`logs/latest.log`) z kodem `HOT-E04xx` (opis w `docs/bledy/`).
//!
//! EN: `hot-host` (D2, M1.3): the project's only native mod, compiled into the engine. It loads
//! `hot-core` as an ordinary hot mod (the same `hot-abi` contract) and turns Horizon hooks into
//! ABI events. It has no mod logic; it only loads the core at server start and hands its state
//! over at stop, forwards events, and swaps the core itself with `hot core load [file]` without a
//! restart (D11, same PID). The core is `$HOT_CORE`, else `<home>/hot-core/<lib>hot_core.<so|dylib>`,
//! where `<home>` is `$HOT_HOME` or the server's working directory. Every load uses a copy in
//! `hot-state/core/` under a unique name, as `dlopen` of the same path would return the old
//! library. Errors go to the engine log (`logs/latest.log`) with a `HOT-E04xx` code
//! (`docs/bledy/`).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use horizon_client::mod_api::{
    DestroyBlockEvent, Flow, Hooks, Host, LeftClickBlockEvent, NativeCommand, PerformCommandEvent,
    PerformConsoleCommandEvent, PlaceBlockEvent, PlayerChatEvent, PlayerCommandEvent,
    PlayerLoggedInEvent, PlayerLoggedOutEvent, RegisterCommandsEvent, RightClickBlockEvent,
    ServerAboutToStartEvent, ServerDataLoadedEvent, ServerStoppingEvent, ServerTickEvent,
    ServerTickPhase, Text, UseItemEvent, UseItemOnEvent,
};
use hot_abi::{
    flow, kind, level, Buf, EntryFn, Event, HostApi, ModVTable, PlayerInfo, Str, Uuid, ABI_MAJOR,
    ENTRY_SYMBOL,
};

/// PL: Punkt wejścia, który woła silnik. EN: The entry point the engine calls.
pub fn hooks() -> Box<dyn Hooks> {
    Box::new(HotHost {
        core: Mutex::new(None),
        home: home(),
    })
}

/// PL: Cel logu. Logger silnika zapisuje do `logs/latest.log` poziom INFO i WARN tylko z celów
/// zaczynających się od `horizon` (błędy zawsze), więc nazwa projektu `horizon_hot` pasuje.
/// EN: The log target. The engine logger writes INFO and WARN to `logs/latest.log` only for
/// targets starting with `horizon` (errors always), so the project name `horizon_hot` fits.
const LOG: &str = "horizon_hot";

const CORE_LOAD_FAILED: &str = "HOT-E0406";
const CORE_PANICKED: &str = "HOT-E0407";

// ---------- PL: rdzeń / EN: the core ----------

/// PL: Wczytany `hot-core` z działającą instancją. EN: The loaded `hot-core` with a live instance.
struct Core {
    vtable: &'static ModVTable,
    instance: *mut c_void,
    file: PathBuf,
}

// SAFETY: instancja rdzenia jest używana tylko pod muteksem `HotHost::core`, w wątku, który akurat
// woła hak; rdzeń nie trzyma odniesień do wątku, który go stworzył.
// EN: the core instance is only used under the `HotHost::core` mutex, on the thread calling the
// hook; the core keeps no ties to the thread that created it.
unsafe impl Send for Core {}

struct HotHost {
    /// PL: `None`, gdy rdzeń się nie wczytał albo spanikował. EN: `None` when not loaded or panicked.
    core: Mutex<Option<Core>>,
    home: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOT_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default()
}

fn default_core_path(home: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("HOT_CORE") {
        return PathBuf::from(p);
    }
    home.join("hot-core").join(format!(
        "{}hot_core{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ))
}

/// PL: Kopia pliku rdzenia pod unikalną nazwą i `dlopen` kopii. Biblioteki nie zamykamy: stary
/// rdzeń może mieć jeszcze wątki ładujące mody.
/// EN: Copies the core file under a unique name and opens the copy. The library is never closed:
/// an old core may still have mod loader threads.
fn open_core(home: &Path, source: &Path) -> Result<(&'static ModVTable, PathBuf), String> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let dir = home.join("hot-state").join("core");
    let file = dir.join(format!("hot_core-{ms}{}", std::env::consts::DLL_SUFFIX));
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::copy(source, &file))
        .map_err(|e| format!("the file cannot be copied to {}: {e}", file.display()))?;
    // SAFETY: wczytujemy tylko `hot-core` zbudowany w CI repozytorium serwera (D14), zaufany
    // jak sam silnik. EN: only `hot-core` built by the server repository's CI (D14) is loaded.
    let lib = unsafe { libloading::Library::new(&file) }
        .map_err(|e| format!("the dynamic library cannot be opened: {e}"))?;
    // SAFETY: `hot_mod!` eksportuje symbol z typem `EntryFn`. EN: exported by `hot_mod!`.
    let entry: EntryFn = match unsafe { lib.get::<EntryFn>(ENTRY_SYMBOL) } {
        Ok(symbol) => *symbol,
        Err(e) => return Err(format!("the library does not export hot_mod_entry: {e}")),
    };
    std::mem::forget(lib);
    // SAFETY: wskaźnik do statycznej tabeli w bibliotece, której nie wyładujemy; pole `abi` jest
    // pierwsze w każdej wersji. EN: a static table in a library never unloaded; `abi` comes first.
    let vtable = unsafe { entry().as_ref() }.ok_or("hot_mod_entry returned no table")?;
    if vtable.abi != ABI_MAJOR {
        return Err(format!(
            "the core uses ABI {}, but hot-host uses {ABI_MAJOR}; build hot-core and hot-host from the same commit",
            vtable.abi
        ));
    }
    Ok((vtable, file))
}

fn core_load_error(source: &Path, why: &str) {
    log::error!(
        target: LOG,
        "{CORE_LOAD_FAILED} hot-core cannot be loaded from {}: {why}. Fix: build hot-core for this machine (cargo build --release -p hot-core) and put it there, or set HOT_CORE; details in docs/bledy/{CORE_LOAD_FAILED}.md. State: the server runs without hot mods.",
        source.display()
    );
}

impl HotHost {
    /// PL: Wczytuje rdzeń, gdy jeszcze go nie ma. EN: Loads the core when there is none yet.
    fn start(&self, host: &mut dyn Host) {
        let Ok(mut slot) = self.core.lock() else {
            return;
        };
        if slot.is_some() {
            return;
        }
        let source = default_core_path(&self.home);
        if !source.is_file() {
            core_load_error(&source, "there is no such file");
            return;
        }
        let (vtable, file) = match open_core(&self.home, &source) {
            Ok(v) => v,
            Err(why) => return core_load_error(&source, &why),
        };
        let mut call = Call::new(Some(host), &self.home);
        let api = call.api();
        let instance = (vtable.init)(&api, Str::EMPTY);
        if instance.is_null() {
            core_load_error(
                &source,
                "its init failed (the reason is in the lines above)",
            );
            return;
        }
        log::info!(target: LOG, "hot-core loaded from {} (copy {})", source.display(), file.display());
        *slot = Some(Core {
            vtable,
            instance,
            file,
        });
    }

    /// PL: Zatrzymanie: rdzeń zapisuje mody i stan. EN: Stop: the core saves mods and state.
    fn stop(&self, host: &mut dyn Host) {
        let Ok(mut slot) = self.core.lock() else {
            return;
        };
        if let Some(core) = slot.take() {
            let mut call = Call::new(Some(host), &self.home);
            let api = call.api();
            let buf = (core.vtable.deactivate)(core.instance, &api);
            (core.vtable.free_buf)(buf);
        }
    }

    /// PL: Jedno zdarzenie dla rdzenia. Zajęty muteks znaczy, że rdzeń sam wywołał hak (np. przez
    /// `run_command`); wtedy zdarzenie pomijamy, zamiast się zakleszczyć.
    /// EN: One event for the core. A busy mutex means the core itself triggered the hook (e.g.
    /// through `run_command`); the event is then skipped instead of deadlocking.
    fn send(&self, call: &mut Call<'_>, event: &Event) -> Flow {
        let Ok(mut slot) = self.core.try_lock() else {
            return Flow::Continue;
        };
        let Some(core) = slot.as_ref() else {
            return Flow::Continue;
        };
        let api = call.api();
        match (core.vtable.on_event)(core.instance, &api, event) {
            flow::CANCEL => Flow::Cancel,
            flow::PANICKED => {
                log::error!(
                    target: LOG,
                    "{CORE_PANICKED} hot-core panicked on event kind {} (the panic is in the line above). Fix: read logs/hot-core.jsonl and the panic place, fix hot-core, then load it with hot core load. State: hot mods are off until then; the panicked core copy {} stays in memory.",
                    event.kind,
                    core.file.display()
                );
                *slot = None;
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    /// PL: `hot core load [plik]`: stary rdzeń oddaje stan, nowy go dostaje; gdy nowy padnie,
    /// stary wraca z tym samym stanem.
    /// EN: `hot core load [file]`: the old core hands its state to the new one; when the new one
    /// fails, the old one comes back with the same state.
    fn swap_core(&self, call: &mut Call<'_>, arg: &str) {
        let source = if arg.is_empty() {
            default_core_path(&self.home)
        } else if Path::new(arg).is_relative() {
            self.home.join(arg)
        } else {
            PathBuf::from(arg)
        };
        let Ok(mut slot) = self.core.try_lock() else {
            return;
        };
        let (vtable, file) = match open_core(&self.home, &source) {
            Ok(v) => v,
            Err(why) => {
                core_load_error(&source, &why);
                call.reply(&format!(
                    "Nie wczytam rdzenia z {}: {why}",
                    source.display()
                ));
                return;
            }
        };
        let api = call.api();
        let state = match slot.take() {
            Some(old) => {
                let buf = (old.vtable.deactivate)(old.instance, &api);
                let bytes = buf_bytes(buf);
                (old.vtable.free_buf)(buf);
                Some((old, bytes))
            }
            None => None,
        };
        let bytes = state.as_ref().map_or(&[][..], |(_, b)| b.as_slice());
        let instance = (vtable.init)(&api, Str::from_bytes(bytes));
        if !instance.is_null() {
            *slot = Some(Core {
                vtable,
                instance,
                file,
            });
            log::info!(target: LOG, "hot-core swapped to {}", source.display());
            call.reply(&format!("Rdzeń podmieniony na {}.", source.display()));
            return;
        }
        core_load_error(
            &source,
            "its init failed (the reason is in the lines above)",
        );
        if let Some((old, bytes)) = state {
            let back = (old.vtable.init)(&api, Str::from_bytes(&bytes));
            if !back.is_null() {
                *slot = Some(Core {
                    instance: back,
                    ..old
                });
                call.reply("Nowy rdzeń nie wystartował; działa poprzedni.");
                return;
            }
        }
        call.reply("Nowy rdzeń nie wystartował, poprzedni też nie; mody gorące są wyłączone.");
    }

    /// PL: Komenda `hot …` z konsoli albo od operatora. EN: A `hot …` command from the console or an op.
    fn command(&self, host: &mut dyn Host, line: &str, player: Option<(u128, &str)>) -> Flow {
        let Some(args) = hot_args(line) else {
            return Flow::Continue;
        };
        let mut call = Call::new(Some(host), &self.home);
        call.feedback = true;
        if let Some(rest) = args.strip_prefix("core") {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                let rest = rest.trim();
                let file = rest.strip_prefix("load").map(str::trim);
                match file {
                    Some(f) => self.swap_core(&mut call, f),
                    None => call.reply("Użycie: hot core load [plik]"),
                }
                return Flow::Cancel;
            }
        }
        let mut event = Event::new(kind::CONSOLE_COMMAND);
        event.text = Str::from_str(line);
        if let Some((uuid, name)) = player {
            event.player = uuid.to_be_bytes();
            event.name = Str::from_str(name);
        }
        if matches!(self.send(&mut call, &event), Flow::Continue) {
            call.reply("hot-core nie działa; szczegóły w logs/latest.log (HOT-E0406, HOT-E0407).");
        }
        Flow::Cancel
    }

    /// PL: Zdarzenie gracza z przedmiotem albo blokiem. EN: A player event with an item or a block.
    fn world_event(
        &self,
        host: &mut dyn Host,
        kind: u32,
        item: Option<&str>,
        pos: [i32; 3],
        face: [i32; 3],
        with_block: bool,
    ) -> Flow {
        let player = host.acting_player();
        let block = if with_block {
            host.block(pos)
        } else {
            String::new()
        };
        let mut event = Event::new(kind);
        if let Some(p) = &player {
            event.player = p.uuid.to_be_bytes();
            event.name = Str::from_str(&p.name);
        }
        event.item = Str::from_str(item.unwrap_or(""));
        event.block = Str::from_str(&block);
        event.pos = pos;
        event.face = face;
        let mut call = Call::new(Some(host), &self.home);
        self.send(&mut call, &event)
    }
}

/// PL: `hot ...` → reszta po słowie `hot`. EN: The arguments after the `hot` word.
fn hot_args(line: &str) -> Option<&str> {
    let line = line.trim().trim_start_matches('/');
    let rest = line.strip_prefix("hot")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

fn buf_bytes(buf: Buf) -> Vec<u8> {
    if buf.ptr.is_null() || buf.len == 0 {
        return Vec::new();
    }
    // SAFETY: bufor od rdzenia, ważny do `free_buf`. EN: a core buffer, valid until `free_buf`.
    unsafe { std::slice::from_raw_parts(buf.ptr, buf.len) }.to_vec()
}

// ---------- PL: haki silnika / EN: engine hooks ----------

impl Hooks for HotHost {
    fn server_about_to_start(&self, host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        self.start(host);
    }

    fn server_stopping(&self, host: &mut dyn Host, _event: &mut ServerStoppingEvent) {
        self.stop(host);
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if let ServerTickPhase::ServerTickEnd {
            world_running: true,
        } = event.phase
        {
            let mut e = Event::new(kind::SERVER_TICK);
            e.number = event.context.tick.get();
            let mut call = Call::new(Some(host), &self.home);
            self.send(&mut call, &e);
        }
    }

    fn server_data_loaded(&self, host: &mut dyn Host, event: &mut ServerDataLoadedEvent) {
        let mut e = Event::new(kind::DATA_LOADED);
        e.number = u64::from(event.reload);
        let mut call = Call::new(Some(host), &self.home);
        self.send(&mut call, &e);
    }

    fn player_logged_in(&self, host: &mut dyn Host, event: &mut PlayerLoggedInEvent<'_>) {
        let mut e = Event::new(kind::PLAYER_JOINED);
        e.player = event.player.to_be_bytes();
        e.name = Str::from_str(event.name);
        let mut call = Call::new(Some(host), &self.home);
        self.send(&mut call, &e);
    }

    fn player_logged_out(&self, host: &mut dyn Host, event: &mut PlayerLoggedOutEvent<'_>) {
        let mut e = Event::new(kind::PLAYER_LEFT);
        e.player = event.player.to_be_bytes();
        e.name = Str::from_str(event.name);
        let mut call = Call::new(Some(host), &self.home);
        self.send(&mut call, &e);
    }

    fn player_chat(&self, event: &mut PlayerChatEvent<'_>) -> Flow {
        let mut e = Event::new(kind::PLAYER_CHAT);
        e.player = event.player.to_be_bytes();
        e.name = Str::from_str(event.name);
        e.text = Str::from_str(event.message);
        let mut call = Call::new(None, &self.home);
        call.replies = Some(&mut *event.replies);
        self.send(&mut call, &e)
    }

    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        event.commands.push(NativeCommand::op("hot [args:text]", 4));
    }

    fn player_command(&self, host: &mut dyn Host, event: &mut PlayerCommandEvent<'_>) -> Flow {
        // PL: `hot …` obsługuje `perform_command` (silnik sprawdza tam poziom operatora).
        // EN: `perform_command` handles `hot …` (the engine checks the operator level there).
        if hot_args(event.command).is_some() {
            return Flow::Continue;
        }
        let mut e = Event::new(kind::PLAYER_COMMAND);
        e.player = event.player.to_be_bytes();
        e.name = Str::from_str(event.name);
        e.text = Str::from_str(event.command);
        let mut call = Call::new(Some(host), &self.home);
        call.replies = Some(&mut *event.replies);
        self.send(&mut call, &e)
    }

    fn perform_console_command(
        &self,
        host: &mut dyn Host,
        event: &mut PerformConsoleCommandEvent<'_>,
    ) -> Flow {
        self.command(host, event.command, None)
    }

    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        let player = (event.player.uuid, event.player.name.as_str());
        self.command(host, event.command, Some(player))
    }

    fn use_item(&self, host: &mut dyn Host, event: &mut UseItemEvent<'_>) -> Flow {
        let item = event.stack.item.clone();
        self.world_event(host, kind::USE_ITEM, Some(&*item), [0; 3], [0; 3], false)
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        let item = event.stack.item.clone();
        let (pos, face) = (event.pos, event.face);
        self.world_event(host, kind::USE_ITEM_ON, Some(&*item), pos, face, true)
    }

    fn right_click_block(&self, host: &mut dyn Host, event: &mut RightClickBlockEvent) -> Flow {
        let (pos, face) = (event.pos, event.face);
        self.world_event(host, kind::RIGHT_CLICK_BLOCK, None, pos, face, true)
    }

    fn left_click_block(&self, host: &mut dyn Host, event: &mut LeftClickBlockEvent) -> Flow {
        let (pos, face) = (event.pos, event.face);
        self.world_event(host, kind::LEFT_CLICK_BLOCK, None, pos, face, true)
    }

    fn place_block(&self, host: &mut dyn Host, event: &mut PlaceBlockEvent<'_>) {
        let item = event.stack.item.clone();
        let pos = event.pos;
        self.world_event(host, kind::PLACE_BLOCK, Some(&*item), pos, [0; 3], true);
    }

    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        let pos = event.pos;
        self.world_event(host, kind::DESTROY_BLOCK, None, pos, [0; 3], true)
    }
}

// ---------- PL: tabela gospodarza / EN: the host table ----------

/// PL: Kontekst jednego wywołania rdzenia: gospodarz silnika (brak w haku czatu), lista odpowiedzi
/// (czat i komendy gracza) albo `feedback` (komendy `hot`).
/// EN: The context of one core call: the engine host (none in the chat hook), the reply list
/// (player chat and commands) or `feedback` (`hot` commands).
struct Call<'a> {
    host: Option<&'a mut dyn Host>,
    replies: Option<&'a mut Vec<Text>>,
    feedback: bool,
    home: &'a Path,
}

impl<'a> Call<'a> {
    fn new(host: Option<&'a mut dyn Host>, home: &'a Path) -> Self {
        Call {
            host,
            replies: None,
            feedback: false,
            home,
        }
    }

    /// PL: Tabela ważna, dopóki `self` się nie przesunie. EN: Valid while `self` stays in place.
    fn api(&mut self) -> HostApi {
        HostApi {
            ctx: (self as *mut Self).cast(),
            log: host_log,
            tell,
            broadcast,
            reply,
            run_command,
            players,
            block,
            data_dir,
        }
    }

    fn reply(&mut self, line: &str) {
        if let Some(replies) = self.replies.as_deref_mut() {
            replies.push(Text::literal(line));
        } else if let (true, Some(host)) = (self.feedback, self.host.as_deref_mut()) {
            host.feedback(Text::literal(line));
        } else {
            log::info!(target: LOG, "{line}");
        }
    }
}

/// SAFETY: `ctx` pochodzi z [`Call::api`], a `Call` żyje przez całe wywołanie rdzenia.
/// EN: `ctx` comes from [`Call::api`] and the `Call` lives through the whole core call.
unsafe fn call<'x>(ctx: *mut c_void) -> &'x mut Call<'x> {
    // SAFETY: kontrakt funkcji. EN: the function's contract.
    unsafe { &mut *ctx.cast::<Call<'x>>() }
}

fn text(s: Str) -> String {
    // SAFETY: napis pożyczony na czas wywołania (kontrakt ABI). EN: borrowed for the call.
    String::from_utf8_lossy(unsafe { s.bytes() }).into_owned()
}

extern "C" fn host_log(_ctx: *mut c_void, lvl: u32, message: Str) {
    let message = text(message);
    match lvl {
        level::ERROR => log::error!(target: LOG, "{message}"),
        level::WARN => log::warn!(target: LOG, "{message}"),
        _ => log::info!(target: LOG, "{message}"),
    }
}

extern "C" fn tell(ctx: *mut c_void, player: &Uuid, message: Str) -> bool {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let line = Text::literal(text(message));
    c.host
        .as_deref_mut()
        .is_some_and(|h| h.tell(u128::from_be_bytes(*player), line))
}

extern "C" fn broadcast(ctx: *mut c_void, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    if let Some(h) = c.host.as_deref() {
        h.broadcast(Text::literal(text(message)));
    }
}

extern "C" fn reply(ctx: *mut c_void, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    c.reply(&text(message));
}

extern "C" fn run_command(ctx: *mut c_void, line: Str) -> bool {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let line = text(line);
    c.host.as_deref_mut().is_some_and(|h| h.run_command(&line))
}

extern "C" fn players(
    ctx: *mut c_void,
    each: extern "C" fn(*mut c_void, &PlayerInfo),
    user: *mut c_void,
) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let Some(h) = c.host.as_deref() else {
        return;
    };
    for p in h.server_players() {
        let info = PlayerInfo {
            uuid: p.uuid.to_be_bytes(),
            name: Str::from_str(&p.name),
            dimension: Str::from_str(&p.dimension),
            position: p.position,
            permission: p.permission,
            game_mode: p.game_mode,
        };
        each(user, &info);
    }
}

extern "C" fn block(
    ctx: *mut c_void,
    pos: &[i32; 3],
    out: extern "C" fn(*mut c_void, Str),
    user: *mut c_void,
) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let name = c
        .host
        .as_deref()
        .map_or_else(|| "minecraft:air".to_owned(), |h| h.block(*pos));
    out(user, Str::from_str(&name));
}

extern "C" fn data_dir(ctx: *mut c_void, out: extern "C" fn(*mut c_void, Str), user: *mut c_void) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let path = c.home.display().to_string();
    out(user, Str::from_str(&path));
}
