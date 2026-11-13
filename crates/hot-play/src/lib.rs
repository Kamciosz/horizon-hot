//! PL: `hot-play`, aplikacja gracza (D26, M11): menu w terminalu, „Graj” sprawdza i naprawia
//! launcher, wersję, pliki gry, instancję i pulę slotów, a potem łączy z serwerem. „Napraw” robi
//! to samo od nowa i zapisuje raport do wysłania właścicielowi serwera.
//! EN: `hot-play`, the player app (D26, M11): a terminal menu; "Play" checks and repairs the
//! launcher, the version, the game files, the instance and the slot pool, then joins the server.
//! "Repair" redoes it all and saves a report to send to the server owner.

pub mod codes;
pub mod game;
pub mod real;
pub mod servers;

use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::Redactor;
use serde_json::{json, Value};

use game::{Setup, State};

/// PL: Maskowanie w logach i raporcie gracza: katalog domowy i nazwa konta systemu (od 4 znaków,
/// krótsza zamaskowałaby zwykłe słowa). Raport idzie do obcej osoby (D28).
/// EN: Masking in the player's logs and report: the home folder and the system account name (from
/// 4 characters; a shorter one would mask ordinary words). The report goes to someone else (D28).
#[must_use]
pub fn player_redactor(home: Option<&str>, user: Option<&str>) -> Redactor {
    let mut r = Redactor::new();
    if let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])) {
        if home.len() > 1 {
            r.add(home);
        }
    }
    if let Some(user) = user.filter(|u| u.chars().count() >= 4) {
        r.add(user);
    }
    r
}

/// PL: Folder danych: `HOT_PLAY_HOME`, inaczej domyślny folder danych aplikacji systemu.
/// EN: The data folder: `HOT_PLAY_HOME`, otherwise the system's default app data folder.
#[must_use]
pub fn data_dir() -> PathBuf {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = var("HOT_PLAY_HOME") {
        return dir;
    }
    if cfg!(windows) {
        return var("LOCALAPPDATA")
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hot-play");
    }
    let home = var("HOME").unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support/hot-play");
    }
    var("XDG_DATA_HOME")
        .unwrap_or_else(|| home.join(".local/share"))
        .join("hot-play")
}

/// PL: Ustawienia z `config.json`: adres gry, adres paczki, nazwa serwera.
/// EN: Settings from `config.json`: game address, pack address, server name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub server: String,
    pub pack: String,
    pub name: String,
}

impl Config {
    /// PL: Ustawienia z samego adresu: port gry 25565, paczka na porcie 8131 tego samego hosta.
    /// EN: Settings from an address alone: game port 25565, the pack on port 8131 of the same host.
    #[must_use]
    pub fn from_address(address: &str) -> Self {
        let address = address.trim();
        let host = match address.rsplit_once(':') {
            Some((host, port)) if port.parse::<u16>().is_ok() && !host.ends_with(':') => host,
            _ => address,
        };
        let server = if host == address {
            format!("{address}:25565")
        } else {
            address.to_owned()
        };
        Self {
            server,
            pack: format!("http://{host}:8131"),
            name: "horizon-hot".to_owned(),
        }
    }

    #[must_use]
    pub fn load(data: &Path) -> Option<Self> {
        let value: Value =
            serde_json::from_slice(&fs::read(data.join("config.json")).ok()?).ok()?;
        let text = |k: &str| value[k].as_str().map(str::to_owned);
        Some(Self {
            server: text("server")?,
            pack: text("pack")?,
            name: text("name").unwrap_or_else(|| "horizon-hot".to_owned()),
        })
    }

    /// # Errors
    /// PL: Zapis się nie udał. EN: The write failed.
    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        fs::create_dir_all(data)?;
        let value = json!({ "server": self.server, "pack": self.pack, "name": self.name });
        fs::write(data.join("config.json"), format!("{value:#}\n"))
    }

    #[must_use]
    pub fn setup(&self) -> Setup {
        Setup {
            server: self.server.clone(),
            name: self.name.clone(),
        }
    }
}

/// PL: Stan z `state.json`; brak albo zły plik = stan pusty (wszystko zostanie sprawdzone).
/// EN: State from `state.json`; a missing or bad file = empty state (everything gets checked).
#[must_use]
pub fn load_state(data: &Path) -> State {
    let value: Value = fs::read(data.join("state.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    State {
        prepared: value["prepared"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    }
}

/// # Errors
/// PL: Zapis się nie udał. EN: The write failed.
pub fn save_state(data: &Path, state: &State) -> std::io::Result<()> {
    fs::create_dir_all(data)?;
    let value = json!({ "prepared": state.prepared });
    fs::write(data.join("state.json"), format!("{value:#}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_and_the_account_name_are_masked() {
        let r = player_redactor(Some("/Users/jan.kowalski/"), Some("jan.kowalski"));
        let out = r.text("/Users/jan.kowalski/Library/x and /tmp/-Users-jan.kowalski-x");
        assert!(!out.contains("jan.kowalski"), "{out}");
        let r = player_redactor(Some("/"), Some("ola"));
        assert_eq!(r.text("ola / Ola"), "ola / Ola");
    }

    #[test]
    fn an_address_alone_gets_the_default_ports() {
        let c = Config::from_address("gra.example.pl");
        assert_eq!(c.server, "gra.example.pl:25565");
        assert_eq!(c.pack, "http://gra.example.pl:8131");
        let c = Config::from_address(" 10.0.0.5:25570 ");
        assert_eq!(c.server, "10.0.0.5:25570");
        assert_eq!(c.pack, "http://10.0.0.5:8131");
    }
}
