//! PL: Program `hot-proxy`: słucha graczy, loguje ich online i przekazuje do silnika (M8).
//! EN: The `hot-proxy` program: listens for players, logs them in online and passes them to the
//! engine (M8).

#![allow(clippy::print_stdout, clippy::print_stderr)] // PL: program wiersza poleceń. EN: a CLI program.

use hot_proxy::auth::DEFAULT_DISCOVERY;
use hot_proxy::config::{read_secret, Config};
use hot_proxy::session::Proxy;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "\
Usage: hot-proxy --engine 127.0.0.1:PORT --secret FILE --home DIR
                 [--listen 0.0.0.0:25565] [--discovery URL] [--offline]
       hot-proxy ping HOST:PORT

--engine    the engine (online_mode = false, the same forwarding.secret).
--secret    the engine's forwarding.secret.
--home      HOT_HOME: logs go to <home>/logs/hot-proxy.jsonl (default $HOT_HOME).
--discovery the session service discovery document (tests only; default: the game's).
--offline   no purchase check, for a local test; refused unless --listen is 127.0.0.1.

ping        prints the status JSON of HOST:PORT, as the game's server list asks for it
            (exit 1 when there is no answer within 10 s).";

struct Args {
    listen: SocketAddr,
    engine: Option<SocketAddr>,
    secret: Option<PathBuf>,
    home: Option<PathBuf>,
    discovery: String,
    offline: bool,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        listen: SocketAddr::from(([0, 0, 0, 0], 25565)),
        engine: None,
        secret: None,
        home: std::env::var_os("HOT_HOME").map(PathBuf::from),
        discovery: DEFAULT_DISCOVERY.into(),
        offline: false,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        let address = |v: String| {
            v.parse::<SocketAddr>()
                .map_err(|_| format!("{flag}: {v} is not an address like 127.0.0.1:25566"))
        };
        match flag.as_str() {
            "--listen" => a.listen = address(value()?)?,
            "--engine" => a.engine = Some(address(value()?)?),
            "--secret" => a.secret = Some(value()?.into()),
            "--home" => a.home = Some(value()?.into()),
            "--discovery" => a.discovery = value()?,
            "--offline" => a.offline = true,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(a)
}

/// PL: `hot-proxy ping`: status serwera na wyjściu, kod 1 bez odpowiedzi.
/// EN: `hot-proxy ping`: the server status on stdout, exit 1 without an answer.
fn ping(address: Option<String>) -> ExitCode {
    let Some(Ok(address)) = address.map(|a| a.parse::<SocketAddr>()) else {
        eprintln!("hot-proxy: ping needs an address like 127.0.0.1:25565\n\n{USAGE}");
        return ExitCode::from(2);
    };
    match hot_proxy::ping::status(address, std::time::Duration::from_secs(10)) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hot-proxy: ping {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1).peekable();
    if argv.peek().map(String::as_str) == Some("ping") {
        return ping(argv.nth(1));
    }
    let args = match parse(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hot-proxy: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (Some(engine), Some(secret), Some(home)) = (args.engine, args.secret, args.home) else {
        eprintln!("hot-proxy: --engine, --secret and --home are required\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let log = match hot_diag::Logger::open(
        hot_diag::LogConfig::new("hot-proxy", hot_proxy::VERSION, home.join("logs")),
        hot_diag::Redactor::new(),
    ) {
        Ok(log) => Arc::new(log),
        Err(e) => {
            eprintln!("hot-proxy: {e}");
            return ExitCode::FAILURE;
        }
    };
    hot_diag::install_panic_hook(Arc::clone(&log));
    let started = read_secret(&secret).and_then(|secret| {
        Proxy::new(
            Config {
                listen: args.listen,
                engine,
                secret,
                online: !args.offline,
                discovery: args.discovery,
            },
            Arc::clone(&log),
        )
    });
    let proxy = match started {
        Ok(p) => Arc::new(p),
        Err(e) => {
            log.error(&e);
            eprintln!("hot-proxy: {e}");
            return ExitCode::FAILURE;
        }
    };
    let listener = match TcpListener::bind(args.listen) {
        Ok(l) => l,
        Err(e) => {
            let e = hot_diag::DiagError::new(
                hot_proxy::codes::CONFIG,
                format!("hot-proxy cannot listen on {}: {e}", args.listen),
                "Stop whatever uses this port (ss -ltnp), or give the engine another port: the proxy takes the players' port",
            )
            .context("listen", args.listen.to_string())
            .state("The proxy did not start");
            log.error(&e);
            eprintln!("hot-proxy: {e}");
            return ExitCode::FAILURE;
        }
    };
    log.info(
        "hot-proxy is listening",
        serde_json::json!({
            "listen": args.listen.to_string(),
            "engine": engine.to_string(),
            "online": !args.offline,
        }),
    );
    proxy.serve(&listener);
    ExitCode::SUCCESS
}
