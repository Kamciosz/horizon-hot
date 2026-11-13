//! PL: Program `hot-play`: menu `[1] Graj  [2] Napraw  [3] Wyjdź` (D26) i te same polecenia
//! z wiersza poleceń. Logi: `<dane>/logs/hot-play.jsonl`; raport „Napraw” na pulpicie.
//! EN: The `hot-play` program: the `[1] Play  [2] Repair  [3] Quit` menu (D26) and the same
//! commands from the command line. Logs: `<data>/logs/hot-play.jsonl`; the Repair report on the
//! desktop.

#![allow(clippy::print_stdout, clippy::print_stderr)] // PL: program wiersza poleceń. EN: a CLI program.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hot_diag::report::{build_report, ReportInput};
use hot_diag::{DiagError, LogConfig, Logger, Redactor};
use hot_play::real::Real;
use hot_play::{codes, data_dir, game, load_state, player_redactor, save_state, Config};
use serde_json::json;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
Użycie: hot-play                 menu: [1] Graj  [2] Napraw  [3] Wyjdź
        hot-play graj            sprawdza, naprawia i uruchamia grę
        hot-play napraw          sprawdza wszystko od nowa i zapisuje raport
        hot-play sprawdz         sprawdza i naprawia, ale nie uruchamia gry
        hot-play ustaw ADRES [--pack URL] [--name NAZWA]
                                 adres serwera, np. serwer.pl albo serwer.pl:25565
        hot-play --version
Dane: HOT_PLAY_HOME (domyślnie folder danych aplikacji). Launcher: HOT_PLAY_LAUNCHER.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = data_dir();
    let log = open_log(&data);
    match args.first().map(String::as_str) {
        None => menu(&data, log),
        Some("graj") => done(run(&data, log, false, true)),
        Some("napraw") => done(run(&data, log, true, false)),
        Some("sprawdz") => done(run(&data, log, false, false)),
        Some("ustaw") => set(&data, &args[1..]),
        Some("--version" | "-V") => {
            println!("hot-play {VERSION}");
            ExitCode::SUCCESS
        }
        _ => {
            println!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn open_log(data: &Path) -> Option<Logger> {
    Logger::open(
        LogConfig::new("hot-play", VERSION, data.join("logs")),
        redactor(),
    )
    .ok()
}

fn redactor() -> Redactor {
    let var = |k: &str| std::env::var(k).ok();
    let home = var(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let user = var(if cfg!(windows) { "USERNAME" } else { "USER" });
    player_redactor(home.as_deref(), user.as_deref())
}

fn done(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn menu(data: &Path, log: Option<Logger>) -> ExitCode {
    let mut log = log;
    loop {
        println!("\n[1] Graj  [2] Napraw  [3] Wyjdź");
        let Some(choice) = ask("Wybierz: ") else {
            return ExitCode::SUCCESS;
        };
        match choice.as_str() {
            "1" => {
                run(data, log.take(), false, true);
            }
            "2" => {
                run(data, log.take(), true, false);
            }
            "3" | "q" => return ExitCode::SUCCESS,
            _ => println!("Wpisz 1, 2 albo 3."),
        }
        if log.is_none() {
            log = open_log(data);
        }
    }
}

fn ask(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_owned()),
    }
}

fn config(data: &Path) -> Option<Config> {
    if let Some(c) = Config::load(data) {
        return Some(c);
    }
    println!("Nie znam jeszcze adresu serwera. Dostaniesz go od właściciela serwera.");
    let address = ask("Adres serwera: ").filter(|a| !a.is_empty())?;
    let c = Config::from_address(&address);
    if let Err(e) = c.save(data) {
        println!("Nie zapisałem adresu ({e}); zapytam znowu następnym razem.");
    }
    Some(c)
}

/// PL: „Graj”, „Napraw” albo samo sprawdzenie (`play` = `false`). Zwraca `true`, gdy się udało.
/// EN: "Play", "Repair" or a check alone (`play` = `false`). Returns `true` on success.
fn run(data: &Path, log: Option<Logger>, repair: bool, play: bool) -> bool {
    let Some(cfg) = config(data) else {
        println!("Bez adresu serwera nie ruszę. Uruchom: hot-play ustaw ADRES");
        return false;
    };
    if let Some(log) = &log {
        log.info(
            match (repair, play) {
                (true, _) => "repair",
                (false, true) => "play",
                (false, false) => "check",
            },
            json!({ "server": cfg.server, "pack": cfg.pack }),
        );
    }
    let mut state = load_state(data);
    let mut world = Real::new(data.to_path_buf(), cfg.pack.clone(), log);
    let result = game::graj(&mut world, &cfg.setup(), &mut state, repair, play);
    if let Err(e) = save_state(data, &state) {
        println!("Nie zapisałem stanu ({e}); następnym razem sprawdzę wszystko od nowa.");
    }
    let ok = match &result {
        Ok(()) => {
            if !play {
                println!("Wszystko sprawdzone i naprawione. Wybierz [1] Graj.");
            }
            true
        }
        Err(e) => {
            fail(&world, e);
            false
        }
    };
    if repair {
        report(data, &cfg);
    }
    ok
}

fn fail(world: &Real, error: &DiagError) {
    if let Some(log) = &world.log {
        log.error(error);
    }
    println!(
        "\n{} ({})",
        codes::for_player(error.code()),
        error.code().as_str()
    );
}

fn report(data: &Path, cfg: &Config) {
    let input = ReportInput {
        versions: vec![
            ("hot-play".to_owned(), VERSION.to_owned()),
            (
                "system".to_owned(),
                format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            ),
        ],
        status: format!(
            "player side (hot-play), server {} pack {}",
            cfg.server, cfg.pack
        ),
        logs_dir: data.join("logs"),
        config: [("config.json", "config.json"), ("state.json", "state.json")]
            .iter()
            .filter_map(|(name, file)| {
                std::fs::read_to_string(data.join(file))
                    .ok()
                    .map(|t| ((*name).to_owned(), t))
            })
            .collect(),
    };
    let text = build_report(&input, &redactor());
    let path = report_path(data);
    match std::fs::write(&path, text) {
        Ok(()) => println!(
            "Raport: {}\nWyślij ten plik właścicielowi serwera, jeśli coś dalej nie działa.",
            path.display()
        ),
        Err(e) => println!("Nie zapisałem raportu ({e})."),
    }
}

fn report_path(data: &Path) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    home.map(|h| PathBuf::from(h).join("Desktop"))
        .filter(|d| d.is_dir() && std::env::var_os("HOT_PLAY_HOME").is_none())
        .unwrap_or_else(|| data.to_path_buf())
        .join("hot-play-raport.md")
}

fn set(data: &Path, args: &[String]) -> ExitCode {
    let Some(address) = args.first().filter(|a| !a.starts_with("--")) else {
        println!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut c = Config::from_address(address);
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--pack", Some(v)) => c.pack = v.trim_end_matches('/').to_owned(),
            ("--name", Some(v)) => c.name.clone_from(v),
            _ => {
                println!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    match c.save(data) {
        Ok(()) => {
            println!("Zapisane: gra {}, paczka {}.", c.server, c.pack);
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("Nie zapisałem ({e}).");
            ExitCode::FAILURE
        }
    }
}
