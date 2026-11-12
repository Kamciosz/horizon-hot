//! PL: Tabela gospodarza dla jednego moda na czas jednego wywołania. Większość funkcji idzie
//! prosto do tabeli `hot-host`. `block`, `set_block` i `give` tłumaczą nazwy bloków modów
//! (`<id>:<nazwa>` ↔ `hot_slots:slot_N`, M5.1). Inaczej działa też pięć:
//! - `log` zapisuje wpis w logu `hot-core` z id moda i generacją (D27), potem przekazuje go silnikowi;
//! - `data_dir` zwraca `hot-data/<mod>/` zamiast katalogu rdzenia;
//! - `run_later` dopisuje komendę do kolejki rdzenia, wykonywanej na początku następnego ticku;
//! - `tell`, gdy silnik odmówi (D6: poza hakiem komendy), wysyła w następnym ticku `tellraw`;
//! - `reply` w odłożonej komendzie gracza (D6a) idzie do tego gracza przez `tell`.
//!
//! EN: The host table for one mod during one call. Most functions go straight to the `hot-host`
//! table; `block`, `set_block` and `give` translate mod block names (`<id>:<name>` ↔
//! `hot_slots:slot_N`, M5.1). Five more differ: `log` writes the entry to the `hot-core` log with the mod id and generation
//! (D27) before passing it to the engine; `data_dir` returns `hot-data/<mod>/` instead of the
//! core's folder; `run_later` adds the command to the core's queue, run at the start of the next
//! tick; `tell`, when the engine refuses (D6: outside the command hook), sends `tellraw` on the
//! next tick; `reply` in a deferred player command (D6a) goes to that player through `tell`.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::Path;

use hot_abi::{level, HostApi, PlayerInfo, Str, Uuid};
use hot_diag::{DiagError, Logger};
use serde_json::json;

use crate::codes;
use crate::slots::SlotNames;

/// PL: Kontekst wywołania moda. EN: The context of one mod call.
pub struct ModCall<'a> {
    pub outer: &'a HostApi,
    pub id: &'a str,
    pub generation: u64,
    pub data_dir: &'a Path,
    pub log: &'a Logger,
    /// PL: Opis paniki od `hot-sdk` (pierwsza linia `panicked at …`). EN: The panic report.
    pub panic: Option<String>,
    /// PL: Kolejka komend na następny tick, wspólna dla modów. EN: The next-tick command queue.
    pub later: &'a RefCell<Vec<Later>>,
    /// PL: Gracz odłożonej komendy (D6a): `reply` idzie do niego przez `tell`.
    /// EN: The player of a deferred command (D6a): `reply` goes to them through `tell`.
    pub reply_to: Option<Uuid>,
    /// PL: Nazwy bloków modów. EN: Mod block names.
    pub slots: &'a SlotNames,
}

/// PL: Komenda odłożona na następny tick i mod, który ją zlecił (do logu przy błędzie).
/// EN: A command put off to the next tick and the mod that asked for it (for the log on failure).
pub struct Later {
    pub id: String,
    pub line: String,
}

/// PL: Najwięcej komend w kolejce; nadmiar mod dostaje jako ostrzeżenie w logu.
/// EN: The most commands in the queue; the mod gets the excess as a log warning.
pub const LATER_MAX: usize = 1000;

impl ModCall<'_> {
    /// PL: Tabela dla moda. Ważna, dopóki `self` się nie przesunie i nie zniknie.
    /// EN: The table for the mod. Valid while `self` stays in place and alive.
    pub fn api(&mut self) -> HostApi {
        HostApi {
            ctx: (self as *mut Self).cast(),
            log,
            tell,
            broadcast,
            reply,
            run_command,
            players,
            block,
            data_dir,
            set_block,
            give,
            particles,
            run_later,
        }
    }

    fn queue(&mut self, line: String) -> bool {
        let mut later = self.later.borrow_mut();
        if later.len() >= LATER_MAX {
            drop(later);
            self.log.warn(
                &format!(
                    "Hot mod \"{}\" queued more than {LATER_MAX} commands for the next tick; this one is dropped. Fix: queue fewer commands per tick in the mod",
                    self.id
                ),
                json!({ "mod": self.id, "generation": self.generation, "command": line }),
            );
            return false;
        }
        later.push(Later {
            id: self.id.to_owned(),
            line,
        });
        true
    }
}

/// PL: Komenda `tellraw` z linią jako zwykłym tekstem. EN: A `tellraw` command with plain text.
#[must_use]
pub fn tellraw(name: &str, message: &str) -> String {
    format!("tellraw {name} {}", json!({ "text": message }))
}

/// SAFETY: `ctx` pochodzi z [`ModCall::api`], a `ModCall` żyje przez całe wywołanie moda.
/// EN: `ctx` comes from [`ModCall::api`] and the `ModCall` lives through the whole mod call.
unsafe fn call<'x>(ctx: *mut c_void) -> &'x mut ModCall<'x> {
    // SAFETY: kontrakt funkcji. EN: the function's contract.
    unsafe { &mut *ctx.cast::<ModCall<'x>>() }
}

fn text(s: Str) -> String {
    // SAFETY: napis pożyczony na czas wywołania (kontrakt ABI). EN: borrowed for the call.
    String::from_utf8_lossy(unsafe { s.bytes() }).into_owned()
}

extern "C" fn log(ctx: *mut c_void, lvl: u32, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let message = text(message);
    let first = message.lines().next().unwrap_or("").to_owned();
    let context = json!({ "mod": c.id, "generation": c.generation });
    match lvl {
        level::ERROR if first.starts_with("panicked at") => {
            // PL: Wpis z kodem HOT-E0403 zapisze rdzeń, gdy dostanie PANICKED.
            // EN: The core writes the HOT-E0403 entry once it gets PANICKED.
            c.panic = Some(message);
        }
        level::ERROR => c.log.error(
            &DiagError::new(
                codes::MOD_ERROR,
                format!("Hot mod \"{}\" reported an error: {first}", c.id),
                "Read the message and the mod's source in the server repository; the text comes from the mod's own code",
            )
            .context("mod", c.id)
            .context("generation", c.generation)
            .chain_line(message.clone())
            .state("The mod keeps running; hot-core only records the message"),
        ),
        level::WARN => c.log.warn(&first, context),
        _ => c.log.info(&first, context),
    }
    let line = format!("[{}] {first}", c.id);
    (c.outer.log)(c.outer.ctx, lvl, Str::from_str(&line));
}

extern "C" fn tell(ctx: *mut c_void, player: &Uuid, message: Str) -> bool {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    if (c.outer.tell)(c.outer.ctx, player, message) {
        return true;
    }
    let Some(name) = player_name(c.outer, player) else {
        return false;
    };
    let line = tellraw(&name, &text(message));
    c.queue(line)
}

/// PL: Nazwa gracza o danym UUID, gdy jest na serwerze. EN: The name of the player with the UUID.
fn player_name(outer: &HostApi, player: &Uuid) -> Option<String> {
    struct Find<'a> {
        uuid: &'a Uuid,
        name: Option<String>,
    }
    extern "C" fn each(user: *mut c_void, p: &PlayerInfo) {
        // SAFETY: `user` to `&mut Find` z wywołania niżej. EN: the `&mut Find` from below.
        let find = unsafe { &mut *user.cast::<Find<'_>>() };
        if &p.uuid == find.uuid {
            find.name = Some(text(p.name));
        }
    }
    let mut find = Find {
        uuid: player,
        name: None,
    };
    (outer.players)(outer.ctx, each, (&raw mut find).cast());
    find.name
}

extern "C" fn broadcast(ctx: *mut c_void, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.broadcast)(c.outer.ctx, message);
}

extern "C" fn reply(ctx: *mut c_void, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    match c.reply_to {
        Some(player) => {
            tell(ctx, &player, message);
        }
        None => (c.outer.reply)(c.outer.ctx, message),
    }
}

extern "C" fn run_command(ctx: *mut c_void, line: Str) -> bool {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.run_command)(c.outer.ctx, line)
}

extern "C" fn players(
    ctx: *mut c_void,
    each: extern "C" fn(*mut c_void, &PlayerInfo),
    user: *mut c_void,
) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.players)(c.outer.ctx, each, user);
}

extern "C" fn block(
    ctx: *mut c_void,
    pos: &[i32; 3],
    out: extern "C" fn(*mut c_void, Str),
    user: *mut c_void,
) {
    /// PL: Odpowiedź silnika w drodze do moda, z nazwą przetłumaczoną.
    /// EN: The engine's answer on its way to the mod, with the name translated.
    struct Relay<'a> {
        out: extern "C" fn(*mut c_void, Str),
        user: *mut c_void,
        slots: &'a SlotNames,
    }
    extern "C" fn relay(ptr: *mut c_void, name: Str) {
        // SAFETY: `ptr` to `Relay` z ramki `block` niżej, żywy przez całe wywołanie silnika.
        // EN: `ptr` is the `Relay` in the `block` frame below, alive for the whole engine call.
        let r = unsafe { &*ptr.cast::<Relay<'_>>() };
        let name = text(name);
        (r.out)(r.user, Str::from_str(r.slots.to_mod(&name)));
    }
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let mut r = Relay {
        out,
        user,
        slots: c.slots,
    };
    (c.outer.block)(c.outer.ctx, pos, relay, (&raw mut r).cast());
}

extern "C" fn data_dir(ctx: *mut c_void, out: extern "C" fn(*mut c_void, Str), user: *mut c_void) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    if let Err(e) = std::fs::create_dir_all(c.data_dir) {
        c.log.error(
            &DiagError::new(
                codes::STATE_FILE,
                format!(
                    "The data folder {} of hot mod \"{}\" cannot be created",
                    c.data_dir.display(),
                    c.id
                ),
                "Check that the server user can write to the hot-data folder and that the disk is not full (df -h)",
            )
            .context("mod", c.id)
            .context("path", c.data_dir.display().to_string())
            .caused_by(&e)
            .state("The mod gets the path anyway; its own file operations will fail"),
        );
    }
    let path = c.data_dir.display().to_string();
    out(user, Str::from_str(&path));
}

extern "C" fn set_block(ctx: *mut c_void, pos: &[i32; 3], block: Str) -> bool {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let block = text(block);
    (c.outer.set_block)(c.outer.ctx, pos, Str::from_str(c.slots.to_engine(&block)))
}

extern "C" fn give(ctx: *mut c_void, player: &Uuid, item: Str, count: u32) -> u32 {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let item = text(item);
    (c.outer.give)(
        c.outer.ctx,
        player,
        Str::from_str(c.slots.to_engine(&item)),
        count,
    )
}

extern "C" fn particles(
    ctx: *mut c_void,
    name: Str,
    at: &[f64; 3],
    spread: &[f64; 3],
    speed: f64,
    count: i32,
) -> bool {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.particles)(c.outer.ctx, name, at, spread, speed, count)
}

extern "C" fn run_later(ctx: *mut c_void, line: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    let line = text(line);
    c.queue(line);
}
