//! PL: Kody błędów `hotd` (zakresy `E02xx` git i CI, `E03xx` budowa silnika, `E06xx` podmiana, `E07xx`
//! aktualizacje). Opisy: `docs/bledy/`. EN: `hotd` error codes (git and CI, the engine build, the swap,
//! updates). Descriptions: `docs/bledy/`.

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

/// PL: Silnik nie wczytuje świata; `hotd` podaje polecenie przywrócenia, nic nie przywraca sam.
/// EN: The engine cannot load the world; `hotd` gives the restore command and restores nothing.
pub const WORLD_DAMAGED: Code = Code::new("HOT-E0212");

/// PL: Silnika z `hot-host` nie da się zbudować (`native-build`, D15). EN: The engine with
/// `hot-host` cannot be built (`native-build`, D15).
pub const ENGINE_BUILD_FAILED: Code = Code::new("HOT-E0301");

/// PL: Nowy silnik nie przygotował się do podmiany; gracze zostali na starym (D11).
/// EN: The new engine did not get ready for the swap; the players stayed on the old one (D11).
pub const SWAP_NOT_STARTED: Code = Code::new("HOT-E0606");

/// PL: Nowy silnik nie zajął świata po zamrożeniu; `hotd` wraca na stary (D11, krok 5).
/// EN: The new engine did not take the world after the freeze; `hotd` goes back to the old one.
pub const SWAP_FAILED: Code = Code::new("HOT-E0607");

/// PL: `hotd engine` nie uruchomił silnika albo swojego gniazda. EN: `hotd engine` cannot start
/// the engine or its socket.
pub const ENGINE_START: Code = Code::new("HOT-E0608");
