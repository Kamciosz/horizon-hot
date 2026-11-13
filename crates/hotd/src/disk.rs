//! PL: Wolne miejsce na dysku serwera (D20, M7.2). `hotd` sprawdza je przed każdym krokiem.
//! Gdy go brakuje, usuwa tylko własne pliki, o których wie, że są zbędne: stare artefakty
//! (`hot-state/artifacts/`) i stare kopie `hot-core` (`hot-state/core/`). Kopii świata, cache
//! launchera ani dziennika systemu nie rusza, bo nie są jego. Pomiar przez `df -P` (POSIX, ten sam
//! układ kolumn w Linuksie i macOS), bez nowej zależności.
//!
//! EN: Free space on the server disk (D20, M7.2). `hotd` checks it before every step. When it runs
//! low, it removes only its own files it knows to be unneeded: old artifacts
//! (`hot-state/artifacts/`) and old `hot-core` copies (`hot-state/core/`). It leaves world backups,
//! the launcher cache and the system journal alone, as they are not its own. Measured with `df -P`
//! (POSIX, the same columns on Linux and macOS), with no new dependency.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

/// PL: Poniżej tylu wolnych bajtów `hotd` nie pobiera i nie stosuje commitów. Zapas na zapis
/// świata i logów silnika. EN: Below this many free bytes `hotd` neither fetches nor applies
/// commits. Room for the engine's world saves and logs.
pub const MIN_FREE_BYTES: u64 = 512 * 1024 * 1024;

/// PL: Wolne bajty na systemie plików z `path` (kolumna „Available” z `df -Pk`); `None`, gdy
/// `df` nie działa. EN: Free bytes on the file system holding `path` (the `df -Pk` "Available"
/// column); `None` when `df` fails.
#[must_use]
pub fn free_bytes(path: &Path) -> Option<u64> {
    let out = Command::new("df").arg("-Pk").arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_df(&String::from_utf8_lossy(&out.stdout))
}

/// PL: Druga linia `df -Pk`: kolumna przed „Capacity” (`NN%`). Liczona od pola z `%`, bo nazwa
/// urządzenia i punkt montowania mogą mieć spacje. EN: The second `df -Pk` line: the column before
/// "Capacity" (`NN%`), found from the `%` field, as the device and mount point may contain spaces.
fn parse_df(text: &str) -> Option<u64> {
    let line = text.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    let pct = fields.iter().position(|f| f.ends_with('%'))?;
    let kib: u64 = fields.get(pct.checked_sub(1)?)?.parse().ok()?;
    Some(kib.saturating_mul(1024))
}

/// PL: Rozmiary (KiB) wskazanych katalogów w `home` z `du -sk`, do kontekstu błędu.
/// EN: Sizes (KiB) of the given folders in `home` from `du -sk`, for the error context.
#[must_use]
pub fn usage(home: &Path, dirs: &[&str]) -> Vec<(String, u64)> {
    dirs.iter()
        .filter_map(|d| {
            let out = crate::brake::output(Command::new("du").arg("-sk").arg(home.join(d))).ok()?;
            let kib = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            Some(((*d).to_owned(), kib))
        })
        .collect()
}

/// PL: Usuwa wszystkie kopie `hot-core` z `dir` oprócz najnowszej i zwraca zwolnione bajty.
/// `hot-host` kopiuje rdzeń pod nową nazwą przy każdym starcie i `hot core load`, a nikt ich nie
/// sprząta. Wczytana biblioteka zostaje w pamięci po usunięciu pliku (Linux, macOS).
/// EN: Removes every `hot-core` copy in `dir` except the newest and returns the bytes freed.
/// `hot-host` copies the core under a new name at every start and `hot core load`, and nothing
/// cleans them up. A loaded library stays in memory after its file is removed (Linux, macOS).
pub fn prune_core_copies(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(SystemTime, u64, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("hot_core-"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then_some(())?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    files.sort();
    files.pop();
    files
        .into_iter()
        .filter(|(_, _, p)| fs::remove_file(p).is_ok())
        .map(|(_, len, _)| len)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn df_available_column_is_read_even_with_spaces() {
        let linux = "Filesystem     1024-blocks    Used Available Capacity Mounted on\n/dev/sda2         102400000 2000000   5242880      28% /\n";
        assert_eq!(parse_df(linux), Some(5_242_880 * 1024));
        let spaces = "Filesystem 1024-blocks Used Available Capacity Mounted on\nmap auto home 0 0 0 100% /System/Volumes/Data/home dir\n";
        assert_eq!(parse_df(spaces), Some(0));
        assert_eq!(parse_df("Filesystem\n"), None);
    }

    #[test]
    fn free_bytes_reads_this_machine() {
        assert!(free_bytes(Path::new(".")).is_some_and(|b| b > 0));
    }

    #[test]
    fn only_the_newest_core_copy_stays() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("hotd-core-copies-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        for (i, name) in ["hot_core-1.so", "hot_core-2.so", "hot_core-3.so"]
            .iter()
            .enumerate()
        {
            fs::write(dir.join(name), vec![0u8; 10 * (i + 1)])?;
            let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i as u64);
            fs::File::options()
                .write(true)
                .open(dir.join(name))?
                .set_modified(t)?;
        }
        fs::write(dir.join("inny.txt"), "x")?;
        let freed = prune_core_copies(&dir);
        let mut left: Vec<String> = fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        fs::remove_dir_all(&dir)?;
        assert_eq!(freed, 30);
        assert_eq!(left, ["hot_core-3.so", "inny.txt"]);
        Ok(())
    }
}
