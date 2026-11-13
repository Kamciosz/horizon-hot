//! PL: `hotd` — demon serwera (D17, D24, D13). Co 10 s pobiera repo serwera z GitHuba, liczy
//! zmiany od ostatniego zastosowanego commita, pobiera podpisane biblioteki modów z gałęzi
//! `hot-builds` (buduje je CI, ADR 0008) i ładuje je w silniku przez `horizon-server cmd hot …`.
//! Stan zapisuje w `hot-state/hotd.json`; czyta go `hot status` (terminal) i `/hot status`
//! (gra, `hot-core`). Używa: `hot-diag`. Rozmawia z: `git`, `ssh-keygen`, `horizon-server`,
//! plikami `hot-state/` silnika.
//!
//! EN: `hotd`, the server daemon (D17, D24, D13). Every 10 s it fetches the server repository from
//! GitHub, works out the changes since the last applied commit, downloads signed mod libraries
//! from the `hot-builds` branch (built by CI, ADR 0008) and loads them in the engine with
//! `horizon-server cmd hot …`. It saves its state in `hot-state/hotd.json`, read by `hot status`
//! (terminal) and `/hot status` (game, `hot-core`). Uses `hot-diag`. Talks to `git`,
//! `ssh-keygen`, `horizon-server` and the engine's `hot-state/` files.

pub mod classify;
pub mod codes;
pub mod daemon;
pub mod data;
pub mod disk;
pub mod engine;
pub mod git;
pub mod pack;
pub mod sign;
pub mod slots;
pub mod state;
pub mod status;
pub mod world;

/// PL: Wersja `hotd` w logach i w `hotd.json`. EN: The `hotd` version in logs and `hotd.json`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
