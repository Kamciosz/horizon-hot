//! PL: Wspólna diagnostyka wszystkich składników `horizon-hot` (D27, D28): błąd z polami D27,
//! zapis JSONL z rotacją, maskowanie sekretów, panika w logu z backtrace i `hot report`.
//! Używają: `hotd`, `hot-core`, `hot-sdk`, `hot-proxy`, `hot-pack`, `hot-play`. Nie używa go
//! `hot-host` (paczka moda natywnego, D2). Rozmawia tylko z systemem plików.
//! Cel: model bez wiedzy o projekcie naprawia błąd z samego wpisu w logu.
//!
//! EN: Shared diagnostics for every `horizon-hot` component (D27, D28): an error with the D27
//! fields, JSONL output with rotation, secret masking, panics logged with a backtrace and
//! `hot report`. Used by `hotd`, `hot-core`, `hot-sdk`, `hot-proxy`, `hot-pack`, `hot-play`; not by
//! `hot-host` (a native mod bundle, D2). Talks only to the file system.
//! Goal: a model that does not know the project fixes the error from the log entry alone.

mod code;
pub mod codes;
mod entry;
mod log;
mod panic;
pub mod redact;
pub mod report;

pub use code::Code;
pub use entry::DiagError;
pub use log::{utc_timestamp, LogConfig, Logger, DEFAULT_FILES, DEFAULT_MAX_BYTES};
pub use panic::install_panic_hook;
pub use redact::Redactor;

mod catalog {
    include!(concat!(env!("OUT_DIR"), "/catalog.rs"));
}

/// PL: Treść wpisu z `docs/bledy/` wklejona przy budowie. EN: A `docs/bledy/` entry embedded at build.
#[must_use]
pub fn catalog_entry(code: &str) -> Option<&'static str> {
    catalog::CATALOG
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, text)| *text)
}

/// PL: Kody, które mają wpis w katalogu. EN: Codes that have a catalog entry.
pub fn catalog_codes() -> impl Iterator<Item = &'static str> {
    catalog::CATALOG.iter().map(|(c, _)| *c)
}
