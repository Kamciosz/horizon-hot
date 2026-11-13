//! PL: Prawdziwy [`World`]: `horizon-launcher` (wiersz poleceń, `--json`), serwer przez HTTP
//! (`hotd serve-pack`) i pliki w folderze gry instancji. Launcher pobiera z wydań GitHuba
//! `horizon-rust/horizon` (`launcher@…`) i sprawdza SHA-256 z `SHA256SUMS`.
//! EN: The real [`World`]: `horizon-launcher` (command line, `--json`), the server over HTTP
//! (`hotd serve-pack`) and files in the instance's game folder. The launcher comes from the GitHub
//! releases of `horizon-rust/horizon` (`launcher@…`), checked against SHA-256 in `SHA256SUMS`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use hot_diag::{Code, DiagError, Logger};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::codes;
use crate::game::{Instance, ServerInfo, Versions, World};

const RELEASES: &str = "https://api.github.com/repos/horizon-rust/horizon/releases?per_page=40";
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

/// PL: Komputer gracza. EN: The player's computer.
pub struct Real {
    /// PL: Folder danych `hot-play` (`launcher/`, `logs/`, `config.json`). EN: The `hot-play`
    /// data folder.
    pub data: PathBuf,
    /// PL: Adres paczki zasobów, np. `http://serwer.pl:8131`. EN: The resource pack address.
    pub pack: String,
    pub log: Option<Logger>,
    launcher: Option<PathBuf>,
}

impl Real {
    #[must_use]
    pub fn new(data: PathBuf, pack: String, log: Option<Logger>) -> Self {
        Self {
            data,
            pack: pack.trim_end_matches('/').to_owned(),
            log,
            launcher: None,
        }
    }

    fn info(&self, what: &str, context: Value) {
        if let Some(log) = &self.log {
            log.info(what, context);
        }
    }

    fn hl(&self) -> Result<Command, DiagError> {
        let path = self.launcher.as_ref().ok_or_else(|| {
            DiagError::new(
                codes::LAUNCHER,
                "the launcher was not found yet",
                "run Play again",
            )
        })?;
        Ok(Command::new(path))
    }

    /// PL: Polecenie launchera z wynikiem do przeczytania; błąd dostaje kod i stderr.
    /// EN: A launcher command whose output is read; a failure gets the code and stderr.
    fn run(&self, code: Code, args: &[&str]) -> Result<String, DiagError> {
        let out = self
            .hl()?
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| {
                DiagError::new(code, "the launcher did not start", "repair the launcher")
                    .caused_by(&e)
            })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        self.info(
            "launcher",
            json!({ "args": args, "status": out.status.code() }),
        );
        if out.status.success() {
            return Ok(stdout);
        }
        Err(DiagError::new(
            code,
            format!("horizon-launcher {} failed", args.join(" ")),
            "read the launcher message in the chain",
        )
        .context("status", out.status.code())
        .chain_line(tail(&String::from_utf8_lossy(&out.stderr)))
        .chain_line(tail(&stdout)))
    }

    /// PL: Polecenie, które rozmawia z graczem (kod urządzenia, gra). EN: A command that talks to
    /// the player (device code, the game).
    fn run_shown(&self, code: Code, args: &[&str]) -> Result<(), DiagError> {
        let status = self.hl()?.args(args).status().map_err(|e| {
            DiagError::new(code, "the launcher did not start", "repair the launcher").caused_by(&e)
        })?;
        self.info("launcher", json!({ "args": args, "status": status.code() }));
        if status.success() {
            return Ok(());
        }
        Err(DiagError::new(
            code,
            format!("horizon-launcher {} failed", args.join(" ")),
            "read the launcher message above",
        )
        .context("status", status.code()))
    }

    fn json(&self, code: Code, args: &[&str]) -> Result<Value, DiagError> {
        let text = self.run(code, args)?;
        serde_json::from_str(&text).map_err(|e| {
            DiagError::new(code, "the launcher printed no JSON", "update the launcher")
                .caused_by(&e)
                .chain_line(tail(&text))
        })
    }

    fn candidates(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(path) = std::env::var_os("HOT_PLAY_LAUNCHER") {
            out.push(PathBuf::from(path));
        }
        if let Some(own) = find_binary(&self.data.join("launcher")) {
            out.push(own);
        }
        let exe = launcher_name();
        if let Some(path) = std::env::var_os("PATH") {
            out.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
        }
        out.push(PathBuf::from(
            "/Applications/Horizon.app/Contents/MacOS/horizon-launcher",
        ));
        out
    }

    /// PL: Pobiera najnowszy launcher do `<dane>/launcher` i zwraca ścieżkę programu.
    /// EN: Downloads the newest launcher into `<data>/launcher` and returns the program's path.
    ///
    /// # Errors
    /// PL: `HOT-E0901`. EN: `HOT-E0901`.
    pub fn install_launcher(&mut self) -> Result<PathBuf, DiagError> {
        let fail = |what: &str| {
            DiagError::new(
                codes::LAUNCHER,
                what.to_owned(),
                "check the network; the launcher comes from github.com/horizon-rust/horizon",
            )
        };
        let asset = launcher_asset().ok_or_else(|| {
            fail("there is no launcher build for this system").context(
                "system",
                format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            )
        })?;
        let releases: Value = serde_json::from_slice(&get(RELEASES, codes::LAUNCHER)?)
            .map_err(|e| fail("GitHub answered without JSON").caused_by(&e))?;
        let release = releases
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| {
                r["tag_name"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("launcher@"))
                    && r["draft"] != true
                    && r["prerelease"] != true
            })
            .ok_or_else(|| fail("no launcher release on GitHub"))?;
        let url = |name: &str| {
            release["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| a["name"] == name)
                .and_then(|a| a["browser_download_url"].as_str())
                .map(str::to_owned)
                .ok_or_else(|| {
                    fail("the launcher release has no file for this system").context("file", name)
                })
        };
        let sums =
            String::from_utf8_lossy(&get(&url("SHA256SUMS")?, codes::LAUNCHER)?).into_owned();
        let want = sums
            .lines()
            .find_map(|l| l.strip_suffix(asset).map(str::trim))
            .ok_or_else(|| {
                fail("SHA256SUMS does not list the launcher file").context("file", asset)
            })?
            .to_owned();
        let bytes = get(&url(asset)?, codes::LAUNCHER)?;
        let got: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != want {
            return Err(
                fail("the downloaded launcher has a different SHA-256 than SHA256SUMS")
                    .context("expected", want)
                    .context("got", got),
            );
        }
        let io = |e: std::io::Error| fail("cannot unpack the launcher").caused_by(&e);
        let next = self.data.join("launcher.next");
        let _ = fs::remove_dir_all(&next);
        fs::create_dir_all(&next).map_err(io)?;
        let archive = next.join(asset);
        fs::write(&archive, &bytes).map_err(io)?;
        let status = Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&next)
            .status()
            .map_err(io)?;
        let _ = fs::remove_file(&archive);
        if !status.success() {
            return Err(fail("tar could not unpack the launcher").context("status", status.code()));
        }
        let dest = self.data.join("launcher");
        let _ = fs::remove_dir_all(&dest);
        fs::rename(&next, &dest).map_err(io)?;
        let tag = release["tag_name"].as_str().unwrap_or_default();
        self.info("launcher installed", json!({ "release": tag }));
        find_binary(&dest)
            .ok_or_else(|| fail("the unpacked launcher has no horizon-launcher program"))
    }
}

impl World for Real {
    fn ensure_launcher(&mut self, _repair: bool) -> Result<(), DiagError> {
        for path in self.candidates() {
            if runs(&path) {
                self.info("launcher", json!({ "path": path.display().to_string() }));
                self.launcher = Some(path);
                return Ok(());
            }
        }
        self.say("Nie ma launchera Horizona: pobieram go…");
        let path = self.install_launcher()?;
        if !runs(&path) {
            return Err(DiagError::new(
                codes::LAUNCHER,
                "the installed launcher does not start",
                "send the log file to the server owner",
            )
            .context("path", path.display().to_string()));
        }
        self.launcher = Some(path);
        Ok(())
    }

    fn server_info(&mut self) -> Result<ServerInfo, DiagError> {
        let bytes = get(&format!("{}/hot-play.json", self.pack), codes::SERVER)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| {
            DiagError::new(
                codes::SERVER,
                "the server's hot-play.json is not JSON",
                "update the server",
            )
            .caused_by(&e)
        })?;
        let text = |k: &str| value[k].as_str().map(str::to_owned);
        Ok(ServerInfo {
            horizon: text("horizon"),
            slots_sha1: text("slots_sha1"),
        })
    }

    fn versions(&mut self) -> Result<Versions, DiagError> {
        let installed = self.json(codes::VERSION, &["versions", "--installed", "--json"])?;
        let installed = strings(&installed["installed"]);
        let available = match self.json(codes::VERSION, &["versions", "--json"]) {
            Ok(list) => list
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v["available_here"] != false)
                .filter_map(|v| v["tag"].as_str().map(str::to_owned))
                .collect(),
            Err(e) => {
                if let Some(log) = &self.log {
                    log.warn(
                        "no release list, only installed versions",
                        json!({ "error": e.to_string() }),
                    );
                }
                Vec::new()
            }
        };
        Ok(Versions {
            available,
            installed,
        })
    }

    fn install_version(&mut self, tag: &str) -> Result<(), DiagError> {
        self.run(codes::VERSION, &["install", tag]).map(drop)
    }

    fn signed_in(&mut self) -> Result<bool, DiagError> {
        let list = self.json(codes::ACCOUNT, &["account", "list", "--json"])?;
        Ok(list["accounts"].as_array().is_some_and(|a| !a.is_empty()))
    }

    fn sign_in(&mut self) -> Result<(), DiagError> {
        self.run_shown(codes::ACCOUNT, &["account", "sign-in"])
    }

    fn prepare(&mut self, tag: &str) -> Result<(), DiagError> {
        match self.run(codes::PREPARE, &["prepare", "--version", tag]) {
            Ok(_) => Ok(()),
            Err(first) => {
                if let Some(log) = &self.log {
                    log.error(&first);
                }
                self.say(
                    "Nie ma Minecrafta na tym komputerze: pobieram pliki gry z Twojego konta…",
                );
                self.run(codes::PREPARE, &["prepare", "--version", tag, "--download"])
                    .map(drop)
            }
        }
    }

    fn instance(&mut self, name: &str) -> Result<Option<Instance>, DiagError> {
        let list = self.json(codes::INSTANCE, &["instance", "list", "--json"])?;
        Ok(list
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| i["name"] == name)
            .and_then(|i| {
                Some(Instance {
                    id: i["id"].as_str()?.to_owned(),
                    home: PathBuf::from(i["home"].as_str()?),
                    version: i["version"].as_str().map(str::to_owned),
                })
            }))
    }

    fn create_instance(&mut self, name: &str) -> Result<Instance, DiagError> {
        self.run(codes::INSTANCE, &["instance", "create", name])?;
        self.instance(name)?.ok_or_else(|| {
            DiagError::new(
                codes::INSTANCE,
                "the new instance is not on the list",
                "run Repair",
            )
            .context("instance", name)
        })
    }

    fn pin_version(&mut self, id: &str, tag: &str) -> Result<(), DiagError> {
        self.run(codes::INSTANCE, &["instance", "version", id, tag])
            .map(drop)
    }

    fn read(&mut self, path: &Path) -> Option<Vec<u8>> {
        fs::read(path).ok()
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), DiagError> {
        let fail = |e: &std::io::Error| {
            DiagError::new(
                codes::INSTANCE,
                "cannot write a game file",
                "check free disk space and permissions",
            )
            .context("path", path.display().to_string())
            .caused_by(e)
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| fail(&e))?;
        }
        let tmp = path.with_extension("hot-play.tmp");
        fs::write(&tmp, bytes).map_err(|e| fail(&e))?;
        fs::rename(&tmp, path).map_err(|e| fail(&e))
    }

    fn download_slots(&mut self) -> Result<Vec<u8>, DiagError> {
        get(&format!("{}/hot_slots.hmod", self.pack), codes::SLOTS)
    }

    fn play(&mut self, id: &str, server: &str) -> Result<(), DiagError> {
        self.run_shown(codes::PLAY, &["play", "--instance", id, "--server", server])
    }

    fn say(&mut self, line: &str) {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

fn get(url: &str, code: Code) -> Result<Vec<u8>, DiagError> {
    let agent = ureq::AgentBuilder::new()
        .user_agent("hot-play")
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .build();
    let fail = |what: &str| {
        DiagError::new(
            code,
            what.to_owned(),
            "check the network and the server address",
        )
        .context("url", url)
    };
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| fail("the download failed").cause(e.to_string()))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_DOWNLOAD)
        .read_to_end(&mut bytes)
        .map_err(|e| fail("the download broke off").caused_by(&e))?;
    Ok(bytes)
}

fn runs(path: &Path) -> bool {
    path.is_file()
        && Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(5)..].join(" | ")
}

fn launcher_name() -> &'static str {
    if cfg!(windows) {
        "horizon-launcher.exe"
    } else {
        "horizon-launcher"
    }
}

/// PL: Plik wydania launchera dla tego systemu. EN: The launcher release file for this system.
#[must_use]
pub fn launcher_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("horizon-launcher-macos-arm64.zip"),
        ("macos", "x86_64") => Some("horizon-launcher-macos-x64.zip"),
        ("linux", "x86_64") => Some("horizon-launcher-linux-x86_64.tar.gz"),
        ("windows", "x86_64") => Some("horizon-launcher-windows-x64.zip"),
        _ => None,
    }
}

/// PL: Pierwszy program launchera w rozpakowanym folderze. EN: The first launcher program in an
/// unpacked folder.
#[must_use]
pub fn find_binary(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == launcher_name()) {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PL: Pobiera prawdziwy launcher z GitHuba (sieć, kilkadziesiąt MB); uruchamiany ręcznie:
    /// `cargo test -p hot-play -- --ignored`. EN: Downloads the real launcher from GitHub (network,
    /// tens of MB); run by hand: `cargo test -p hot-play -- --ignored`.
    #[test]
    #[ignore = "network: downloads the launcher from GitHub"]
    #[allow(clippy::unwrap_used)]
    fn the_launcher_downloads_verifies_and_runs() {
        let data = std::env::temp_dir().join(format!("hot-play-launcher-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let mut real = Real::new(data.clone(), String::new(), None);
        let path = real.install_launcher().unwrap();
        assert!(runs(&path), "{}", path.display());
        fs::remove_dir_all(&data).unwrap();
    }
}
