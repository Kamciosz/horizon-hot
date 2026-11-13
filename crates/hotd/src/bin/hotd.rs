//! PL: Program `hotd`: `run` (usługa systemd, krok co 10 s), `once` (jeden krok, do testów
//! i ręcznej naprawy), `status` (to samo co `hot status`), `serve-pack` (paczka zasobów dla
//! graczy, osobna usługa, D9) i `slots-source` (źródło `hot-slots.hmod`, M5.1).
//! EN: The `hotd` program: `run` (systemd service, a step every 10 s), `once` (a single step, for
//! tests and manual repair), `status` (same as `hot status`), `serve-pack` (the resource pack for
//! players, a separate service, D9) and `slots-source` (the `hot-slots.hmod` source, M5.1).

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
       hotd serve-pack [--home DIR] [--port 8131]
       hotd slots-source DIR   (then: cd DIR && cargo horizon-mod build)

--home is the engine's HOT_HOME (hot-state/ and logs/ live there; default $HOT_HOME).
--engine-dir is the horizon-server --dir of the engine (default: --home); mod data packs go
to the world folder named in its server.toml.
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
    port: u16,
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
        port: 8131,
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
            "--port" => a.port = value()?.parse().map_err(|e| format!("--port: {e}"))?,
            "--interval" => {
                a.interval = value()?.parse().map_err(|e| format!("--interval: {e}"))?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1).peekable();
    if argv.peek().map(String::as_str) == Some("slots-source") {
        let Some(dir) = argv.nth(1) else {
            eprintln!("hotd: slots-source needs a folder\n\n{USAGE}");
            return ExitCode::from(2);
        };
        return match hotd::slots::write_pool_source(std::path::Path::new(&dir)) {
            Ok(()) => {
                println!(
                    "hot_slots {} ({} blocks, {} items) in {dir}",
                    hotd::slots::version(),
                    hotd::slots::BLOCKS,
                    hotd::slots::ITEMS
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("hotd: cannot write {dir}: {e} (the folder must not exist)");
                ExitCode::FAILURE
            }
        };
    }
    let args = match parse(argv) {
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
    if args.command == "serve-pack" {
        return serve_pack(&home, args.port);
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
    let engine_dir = args.engine_dir.unwrap_or(home);
    let engine_dir = std::path::absolute(&engine_dir).unwrap_or(engine_dir);
    cfg.world = hotd::data::world_dir(&engine_dir);
    cfg.engine_dir.clone_from(&engine_dir);
    cfg.engine_program.clone_from(&args.engine);
    let program = args.engine;
    let probe_dir = engine_dir.clone();
    let engine = HorizonServer {
        program: program.clone(),
        dir: engine_dir,
    };
    let mut daemon = match Daemon::open(cfg, engine) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("hotd: {}", serde_json::Value::Object(e.fields()));
            return ExitCode::FAILURE;
        }
    };
    hot_diag::install_panic_hook(daemon.logger());
    if args.command == "run" {
        // PL: Hamulec MSPT we własnym wątku (D16): główny wątek czeka na procesy, które on
        // zatrzymuje. EN: The MSPT brake on its own thread (D16): the main thread waits on the
        // processes it stops.
        let probe = HorizonServer {
            program: program.clone(),
            dir: probe_dir,
        };
        let log = daemon.logger();
        std::thread::spawn(move || hotd::brake::run(|| probe.status_json(), &log));
    }
    loop {
        daemon.step();
        if args.command == "once" {
            return ExitCode::SUCCESS;
        }
        std::thread::sleep(Duration::from_secs(args.interval.max(1)));
    }
}

/// PL: Usługa paczki zasobów; błąd portu idzie do `logs/hotd-pack.jsonl` z kodem `HOT-E0210`.
/// EN: The resource pack service; a port error goes to `logs/hotd-pack.jsonl` as `HOT-E0210`.
fn serve_pack(home: &std::path::Path, port: u16) -> ExitCode {
    let log = match hot_diag::Logger::open(
        hot_diag::LogConfig::new("hotd-pack", hotd::VERSION, home.join("logs")),
        hot_diag::Redactor::new(),
    ) {
        Ok(l) => std::sync::Arc::new(l),
        Err(e) => {
            eprintln!("hotd: {}", serde_json::Value::Object(e.fields()));
            return ExitCode::FAILURE;
        }
    };
    hot_diag::install_panic_hook(std::sync::Arc::clone(&log));
    let listener = match std::net::TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            let err = hot_diag::DiagError::new(
                hotd::codes::PACK_FAILED,
                format!("hotd serve-pack cannot listen on port {port}"),
                "Stop what holds the port (ss -ltnp | grep PORT) or start with --port N and set the same port in resource-pack in server.toml (an engine restart applies it)",
            )
            .caused_by(&e)
            .context("port", port)
            .state("Players get no resource pack: slot blocks look like missing textures");
            log.error(&err);
            eprintln!("hotd: {}", serde_json::Value::Object(err.fields()));
            return ExitCode::FAILURE;
        }
    };
    log.info(
        "hotd serve-pack is listening",
        serde_json::json!({ "port": port, "file": hotd::pack::dir(home).join("pack.zip") }),
    );
    hotd::pack::serve(&listener, home, &log);
    ExitCode::SUCCESS
}
