//! PL: Program `hotd`: `run` (usługa systemd, krok co 10 s), `once` (jeden krok, do testów
//! i ręcznej naprawy), `status` (to samo co `hot status`), `serve-pack` (paczka zasobów dla
//! graczy, osobna usługa, D9), `slots-source` (źródło `hot-slots.hmod`, M5.1), `engine` (proces
//! usługi silnika, podmiana D11) i `swap` (zleca podmianę).
//! EN: The `hotd` program: `run` (systemd service, a step every 10 s), `once` (a single step, for
//! tests and manual repair), `status` (same as `hot status`), `serve-pack` (the resource pack for
//! players, a separate service, D9), `slots-source` (the `hot-slots.hmod` source, M5.1), `engine`
//! (the engine service process, the D11 swap) and `swap` (requests a swap).

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
       hotd engine [--home DIR] [--engine horizon-server] [--reclaim-mb 256]
       hotd swap [--to FILE] [--home DIR]
       hotd build-engine --launcher FILE --bundle DIR --rev HASH --out DIR [--trust-native-code]
       hotd horizon --home DIR --launcher FILE --bundle DIR --engines DIR [--engine horizon-server]
                    [--bots horizon-bots] [--trial-bots 10] [--trial-seconds 300]
                    [--horizon-repo URL] [--trust-native-code] [--once]
       hotd upgrade --home DIR --root DIR --src DIR --bin DIR --source URL --ref NAME
                    [--engine horizon-server] [--user horizon] [--unit NAME]... [--every 86400]

--home is the engine's HOT_HOME (hot-state/ and logs/ live there; default $HOT_HOME).
--engine-dir is the horizon-server --dir of the engine (default: --home); mod data packs go
to the world folder named in its server.toml.
--allowed-signers defaults to <home>/hot-keys/allowed_signers.
engine runs the engine of the active side (the main process of the engine service, instead of
horizon-server run) and swaps it on request (D11); logs go to <home>/logs/hotd-engine.jsonl.
It also swaps to the same engine by itself when old mod generations hold more than
--reclaim-mb MiB of library files (D4; 0 turns it off).
swap asks it to swap to the engine file --to (default: the same engine) with the players in
game; prints a JSON report, exit 1 when the players stayed on or went back to the old engine.
build-engine builds the engine with hot-host for Horizon commit --rev into --out (the launcher's
native-build; the bundle's pin chooses the commit, any installed release or Nightly gives the game
files); prints the engine file. Native code runs only with --trust-native-code (the owner's word).
horizon is the Horizon update (D18): once an hour it checks the newest Nightly (libloading gate),
builds it into --engines, tries it on a copy of the world with horizon-bots, swaps the engine within
24 h (sooner when nobody plays), watches it for 10 minutes and goes back when it is worse. State:
<home>/hot-state/horizon.json; logs: <home>/logs/hotd-horizon.jsonl. Without --trust-native-code it
stops before the build and says so in hot status.
upgrade is the horizon-hot self-update (D19, ADR 0014), one step per run (a systemd timer runs it
every 10 minutes as root): at most once per --every seconds it fetches the head of branch --ref
from --source into the clone --src and builds it; a change of hot-core alone is loaded into the
running engine at once, other changes wait for a moment without players, then the programs in
--bin change and the --unit services restart in the given order (the previous files come back
when one does not come up). git, cargo and the new programs run as --user. State:
<home>/hot-state/upgrade.json; logs: <home>/logs/hotd-upgrade.jsonl.";

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
    to: Option<PathBuf>,
    reclaim_mb: u64,
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
        to: None,
        reclaim_mb: hotd::reclaim::DEFAULT_MB,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--home" => a.home = Some(value()?.into()),
            "--repo" => a.repo = Some(value()?),
            "--branch" => a.branch = value()?,
            "--engine" => a.engine = value()?.into(),
            "--engine-dir" => a.engine_dir = Some(value()?.into()),
            "--to" => a.to = Some(value()?.into()),
            "--allowed-signers" => a.allowed_signers = Some(value()?.into()),
            "--reclaim-mb" => {
                a.reclaim_mb = value()?.parse().map_err(|e| format!("--reclaim-mb: {e}"))?;
            }
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
    if argv.peek().map(String::as_str) == Some("build-engine") {
        return build_engine(argv.skip(1));
    }
    if argv.peek().map(String::as_str) == Some("horizon") {
        return horizon(argv.skip(1));
    }
    if argv.peek().map(String::as_str) == Some("upgrade") {
        return upgrade(argv.skip(1));
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
    if args.command == "engine" {
        return engine(&home, &args.engine, args.reclaim_mb);
    }
    if args.command == "swap" {
        let to = args.to.map(|f| std::path::absolute(&f).unwrap_or(f));
        let command = match &to {
            Some(f) => format!("swap {}", f.display()),
            None => "swap".to_owned(),
        };
        return match hotd::keeper::request(&home, &command) {
            Ok(reply) => {
                println!("{reply}");
                let ok = serde_json::from_str::<serde_json::Value>(&reply)
                    .is_ok_and(|v| v["ok"] == true);
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!(
                    "hotd: hotd engine does not answer on {}: {e} (is the engine service running?)",
                    hotd::keeper::socket_path(&home).display()
                );
                ExitCode::FAILURE
            }
        };
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

/// PL: `hotd build-engine` dla instalatora i obrazu (`build.rs`). EN: `hotd build-engine` for the
/// installer and the image (`build.rs`).
fn build_engine(mut it: impl Iterator<Item = String>) -> ExitCode {
    let (mut launcher, mut bundle, mut rev, mut out, mut trust) = (None, None, None, None, false);
    while let Some(flag) = it.next() {
        let target = match flag.as_str() {
            "--trust-native-code" => {
                trust = true;
                continue;
            }
            "--launcher" => &mut launcher,
            "--bundle" => &mut bundle,
            "--rev" => &mut rev,
            "--out" => &mut out,
            other => {
                eprintln!("hotd: unknown option {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        };
        *target = it.next();
    }
    let (Some(launcher), Some(bundle), Some(rev), Some(out)) = (launcher, bundle, rev, out) else {
        eprintln!("hotd: build-engine needs --launcher, --bundle, --rev and --out\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let out = PathBuf::from(out);
    let out = std::path::absolute(&out).unwrap_or(out);
    match hotd::build::build_engine(launcher.as_ref(), bundle.as_ref(), &rev, &out, trust) {
        Ok(engine) => {
            println!("{}", engine.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hotd: {}", serde_json::Value::Object(e.fields()));
            ExitCode::FAILURE
        }
    }
}

/// PL: `hotd horizon`: aktualizacja Horizona (`update.rs`), krok co 60 s, hamulec MSPT we własnym
/// wątku jak w `hotd run` (budowa i git to praca w tle). EN: `hotd horizon`: the Horizon update
/// (`update.rs`), a step every 60 s, the MSPT brake on its own thread as in `hotd run` (the build and
/// git are background work).
fn horizon(mut it: impl Iterator<Item = String>) -> ExitCode {
    let mut home = std::env::var_os("HOT_HOME").map(PathBuf::from);
    let (mut launcher, mut bundle, mut engines) = (None, None, None);
    let mut server = PathBuf::from("horizon-server");
    let mut bots = PathBuf::from("horizon-bots");
    let mut repo = hotd::update_env::HORIZON_REPO.to_owned();
    let (mut trust, mut once) = (false, false);
    let (mut bot_count, mut trial_seconds) = (10_u32, 300_u64);
    while let Some(flag) = it.next() {
        let value = match flag.as_str() {
            "--trust-native-code" => {
                trust = true;
                continue;
            }
            "--once" => {
                once = true;
                continue;
            }
            _ => it.next(),
        };
        let Some(value) = value else {
            eprintln!("hotd: {flag} needs a value\n\n{USAGE}");
            return ExitCode::from(2);
        };
        match flag.as_str() {
            "--home" => home = Some(value.into()),
            "--launcher" => launcher = Some(PathBuf::from(value)),
            "--bundle" => bundle = Some(PathBuf::from(value)),
            "--engines" => engines = Some(PathBuf::from(value)),
            "--engine" => server = value.into(),
            "--bots" => bots = value.into(),
            "--horizon-repo" => repo = value,
            "--trial-bots" | "--trial-seconds" => {
                let parsed = value.parse::<u32>();
                let Ok(n) = parsed else {
                    eprintln!("hotd: {flag}: not a number\n\n{USAGE}");
                    return ExitCode::from(2);
                };
                if flag == "--trial-bots" {
                    bot_count = n;
                } else {
                    trial_seconds = u64::from(n);
                }
            }
            other => {
                eprintln!("hotd: unknown option {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(home), Some(launcher), Some(bundle), Some(engines)) =
        (home, launcher, bundle, engines)
    else {
        eprintln!("hotd: horizon needs --home, --launcher, --bundle and --engines\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let abs = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
    let home = abs(home);
    let log = match hot_diag::Logger::open(
        hot_diag::LogConfig::new("hotd-horizon", hotd::VERSION, home.join("logs")),
        hot_diag::Redactor::new(),
    ) {
        Ok(l) => std::sync::Arc::new(l),
        Err(e) => {
            eprintln!("hotd: {}", serde_json::Value::Object(e.fields()));
            return ExitCode::FAILURE;
        }
    };
    hot_diag::install_panic_hook(std::sync::Arc::clone(&log));
    let cfg = hotd::update::Config {
        trust,
        ..hotd::update::Config::default()
    };
    let mut env = hotd::update_env::Real {
        home: home.clone(),
        server: server.clone(),
        launcher: abs(launcher),
        bundle: abs(bundle),
        engines: abs(engines),
        repo,
        trust,
        bots,
        bot_count,
        trial_seconds,
        log: std::sync::Arc::clone(&log),
    };
    if !once {
        let probe = HorizonServer {
            program: server,
            dir: home.clone(),
        };
        let brake_log = std::sync::Arc::clone(&log);
        std::thread::spawn(move || hotd::brake::run(|| probe.status_json(), &brake_log));
    }
    let mut update = hotd::update::Update::load(&home);
    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        update.step(&mut env, &cfg, now);
        if let Err(e) = update.save(&home) {
            log.error(&e);
        }
        if once {
            return ExitCode::SUCCESS;
        }
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// PL: Usługa paczki zasobów; błąd portu idzie do `logs/hotd-pack.jsonl` z kodem `HOT-E0210`.
/// EN: The resource pack service; a port error goes to `logs/hotd-pack.jsonl` as `HOT-E0210`.
/// PL: `hotd upgrade`: jeden krok samoaktualizacji (ADR 0014). EN: `hotd upgrade`: one step of the
/// self-update (ADR 0014).
fn upgrade(mut it: impl Iterator<Item = String>) -> ExitCode {
    let mut home = std::env::var_os("HOT_HOME").map(PathBuf::from);
    let (mut root, mut src, mut bin, mut source, mut git_ref) = (None, None, None, None, None);
    let mut server = PathBuf::from("horizon-server");
    let (mut user, mut units, mut every) = (None, Vec::new(), hotd::upgrade::EVERY);
    while let Some(flag) = it.next() {
        let Some(value) = it.next() else {
            eprintln!("hotd: {flag} needs a value\n\n{USAGE}");
            return ExitCode::from(2);
        };
        match flag.as_str() {
            "--home" => home = Some(value.into()),
            "--root" => root = Some(PathBuf::from(value)),
            "--src" => src = Some(PathBuf::from(value)),
            "--bin" => bin = Some(PathBuf::from(value)),
            "--source" => source = Some(value),
            "--ref" => git_ref = Some(value),
            "--engine" => server = value.into(),
            "--user" => user = Some(value),
            "--unit" => units.push(value),
            "--every" => {
                let Ok(n) = value.parse() else {
                    eprintln!("hotd: --every: not a number\n\n{USAGE}");
                    return ExitCode::from(2);
                };
                every = n;
            }
            other => {
                eprintln!("hotd: unknown option {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(home), Some(root), Some(src), Some(bin), Some(source), Some(git_ref)) =
        (home, root, src, bin, source, git_ref)
    else {
        eprintln!(
            "hotd: upgrade needs --home, --root, --src, --bin, --source and --ref\n\n{USAGE}"
        );
        return ExitCode::from(2);
    };
    let abs = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
    let home = abs(home);
    let owner = match user.as_deref().map(user_ids).transpose() {
        Ok(o) => o,
        Err(why) => {
            eprintln!("hotd: --user: {why}");
            return ExitCode::from(2);
        }
    };
    // PL: Usługa działa jako root, a te pliki czyta i pisze też `horizon` (`hot status`, `hot-diag`).
    // EN: The service runs as root, and `horizon` reads and writes these files too (`hot status`,
    // `hot-diag`).
    let give = |path: &std::path::Path| {
        if let Some((uid, gid)) = owner {
            let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
        }
    };
    let log = match hot_diag::Logger::open(
        hot_diag::LogConfig::new("hotd-upgrade", hotd::VERSION, home.join("logs")),
        hot_diag::Redactor::new(),
    ) {
        Ok(l) => std::sync::Arc::new(l),
        Err(e) => {
            eprintln!("hotd: {}", serde_json::Value::Object(e.fields()));
            return ExitCode::FAILURE;
        }
    };
    give(&home.join("logs").join("hotd-upgrade.jsonl"));
    hot_diag::install_panic_hook(std::sync::Arc::clone(&log));
    let mut env = hotd::upgrade_env::Real {
        home: home.clone(),
        root: abs(root),
        src: abs(src),
        bin: abs(bin),
        source,
        git_ref,
        server,
        user: owner,
        units,
        log: std::sync::Arc::clone(&log),
    };
    let probe = env.status_probe();
    let brake_log = std::sync::Arc::clone(&log);
    std::thread::spawn(move || hotd::brake::run(probe, &brake_log));
    let mut state = hotd::upgrade::Upgrade::load(&home);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    state.step(&mut env, now, every);
    if let Err(e) = state.save(&home) {
        log.error(&e);
        return ExitCode::FAILURE;
    }
    give(&home.join("hot-state").join("upgrade.json"));
    give(&home.join("logs").join("hotd-upgrade.jsonl"));
    println!("{}", state.to_json());
    ExitCode::SUCCESS
}

/// PL: uid i gid użytkownika z `id`, jak w instalatorze. EN: The user's uid and gid from `id`, as
/// in the installer.
fn user_ids(user: &str) -> Result<(u32, u32), String> {
    let id = |flag: &str| -> Result<u32, String> {
        let out = std::process::Command::new("id")
            .args([flag, user])
            .output()
            .map_err(|e| format!("id {flag} {user}: {e}"))?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .map_err(|_| format!("no user {user}"))
    };
    Ok((id("-u")?, id("-g")?))
}

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

/// PL: `hotd engine`: kod 0 po SIGTERM albo zatrzymaniu silnika, 1 po błędzie albo gdy supervisor
/// przestał wznawiać silnik (systemd wtedy go uruchamia ponownie).
/// EN: `hotd engine`: exit 0 after SIGTERM or an engine stop, 1 after an error or when the
/// supervisor stopped restarting the engine (systemd then starts it again).
fn engine(home: &std::path::Path, program: &std::path::Path, reclaim_mb: u64) -> ExitCode {
    let log = match hot_diag::Logger::open(
        hot_diag::LogConfig::new("hotd-engine", hotd::VERSION, home.join("logs")),
        hot_diag::Redactor::new(),
    ) {
        Ok(l) => std::sync::Arc::new(l),
        Err(e) => {
            eprintln!("hotd: {e}");
            return ExitCode::FAILURE;
        }
    };
    hot_diag::install_panic_hook(std::sync::Arc::clone(&log));
    let reclaim = hotd::reclaim::Reclaim::new(reclaim_mb);
    match hotd::keeper::run(home, program, reclaim, &log) {
        Ok(hotd::keeper::End::Stopped) => ExitCode::SUCCESS,
        Ok(hotd::keeper::End::GaveUp) => ExitCode::FAILURE,
        Err(e) => {
            log.error(&e);
            eprintln!("hotd: {e}");
            ExitCode::FAILURE
        }
    }
}
