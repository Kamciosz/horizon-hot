//! PL: `hot-core` — gospodarz modów gorących wewnątrz silnika (D1, D2, D4, D13). Ładuje go
//! `hot-host` (paczka moda natywnego) przez tę samą tabelę C ABI, którą mają mody, więc rdzeń też
//! da się podmienić bez restartu. Rdzeń wczytuje mody (`hot load`), przełącza generacje na
//! granicy ticku, przekazuje im zdarzenia silnika i po panice wraca do poprzedniej generacji.
//! Używa: `hot-abi`, `hot-sdk` (eksport i łapanie paniki), `hot-diag` (log
//! `logs/hot-core.jsonl`). Rozmawia z: `hot-host` (wywołania), systemem plików (`hot-state/`,
//! `hot-data/`), `hotd` (czyta `hot-state/status.json`, wysyła `hot load` przez konsolę).
//!
//! EN: `hot-core` — the hot mod host inside the engine (D1, D2, D4, D13). `hot-host` (a native
//! mod bundle) loads it through the same C ABI table mods have, so the core can be swapped without
//! a restart as well. The core loads mods (`hot load`), switches generations at the tick boundary,
//! forwards engine events to them and falls back to the previous generation after a panic.
//! Uses `hot-abi`, `hot-sdk` (export and panic catching), `hot-diag` (log `logs/hot-core.jsonl`).
//! Talks to `hot-host` (calls), the file system (`hot-state/`, `hot-data/`) and `hotd` (reads
//! `hot-state/status.json`, sends `hot load` through the console).

pub mod codes;
mod core;
pub mod loader;
mod slots;
pub mod store;
mod wrap;

pub use crate::core::{Core, GOOD_AFTER_TICKS};

/// PL: Wersja z `Cargo.toml`. EN: The version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

hot_sdk::hot_mod!(Core, id = "hot-core", version = VERSION);
