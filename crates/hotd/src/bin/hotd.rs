//! PL: Program `hotd`: `run` (usługa systemd, krok co 10 s), `once` (jeden krok, do testów
//! i ręcznej naprawy), `status` (to samo co `hot status`).
//! EN: The `hotd` program: `run` (systemd service, a step every 10 s), `once` (a single step, for
//! tests and manual repair), `status` (same as `hot status`).

#![allow(clippy::print_stdout, clippy::print_stderr)] // PL: program wiersza poleceń. EN: a CLI program.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use hotd::daemon::{Config, Daemon};
use hotd::engine::HorizonServer;

const USAGE: &str = "\
Usage: hotd run  --home DIR --repo URL [--branch main] [--engine horizon-server]
                 [--engine-dir DIR] [--allowed-signers FILE] [--interval SECONDS]
       hotd once (same options; one step, then exit)
       hotd status [--home DIR]

--home is the engine's HOT_HOME (hot-state/ and logs/ live there; default $HOT_HOME).
--engine-dir is the horizon-server --dir of the engine (default: --home).
--allowed-signers defaults to <home>/hot-keys/allowed_signers.";

struct Args {
    command: String,
    home: Option<PathBuf>,
    repo: Option<String>,
    branch: String,
    engine: PathBuf,
    engine_dir: Option<PathBuf>,
    allowed_signers: Option<PathBuf>,
    interval: u64,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let command = it.next().ok_or("a command is missing")?;
    let mut a = Args {
        command,
        home: std::env::var_os("HOT_HOME").map(PathBuf::from),
        repo: None,
        branch: "main".into(),
        engine: "horizon-server".into(),
        engine_dir: None,
        allowed_signers: None,
        interval: 10,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--home" => a.home = Some(value()?.into()),
            "--repo" => a.repo = Some(value()?),
            "--branch" => a.branch = value()?,
            "--engine" => a.engine = value()?.into(),
            "--engine-dir" => a.engine_dir = Some(value()?.into()),
            "--allowed-signers" => a.allowed_signers = Some(value()?.into()),
            "--interval" => {
                a.interval = value()?.parse().map_err(|e| format!("--interval: {e}"))?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hotd: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(home) = args.home.clone() else {
        eprintln!("hotd: --home (or HOT_HOME) is required\n\n{USAGE}");
        return ExitCode::from(2);
    };
    // PL: Ścieżki trafiają do komend silnika, więc muszą być bezwzględne.
    // EN: Paths go into engine commands, so they must be absolute.
    let home = std::path::absolute(&home).unwrap_or(home);
    if args.command == "status" {
        print!("{}", hotd::status::render(&home));
        return ExitCode::SUCCESS;
    }
    if args.command != "run" && args.command != "once" {
        eprintln!("hotd: unknown command {}\n\n{USAGE}", args.command);
        return ExitCode::from(2);
    }
    let Some(repo) = args.repo.as_deref() else {
        eprintln!("hotd: --repo is required\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let mut cfg = Config::new(home.clone(), repo, &args.branch);
    if let Some(file) = args.allowed_signers {
        cfg.allowed_signers = std::path::absolute(&file).unwrap_or(file);
    }
    let engine = HorizonServer {
        program: args.engine,
        dir: args.engine_dir.unwrap_or(home),
    };
    let mut daemon = match Daemon::open(cfg, engine) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("hotd: {}", serde_json::Value::Object(e.fields()));
            return ExitCode::FAILURE;
        }
    };
    hot_diag::install_panic_hook(daemon.logger());
    loop {
        daemon.step();
        if args.command == "once" {
            return ExitCode::SUCCESS;
        }
        std::thread::sleep(Duration::from_secs(args.interval.max(1)));
    }
}
