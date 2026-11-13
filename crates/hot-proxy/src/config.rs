//! PL: Ustawienia proxy i ich kontrola przed startem. Tryb offline (bez kontroli zakupu) działa
//! tylko na adresie lokalnym: do testów na jednej maszynie. Instalator go nie ustawia.
//! EN: Proxy settings and their check before start. Offline mode (no purchase check) works only on a
//! local address, for tests on one machine. The installer never sets it.

use crate::codes;
use hot_diag::DiagError;
use std::net::SocketAddr;
use std::path::Path;

/// PL: Próg kompresji pakietów do gracza, jak domyślny w grze. EN: The game's default threshold.
pub const COMPRESSION: i32 = 256;

/// PL: Ustawienia proxy. EN: Proxy settings.
pub struct Config {
    /// PL: Adres dla graczy. EN: The address players connect to.
    pub listen: SocketAddr,
    /// PL: Silnik z `online_mode = false` i tym samym sekretem. EN: The engine, same secret.
    pub engine: SocketAddr,
    /// PL: Zawartość `forwarding.secret` silnika. EN: The engine's `forwarding.secret`.
    pub secret: Vec<u8>,
    /// PL: `false` tylko do testów lokalnych. EN: `false` only for local tests.
    pub online: bool,
    /// PL: Dokument discovery usługi sesji. EN: The session service discovery document.
    pub discovery: String,
}

impl Config {
    /// PL: Sprawdza, czy tryb offline nie wystawia serwera na zewnątrz.
    /// EN: Checks that offline mode does not expose the server.
    ///
    /// # Errors
    /// PL: Tryb offline na adresie innym niż lokalny. EN: Offline mode on a non-local address.
    pub fn check(&self) -> Result<(), DiagError> {
        if !self.online && !self.listen.ip().is_loopback() {
            return Err(DiagError::new(
                codes::CONFIG,
                format!(
                    "hot-proxy refuses offline mode on {}: anyone could join without a bought game",
                    self.listen
                ),
                "Remove --offline, or listen on 127.0.0.1 for a local test",
            )
            .expected("offline mode only on a loopback address")
            .context("listen", self.listen.to_string())
            .state("The proxy did not start"));
        }
        Ok(())
    }
}

/// PL: Czyta sekret jak silnik: obcina białe znaki z brzegów, pusty plik to błąd.
/// EN: Reads the secret as the engine does: trims surrounding whitespace; an empty file is an error.
///
/// # Errors
/// PL: Pliku nie da się odczytać albo jest pusty. EN: The file cannot be read or is empty.
pub fn read_secret(path: &Path) -> Result<Vec<u8>, DiagError> {
    let fail = |reason: String| {
        DiagError::new(
            codes::CONFIG,
            format!("hot-proxy cannot use the forwarding secret {}: {reason}", path.display()),
            "Point --secret at the engine's forwarding.secret (the file next to its server.toml); it must hold the same non-empty text for both",
        )
        .context("file", path.display().to_string())
        .state("The proxy did not start")
    };
    let bytes = std::fs::read(path).map_err(|e| fail(e.to_string()))?;
    let secret = bytes.trim_ascii();
    if secret.is_empty() {
        return Err(fail("the file is empty".into()));
    }
    Ok(secret.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(listen: &str, online: bool) -> Config {
        Config {
            listen: listen
                .parse()
                .unwrap_or_else(|_| SocketAddr::from(([0; 4], 0))),
            engine: SocketAddr::from(([127, 0, 0, 1], 25566)),
            secret: b"s".to_vec(),
            online,
            discovery: String::new(),
        }
    }

    #[test]
    fn offline_mode_is_refused_off_loopback() {
        assert!(config("0.0.0.0:25565", false).check().is_err());
        assert!(config("192.168.1.5:25565", false).check().is_err());
        assert!(config("127.0.0.1:25565", false).check().is_ok());
        assert!(config("0.0.0.0:25565", true).check().is_ok());
    }

    #[test]
    fn the_secret_is_trimmed_and_must_not_be_empty() {
        let dir = std::env::temp_dir().join(format!("hot-proxy-secret-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("forwarding.secret");
        let _ = std::fs::write(&file, b"  abc\n");
        assert_eq!(read_secret(&file).ok(), Some(b"abc".to_vec()));
        let _ = std::fs::write(&file, b" \n");
        assert!(read_secret(&file).is_err());
        assert!(read_secret(&dir.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
