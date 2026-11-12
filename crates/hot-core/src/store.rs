//! PL: Pliki stanu w `<HOT_HOME>/hot-state/` (D13, D21):
//! - `mods.json` — aktywne mody: bieżąca i ostatnia dobra generacja, znacznik czystego zatrzymania;
//! - `status.json` — stan dla `hotd` i `hot status`;
//! - `saved/<mod>.bin` — stan moda oddany przy zatrzymaniu, zużywany przy następnym starcie;
//! - `gen/` — kopie plików modów (unikalna nazwa na generację).
//!
//! Każdy zapis idzie przez plik tymczasowy i `rename`, więc awaria w połowie zapisu nie zostawia
//! uciętego pliku, który zablokowałby następny start.
//!
//! EN: State files in `<HOT_HOME>/hot-state/` (D13, D21): `mods.json` (active mods: current and
//! last good generation, clean stop flag), `status.json` (state for `hotd` and `hot status`),
//! `saved/<mod>.bin` (a mod's state handed over at stop, consumed at the next start), `gen/`
//! (copies of mod files, one unique name per generation). Every write goes through a temporary
//! file and `rename`, so a crash mid-write never leaves a torn file that would block the next start.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use hot_diag::DiagError;
use serde_json::{json, Value};

use crate::codes;

/// PL: Jeden mod w `mods.json`. EN: One mod in `mods.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModRecord {
    pub id: String,
    pub version: String,
    pub generation: u64,
    /// PL: Plik z `hot load`. EN: The file from `hot load`.
    pub source: PathBuf,
    /// PL: Kopia bieżącej generacji. EN: The current generation's copy.
    pub current: PathBuf,
    /// PL: Kopia i numer generacji, która przetrwała próbę (D13), jeśli jest.
    /// EN: The copy and number of the generation that passed probation (D13), if any.
    pub good: Option<(PathBuf, u64)>,
}

/// PL: Zawartość `mods.json`. EN: The contents of `mods.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModsFile {
    pub mods: Vec<ModRecord>,
    /// PL: Rdzeń zatrzymał się sam (nie awaria). EN: The core stopped cleanly (no crash).
    pub clean_stop: bool,
}

/// PL: Ścieżki plików stanu. EN: State file paths.
#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn state(&self) -> PathBuf {
        self.home.join("hot-state")
    }
    pub fn mods(&self) -> PathBuf {
        self.state().join("mods.json")
    }
    pub fn status(&self) -> PathBuf {
        self.state().join("status.json")
    }
    pub fn gen_dir(&self) -> PathBuf {
        self.state().join("gen")
    }
    pub fn saved(&self, id: &str) -> PathBuf {
        self.state().join("saved").join(format!("{id}.bin"))
    }
    pub fn data(&self, id: &str) -> PathBuf {
        self.home.join("hot-data").join(id)
    }
    pub fn logs(&self) -> PathBuf {
        self.home.join("logs")
    }
}

/// PL: Zapis przez plik tymczasowy i `rename`. EN: Write via a temporary file and `rename`.
///
/// # Errors
/// PL: `HOT-E0404`. EN: `HOT-E0404`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DiagError> {
    let tmp = path.with_extension("tmp");
    let result = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&tmp, bytes))
        .and_then(|()| fs::rename(&tmp, path));
    result.map_err(|e| state_error(path, "cannot be written", &e))
}

/// PL: Czyta `mods.json`. Brak pliku to pusty zestaw. Uszkodzony plik zostaje przeniesiony obok
/// (`mods.json.broken-<ms>`), a błąd wraca do logu; rdzeń startuje wtedy bez modów.
/// EN: Reads `mods.json`. A missing file is an empty set. A broken file is moved aside and the
/// error goes to the log; the core then starts without mods.
///
/// # Errors
/// PL: `HOT-E0404` przy pliku nieczytelnym albo uszkodzonym. EN: `HOT-E0404`.
pub fn read_mods(paths: &Paths) -> Result<ModsFile, DiagError> {
    let path = paths.mods();
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ModsFile::default()),
        Err(e) => return Err(state_error(&path, "cannot be read", &e)),
    };
    parse_mods(&text).ok_or_else(|| {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let aside = path.with_extension(format!("json.broken-{ms}"));
        let moved = fs::rename(&path, &aside).is_ok();
        DiagError::new(
            codes::STATE_FILE,
            format!("The hot mod list {} is not valid JSON of the expected shape", path.display()),
            "Look at the moved file to see what broke it; hotd loads the mods again from the server repository, or run hot load for each mod",
        )
        .expected("{\"clean_stop\": bool, \"mods\": [{\"id\", \"version\", \"generation\", \"source\", \"current\", \"good\": null | {\"file\", \"generation\"}}]}")
        .context("path", path.display().to_string())
        .context("moved_to", if moved { aside.display().to_string() } else { String::new() })
        .state("hot-core started without hot mods")
    })
}

fn parse_mods(text: &str) -> Option<ModsFile> {
    let v: Value = serde_json::from_str(text).ok()?;
    let clean_stop = v.get("clean_stop")?.as_bool()?;
    let mut mods = Vec::new();
    for m in v.get("mods")?.as_array()? {
        let s = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_owned);
        mods.push(ModRecord {
            id: s("id")?,
            version: s("version")?,
            generation: m.get("generation")?.as_u64()?,
            source: s("source")?.into(),
            current: s("current")?.into(),
            good: match m.get("good")? {
                Value::Null => None,
                g => Some((
                    g.get("file")?.as_str()?.into(),
                    g.get("generation")?.as_u64()?,
                )),
            },
        });
    }
    Some(ModsFile { mods, clean_stop })
}

/// PL: Zapisuje `mods.json`. EN: Writes `mods.json`.
///
/// # Errors
/// PL: `HOT-E0404`. EN: `HOT-E0404`.
pub fn write_mods(paths: &Paths, file: &ModsFile) -> Result<(), DiagError> {
    let mods: Vec<Value> = file
        .mods
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "version": m.version,
                "generation": m.generation,
                "source": m.source.display().to_string(),
                "current": m.current.display().to_string(),
                "good": m.good.as_ref().map(|(file, generation)| {
                    json!({ "file": file.display().to_string(), "generation": generation })
                }),
            })
        })
        .collect();
    let v = json!({ "clean_stop": file.clean_stop, "mods": mods });
    write_atomic(&paths.mods(), format!("{v:#}\n").as_bytes())
}

/// PL: Stan moda zapisany przy zatrzymaniu; plik znika po odczycie, żeby po awarii nie wrócił
/// stary stan. EN: A mod's state saved at stop; the file is removed after reading so an old state
/// never comes back after a crash.
pub fn take_saved(paths: &Paths, id: &str) -> Vec<u8> {
    let path = paths.saved(id);
    let bytes = fs::read(&path).unwrap_or_default();
    let _ = fs::remove_file(&path);
    bytes
}

/// PL: Usuwa z `gen/` kopie, których nie wskazuje `mods.json` (generacje sprzed restartu).
/// EN: Removes copies in `gen/` that `mods.json` does not point to (pre-restart generations).
pub fn prune_generations(paths: &Paths, keep: &[PathBuf]) -> usize {
    let mut removed = 0;
    for entry in fs::read_dir(paths.gen_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.is_file() && !keep.contains(&path) && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub fn state_error(path: &Path, what: &str, e: &io::Error) -> DiagError {
    DiagError::new(
        codes::STATE_FILE,
        format!("The hot-core state file {} {what}", path.display()),
        "Check that the server user owns the hot-state folder, that it is writable and that the disk is not full (df -h)",
    )
    .context("path", path.display().to_string())
    .caused_by(e)
    .state("hot-core keeps running with the state it has in memory")
}
