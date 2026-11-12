//! PL: `hot status` w terminalu (D21): stan `hotd` i `hot-core` po polsku, z plików
//! `hot-state/hotd.json` i `hot-state/status.json`. Działa także wtedy, gdy silnik albo `hotd`
//! nie działa — mówi wtedy, czego brakuje.
//!
//! EN: `hot status` in the terminal (D21): `hotd` and `hot-core` state in Polish, from
//! `hot-state/hotd.json` and `hot-state/status.json`. Works even when the engine or `hotd` is down,
//! and then says what is missing.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::Value;

fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn short(v: &Value) -> String {
    v.as_str()
        .map_or_else(|| "brak".to_owned(), |s| s.get(..7).unwrap_or(s).to_owned())
}

/// PL: Tekst `hot status` dla katalogu `HOT_HOME`. EN: The `hot status` text for a `HOT_HOME`.
#[must_use]
pub fn render(home: &Path) -> String {
    let mut out = String::new();
    let state = home.join("hot-state");
    match read(&state.join("hotd.json")) {
        None => out.push_str(
            "hotd: brak hot-state/hotd.json — hotd nie działa albo jeszcze nie zrobił kroku\n",
        ),
        Some(d) => {
            let _ = writeln!(
                out,
                "hotd {}: repo {}, zastosowany {} (stan z {})",
                d["version"].as_str().unwrap_or("?"),
                short(&d["head"]),
                short(&d["applied"]),
                d["updated"].as_str().unwrap_or("?")
            );
            if let Some(f) = d["fetch_error"].as_str() {
                let _ = writeln!(out, "  repo teraz niedostępne: {f}");
            }
            if let Some(w) = d["waiting"].as_str() {
                let _ = writeln!(out, "  czeka: {w}");
            }
            for item in d["deferred"].as_array().into_iter().flatten() {
                let _ = writeln!(out, "  odłożone: {}", item.as_str().unwrap_or(""));
            }
            if let Some(f) = d["failed"].as_array().and_then(|a| a.last()) {
                let _ = writeln!(
                    out,
                    "  ostatni nieudany commit {}: {}",
                    short(&f["commit"]),
                    f["reason"].as_str().unwrap_or("")
                );
            }
            if let Some(b) = d["bad"].as_array().and_then(|a| a.last()) {
                let _ = writeln!(out, "  ostatni zły commit (silnik padł): {}", short(b));
            }
            for e in d["recent_errors"]
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .take(3)
            {
                let _ = writeln!(
                    out,
                    "  błąd {} {}: {}",
                    e["time"].as_str().unwrap_or(""),
                    e["code"].as_str().unwrap_or(""),
                    e["what"].as_str().unwrap_or("")
                );
            }
        }
    }
    match read(&state.join("status.json")) {
        None => out.push_str("hot-core: brak hot-state/status.json — silnik bez hot-host albo jeszcze nie wystartował\n"),
        Some(c) => {
            let mods = c["mods"].as_array().cloned().unwrap_or_default();
            let _ = writeln!(
                out,
                "hot-core {}: mody {}, tick {} (stan z {})",
                c["version"].as_str().unwrap_or("?"),
                mods.len(),
                c["tick"],
                c["updated"].as_str().unwrap_or("?")
            );
            for m in &mods {
                let _ = writeln!(
                    out,
                    "  {} {} — generacja {}, {}",
                    m["id"].as_str().unwrap_or("?"),
                    m["version"].as_str().unwrap_or("?"),
                    m["generation"],
                    if m["good"] == true { "dobra" } else { "na próbie" }
                );
            }
            for e in c["recent_errors"].as_array().into_iter().flatten().rev().take(3) {
                let _ = writeln!(
                    out,
                    "  błąd {}: {}",
                    e["code"].as_str().unwrap_or(""),
                    e["what"].as_str().unwrap_or("")
                );
            }
        }
    }
    out
}
