//! PL: Samoaktualizacja `hot-play` (M11.3, ADR 0016). Działa tylko w plikach zbudowanych przez CI:
//! workflow ustawia przy kompilacji `HOT_PLAY_ASSET` (nazwa pliku w wydaniu) i
//! `HOT_PLAY_UPDATE_URL` (adres wydania `hot-play`). Przy starcie program porównuje SHA-256 swojego
//! pliku z `SHA256SUMS` wydania. Gdy się różnią, pobiera nowy plik, sprawdza jego sumę, wstawia go
//! na miejsce swojego i uruchamia się od nowa. Błąd niczego nie zatrzymuje: gra idzie na obecnej
//! wersji (`HOT-E0909` w logu).
//! EN: `hot-play` self-update (M11.3, ADR 0016). Only in CI-built files: the workflow sets
//! `HOT_PLAY_ASSET` (the file name in the release) and `HOT_PLAY_UPDATE_URL` (the `hot-play`
//! release address) at compile time. At start the program compares its own file's SHA-256 with
//! the release `SHA256SUMS`. When they differ, it downloads the new file, checks its checksum, puts
//! it in place of its own and starts again. An error stops nothing: play goes on with the current
//! version (`HOT-E0909` in the log).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use hot_diag::DiagError;
use sha2::{Digest, Sha256};

use crate::codes;
use crate::real::get;

/// PL: Nazwa pliku tego programu w wydaniu; brak = samoaktualizacja wyłączona.
/// EN: This program's file name in the release; none = self-update off.
pub const ASSET: Option<&str> = option_env!("HOT_PLAY_ASSET");

const URL: Option<&str> = option_env!("HOT_PLAY_UPDATE_URL");

/// PL: Zmienna, która wyłącza samoaktualizację (też po ponownym uruchomieniu nowej wersji).
/// EN: The variable that turns self-update off (also set for the restarted new version).
pub const OFF: &str = "HOT_PLAY_NO_UPDATE";

/// PL: Adres wydania: `HOT_PLAY_UPDATE_URL` z otoczenia (test na własnym serwerze) albo z kompilacji.
/// `None`, gdy plik nie pochodzi z CI albo ustawiono `HOT_PLAY_NO_UPDATE`.
/// EN: The release address: `HOT_PLAY_UPDATE_URL` from the environment (a test on one's own
/// server) or from the build. `None` when the file is not from CI or `HOT_PLAY_NO_UPDATE` is set.
#[must_use]
pub fn source() -> Option<(String, &'static str)> {
    if std::env::var_os(OFF).is_some() {
        return None;
    }
    let asset = ASSET?;
    let url = std::env::var("HOT_PLAY_UPDATE_URL")
        .ok()
        .or_else(|| URL.map(str::to_owned))?;
    Some((url.trim_end_matches('/').to_owned(), asset))
}

/// PL: Suma pliku `asset` z treści `SHA256SUMS` (`<hex>  <nazwa>` albo `<hex> *<nazwa>`).
/// EN: The checksum of `asset` from `SHA256SUMS` (`<hex>  <name>` or `<hex> *<name>`).
#[must_use]
pub fn wanted(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == asset && hex.len() == 64).then(|| hex.to_ascii_lowercase())
    })
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// PL: Nowy plik programu, gdy wydanie ma inny niż `exe`; `None`, gdy `exe` jest aktualny.
/// EN: The new program file when the release has a different one than `exe`; `None` when `exe`
/// is current.
///
/// # Errors
/// PL: `HOT-E0909`: brak sieci, brak pliku w `SHA256SUMS` albo pobrany plik z inną sumą.
/// EN: `HOT-E0909`: no network, the file missing from `SHA256SUMS`, or a download with a
/// different checksum.
pub fn check(url: &str, asset: &str, exe: &Path) -> Result<Option<Vec<u8>>, DiagError> {
    let fail = |what: &str| {
        DiagError::new(
            codes::UPDATE,
            what.to_owned(),
            "nothing; the next start tries again",
        )
        .context("url", url)
        .context("file", asset)
    };
    let sums = get(&format!("{url}/SHA256SUMS"), codes::UPDATE)?;
    let want = wanted(&String::from_utf8_lossy(&sums), asset)
        .ok_or_else(|| fail("SHA256SUMS of the hot-play release does not list this file"))?;
    let own = fs::read(exe).map_err(|e| fail("hot-play cannot read its own file").caused_by(&e))?;
    if sha256_hex(&own) == want {
        return Ok(None);
    }
    let bytes = get(&format!("{url}/{asset}"), codes::UPDATE)?;
    let got = sha256_hex(&bytes);
    if got != want {
        return Err(
            fail("the downloaded hot-play has a different SHA-256 than SHA256SUMS")
                .context("expected", want)
                .context("got", got),
        );
    }
    Ok(Some(bytes))
}

/// PL: Wstawia `bytes` na miejsce `exe`. Unix: nowy plik obok i `rename`, więc przerwanie nie
/// zostawia połowy programu. Windows nie pozwala nadpisać działającego pliku, ale pozwala zmienić
/// mu nazwę: stary dostaje końcówkę `.old` (usuwa ją [`cleanup`] przy następnym starcie).
/// EN: Puts `bytes` in place of `exe`. Unix: a new file next to it, then `rename`, so a break
/// never leaves half a program. Windows does not let a running file be overwritten but lets it be
/// renamed: the old one gets an `.old` suffix ([`cleanup`] removes it on the next start).
///
/// # Errors
/// PL: Błąd zapisu; `exe` zostaje wtedy bez zmian. EN: A write error; `exe` is then unchanged.
pub fn replace(exe: &Path, bytes: &[u8]) -> io::Result<()> {
    let next = sibling(exe, "new");
    fs::write(&next, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&next, fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(windows) {
        let old = sibling(exe, "old");
        let _ = fs::remove_file(&old);
        fs::rename(exe, &old)?;
        if let Err(e) = fs::rename(&next, exe) {
            let _ = fs::rename(&old, exe);
            return Err(e);
        }
        Ok(())
    } else {
        fs::rename(&next, exe).inspect_err(|_| {
            let _ = fs::remove_file(&next);
        })
    }
}

/// PL: Usuwa pozostałości po poprzedniej aktualizacji. EN: Removes leftovers of an earlier update.
pub fn cleanup(exe: &Path) {
    for end in ["old", "new"] {
        let _ = fs::remove_file(sibling(exe, end));
    }
}

fn sibling(exe: &Path, end: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{end}"));
    exe.with_file_name(name)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)] // PL: testy. EN: tests.

    use super::*;

    #[test]
    fn the_checksum_list_gives_the_right_file() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = format!(
            "{a}  hot-play-linux-x64\n{b} *hot-play-windows-x64.exe\nshort  hot-play-macos-arm64\n"
        );
        assert_eq!(wanted(&sums, "hot-play-linux-x64"), Some(a));
        assert_eq!(
            wanted(&sums, "hot-play-windows-x64.exe"),
            Some("b".repeat(64))
        );
        assert_eq!(wanted(&sums, "hot-play-macos-arm64"), None, "not a SHA-256");
        assert_eq!(wanted(&sums, "hot-play-linux"), None, "no prefix match");
    }

    #[test]
    fn the_new_file_takes_the_old_ones_place() {
        let dir = std::env::temp_dir().join(format!("hot-play-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("hot-play");
        fs::write(&exe, b"old").unwrap();
        replace(&exe, b"new").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert!(!sibling(&exe, "new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        cleanup(&exe);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
