//! PL: Kody błędów `hot-core` (zakres `E04xx`, mody gorące). Opisy: `docs/bledy/`.
//! EN: `hot-core` error codes (range `E04xx`, hot mods). Descriptions: `docs/bledy/`.

use hot_diag::Code;

/// PL: Pliku moda nie da się skopiować, otworzyć albo nie ma w nim tabeli moda.
/// EN: The mod file cannot be copied or opened, or it has no mod table.
pub const MOD_LOAD_FAILED: Code = Code::new("HOT-E0401");

/// PL: Mod zbudowany z inną wersją ABI niż `hot-core`. EN: Mod built for another ABI version.
pub const ABI_MISMATCH: Code = Code::new("HOT-E0402");

/// PL: Mod spanikował; wróciła poprzednia generacja albo mod jest wyłączony.
/// EN: The mod panicked; the previous generation is back or the mod is disabled.
pub const MOD_PANICKED: Code = Code::new("HOT-E0403");

/// PL: Pliku stanu w `hot-state/` nie da się odczytać ani zapisać. EN: A `hot-state/` file failed.
pub const STATE_FILE: Code = Code::new("HOT-E0404");

/// PL: Mod sam zgłosił błąd (`Host::error`). EN: The mod reported an error itself.
pub const MOD_ERROR: Code = Code::new("HOT-E0405");

/// PL: Serwer padł kilka razy z rzędu krótko po starcie; mody gorące śpią (tryb bezpieczny).
/// EN: The server crashed several times in a row soon after starting; hot mods sleep (safe mode).
pub const CRASH_LOOP: Code = Code::new("HOT-E0408");
