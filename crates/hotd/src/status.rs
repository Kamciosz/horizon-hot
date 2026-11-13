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
            if let Some(w) = d["world_damage"].as_str() {
                let _ = writeln!(out, "  ŚWIAT: {w}");
            }
            if let Some(b) = d["brake"].as_str() {
                let _ = writeln!(out, "  hamulec: {b}");
            }
            if let Some(f) = d["fetch_error"].as_str() {
                let _ = writeln!(out, "  repo teraz niedostępne: {f}");
            }
            if let Some(w) = d["waiting"].as_str() {
                let _ = writeln!(out, "  czeka: {w}");
            }
            for item in d["data_warnings"].as_array().into_iter().flatten() {
                let _ = writeln!(
                    out,
                    "  paczka danych, plik pominięty przez silnik: {}",
                    item.as_str().unwrap_or("")
                );
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
            if let Some(dormant) = c["safe_mode"]["dormant"].as_array() {
                let ids: Vec<&str> = dormant.iter().filter_map(Value::as_str).collect();
                let _ = writeln!(
                    out,
                    "  TRYB BEZPIECZNY: serwer padł {} razy z rzędu krótko po starcie, mody śpią: {} (HOT-E0408)",
                    c["safe_mode"]["crash_starts"],
                    ids.join(", ")
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
    if let Some(s) = read(&state.join("slots.json")) {
        let blocks = s["blocks"].as_object().cloned().unwrap_or_default();
        let sha1 = std::fs::read_to_string(state.join("pack/pack.sha1")).unwrap_or_default();
        let _ = writeln!(
            out,
            "bloki modów: {} z {} (paczka zasobów {})",
            blocks.len(),
            s["pool"]["blocks"],
            sha1.get(..7).unwrap_or("brak")
        );
        for (name, slot) in &blocks {
            let _ = writeln!(out, "  {name} = hot_slots:slot_{slot}");
        }
    }
    if let Some(horizon) = crate::update::status_lines(home) {
        out.push_str(&horizon);
    }
    if let Some(upgrade) = crate::upgrade::status_lines(home) {
        out.push_str(&upgrade);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_mode_is_shown() -> std::io::Result<()> {
        let home = std::env::temp_dir().join(format!("hotd-status-{}", std::process::id()));
        let state = home.join("hot-state");
        std::fs::create_dir_all(&state)?;
        std::fs::write(
            state.join("status.json"),
            r#"{"version":"0.1.0","tick":0,"updated":"t","mods":[],"safe_mode":{"crash_starts":2,"dormant":["sklep","licznik"]},"recent_errors":[]}"#,
        )?;
        let out = render(&home);
        let _ = std::fs::remove_dir_all(&home);
        assert!(
            out.contains("TRYB BEZPIECZNY: serwer padł 2 razy z rzędu krótko po starcie, mody śpią: sklep, licznik"),
            "{out}"
        );
        Ok(())
    }
}
