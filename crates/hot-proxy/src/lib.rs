//! PL: `hot-proxy` — proxy przed silnikiem (M8). Gracz łączy się z proxy, nie z silnikiem. Proxy
//! loguje gracza online (klucz RSA, szyfrowanie, `hasJoined` w usłudze sesji), łączy się
//! z silnikiem i podpisuje mu dane gracza (przekazywanie Velocity), a potem przesyła pakiety
//! w obie strony. Silnik ma `online_mode = false`, słucha tylko na 127.0.0.1 i odrzuca dane bez
//! podpisu, więc gra bez kupionej kopii nie wejdzie żadną drogą. Używa: `hot-diag`,
//! `horizon-protocol` (ADR 0010). Rozmawia z: graczami, silnikiem, usługą sesji (HTTPS).
//!
//! EN: `hot-proxy`, the proxy in front of the engine (M8). Players connect to the proxy, not the
//! engine. The proxy logs a player in online (RSA key, encryption, `hasJoined` at the session
//! service), connects to the engine and signs the player's data for it (Velocity forwarding), then
//! relays packets both ways. The engine has `online_mode = false`, listens on 127.0.0.1 only and
//! refuses unsigned data, so a game without a bought copy gets in by no route. Uses `hot-diag`,
//! `horizon-protocol` (ADR 0010). Talks to players, the engine and the session service (HTTPS).

pub mod auth;
pub mod client_io;
pub mod codes;
pub mod config;
pub mod control;
pub mod forwarding;
pub mod link;
pub mod ping;
pub mod remap;
pub mod session;
pub mod upgrade;

/// PL: Wersja `hot-proxy` w logach. EN: The `hot-proxy` version in logs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
