//! PL: Tabela gospodarza dla jednego moda na czas jednego wywołania. Większość funkcji idzie
//! prosto do tabeli `hot-host`. Inaczej działają dwie: `log` zapisuje wpis w logu `hot-core`
//! z id moda i generacją (D27) i dopiero potem przekazuje go silnikowi, a `data_dir` zwraca
//! `hot-data/<mod>/` zamiast katalogu rdzenia.
//!
//! EN: The host table for one mod during one call. Most functions go straight to the `hot-host`
//! table. Two differ: `log` writes the entry to the `hot-core` log with the mod id and generation
//! (D27) before passing it to the engine, and `data_dir` returns `hot-data/<mod>/` instead of
//! the core's folder.

use std::ffi::c_void;
use std::path::Path;

use hot_abi::{level, HostApi, PlayerInfo, Str, Uuid};
use hot_diag::{DiagError, Logger};
use serde_json::json;

use crate::codes;

/// PL: Kontekst wywołania moda. EN: The context of one mod call.
pub struct ModCall<'a> {
    pub outer: &'a HostApi,
    pub id: &'a str,
    pub generation: u64,
    pub data_dir: &'a Path,
    pub log: &'a Logger,
    /// PL: Opis paniki od `hot-sdk` (pierwsza linia `panicked at …`). EN: The panic report.
    pub panic: Option<String>,
}

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
        }
    }
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
    (c.outer.tell)(c.outer.ctx, player, message)
}

extern "C" fn broadcast(ctx: *mut c_void, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.broadcast)(c.outer.ctx, message);
}

extern "C" fn reply(ctx: *mut c_void, message: Str) {
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.reply)(c.outer.ctx, message);
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
    // SAFETY: opis `call`. EN: see `call`.
    let c = unsafe { call(ctx) };
    (c.outer.block)(c.outer.ctx, pos, out, user);
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
