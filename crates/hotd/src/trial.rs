//! PL: Test nowego silnika przed podmianą (D18, M9.1). Na kopii świata z ostatniej kopii zapasowej
//! uruchamia po kolei stary i nowy silnik, z tymi samymi modami gorącymi, i puszcza na każdy
//! `horizon-bots` (domyślnie 10 botów na 5 minut). Co 5 s czyta p95 czasu ticku. Nowy silnik
//! odpada, gdy nie wstaje, gdy boty nie wchodzą, gdy `hot-core` wczytał inną liczbę modów albo
//! zgłosił błędy, albo gdy jego p95 jest gorsze o ponad 20 % i ponad 5 ms ([`verdict`]).
//! Test leży w `<home>/hot-trial`: port z puli wolnych, tylko 127.0.0.1, bez panelu, API, kopii
//! zapasowych i proxy. Procesy idą z `nice`, żeby gracze na prawdziwym serwerze mieli pierwszeństwo.
//! Błąd ustawienia testu (nie winny nowy silnik) ma kod `HOT-E0704` i aktualizacja ponawia go za
//! godzinę; winny nowy silnik daje `HOT-E0702`.
//!
//! EN: The new engine's trial before the swap (D18, M9.1). On a copy of the world from the newest
//! backup it runs the old engine and then the new one, with the same hot mods, and points
//! `horizon-bots` at each (10 bots for 5 minutes by default). Every 5 s it reads the p95 tick time.
//! The new engine fails when it does not start, when the bots do not get in, when `hot-core` loaded
//! another number of mods or reported errors, or when its p95 is over 20 % and over 5 ms worse
//! ([`verdict`]). The trial lives in `<home>/hot-trial`: a free port, 127.0.0.1 only, no panel, API,
//! backups or proxy. Processes run under `nice`, so players on the real server come first. A broken
//! trial setup (not the new engine's fault) is `HOT-E0704` and the update retries it in an hour; the
//! new engine at fault is `HOT-E0702`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hot_diag::DiagError;
use serde_json::{json, Value};

use crate::brake::{reading, Load};
use crate::codes;
use crate::update::{WORSE_MS, WORSE_RATIO};

/// PL: Folder testu w `HOT_HOME`. EN: The trial folder in `HOT_HOME`.
pub const DIR: &str = "hot-trial";
/// PL: Nazwa kopii zapasowej, którą test robi sam, gdy żadnej nie ma. EN: The backup the trial
/// makes itself when there is none.
const OWN_BACKUP: &str = "hot-trial";
/// PL: Ile czekać, aż silnik wstanie. EN: How long to wait for the engine to start.
const READY: Duration = Duration::from_secs(180);
const SAMPLE_EVERY: Duration = Duration::from_secs(5);
/// PL: Odczyty z pierwszych sekund (wejście botów, ładowanie chunków) pomijamy.
/// EN: Readings from the first seconds (bots joining, chunks loading) are skipped.
const WARM_UP: Duration = Duration::from_secs(20);
/// PL: Co jest dzielone z prawdziwym serwerem tylko do odczytu (dowiązania).
/// EN: What is shared with the real server read-only (links).
const LINKED: [&str; 4] = ["resources", "horizon-mods", "horizon-mods.txt", "hot-core"];
/// PL: Co jest kopiowane, bo silnik albo mody mogą to zmienić.
/// EN: What is copied, as the engine or the mods may change it.
const COPIED: [&str; 3] = ["eula.txt", "config", "ops.json"];

/// PL: Ustawienia testu. EN: The trial's settings.
#[derive(Debug, Clone)]
pub struct Trial {
    /// PL: `HOT_HOME` prawdziwego serwera. EN: The real server's `HOT_HOME`.
    pub home: PathBuf,
    /// PL: Katalog aktywnej strony (`server.toml`, `backups/`). EN: The active side's folder.
    pub active: PathBuf,
    pub server: PathBuf,
    pub bots: PathBuf,
    pub bot_count: u32,
    pub seconds: u64,
}

/// PL: Wynik jednego silnika. EN: One engine's result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub p95: Vec<f64>,
    pub joined: u64,
    pub mods: usize,
    pub errors: Vec<Value>,
}

impl Run {
    fn to_json(&self) -> Value {
        json!({
            "p95_mean": mean(&self.p95),
            "samples": self.p95.len(),
            "joined": self.joined,
            "mods": self.mods,
            "errors": self.errors,
        })
    }
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// PL: Czy nowy silnik przeszedł; `Err` mówi po angielsku, dlaczego nie (trafia do `what` błędu).
/// EN: Whether the new engine passed; `Err` says in English why not (it goes into the error's
/// `what`).
///
/// # Errors
/// PL: Opis porażki. EN: What failed.
pub fn verdict(old: &Run, new: &Run, mods: usize, bots: u32) -> Result<(), String> {
    if new.joined < u64::from(bots) {
        return Err(format!(
            "only {} of {bots} bots got into the new engine",
            new.joined
        ));
    }
    if new.mods != mods {
        return Err(format!(
            "hot-core on the new engine runs {} hot mods instead of {mods}",
            new.mods
        ));
    }
    if !new.errors.is_empty() {
        return Err(format!(
            "hot-core on the new engine reported {} errors, the first: {}",
            new.errors.len(),
            new.errors[0]
        ));
    }
    let (Some(before), Some(after)) = (mean(&old.p95), mean(&new.p95)) else {
        return Err("no tick time readings from the new engine".into());
    };
    if after > before * WORSE_RATIO && after - before > WORSE_MS {
        return Err(format!(
            "p95 {after:.1} ms on the new engine against {before:.1} ms on the old one"
        ));
    }
    Ok(())
}

fn setup_error(what: String, cause: String) -> DiagError {
    DiagError::new(
        codes::HORIZON_CHECK_FAILED,
        what,
        "Read the chain and logs/supervisor.log in <home>/hot-trial; hotd horizon retries in an hour",
    )
    .chain_line(cause)
    .state("The server stays on its engine; the revision is not marked bad")
}

fn engine_error(what: String, engine: &Path, dir: &Path) -> DiagError {
    DiagError::new(
        codes::HORIZON_TRIAL_FAILED,
        what,
        "Read logs/ in the trial folder; a new Nightly comes with the next check, or fix hot-host for this revision",
    )
    .context("engine", engine.display().to_string())
    .context("trial", dir.display().to_string())
    .state("The server stays on its engine; this revision is marked bad and never tried again")
}

/// PL: Uruchamia test: najpierw stary silnik, potem nowy, na tej samej kopii świata.
/// EN: Runs the trial: the old engine first, then the new one, on the same copy of the world.
///
/// # Errors
/// PL: `HOT-E0702` (winny nowy silnik), `HOT-E0704` (test się nie odbył).
/// EN: `HOT-E0702` (the new engine is at fault), `HOT-E0704` (the trial did not happen).
pub fn run(t: &Trial, old: &Path, new: &Path) -> Result<Value, DiagError> {
    let (backup, own) = newest_backup(t)?;
    let result = (|| {
        let mods = live_mods(&t.home);
        let before = engine_run(t, &backup, old).map_err(|e| {
            setup_error(
                "the old engine did not run on the copy of the world".into(),
                e,
            )
        })?;
        if before.joined < u64::from(t.bot_count) {
            return Err(setup_error(
                format!(
                    "only {} of {} bots got into the old engine",
                    before.joined, t.bot_count
                ),
                "horizon-bots cannot load the server: check --bots and the bots' log in hot-trial"
                    .into(),
            ));
        }
        let dir = t.home.join(DIR);
        let after = engine_run(t, &backup, new)
            .map_err(|e| engine_error(format!("the new engine failed: {e}"), new, &dir))?;
        let report = json!({ "old": before.to_json(), "new": after.to_json(), "mods": mods });
        verdict(&before, &after, mods, t.bot_count)
            .map_err(|what| engine_error(what, new, &dir).context("report", report.to_string()))?;
        let _ = fs::remove_dir_all(&dir);
        Ok(report)
    })();
    if own {
        let _ = fs::remove_file(t.active.join("backups").join(&backup));
    }
    result
}

/// PL: Najnowsza kopia zapasowa; gdy nie ma żadnej, test robi własną (i potem ją usuwa).
/// EN: The newest backup; when there is none, the trial makes its own (and removes it later).
fn newest_backup(t: &Trial) -> Result<(String, bool), DiagError> {
    let list = server(t, &t.active, &["backups", "--json"])
        .map_err(|e| setup_error("the backups cannot be listed".into(), e))?;
    let newest = serde_json::from_str::<Value>(&list).ok().and_then(|v| {
        v.as_array()?
            .iter()
            .filter(|b| b["name"].as_str().is_some())
            .max_by_key(|b| b["modified"].as_u64().unwrap_or(0))
            .and_then(|b| b["name"].as_str().map(str::to_owned))
    });
    if let Some(name) = newest {
        return Ok((name, false));
    }
    let out = server(t, &t.active, &["backup", OWN_BACKUP])
        .map_err(|e| setup_error("a backup for the trial cannot be made".into(), e))?;
    let name = out
        .split("backups/")
        .nth(1)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            setup_error(
                "horizon-server backup did not name the file".into(),
                out.clone(),
            )
        })?;
    Ok((name, true))
}

fn live_mods(home: &Path) -> usize {
    fs::read_to_string(home.join("hot-state").join("mods.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["mods"].as_array().map(Vec::len))
        .unwrap_or(0)
}

fn server(t: &Trial, dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(&t.server)
        .arg("--dir")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{} cannot be started: {e}", t.server.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "horizon-server {}: {} {} {}",
            args.join(" "),
            out.status,
            stdout.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// PL: Świeży folder testu z kopią świata i ustawieniami tylko dla testu.
/// EN: A fresh trial folder with a copy of the world and trial-only settings.
fn prepare(t: &Trial, backup: &str, engine: &Path, port: u16) -> Result<PathBuf, String> {
    let dir = t.home.join(DIR);
    let at = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| at(&dir, e))?;
    }
    let backups = dir.join("backups");
    fs::create_dir_all(&backups).map_err(|e| at(&backups, e))?;
    fs::create_dir_all(dir.join("hot-state")).map_err(|e| at(&dir, e))?;
    fs::copy(t.active.join("server.toml"), dir.join("server.toml"))
        .map_err(|e| at(&t.active.join("server.toml"), e))?;
    let (from, to) = (t.active.join("backups").join(backup), backups.join(backup));
    fs::hard_link(&from, &to)
        .or_else(|_| fs::copy(&from, &to).map(drop))
        .map_err(|e| at(&from, e))?;
    for name in LINKED {
        let target = t.active.join(name);
        if target.exists() {
            let target = fs::canonicalize(&target).unwrap_or(target);
            std::os::unix::fs::symlink(&target, dir.join(name)).map_err(|e| at(&target, e))?;
        }
    }
    for name in COPIED {
        let from = t.active.join(name);
        if from.exists() {
            copy_tree(&from, &dir.join(name)).map_err(|e| at(&from, e))?;
        }
    }
    // PL: Mody z tej samej generacji co na serwerze; czyste zatrzymanie, żeby `hot-core` nie
    // liczył startu jako awarii. EN: The mods from the server's generation; a clean stop, so
    // `hot-core` does not count the start as a crash.
    if let Ok(text) = fs::read_to_string(t.home.join("hot-state").join("mods.json")) {
        let mut v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if v.is_object() {
            v["clean_stop"] = true.into();
            v["crash_starts"] = 0.into();
            fs::write(dir.join("hot-state").join("mods.json"), v.to_string())
                .map_err(|e| at(&dir, e))?;
        }
    }
    server(t, &dir, &["restore", backup, "--yes"])?;
    let engine_path = format!("engine.path={}", engine.display());
    let port = format!("server.port={port}");
    server(
        t,
        &dir,
        &[
            "config",
            "set",
            &port,
            "server.bind=127.0.0.1",
            "server.online_mode=false",
            "transfer.enabled=false",
            "panel.bind=",
            "api.bind=",
            "backups.every=0",
            &engine_path,
        ],
    )?;
    Ok(dir)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(drop)
    }
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("no free port on 127.0.0.1: {e}"))
}

/// PL: Zatrzymuje silnik testu także przy błędzie. EN: Stops the trial engine on errors too.
struct Running<'a> {
    t: &'a Trial,
    dir: PathBuf,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let _ = server(self.t, &self.dir, &["stop"]);
    }
}

fn niced(program: &Path) -> Command {
    let mut c = Command::new("nice");
    c.arg("-n").arg("10").arg(program);
    c
}

/// PL: Jeden silnik na kopii świata pod botami. `Err` opisuje, co nie wyszło.
/// EN: One engine on the copy of the world under the bots. `Err` says what went wrong.
fn engine_run(t: &Trial, backup: &str, engine: &Path) -> Result<Run, String> {
    let port = free_port()?;
    let dir = prepare(t, backup, engine, port)?;
    let started = niced(&t.server)
        .arg("--dir")
        .arg(&dir)
        .arg("start")
        .env("HOT_HOME", &dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{} cannot be started: {e}", t.server.display()))?;
    let running = Running {
        t,
        dir: dir.clone(),
    };
    if !started.status.success() {
        return Err(format!(
            "horizon-server start: {} {}",
            started.status,
            String::from_utf8_lossy(&started.stderr).trim()
        ));
    }
    let since = Instant::now();
    loop {
        let status = server(t, &dir, &["status", "--json"]).unwrap_or_default();
        if serde_json::from_str::<Value>(&status).is_ok_and(|v| v["phase"] == "running") {
            break;
        }
        if since.elapsed() > READY {
            return Err(format!(
                "the engine did not reach the running phase within {} s",
                READY.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let report = dir.join("bots.json");
    let mut bots: Child = niced(&t.bots)
        .arg("--server")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--bots")
        .arg(t.bot_count.to_string())
        .arg("--seconds")
        .arg(t.seconds.to_string())
        .arg("--report")
        .arg(&report)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{} cannot be started: {e}", t.bots.display()))?;
    let since = Instant::now();
    let mut run = Run::default();
    let limit = Duration::from_secs(t.seconds + 120);
    while bots.try_wait().map_err(|e| e.to_string())?.is_none() {
        if since.elapsed() > limit {
            let _ = bots.kill();
            let _ = bots.wait();
            return Err("horizon-bots did not finish".into());
        }
        std::thread::sleep(SAMPLE_EVERY);
        if since.elapsed() < WARM_UP {
            continue;
        }
        let status = server(t, &dir, &["status", "--json"]).unwrap_or_default();
        if let Some(Load::P95(ms)) = reading(&status) {
            run.p95.push(ms);
        }
    }
    let read = |p: PathBuf| {
        fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null)
    };
    run.joined = read(report)["joined"].as_u64().unwrap_or(0);
    let core = read(dir.join("hot-state").join("status.json"));
    run.mods = core["mods"].as_array().map_or(0, Vec::len);
    run.errors = core["recent_errors"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    drop(running);
    Ok(run)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // PL: testy. EN: tests.
mod tests {
    use super::*;

    fn run(p95: &[f64]) -> Run {
        Run {
            p95: p95.to_vec(),
            joined: 10,
            mods: 2,
            errors: Vec::new(),
        }
    }

    #[test]
    fn the_verdict_needs_bots_mods_no_errors_and_no_slowdown() {
        let old = run(&[10.0, 12.0]);
        assert!(verdict(&old, &run(&[11.0, 13.0]), 2, 10).is_ok());
        assert!(
            verdict(&old, &run(&[14.0, 15.0]), 2, 10).is_ok(),
            "+3.5 ms is noise"
        );
        let slow = verdict(&old, &run(&[30.0, 32.0]), 2, 10).unwrap_err();
        assert!(slow.contains("p95 31.0 ms"), "{slow}");
        let mut few = run(&[10.0]);
        few.joined = 3;
        assert!(verdict(&old, &few, 2, 10).unwrap_err().contains("3 of 10"));
        assert!(verdict(&old, &run(&[10.0]), 1, 10)
            .unwrap_err()
            .contains("2 hot mods"));
        let mut broken = run(&[10.0]);
        broken.errors.push(json!({"code": "HOT-E0402"}));
        assert!(verdict(&old, &broken, 2, 10)
            .unwrap_err()
            .contains("HOT-E0402"));
        assert!(
            verdict(&old, &run(&[]), 2, 10).is_err(),
            "no readings is no pass"
        );
    }
}
