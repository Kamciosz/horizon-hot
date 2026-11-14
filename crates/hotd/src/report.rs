//! PL: `hot report` na serwerze (D28): `hotd report` zbiera wszystko, czego obcy model potrzebuje do
//! diagnozy, i oddaje to `hot_diag::build_report`. Bierze `hot status`, logi `<home>/logs/*.jsonl`,
//! log silnika aktywnej strony (`latest.log`, `supervisor.log`), log instalatora albo kontenera
//! i `server.toml`.
//! Sekrety maskuje: `forwarding.secret`, klucze prywatne z `hot-keys/` i z `.ssh/` użytkownika
//! `horizon`. Adresy IPv4 graczy maskuje sam raport.
//!
//! EN: `hot report` on the server (D28): `hotd report` gathers everything an outside model needs to
//! diagnose a problem and hands it to `hot_diag::build_report`: `hot status`, `<home>/logs/*.jsonl`,
//! the active side's engine logs (`latest.log`, `supervisor.log`), the installer or container log
//! and `server.toml`. It masks secrets: `forwarding.secret` and private keys from `hot-keys/` and the
//! `horizon` user's `.ssh/`. The report itself masks players' IPv4 addresses.

use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::report::ReportInput;
use hot_diag::Redactor;

use crate::swap::Layout;

/// PL: Dane raportu dla `HOT_HOME` i maskowanie jego sekretów.
/// EN: The report input for a `HOT_HOME` and the masking of its secrets.
#[must_use]
pub fn input(home: &Path) -> (ReportInput, Redactor) {
    let active = Layout {
        home: home.to_path_buf(),
    }
    .active_dir();
    let root = home.parent().unwrap_or(home);
    let mut text_logs = vec![
        active.join("logs").join("latest.log"),
        active.join("logs").join("supervisor.log"),
        root.join("logs").join("install.jsonl"),
        root.join("logs").join("container.jsonl"),
    ];
    text_logs.dedup();
    let config = [active.join("server.toml")]
        .iter()
        .filter(|p| p.is_file())
        .map(|p| {
            let text = fs::read_to_string(p).unwrap_or_else(|e| format!("(cannot read: {e})"));
            (p.display().to_string(), text)
        })
        .collect();
    let input = ReportInput {
        versions: versions(home),
        status: crate::status::render(home),
        logs_dir: home.join("logs"),
        text_logs,
        config,
    };
    (input, redactor(home, root))
}

fn versions(home: &Path) -> Vec<(String, String)> {
    let or_unknown = |v: Option<String>| v.unwrap_or_else(|| "unknown".into());
    let os = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| std::env::consts::OS.to_owned());
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let memory = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| {
            t.lines()
                .next()
                .map(|l| l.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))
        })
        .unwrap_or_else(|| "unknown".into());
    vec![
        ("hotd".into(), crate::VERSION.into()),
        (
            "horizon-hot commit (hot-state/upgrade.json)".into(),
            or_unknown(crate::upgrade::Upgrade::load(home).current),
        ),
        (
            "Horizon revision (hot-state/horizon.json)".into(),
            or_unknown(crate::update::Update::load(home).current),
        ),
        ("System".into(), format!("{os}, {}", std::env::consts::ARCH)),
        (
            "Hardware".into(),
            format!("{cpus} CPU threads, memory {memory}"),
        ),
    ]
}

/// PL: Sekrety z plików, które zna tylko serwer. Klucz prywatny maskujemy w całości i linia po
/// linii, bo w logu może trafić się jego fragment.
/// EN: Secrets from files only the server knows. A private key is masked whole and line by line,
/// as a log may hold a part of it.
fn redactor(home: &Path, root: &Path) -> Redactor {
    let mut r = Redactor::new();
    let mut files = vec![home.join("forwarding.secret")];
    for dir in [home.join("hot-keys"), root.join(".ssh")] {
        files.extend(private_keys(&dir));
    }
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        r.add(text.trim());
        for line in text.lines().map(str::trim) {
            if line.len() >= 16 && !line.starts_with("-----") {
                r.add(line);
            }
        }
    }
    r
}

fn private_keys(dir: &Path) -> Vec<PathBuf> {
    let mut keys: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            p.is_file()
                && !name.ends_with(".pub")
                && name != "allowed_signers"
                && name != "known_hosts"
        })
        .collect();
    keys.sort();
    keys
}
