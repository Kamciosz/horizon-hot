//! PL: Sprawdzenie podpisu artefaktu z CI przez `ssh-keygen -Y verify` (OpenSSH jest na Ubuntu
//! i w GitHub Actions; ADR 0008). Klucz prywatny jest tylko w sekretach CI, a serwer ma plik
//! `allowed_signers` z kluczem publicznym. Bez tego pliku `hotd` niczego nie ładuje.
//!
//! EN: Checks the signature of a CI artifact with `ssh-keygen -Y verify` (OpenSSH ships with
//! Ubuntu and GitHub Actions; ADR 0008). The private key lives only in the CI secrets; the server
//! has an `allowed_signers` file with the public key. Without that file `hotd` loads nothing.

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};

use hot_diag::DiagError;

use crate::codes;

/// PL: Tożsamość podpisującego w `allowed_signers`. EN: The signer identity in `allowed_signers`.
pub const PRINCIPAL: &str = "hot-ci";
/// PL: Przestrzeń nazw podpisu; podpis z innego narzędzia się nie nada.
/// EN: The signature namespace; a signature made for another tool does not fit.
pub const NAMESPACE: &str = "horizon-hot-mod";

/// PL: Sprawdza `file` z podpisem `sig`. EN: Checks `file` against the signature `sig`.
///
/// # Errors
/// PL: `HOT-E0203`. EN: `HOT-E0203`.
pub fn verify(allowed_signers: &Path, file: &Path, sig: &Path) -> Result<(), DiagError> {
    let fail = |cause: String| {
        DiagError::new(
            codes::BAD_SIGNATURE,
            format!(
                "The signature of the CI artifact {} does not match the server's key",
                file.display()
            ),
            "Check that the private key in the repository's HOT_SIGNING_KEY secret pairs with the public key in the allowed_signers file named in context, then push again; never load the file by hand",
        )
        .expected(format!(
            "ssh-keygen -Y verify -I {PRINCIPAL} -n {NAMESPACE} accepts the signature"
        ))
        .cause(cause)
        .context("file", file.display().to_string())
        .context("signature", sig.display().to_string())
        .context("allowed_signers", allowed_signers.display().to_string())
        .state("The commit was not applied; the server keeps the mods it had")
    };
    if !allowed_signers.is_file() {
        return Err(fail(format!(
            "the allowed_signers file {} does not exist",
            allowed_signers.display()
        )));
    }
    let input = File::open(file).map_err(|e| fail(format!("the artifact cannot be read: {e}")))?;
    let out = Command::new("ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(allowed_signers)
        .args(["-I", PRINCIPAL, "-n", NAMESPACE, "-s"])
        .arg(sig)
        .stdin(Stdio::from(input))
        .output()
        .map_err(|e| fail(format!("ssh-keygen cannot be started: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(fail(format!(
            "ssh-keygen exit {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}
