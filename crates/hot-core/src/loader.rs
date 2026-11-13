//! PL: Wczytanie pliku moda: kopia pod unikalną nazwą, `dlopen`, sprawdzenie ABI (D3, D4).
//! Biblioteki nigdy nie wyładowujemy, bo stara generacja może mieć jeszcze wątki albo dane
//! w pamięci; pamięć odzyskuje podmiana silnika (D11). Wczytanie nowego pliku idzie w osobnym
//! wątku i nie dotyka tabeli gospodarza, bo ta jest ważna tylko w wątku serwera.
//!
//! EN: Loading a mod file: a copy under a unique name, `dlopen`, an ABI check (D3, D4). A
//! library is never unloaded, as an old generation may still have threads or data in memory;
//! an engine swap (D11) reclaims the memory. A new file loads on its own thread and never touches
//! the host table, which is only valid on the server thread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use hot_abi::{EntryFn, ModVTable, ABI_MAJOR, ENTRY_SYMBOL};
use hot_diag::DiagError;

use crate::codes;

/// PL: Biblioteki otwarte w tym procesie (ścieżka kopii → rozmiar pliku). Zostają w pamięci do
/// końca procesu, także te odrzucone po otwarciu (zła wersja ABI, złe id). Zbiór, nie licznik:
/// ta sama kopia `hot-core` startuje w testach wiele razy, a każda nowa kopia (`hot core load`)
/// dostaje go z `hot-state/libraries.json` (M8.5).
/// EN: Libraries opened in this process (copy path → file size). They stay in memory until the
/// process ends, including ones refused after opening (wrong ABI version, bad id). A set, not a
/// counter: the same `hot-core` copy starts many times in tests, and every new copy
/// (`hot core load`) gets it from `hot-state/libraries.json` (M8.5).
static MAPPED: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

/// PL: Kopia zbioru otwartych bibliotek. EN: A copy of the opened library set.
#[must_use]
pub fn mapped() -> BTreeMap<PathBuf, u64> {
    MAPPED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// PL: Dopisuje biblioteki otwarte przez poprzednią kopię `hot-core` w tym samym procesie.
/// EN: Adds libraries a previous `hot-core` copy opened in the same process.
pub fn note_mapped(files: impl IntoIterator<Item = (PathBuf, u64)>) {
    MAPPED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(files);
}

/// PL: Wczytana biblioteka moda. EN: A loaded mod library.
#[derive(Clone)]
pub struct Gen {
    pub vtable: &'static ModVTable,
    pub id: String,
    pub version: String,
    /// PL: Komendy najwyższego poziomu moda (D6a). EN: The mod's top-level commands (D6a).
    pub commands: Vec<String>,
    /// PL: Kopia w `hot-state/gen/`, z której wczytano bibliotekę. EN: The copy that was loaded.
    pub file: PathBuf,
    /// PL: Plik wskazany w `hot load` (artefakt CI). EN: The file named in `hot load`.
    pub source: PathBuf,
}

/// PL: Wynik wczytania w tle. EN: The result of a background load.
pub enum Outcome {
    Ready(Gen),
    Failed(PathBuf, Box<DiagError>),
}

/// PL: Kopiuje plik i wczytuje go w osobnym wątku; wynik trafia do kanału.
/// EN: Copies and loads the file on its own thread; the result goes into the channel.
pub fn spawn(source: PathBuf, gen_dir: PathBuf, seq: u64, tx: Sender<Outcome>) {
    let spawned = std::thread::Builder::new()
        .name("hot-core-loader".into())
        .spawn({
            let source = source.clone();
            let tx = tx.clone();
            move || {
                let outcome = match copy_and_open(&source, &gen_dir, seq) {
                    Ok(g) => Outcome::Ready(g),
                    Err(e) => Outcome::Failed(source, Box::new(e)),
                };
                // PL: Odbiorca znika tylko razem z rdzeniem; wtedy wyniku nikt już nie potrzebuje.
                // EN: The receiver goes away only with the core; then nobody needs the result.
                let _ = tx.send(outcome);
            }
        });
    if let Err(e) = spawned {
        let error = load_error(&source, "a loader thread cannot be started", &e);
        let _ = tx.send(Outcome::Failed(source, Box::new(error)));
    }
}

/// PL: Kopia do `gen_dir` pod nazwą `<plik>-<ms>-<seq>.<roz>` i wczytanie kopii.
/// EN: Copies into `gen_dir` as `<file>-<ms>-<seq>.<ext>` and loads the copy.
///
/// # Errors
/// PL: `HOT-E0401` albo `HOT-E0402`. EN: `HOT-E0401` or `HOT-E0402`.
pub fn copy_and_open(source: &Path, gen_dir: &Path, seq: u64) -> Result<Gen, DiagError> {
    let stem = source
        .file_stem()
        .map_or_else(|| "mod".into(), |s| s.to_string_lossy().into_owned());
    let ext = source
        .extension()
        .map_or_else(String::new, |e| format!(".{}", e.to_string_lossy()));
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let file = gen_dir.join(format!("{stem}-{ms}-{seq}{ext}"));
    std::fs::create_dir_all(gen_dir)
        .and_then(|()| std::fs::copy(source, &file))
        .map_err(|e| load_error(source, "the file cannot be copied into hot-state/gen", &e))?;
    open(&file, source)
}

/// PL: Wczytuje bibliotekę z `file` i sprawdza tabelę moda.
/// EN: Loads the library from `file` and checks the mod table.
///
/// # Errors
/// PL: `HOT-E0401` albo `HOT-E0402`. EN: `HOT-E0401` or `HOT-E0402`.
pub fn open(file: &Path, source: &Path) -> Result<Gen, DiagError> {
    // SAFETY: wczytanie uruchamia kod inicjujący biblioteki. Ładujemy tylko artefakty z CI
    // repozytorium serwera (D14), czyli kod zaufany tak jak sam silnik.
    // EN: loading runs the library's initialisers. We only load artifacts from the server
    // repository's CI (D14), code trusted like the engine itself.
    let lib = unsafe { libloading::Library::new(file) }
        .map_err(|e| load_error(source, "the dynamic library cannot be opened", &e))?;
    // SAFETY: symbol eksportuje makro `hot_mod!` z typem `EntryFn`.
    // EN: the `hot_mod!` macro exports the symbol with the `EntryFn` type.
    let entry: EntryFn = match unsafe { lib.get::<EntryFn>(ENTRY_SYMBOL) } {
        Ok(symbol) => *symbol,
        Err(e) => return Err(load_error(
            source,
            "the library does not export hot_mod_entry (it was not built with hot_sdk::hot_mod!)",
            &e,
        )),
    };
    // PL: Biblioteki nie zamykamy (opis modułu). EN: The library is never closed (module docs).
    std::mem::forget(lib);
    note_mapped([(
        file.to_path_buf(),
        std::fs::metadata(file).map_or(0, |m| m.len()),
    )]);
    // SAFETY: funkcja z tabeli zwraca wskaźnik do statycznej tabeli w bibliotece, która nie
    // zostanie wyładowana. EN: returns a pointer to a static table in a library never unloaded.
    let table = unsafe { entry() };
    // SAFETY: wskaźnik niepusty wskazuje statyczną tabelę; pole `abi` jest pierwsze w każdej
    // wersji ABI, więc czytamy je przed resztą. EN: a non-null pointer is a static table; the
    // `abi` field comes first in every ABI version, so it is read before the rest.
    let Some(vtable) = (unsafe { table.as_ref() }) else {
        return Err(DiagError::new(
            codes::MOD_LOAD_FAILED,
            format!("Hot mod file {} returned no mod table", source.display()),
            "Export the mod with hot_sdk::hot_mod!(Type, id = \"...\", version = \"...\") and rebuild it",
        )
        .context("source", source.display().to_string())
        .context("file", file.display().to_string()));
    };
    if vtable.abi != ABI_MAJOR {
        return Err(DiagError::new(
            codes::ABI_MISMATCH,
            format!(
                "Hot mod file {} uses ABI version {}, but hot-core uses {ABI_MAJOR}",
                source.display(),
                vtable.abi
            ),
            "Rebuild the mod against the hot-sdk version that matches this server's hot-core",
        )
        .expected(format!("The mod table reports ABI version {ABI_MAJOR}"))
        .context("source", source.display().to_string())
        .context("mod_abi", vtable.abi)
        .context("core_abi", ABI_MAJOR)
        .state("The file was not activated; the previous generation of the mod keeps running"));
    }
    // SAFETY: napisy w statycznej tabeli żyją razem z biblioteką. EN: static strings.
    let id = unsafe { vtable.id.as_str() }.unwrap_or("").to_owned();
    // SAFETY: jak wyżej. EN: as above.
    let version = unsafe { vtable.version.as_str() }.unwrap_or("").to_owned();
    // SAFETY: jak wyżej. EN: as above.
    let commands = unsafe { vtable.commands.as_str() }
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if !valid_id(&id) {
        return Err(DiagError::new(
            codes::MOD_LOAD_FAILED,
            format!("Hot mod file {} has an invalid mod id {id:?}", source.display()),
            "Use an id of 1-64 characters from a-z, 0-9, '_' and '-' in hot_mod!(..., id = \"...\")",
        )
        .cause("The id becomes a folder name in hot-data/, so other characters are refused")
        .context("source", source.display().to_string())
        .context("id", id));
    }
    Ok(Gen {
        vtable,
        id,
        version,
        commands,
        file: file.to_path_buf(),
        source: source.to_path_buf(),
    })
}

/// PL: Id moda trafia do ścieżek, więc tylko bezpieczne znaki. EN: The id ends up in paths.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn load_error(source: &Path, what: &str, e: &(dyn std::error::Error + 'static)) -> DiagError {
    DiagError::new(
        codes::MOD_LOAD_FAILED,
        format!("Hot mod file {} cannot be loaded: {what}", source.display()),
        "Check that the path points to the mod's .so/.dylib built by CI for this machine's architecture (uname -m), then run hot load again",
    )
    .expected("The file is copied to hot-state/gen and opened as a dynamic library")
    .context("source", source.display().to_string())
    .caused_by(e)
    .state("The file was not activated; the previous generation of the mod keeps running")
}
