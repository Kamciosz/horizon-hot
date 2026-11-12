//! PL: Kody błędów samego `hot-diag` (zakres `E00xx`). Opisy: `docs/bledy/`.
//! EN: Error codes of `hot-diag` itself (range `E00xx`). Descriptions: `docs/bledy/`.

use crate::Code;

/// PL: Nie da się zapisać wpisu do pliku logu. EN: A log entry cannot be written to its file.
pub const LOG_WRITE_FAILED: Code = Code::new("HOT-E0001");

/// PL: Program spanikował. EN: The program panicked.
pub const PANIC: Code = Code::new("HOT-E0002");
