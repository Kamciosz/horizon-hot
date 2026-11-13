//! PL: Prawdziwy świat aktualizacji Horizona ([`crate::update::Env`]): tagi i `Cargo.lock` z GitHuba
//! (git), budowa (`build.rs`), test (`trial.rs`), stan silnika z `horizon-server status --json`,
//! czat przez `horizon-server cmd say`, podmiana przez gniazdo `hotd engine` (D11). Polecenia
//! git idą przez hamulec (`brake::output`), bo to praca w tle.
//! EN: The Horizon update's real world ([`crate::update::Env`]): tags and `Cargo.lock` from GitHub
//! (git), the build (`build.rs`), the trial (`trial.rs`), the engine state from
//! `horizon-server status --json`, chat through `horizon-server cmd say`, the swap through the
//! `hotd engine` socket (D11). git commands go through the brake (`brake::output`), as they are
//! background work.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use hot_diag::{DiagError, Logger};
use serde_json::Value;

use crate::build::{build_engine, engine_revision, is_revision, ENGINE_FILE};
use crate::codes;
use crate::swap::Layout;
use crate::trial::Trial;
use crate::update::{Env, Health, Upstream};

/// PL: Repozytorium Horizona. EN: The Horizon repository.
pub const HORIZON_REPO: &str = "https://github.com/horizon-rust/horizon";
/// PL: Tagi ze źródłem każdego Nightly. EN: The tags holding each Nightly's source.
const SOURCE_TAGS: &str = "refs/tags/nightly-source/";
/// PL: Przedrostek folderów silników, które buduje aktualizacja. EN: The prefix of the engine
/// folders the update builds.
const BUILT: &str = "update-";

/// PL: Ustawienia i narzędzia prawdziwego świata. EN: The real world's settings and tools.
pub struct Real {
    pub home: PathBuf,
    pub server: PathBuf,
    pub launcher: PathBuf,
    pub bundle: PathBuf,
    pub engines: PathBuf,
    pub repo: String,
    pub trust: bool,
    pub bots: PathBuf,
    pub bot_count: u32,
    pub trial_seconds: u64,
    pub log: Arc<Logger>,
}

fn check_error(what: String, cause: String) -> DiagError {
    DiagError::new(
        codes::HORIZON_CHECK_FAILED,
        what,
        "Check the network and git (git ls-remote --tags https://github.com/horizon-rust/horizon); hotd horizon retries in an hour",
    )
    .chain_line(cause)
    .state("The server stays on its Horizon revision")
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = crate::brake::output(Command::new("git").args(args))
        .map_err(|e| format!("git cannot be started: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "git {}: {} {}",
            args.first().copied().unwrap_or_default(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// PL: Najnowszy tag `nightly-source/<data>-<skrót>` z wyjścia `git ls-remote`. Nazwy zaczynają
/// się od daty, więc najnowszy jest największy. Tag z adnotacją (`^{}`) wskazuje na commit przez
/// tę linię. EN: The newest `nightly-source/<date>-<short>` tag from `git ls-remote` output. Names
/// start with the date, so the newest is the largest. An annotated tag (`^{}`) points at the commit
/// through that line.
#[must_use]
pub fn newest_tag(ls_remote: &str) -> Option<Upstream> {
    let mut tags: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for line in ls_remote.lines() {
        let Some((rev, name)) = line.split_once('\t') else {
            continue;
        };
        let Some(tag) = name.strip_prefix(SOURCE_TAGS) else {
            continue;
        };
        if !is_revision(rev) {
            continue;
        }
        match tag.strip_suffix("^{}") {
            Some(tag) => {
                tags.insert(tag.to_owned(), rev.to_owned());
            }
            None => {
                tags.entry(tag.to_owned()).or_insert_with(|| rev.to_owned());
            }
        }
    }
    tags.pop_last().map(|(tag, rev)| Upstream {
        tag: format!("nightly-source/{tag}"),
        rev,
    })
}

/// PL: Liczba graczy z `status --json` (`sample.game.players` to lista).
/// EN: The player count from `status --json` (`sample.game.players` is a list).
#[must_use]
pub fn health_from(status_json: &str) -> Health {
    let v: Value = serde_json::from_str(status_json).unwrap_or(Value::Null);
    let running = v["phase"] == "running";
    let p95 = match crate::brake::reading(status_json) {
        Some(crate::brake::Load::P95(ms)) => Some(ms),
        _ => None,
    };
    Health {
        pid: v["engine_pid"].as_u64().filter(|_| running),
        p95,
        players: v["sample"]["game"]["players"]
            .as_array()
            .map(|p| p.len() as u64)
            .filter(|_| running),
    }
}

impl Real {
    fn active(&self) -> PathBuf {
        Layout {
            home: self.home.clone(),
        }
        .active_dir()
    }

    fn server_cmd(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(&self.server)
            .arg("--dir")
            .arg(self.active())
            .args(args)
            .output()
            .map_err(|e| format!("{} cannot be started: {e}", self.server.display()))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!(
                "{}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

impl Env for Real {
    fn newest(&mut self) -> Result<Option<Upstream>, DiagError> {
        let pattern = format!("{SOURCE_TAGS}*");
        let out = git(&["ls-remote", "--tags", &self.repo, &pattern]).map_err(|e| {
            check_error(
                format!("the Nightly tags of {} cannot be read", self.repo),
                e,
            )
        })?;
        Ok(newest_tag(&out))
    }

    fn cargo_lock(&mut self, rev: &str) -> Result<String, DiagError> {
        let repo = self.home.join("hot-state").join("horizon.git");
        let repo_s = repo.display().to_string();
        let fail =
            |e: String| check_error(format!("Cargo.lock of Horizon {rev} cannot be read"), e);
        if !repo.join("HEAD").exists() {
            git(&["init", "-q", "--bare", &repo_s]).map_err(fail)?;
        }
        let _ = git(&["-C", &repo_s, "remote", "remove", "origin"]);
        git(&["-C", &repo_s, "remote", "add", "origin", &self.repo]).map_err(fail)?;
        git(&[
            "-C",
            &repo_s,
            "fetch",
            "-q",
            "--filter=blob:none",
            "--depth",
            "1",
            "origin",
            rev,
        ])
        .map_err(fail)?;
        git(&["-C", &repo_s, "show", &format!("{rev}:Cargo.lock")]).map_err(fail)
    }

    fn free_bytes(&mut self) -> Option<u64> {
        let _ = fs::create_dir_all(&self.engines);
        crate::disk::free_bytes(&self.engines)
    }

    fn current_engine(&mut self) -> Option<PathBuf> {
        let text = fs::read_to_string(self.active().join("server.toml")).ok()?;
        crate::swap::toml_value(&text, "engine", "path")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    }

    fn build(&mut self, rev: &str) -> Result<PathBuf, DiagError> {
        let out = self
            .engines
            .join(format!("{BUILT}{}", rev.get(..12).unwrap_or(rev)));
        let engine = out.join(ENGINE_FILE);
        // PL: Silnik z poprzedniej próby (test się nie odbył) służy dalej.
        // EN: The engine from an earlier attempt (the trial did not happen) is used again.
        if engine.is_file() && engine_revision(&engine).as_deref() == Some(rev) {
            return Ok(engine);
        }
        if out.exists() {
            let _ = fs::remove_dir_all(&out);
        }
        build_engine(&self.launcher, &self.bundle, rev, &out, self.trust)
    }

    fn trial(&mut self, old: &Path, new: &Path) -> Result<Value, DiagError> {
        let t = Trial {
            home: self.home.clone(),
            active: self.active(),
            server: self.server.clone(),
            bots: self.bots.clone(),
            bot_count: self.bot_count,
            seconds: self.trial_seconds,
        };
        crate::trial::run(&t, old, new)
    }

    fn health(&mut self) -> Health {
        health_from(&self.server_cmd(&["status", "--json"]).unwrap_or_default())
    }

    fn say(&mut self, text: &str) {
        if let Err(e) = self.server_cmd(&["cmd", &format!("say {text}")]) {
            self.log.warn(
                "hotd horizon cannot post in the game chat",
                serde_json::json!({ "text": text, "error": e }),
            );
        }
    }

    fn swap(&mut self, engine: &Path) -> Result<Value, DiagError> {
        let reply = crate::keeper::request(&self.home, &format!("swap {}", engine.display()))
            .map_err(|e| {
                DiagError::new(
                    codes::SWAP_NOT_STARTED,
                    "hotd engine does not answer the swap request",
                    "Check systemctl status horizon-hot-server (hotd engine); the update tries again in its next step",
                )
                .caused_by(&e)
                .context("engine", engine.display().to_string())
                .state("The players stay on the old engine")
            })?;
        let v: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
        if v["ok"] == true {
            return Ok(v["report"].clone());
        }
        let err = &v["error"];
        let code = if err["code"] == codes::SWAP_FAILED.as_str() {
            codes::SWAP_FAILED
        } else {
            codes::SWAP_NOT_STARTED
        };
        Err(DiagError::new(
            code,
            err["what"].as_str().unwrap_or("the swap failed").to_owned(),
            err["fix"]
                .as_str()
                .unwrap_or("Read logs/hotd-engine.jsonl")
                .to_owned(),
        )
        .context("reply", reply.as_str())
        .state("The players stay on the old engine"))
    }

    fn cleanup(&mut self, keep: &[PathBuf]) {
        let Ok(entries) = fs::read_dir(&self.engines) else {
            return;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let ours = entry.file_name().to_string_lossy().starts_with(BUILT)
                && dir.join("native-mods.json").is_file();
            if ours && !keep.iter().any(|k| k.starts_with(&dir)) {
                match fs::remove_dir_all(&dir) {
                    Ok(()) => self.log.info(
                        "hotd horizon removed an old engine",
                        serde_json::json!({ "dir": dir.display().to_string() }),
                    ),
                    Err(e) => self.log.warn(
                        "hotd horizon cannot remove an old engine",
                        serde_json::json!({ "dir": dir.display().to_string(), "error": e.to_string() }),
                    ),
                }
            }
        }
    }

    fn info(&mut self, what: &str, context: Value) {
        self.log.info(what, context);
    }

    fn error(&mut self, error: &DiagError) {
        self.log.error(error);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // PL: testy. EN: tests.
mod tests {
    use super::*;

    #[test]
    fn the_newest_tag_wins_and_annotated_tags_point_at_the_commit() {
        let a = "6560641".repeat(5) + "aaaaa";
        let b = "9f42064".repeat(5) + "bbbbb";
        let c = "c".repeat(40);
        let out = format!(
            "{a}\trefs/tags/nightly-source/2026-11-07-6560641\n\
             {c}\trefs/tags/nightly-source/2026-11-13-9f42064\n\
             {b}\trefs/tags/nightly-source/2026-11-13-9f42064^{{}}\n\
             {a}\trefs/tags/v0.2.97\n"
        );
        let n = newest_tag(&out).unwrap();
        assert_eq!(n.tag, "nightly-source/2026-11-13-9f42064");
        assert_eq!(n.rev, b);
        assert_eq!(newest_tag("garbage\n"), None);
    }

    #[test]
    fn health_counts_players_only_while_running() {
        let up = r#"{"phase":"running","engine_pid":42,"sample":{"game":{"paused":false,"mspt":{"p95":12.5},"players":[{"name":"a"},{"name":"b"}]}}}"#;
        let h = health_from(up);
        assert_eq!((h.pid, h.p95, h.players), (Some(42), Some(12.5), Some(2)));
        let down = health_from(r#"{"phase":"stopped"}"#);
        assert_eq!((down.pid, down.p95, down.players), (None, None, None));
        assert_eq!(health_from("").pid, None);
    }
}
