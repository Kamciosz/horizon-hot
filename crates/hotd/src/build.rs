//! PL: Budowa silnika z `hot-host` (D15, D18, ADR 0012) przez `horizon-launcher native-build`.
//! Rewizję Horizona wybiera przypięcie w paczce (`[compatibility] horizon_revision`), nie wersja
//! bazowa: wersja bazowa daje tylko przygotowane pliki gry i platformę. Dlatego bazą jest dowolne
//! zainstalowane wydanie albo Nightly (nigdy wersja `native-…`), a gdy nie ma żadnego, najnowsze
//! wydanie (`install latest`). Datowane wydania znikają z GitHuba po kilku dniach, a źródło rewizji
//! zostaje pod tagiem `nightly-source/…`, więc ta droga działa też dla starych i nowych rewizji.
//! Używają jej instalator i obraz (`hotd build-engine`) oraz aktualizacja Horizona (`update.rs`).
//!
//! EN: Building the engine with `hot-host` (D15, D18, ADR 0012) through `horizon-launcher
//! native-build`. The bundle's pin (`[compatibility] horizon_revision`) chooses the Horizon
//! revision, not the base version: the base only gives the prepared game files and the platform.
//! So the base is any installed release or Nightly (never a `native-…` version), and when there is
//! none, the newest release (`install latest`). Dated releases vanish from GitHub after a few days
//! while a revision's source stays under its `nightly-source/…` tag, so this works for old and new
//! revisions alike. Used by the installer and the image (`hotd build-engine`) and by the Horizon
//! update (`update.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hot_diag::DiagError;
use serde_json::Value;

use crate::{brake, codes};

/// PL: Manifest paczki moda natywnego. EN: The native mod bundle's manifest.
pub const MANIFEST: &str = "horizon-mixin.toml";
/// PL: Plik silnika w katalogu wyjściowym `--server-output`. EN: The engine file in the output.
pub const ENGINE_FILE: &str = "horizon-engine";

/// PL: Pełny hash commita: 40 znaków szesnastkowych. EN: A full commit hash: 40 hex digits.
#[must_use]
pub fn is_revision(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// PL: Ustawia przypięcie `horizon_revision` w manifeście paczki. Zmienia tylko tę linię.
/// EN: Sets the `horizon_revision` pin in the bundle's manifest. Changes that line only.
///
/// # Errors
/// PL: Brak manifestu albo linii z przypięciem. EN: The manifest or its pin line is missing.
pub fn pin(bundle: &Path, rev: &str) -> Result<(), String> {
    let path = bundle.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut found = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.trim_start().starts_with("horizon_revision") && line.contains('=') {
            out.push_str(&format!("horizon_revision = \"{rev}\""));
            found = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !found {
        return Err(format!("{} has no horizon_revision line", path.display()));
    }
    fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// PL: Rewizja przypięta w manifeście paczki. EN: The revision pinned in the bundle's manifest.
#[must_use]
pub fn pinned(bundle: &Path) -> Option<String> {
    let text = fs::read_to_string(bundle.join(MANIFEST)).ok()?;
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("horizon_revision") && l.contains('='))
        .and_then(|l| l.split('"').nth(1))
        .map(str::to_owned)
}

/// PL: Rewizja Horizona, z której zbudowano silnik: `native-mods.json` obok pliku silnika.
/// EN: The Horizon revision an engine was built from: `native-mods.json` beside the engine file.
#[must_use]
pub fn engine_revision(engine_file: &Path) -> Option<String> {
    let about = engine_file.parent()?.join("native-mods.json");
    let v: Value = serde_json::from_slice(&fs::read(about).ok()?).ok()?;
    v["horizon"]
        .as_str()
        .filter(|r| is_revision(r))
        .map(str::to_owned)
}

/// PL: Wersja bazowa z listy `versions --installed --json`: pierwsze (najnowsze) wydanie albo
/// Nightly. EN: The base version from `versions --installed --json`: the first (newest) release
/// or Nightly.
#[must_use]
pub fn base_from(installed_json: &Value) -> Option<String> {
    installed_json["installed"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|t| {
            t.starts_with("nightly-")
                || (t.starts_with('v') && t[1..].starts_with(|c: char| c.is_ascii_digit()))
        })
        .map(str::to_owned)
}

fn launcher_error(what: &str, launcher: &Path, out: &Output) -> DiagError {
    let mut e = DiagError::new(
        codes::ENGINE_BUILD_FAILED,
        format!("horizon-launcher {what} failed"),
        "Read chain: the launcher's own words say what is missing (often: no prepared game files \
         for the server user; prepare once with `horizon-launcher prepare --download` as that user)",
    )
    .expected(format!("horizon-launcher {what} exits with 0"))
    .context("launcher", launcher.display().to_string())
    .state("the running engine is unchanged");
    let text =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    for line in text.lines().filter(|l| !l.trim().is_empty()).rev().take(20) {
        e = e.chain_line(line);
    }
    e
}

fn run(launcher: &Path, args: &[&str]) -> Result<Output, DiagError> {
    brake::output(Command::new(launcher).args(args)).map_err(|e| {
        DiagError::new(
            codes::ENGINE_BUILD_FAILED,
            "horizon-launcher cannot be started",
            "Install the Horizon launcher for this machine and pass its path (--launcher)",
        )
        .context("launcher", launcher.display().to_string())
        .caused_by(&e)
    })
}

/// PL: Wersja bazowa do budowy; gdy nic nie jest zainstalowane, instaluje najnowsze wydanie.
/// EN: The base version for the build; when nothing is installed, installs the newest release.
///
/// # Errors
/// PL: `HOT-E0301`. EN: `HOT-E0301`.
pub fn base_version(launcher: &Path) -> Result<String, DiagError> {
    let list = || -> Result<Option<String>, DiagError> {
        let out = run(launcher, &["versions", "--installed", "--json"])?;
        if !out.status.success() {
            return Err(launcher_error("versions --installed", launcher, &out));
        }
        Ok(serde_json::from_slice(&out.stdout)
            .ok()
            .and_then(|v| base_from(&v)))
    };
    if let Some(base) = list()? {
        return Ok(base);
    }
    let out = run(launcher, &["install", "latest"])?;
    if !out.status.success() {
        return Err(launcher_error("install latest", launcher, &out));
    }
    list()?.ok_or_else(|| {
        DiagError::new(
            codes::ENGINE_BUILD_FAILED,
            "No Horizon release or Nightly is installed after install latest",
            "Run horizon-launcher versions --installed as the server user and install a release",
        )
        .context("launcher", launcher.display().to_string())
    })
}

/// PL: Buduje silnik rewizji `rev` z paczki `bundle` do katalogu `out` (`out/horizon-engine`).
/// Paczkę kopiuje obok `out` i przypina w kopii; oryginał zostaje bez zmian. `trust` to zgoda
/// właściciela na kod natywny (`--trust-native-code`); bez niej nic nie rusza. Po błędzie `out`
/// nie istnieje.
/// EN: Builds the engine of revision `rev` from bundle `bundle` into `out` (`out/horizon-engine`).
/// Copies the bundle beside `out` and pins the copy; the original stays unchanged. `trust` is the
/// owner's consent to native code (`--trust-native-code`); without it nothing runs. After an error
/// `out` does not exist.
///
/// # Errors
/// PL: `HOT-E0301`. EN: `HOT-E0301`.
pub fn build_engine(
    launcher: &Path,
    bundle: &Path,
    rev: &str,
    out: &Path,
    trust: bool,
) -> Result<PathBuf, DiagError> {
    let refuse = |what: String, fix: &str| {
        DiagError::new(codes::ENGINE_BUILD_FAILED, what, fix)
            .context("rev", rev)
            .context("out", out.display().to_string())
            .state("the running engine is unchanged")
    };
    if !trust {
        return Err(refuse(
            "The engine is not built: the owner has not confirmed --trust-native-code".into(),
            "Read crates/hot-host, then run again with --trust-native-code (the owner's decision)",
        ));
    }
    if !is_revision(rev) {
        return Err(refuse(
            format!("{rev} is not a full Horizon commit (40 hex digits)"),
            "Pass the full commit hash",
        ));
    }
    let copy = out.with_extension("bundle");
    let _ = fs::remove_dir_all(&copy);
    let _ = fs::remove_dir_all(out);
    let copied = copy_dir(bundle, &copy).and_then(|()| pin(&copy, rev));
    if let Err(e) = copied {
        let _ = fs::remove_dir_all(&copy);
        return Err(refuse(
            format!("The hot-host bundle cannot be prepared: {e}"),
            "Rebuild it with crates/hot-host/build-bundle.sh",
        )
        .context("bundle", bundle.display().to_string()));
    }
    let result = base_version(launcher).and_then(|base| {
        let (copy_s, out_s) = (copy.display().to_string(), out.display().to_string());
        let built = run(
            launcher,
            &[
                "native-build",
                &copy_s,
                "--server-output",
                &out_s,
                "--version",
                &base,
                "--trust-native-code",
            ],
        )?;
        let engine = out.join(ENGINE_FILE);
        if built.status.success() && engine.is_file() {
            Ok(engine)
        } else {
            Err(launcher_error("native-build", launcher, &built).context("base", base))
        }
    });
    let _ = fs::remove_dir_all(&copy);
    if result.is_err() {
        let _ = fs::remove_dir_all(out);
    }
    result.map_err(|e| {
        e.context("rev", rev)
            .context("out", out.display().to_string())
    })
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_base_is_a_release_or_nightly_never_a_native_build() {
        let v = json!({ "installed": ["native-66f3981-782a", "nightly-2026-11-05-66f3981", "v0.2.97"] });
        assert_eq!(base_from(&v).as_deref(), Some("nightly-2026-11-05-66f3981"));
        let v = json!({ "installed": ["native-66f3981-782a", "v0.2.97"] });
        assert_eq!(base_from(&v).as_deref(), Some("v0.2.97"));
        assert_eq!(base_from(&json!({ "installed": ["native-1-2"] })), None);
        assert_eq!(base_from(&json!({})), None);
    }

    #[test]
    fn a_revision_is_forty_hex_digits() {
        assert!(is_revision("66f3981f522b24c250b21523a70080965b940424"));
        assert!(!is_revision("66f3981"));
        assert!(!is_revision(
            "nightly-2026-11-05-66f3981f522b24c250b21523a700"
        ));
    }
}
