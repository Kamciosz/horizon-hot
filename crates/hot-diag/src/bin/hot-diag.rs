//! PL: Program `hot-diag`. Dziś jedna komenda: `report`, czyli szkielet `hot report` (D28).
//! Docelowo `hot report` wywołuje `hotd`, który poda status i wersje sam.
//! EN: The `hot-diag` program. One command for now: `report`, the skeleton of `hot report` (D28).
//! Later `hotd` runs `hot report` and supplies status and versions itself.

#![allow(clippy::print_stdout, clippy::print_stderr)] // PL: program wiersza poleceń. EN: a command-line program.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use hot_diag::report::{build_report, ReportInput};
use hot_diag::Redactor;

const USAGE: &str = "\
Usage: hot-diag report --logs DIR [--status-file FILE] [--config FILE]... [--secret-file FILE]...
                       [--horizon VERSION] [--out FILE]

Writes one Markdown report (under 200 KB) to paste into a chat with any model.
Secrets read from --secret-file and players' IPv4 addresses are masked.";

#[derive(Default)]
struct Args {
    logs: Option<PathBuf>,
    status_file: Option<PathBuf>,
    config: Vec<PathBuf>,
    secret_files: Vec<PathBuf>,
    horizon: Option<String>,
    out: Option<PathBuf>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    if it.next().as_deref() != Some("report") {
        return Err("the first argument must be the command `report`".into());
    }
    let mut a = Args::default();
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--logs" => a.logs = Some(value()?.into()),
            "--status-file" => a.status_file = Some(value()?.into()),
            "--config" => a.config.push(value()?.into()),
            "--secret-file" => a.secret_files.push(value()?.into()),
            "--horizon" => a.horizon = Some(value()?),
            "--out" => a.out = Some(value()?.into()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn read_or_note(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| format!("(cannot read {}: {e})", path.display()))
}

fn versions(horizon: Option<String>) -> Vec<(String, String)> {
    let mut v = vec![
        ("hot-diag".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
        (
            "Horizon".to_owned(),
            horizon.unwrap_or_else(|| "unknown (pass --horizon; hotd will fill it in)".into()),
        ),
        (
            "System".to_owned(),
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        ),
    ];
    if let Ok(text) = fs::read_to_string("/etc/os-release") {
        if let Some(name) = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")) {
            v.push(("Distribution".into(), name.trim_matches('"').to_owned()));
        }
    }
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    v.push(("CPU threads".into(), cpus.to_string()));
    if let Ok(text) = fs::read_to_string("/proc/meminfo") {
        if let Some(total) = text.lines().find(|l| l.starts_with("MemTotal:")) {
            v.push((
                "Memory".into(),
                total.trim_start_matches("MemTotal:").trim().to_owned(),
            ));
        }
    }
    v
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hot-diag: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(logs_dir) = args.logs else {
        eprintln!("hot-diag: --logs is required\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let mut redactor = Redactor::new();
    for path in &args.secret_files {
        match fs::read_to_string(path) {
            Ok(s) => redactor.add(s.trim()),
            Err(e) => {
                // PL: Bez sekretu raport mógłby go ujawnić, więc nie powstaje.
                // EN: Without the secret the report could leak it, so it is not written.
                eprintln!(
                    "hot-diag: cannot read secret file {}: {e}; no report written",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        }
    }
    let input = ReportInput {
        versions: versions(args.horizon),
        status: args.status_file.as_ref().map_or_else(
            || "(no status given; hotd will fill it in)".into(),
            read_or_note,
        ),
        logs_dir,
        config: args
            .config
            .iter()
            .map(|p| (p.display().to_string(), read_or_note(p)))
            .collect(),
    };
    let report = build_report(&input, &redactor);
    match args.out {
        None => print!("{report}"),
        Some(path) => {
            if let Err(e) = fs::write(&path, &report) {
                eprintln!(
                    "hot-diag: cannot write the report to {}: {e}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
            println!("{}", path.display());
        }
    }
    ExitCode::SUCCESS
}
