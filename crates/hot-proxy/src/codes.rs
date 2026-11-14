//! PL: Kody błędów `hot-proxy` (zakres `E06xx`, proxy i podmiana silnika). Opisy: `docs/bledy/`.
//! EN: `hot-proxy` error codes (range `E06xx`, proxy and engine swap). Descriptions: `docs/bledy/`.

use hot_diag::Code;

/// PL: Usługa sesji nie potwierdziła gracza. EN: The session service did not confirm the player.
pub const UNVERIFIED: Code = Code::new("HOT-E0601");

/// PL: Usługa sesji nie odpowiada. EN: The session service does not answer.
pub const SESSION_SERVICE_DOWN: Code = Code::new("HOT-E0602");

/// PL: Silnik nie przyjął gracza albo nie da się z nim połączyć.
/// EN: The engine refused the player or cannot be reached.
pub const ENGINE_LOGIN: Code = Code::new("HOT-E0603");

/// PL: Zła konfiguracja proxy. EN: The proxy is misconfigured.
pub const CONFIG: Code = Code::new("HOT-E0604");

/// PL: Gracza nie dało się przenieść na silnik (podmiana albo powrót silnika po awarii).
/// EN: A player could not be moved to the engine (a swap, or the engine's return after a crash).
pub const REJOIN: Code = Code::new("HOT-E0605");

/// PL: Programu proxy nie dało się podmienić przy graczach (`hot-proxy upgrade`).
/// EN: The proxy program could not be swapped with players on (`hot-proxy upgrade`).
pub const UPGRADE: Code = Code::new("HOT-E0609");
