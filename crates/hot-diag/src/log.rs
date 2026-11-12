//! PL: Zapis wpisów JSONL do `<katalog>/<składnik>.jsonl` z rotacją (D27: 50 MB × 5 plików).
//! EN: Writes JSONL entries to `<dir>/<component>.jsonl` with rotation (D27: 50 MB × 5 files).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::{codes, DiagError, Redactor};

/// PL: Rozmiar jednego pliku logu przed rotacją. EN: One log file's size before rotation.
pub const DEFAULT_MAX_BYTES: u64 = 50 * 1024 * 1024;
/// PL: Liczba plików razem z bieżącym. EN: Number of files including the current one.
pub const DEFAULT_FILES: u32 = 5;

/// PL: Ustawienia loggera jednego składnika. EN: One component's logger settings.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// PL: Nazwa składnika, np. `hotd`. EN: Component name, e.g. `hotd`.
    pub component: String,
    /// PL: Wersja i commit, z którego zbudowano składnik. EN: Version and commit it was built from.
    pub version: String,
    /// PL: Katalog logów, np. `/srv/horizon/logs`. EN: Log directory, e.g. `/srv/horizon/logs`.
    pub dir: PathBuf,
    /// PL: Rozmiar pliku przed rotacją. EN: File size before rotation.
    pub max_bytes: u64,
    /// PL: Liczba plików razem z bieżącym. EN: Number of files including the current one.
    pub files: u32,
}

impl LogConfig {
    /// PL: Domyślne limity D27. EN: The default D27 limits.
    #[must_use]
    pub fn new(component: &str, version: &str, dir: impl Into<PathBuf>) -> Self {
        Self {
            component: component.to_owned(),
            version: version.to_owned(),
            dir: dir.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            files: DEFAULT_FILES,
        }
    }
}

struct Sink {
    file: File,
    size: u64,
}

/// PL: Logger jednego składnika. Bezpieczny między wątkami.
/// EN: One component's logger. Safe to share between threads.
pub struct Logger {
    config: LogConfig,
    redactor: Redactor,
    sink: Mutex<Sink>,
}

impl Logger {
    /// PL: Otwiera (albo tworzy) bieżący plik logu.
    /// EN: Opens (or creates) the current log file.
    ///
    /// # Errors
    /// PL: Katalogu albo pliku nie da się utworzyć. EN: The directory or file cannot be created.
    pub fn open(config: LogConfig, redactor: Redactor) -> Result<Self, DiagError> {
        let path = current_path(&config);
        let sink = open_sink(&config.dir, &path).map_err(|e| write_error(&path, &e))?;
        Ok(Self {
            config,
            redactor,
            sink: Mutex::new(sink),
        })
    }

    /// PL: Ścieżka bieżącego pliku. EN: The current file's path.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        current_path(&self.config)
    }

    /// PL: Lista sekretów, której używa ten logger. EN: The secret list this logger uses.
    #[must_use]
    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// PL: Zapisuje błąd. Gdy zapis się nie uda, wypisuje oba wpisy na stderr (trafiają do
    /// journala systemd albo `supervisor.log`), bo logger nie ma gdzie zgłosić własnego błędu.
    ///
    /// EN: Writes an error. When writing fails, prints both entries to stderr (they reach the
    /// systemd journal or `supervisor.log`), as the logger has nowhere else to report its own error.
    pub fn error(&self, error: &DiagError) {
        let line = self.line(error.fields());
        if let Err(e) = self.write_line(&line) {
            let failure = self.line(write_error(&self.path(), &e).fields());
            fallback_stderr(&line, &failure);
        }
    }

    fn line(&self, fields: Map<String, Value>) -> String {
        let mut v = Value::Object(fields);
        if let Value::Object(m) = &mut v {
            m.insert("ts".into(), utc_timestamp(SystemTime::now()).into());
            m.insert("component".into(), self.config.component.clone().into());
            m.insert("version".into(), self.config.version.clone().into());
        }
        self.redactor.value(&mut v);
        v.to_string()
    }

    fn write_line(&self, line: &str) -> io::Result<()> {
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let len = line.len() as u64 + 1;
        if sink.size > 0 && sink.size + len > self.config.max_bytes {
            rotate(&self.config)?;
            *sink = open_sink(&self.config.dir, &current_path(&self.config))?;
        }
        sink.file.write_all(line.as_bytes())?;
        sink.file.write_all(b"\n")?;
        sink.size += len;
        Ok(())
    }
}

fn current_path(c: &LogConfig) -> PathBuf {
    c.dir.join(format!("{}.jsonl", c.component))
}

fn numbered_path(c: &LogConfig, n: u32) -> PathBuf {
    c.dir.join(format!("{}.{n}.jsonl", c.component))
}

fn open_sink(dir: &Path, path: &Path) -> io::Result<Sink> {
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata()?.len();
    Ok(Sink { file, size })
}

/// PL: `x.jsonl` → `x.1.jsonl` → … → `x.<files-1>.jsonl`; najstarszy znika.
/// EN: `x.jsonl` → `x.1.jsonl` → … → `x.<files-1>.jsonl`; the oldest is dropped.
fn rotate(c: &LogConfig) -> io::Result<()> {
    let last = c.files.saturating_sub(1);
    if last == 0 {
        return fs::remove_file(current_path(c));
    }
    match fs::remove_file(numbered_path(c, last)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    for n in (1..last).rev() {
        let from = numbered_path(c, n);
        if from.exists() {
            fs::rename(from, numbered_path(c, n + 1))?;
        }
    }
    fs::rename(current_path(c), numbered_path(c, 1))
}

fn write_error(path: &Path, e: &io::Error) -> DiagError {
    DiagError::new(
        codes::LOG_WRITE_FAILED,
        format!("The log file {} cannot be written", path.display()),
        "Check that the log directory exists, is writable by this service's user and that the disk is not full (df -h)",
    )
    .expected("Every log entry is appended to the component's .jsonl file")
    .context("path", path.display().to_string())
    .caused_by(e)
    .state("The entry was printed to stderr instead; the component keeps running")
}

#[allow(clippy::print_stderr)] // PL: jedyne wyjście, gdy plik logu zawiódł. EN: the only way out when the log file failed.
fn fallback_stderr(entry: &str, failure: &str) {
    eprintln!("{failure}");
    eprintln!("{entry}");
}

/// PL: Czas UTC w formacie RFC 3339 z milisekundami, bez zależności od biblioteki czasu.
/// EN: UTC time as RFC 3339 with milliseconds, without a time library.
#[must_use]
pub fn utc_timestamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, dd) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{dd:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// PL: Dni od 1970-01-01 → data (algorytm H. Hinnanta „civil_from_days”).
/// EN: Days since 1970-01-01 → date (H. Hinnant's "civil_from_days").
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamp_matches_known_dates() {
        let at = |s| utc_timestamp(UNIX_EPOCH + Duration::from_millis(s));
        assert_eq!(at(0), "1970-01-01T00:00:00.000Z");
        // PL: 2000-02-29 (rok przestępny), 2026-11-12 12:34:56.789.
        // EN: 2000-02-29 (leap year), 2026-11-12 12:34:56.789.
        assert_eq!(at(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(at(1_794_486_896_789), "2026-11-12T12:34:56.789Z");
    }
}
