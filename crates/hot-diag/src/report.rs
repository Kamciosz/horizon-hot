//! PL: `hot report` (D28): jeden plik Markdown < 200 KB, który wystarczy obcemu modelowi do
//! diagnozy. Treść po angielsku, jak logi (D27). Szkielet: wersje i status podaje wywołujący.
//! EN: `hot report` (D28): one Markdown file under 200 KB that is enough for an outside model to
//! diagnose a problem. English, like the logs (D27). Skeleton: the caller passes versions and status.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::redact::mask_ipv4;
use crate::{catalog_entry, Redactor};

/// PL: Górna granica rozmiaru raportu (D28). EN: The report's size limit (D28).
pub const MAX_REPORT_BYTES: usize = 200 * 1024;
/// PL: Ile ostatnich linii logu każdego składnika. EN: How many last log lines per component.
pub const LOG_TAIL_LINES: usize = 200;
/// PL: Ile ostatnich błędów. EN: How many recent errors.
pub const RECENT_ERRORS: usize = 20;

const READ_WINDOW: u64 = 512 * 1024;
const ERRORS_BUDGET: usize = 80 * 1024;
const CONFIG_BUDGET: usize = 16 * 1024;
const SAFETY_MARGIN: usize = 4 * 1024;

/// PL: Tytuły pięciu części D28, w tej kolejności. EN: The five D28 part titles, in this order.
pub const SECTIONS: [&str; 5] = [
    "## 1. What this system is",
    "## 2. Versions",
    "## 3. Recent errors",
    "## 4. Status, logs and configuration",
    "## 5. Instructions for the model",
];

const OVERVIEW: &str = "\
horizon-hot runs a game server for a small group of friends. The game is Horizon, a Rust
reimplementation of Minecraft Java Edition 26.3. Every player owns the game; nothing here bypasses
the purchase check.

Parts and flow:
- Engine: the Horizon dedicated server (`horizon-server`). Two folders, engine-a and engine-b, take
  turns on one shared world; only one runs at a time.
- hot-proxy: players connect to it on port 25565. It forwards traffic to the running engine and keeps
  players connected while the engine is replaced (the world freezes; target under 1 s, no loading screen).
- hot-host: a tiny native mod compiled into the engine. It loads hot-core.
- hot-core: loads hot mods (cdylib) into the running engine without a restart, manages block and
  item slots (hot-slots.hmod) and the /hot command.
- hotd: background daemon. Pulls the server repository from GitHub, downloads mods built by CI,
  builds new engines, updates Horizon, makes backups, slows background work when the game lags.
- hot-pack: serves the resource pack on port 8131.
- hot-play: the players' terminal app; installs and starts the game client.
- hot-diag: logs (JSON Lines, one file per component) and this report.
Files on the server: /srv/horizon (engines, world, hot-state, hot-data), logs in /srv/horizon/logs.
Source code: https://github.com/Kamciosz/horizon-hot (start with AGENTS.md).
";

/// PL: Dane wejściowe raportu. EN: The report's input.
#[derive(Debug, Clone, Default)]
pub struct ReportInput {
    /// PL: Para (co, wersja), np. („Horizon”, `nightly-…`). EN: (what, version) pairs.
    pub versions: Vec<(String, String)>,
    /// PL: Wynik `hot status`. EN: The `hot status` output.
    pub status: String,
    /// PL: Katalog z plikami `<składnik>.jsonl`. EN: The folder with `<component>.jsonl` files.
    pub logs_dir: PathBuf,
    /// PL: Pliki konfiguracji: (nazwa, treść). EN: Configuration files: (name, content).
    pub config: Vec<(String, String)>,
}

/// PL: Buduje raport. Sekrety i adresy IPv4 graczy są zamaskowane, rozmiar < 200 KB.
/// EN: Builds the report. Secrets and players' IPv4 addresses are masked, size < 200 KB.
#[must_use]
pub fn build_report(input: &ReportInput, redactor: &Redactor) -> String {
    let mask = |s: &str| mask_ipv4(&redactor.text(s));
    let logs = log_files(&input.logs_dir);

    let mut head = format!("# horizon-hot report\n\n{}\n\n{OVERVIEW}\n", SECTIONS[0]);
    head.push_str(&mask(&versions_part(input)));
    head.push_str(&mask(&errors_part(&logs, redactor)));
    let tail = mask(&instructions_part());

    let mut middle = format!(
        "{}\n\n### hot status\n\n```text\n{}\n```\n",
        SECTIONS[3],
        mask(&input.status)
    );
    for (name, content) in &input.config {
        let body = keep_last_bytes(&mask(content), CONFIG_BUDGET);
        let _ = write!(
            middle,
            "\n### Configuration: {name}\n\n```text\n{body}\n```\n"
        );
    }

    let used = head.len() + middle.len() + tail.len() + SAFETY_MARGIN;
    let per_log = MAX_REPORT_BYTES.saturating_sub(used) / logs.len().max(1);
    for path in &logs {
        let lines = tail_lines(path, LOG_TAIL_LINES)
            .unwrap_or_else(|e| vec![format!("(cannot read: {e})")]);
        let body = keep_last_bytes(&mask(&lines.join("\n")), per_log.saturating_sub(200));
        let _ = write!(
            middle,
            "\n### Log: {}\n\n```text\n{body}\n```\n",
            mask(&path.display().to_string())
        );
    }
    format!("{head}\n{middle}\n{tail}")
}

fn versions_part(input: &ReportInput) -> String {
    let mut s = format!("{}\n\n", SECTIONS[1]);
    for (what, version) in &input.versions {
        let _ = writeln!(s, "- {what}: {version}");
    }
    s.push('\n');
    s
}

fn errors_part(logs: &[PathBuf], redactor: &Redactor) -> String {
    let mut errors: Vec<Value> = Vec::new();
    for path in logs {
        for line in tail_lines(path, usize::MAX).unwrap_or_default() {
            match serde_json::from_str::<Value>(&line) {
                Ok(v) if v.get("level").and_then(Value::as_str) == Some("error") => errors.push(v),
                _ => {}
            }
        }
    }
    let ts = |v: &Value| v.get("ts").and_then(Value::as_str).unwrap_or("").to_owned();
    errors.sort_by_key(ts);
    let start = errors.len().saturating_sub(RECENT_ERRORS);
    let mut errors = errors.split_off(start);
    errors.iter_mut().for_each(|v| redactor.value(v));
    loop {
        let part = render_errors(&errors);
        if part.len() <= ERRORS_BUDGET || errors.is_empty() {
            return part;
        }
        errors.remove(0);
    }
}

fn render_errors(errors: &[Value]) -> String {
    let mut s = format!("{}\n\n", SECTIONS[2]);
    if errors.is_empty() {
        s.push_str("No error entries in the current log files.\n\n");
        return s;
    }
    let mut codes = BTreeSet::new();
    for e in errors {
        let pretty = serde_json::to_string_pretty(e).unwrap_or_else(|_| e.to_string());
        let _ = write!(s, "```json\n{pretty}\n```\n\n");
        if let Some(code) = e.get("code").and_then(Value::as_str) {
            codes.insert(code.to_owned());
        }
    }
    for code in codes {
        let _ = write!(s, "### Catalog entry {code} (docs/bledy/{code}.md)\n\n");
        match catalog_entry(&code) {
            Some(text) => s.push_str(&demote_headings(text)),
            None => s.push_str(
                "No catalog entry. This is a bug: every code needs a file in docs/bledy/.\n",
            ),
        }
        s.push('\n');
    }
    s
}

/// PL: Nagłówki wpisu katalogu o trzy poziomy niżej, żeby nie udawały pięciu części raportu.
/// EN: Catalog entry headings three levels lower, so they don't pose as the report's five parts.
fn demote_headings(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.starts_with('#') {
                format!("###{l}\n")
            } else {
                format!("{l}\n")
            }
        })
        .collect()
}

fn instructions_part() -> String {
    let rules = agents_rules().unwrap_or("(AGENTS.md section 6 not found in this build)");
    format!(
        "{}\n\n\
Repository: https://github.com/Kamciosz/horizon-hot. Read AGENTS.md first, then the catalog file \
named in each error's `doc` field and the code at its `source`.\n\
To reproduce: check out the commit in the error's `version`, use the identifiers in `context`, \
and follow the catalog entry's Diagnostics section.\n\
Answer in Polish if the person who sent this report writes in Polish.\n\n\
Rules you must not break (AGENTS.md, section 6; written in Polish):\n\n{rules}\n",
        SECTIONS[4]
    )
}

fn agents_rules() -> Option<&'static str> {
    let text = include_str!("../../../AGENTS.md");
    let start = text.find("## 6.")?;
    let body = &text[start..];
    let after_title = body.find('\n')? + 1;
    let end = body[after_title..]
        .find("\n## ")
        .map_or(body.len(), |i| after_title + i);
    Some(body[after_title..end].trim())
}

/// PL: Bieżące pliki `*.jsonl` (bez rotowanych `*.N.jsonl`), posortowane.
/// EN: The current `*.jsonl` files (without rotated `*.N.jsonl`), sorted.
fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".jsonl") && name.matches('.').count() == 1
        })
        .collect();
    files.sort();
    files
}

/// PL: Ostatnie `n` linii z końcowego okna pliku (pliki mają do 50 MB, nie czytamy całości).
/// EN: The last `n` lines from the file's final window (files reach 50 MB; we don't read it all).
fn tail_lines(path: &Path, n: usize) -> io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let from = len.saturating_sub(READ_WINDOW);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if from > 0 && !lines.is_empty() {
        lines.remove(0); // PL: pierwsza linia okna jest ucięta. EN: the window's first line is cut.
    }
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].iter().map(|s| (*s).to_owned()).collect())
}

/// PL: Ostatnie pełne linie, które mieszczą się w `budget` bajtach.
/// EN: The last whole lines that fit in `budget` bytes.
fn keep_last_bytes(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut size = 0;
    for line in text.lines().rev() {
        if size + line.len() + 1 > budget {
            break;
        }
        size += line.len() + 1;
        kept.push(line);
    }
    kept.reverse();
    format!(
        "(earlier lines cut to keep the report under 200 KB)\n{}",
        kept.join("\n")
    )
}
