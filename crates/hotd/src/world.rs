//! PL: Świat, którego silnik nie wczyta (D20, M7.3). Horizon w takim wypadku kończy proces kodem 1
//! i zapisuje w `crash-reports/` raport z linią `Failed to load world "<folder>": <powód>`, a
//! supervisor po 5 takich awariach w 10 minut przestaje go wznawiać. `hotd` czyta najnowszy raport,
//! gdy silnik nie działa, i podaje przyczynę oraz polecenie przywrócenia najnowszej kopii z
//! `backups/`. Sam niczego nie przywraca: przywrócenie zastępuje świat, więc decyduje operator
//! (`horizon-server restore` bez `--yes` odmawia).
//!
//! EN: A world the engine cannot load (D20, M7.3). Horizon then exits with code 1 and writes a
//! report in `crash-reports/` with the line `Failed to load world "<folder>": <reason>`; after 5
//! such crashes in 10 minutes the supervisor stops restarting it. `hotd` reads the newest report
//! while the engine is down and gives the reason and the command that restores the newest backup
//! from `backups/`. It restores nothing itself: a restore replaces the world, so the operator
//! decides (`horizon-server restore` without `--yes` refuses).

use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// PL: Początek komunikatu Horizona (`world_loading.rs`). EN: Horizon's message prefix.
const MARKER: &str = "Failed to load world \"";

/// PL: Świat, który się nie wczytał. EN: A world that did not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    /// PL: Nazwa pliku raportu w `crash-reports/`. EN: The report's file name.
    pub report: String,
    /// PL: Powód z raportu, np. „its level data does not read (…)”. EN: The reason from the report.
    pub reason: String,
    /// PL: Cała linia z raportu. EN: The whole line from the report.
    pub line: String,
}

/// PL: Najnowszy raport w `<dir>/crash-reports/`, jeśli jest nowszy niż `since` i mówi, że świat
/// się nie wczytał. EN: The newest report in `<dir>/crash-reports/`, if newer than `since` and it
/// says the world did not load.
#[must_use]
pub fn load_failure(dir: &Path, since: Option<SystemTime>) -> Option<LoadFailure> {
    let (modified, path) = newest(&dir.join("crash-reports"), |name| {
        Path::new(name).extension().is_some_and(|x| x == "txt")
    })?;
    if since.is_some_and(|s| modified < s) {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    let line = text.lines().find(|l| l.contains(MARKER))?;
    let line = line[line.find(MARKER)?..].trim().to_owned();
    let reason = line
        .split_once("\": ")
        .map_or("", |(_, r)| r)
        // PL: Rada dla listy światów w kliencie; serwer dedykowany już próbował `level.dat_old`.
        // EN: Advice for the client's world list; the dedicated server already tried
        // `level.dat_old`.
        .split(" Open it from the world list")
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.')
        .to_owned();
    Some(LoadFailure {
        report: path.file_name()?.to_string_lossy().into_owned(),
        reason,
        line,
    })
}

/// PL: Nazwa najnowszej kopii (`*.zip`) w `<dir>/backups/`. Katalogi `replaced-*` to światy
/// zastąpione przy przywracaniu, nie kopie. EN: The newest backup (`*.zip`) in `<dir>/backups/`.
/// `replaced-*` folders are worlds replaced by a restore, not backups.
#[must_use]
pub fn newest_backup(dir: &Path) -> Option<String> {
    let (_, path) = newest(&dir.join("backups"), |name| {
        Path::new(name).extension().is_some_and(|x| x == "zip")
    })?;
    Some(path.file_name()?.to_string_lossy().into_owned())
}

fn newest(dir: &Path, keep: impl Fn(&str) -> bool) -> Option<(SystemTime, std::path::PathBuf)> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then_some(())?;
            Some((m.modified().ok()?, e.path()))
        })
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn file(path: &Path, text: &str, secs: u64) -> std::io::Result<()> {
        fs::create_dir_all(path.parent().unwrap_or(path))?;
        fs::write(path, text)?;
        fs::File::options()
            .write(true)
            .open(path)?
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[test]
    fn the_newest_report_and_backup_are_read() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("hotd-world-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let reports = dir.join("crash-reports");
        file(&reports.join("a.txt"), "Description: Fatal error\n", 1000)?;
        file(
            &reports.join("b.txt"),
            "---- Horizon Crash Report ----\n\nFailed to load world \"/srv/x/./world\": its level data does not read (NBT at byte 3: truncated input). Open it from the world list to try restoring level.dat_old.\n",
            2000,
        )?;
        file(&dir.join("backups/stara.zip"), "z", 1000)?;
        file(&dir.join("backups/nowa.zip"), "z", 3000)?;
        fs::create_dir_all(dir.join("backups/replaced-1/world"))?;

        let found = load_failure(&dir, None);
        let since = load_failure(
            &dir,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(2500)),
        );
        let backup = newest_backup(&dir);
        file(&reports.join("c.txt"), "inna awaria\n", 3000)?;
        let other = load_failure(&dir, None);
        fs::remove_dir_all(&dir)?;

        let found = found.ok_or_else(|| std::io::Error::other("no failure found"))?;
        assert_eq!(found.report, "b.txt");
        assert_eq!(
            found.reason,
            "its level data does not read (NBT at byte 3: truncated input)"
        );
        assert!(found
            .line
            .starts_with("Failed to load world \"/srv/x/./world\""));
        assert_eq!(since, None, "a report older than the last run is ignored");
        assert_eq!(backup.as_deref(), Some("nowa.zip"));
        assert_eq!(
            other, None,
            "a newer crash of another kind hides the old one"
        );
        Ok(())
    }
}
