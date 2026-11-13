//! PL: Kody błędów `hotd` (zakres `E02xx`, git i CI). Opisy: `docs/bledy/`.
//! EN: `hotd` error codes (range `E02xx`, git and CI). Descriptions: `docs/bledy/`.

use hot_diag::Code;

/// PL: `git fetch` repo serwera się nie udał. EN: `git fetch` of the server repository failed.
pub const GIT_FETCH_FAILED: Code = Code::new("HOT-E0201");

/// PL: CI nie zbudowało commita. EN: CI failed to build the commit.
pub const CI_BUILD_FAILED: Code = Code::new("HOT-E0202");

/// PL: Podpis artefaktu nie pasuje do klucza serwera. EN: The artifact signature does not match.
pub const BAD_SIGNATURE: Code = Code::new("HOT-E0203");

/// PL: W `hot-builds` brakuje pliku dla tej maszyny. EN: `hot-builds` lacks a file for this machine.
pub const ARTIFACT_MISSING: Code = Code::new("HOT-E0204");

/// PL: Silnik nie przyjął albo nie potwierdził zmian. EN: The engine refused or never confirmed.
pub const APPLY_FAILED: Code = Code::new("HOT-E0205");

/// PL: Silnik padł zaraz po zastosowaniu commita; commit oznaczony jako zły (D13).
/// EN: The engine died right after a commit was applied; the commit is marked bad (D13).
pub const BAD_COMMIT: Code = Code::new("HOT-E0206");

/// PL: Pliku stanu `hotd` nie da się odczytać ani zapisać. EN: A `hotd` state file failed.
pub const STATE_FILE: Code = Code::new("HOT-E0207");

/// PL: Paczki danych moda nie da się przygotować albo silnik jej nie włączył (M3).
/// EN: A mod's data pack cannot be prepared or the engine did not enable it (M3).
pub const DATA_PACK_FAILED: Code = Code::new("HOT-E0208");

/// PL: W puli slotów brakuje miejsca na nowe bloki (M5). EN: The slot pool has no room left (M5).
pub const POOL_FULL: Code = Code::new("HOT-E0209");

/// PL: Paczki zasobów nie da się zbudować albo podać graczom (M5). EN: The resource pack cannot be
/// built or served (M5).
pub const PACK_FAILED: Code = Code::new("HOT-E0210");

/// PL: Mało miejsca na dysku nawet po sprzątaniu; `hotd` wstrzymuje pobieranie i commity.
/// EN: Low disk space even after cleanup; `hotd` pauses fetching and commits.
pub const DISK_LOW: Code = Code::new("HOT-E0211");
