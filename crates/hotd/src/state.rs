//! PL: Stan `hotd` w `hot-state/hotd.json`: commit na czubku repo, ostatni zastosowany, złe
//! i nieudane commity, oczekiwanie na CI, odłożone zmiany i ostatnie błędy. Ten sam plik czyta
//! `hot status` w terminalu i `/hot status` w grze (`hot-core`), więc teksty dla ludzi są po
//! polsku, a klucze po angielsku.
//!
//! EN: `hotd` state in `hot-state/hotd.json`: the repository tip, the last applied commit, bad and
//! failed commits, waiting for CI, deferred changes and recent errors. `hot status` in the terminal
//! and `/hot status` in game (`hot-core`) read the same file, so texts for people are in Polish and
//! keys are in English.

use std::fs;
use std::path::Path;

use hot_diag::DiagError;
use serde_json::{json, Value};

use crate::codes;

/// PL: Ile wpisów trzymają listy w pliku. EN: How many entries the lists in the file keep.
const KEEP_FAILED: usize = 20;
const KEEP_BAD: usize = 50;
const KEEP_ERRORS: usize = 10;

/// PL: Stan zapisywany po każdym kroku. EN: State saved after every step.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub head: Option<String>,
    pub applied: Option<String>,
    /// PL: Commit zastosowany przed `applied` (powrót po złym commicie). EN: Before `applied`.
    pub previous: Option<String>,
    /// PL: Czas zastosowania `applied` w ms od 1970 i PID silnika w tej chwili (D13).
    /// EN: When `applied` was applied (ms since 1970) and the engine PID at that moment (D13).
    pub applied_at_ms: u64,
    pub applied_pid: Option<u32>,
    /// PL: Na co czeka `hotd`, po polsku. EN: What `hotd` waits for, in Polish.
    pub waiting: Option<String>,
    /// PL: Zmiany, których `hotd` jeszcze nie stosuje. EN: Changes `hotd` does not apply yet.
    pub deferred: Vec<String>,
    /// PL: Commity, których nie dało się zastosować, z powodem. EN: Commits that failed, with why.
    pub failed: Vec<(String, String)>,
    /// PL: Commity, po których silnik padł (D13). EN: Commits after which the engine died (D13).
    pub bad: Vec<String>,
    pub errors: Vec<Value>,
    /// PL: Czemu ostatnie pobranie repo się nie udało; `None` = udało się. To stan na teraz,
    /// a `errors` to historia. EN: Why the last fetch failed; `None` = it worked. The current
    /// state, while `errors` is the history.
    pub fetch_error: Option<String>,
}

impl State {
    /// PL: Commit już rozstrzygnięty: zastosowany, zły albo nieudany. EN: Already decided.
    #[must_use]
    pub fn settled(&self, commit: &str) -> bool {
        self.applied.as_deref() == Some(commit)
            || self.bad.iter().any(|c| c == commit)
            || self.failed.iter().any(|(c, _)| c == commit)
    }

    pub fn fail(&mut self, commit: &str, reason: &str) {
        self.failed.push((commit.to_owned(), reason.to_owned()));
        trim(&mut self.failed, KEEP_FAILED);
    }

    pub fn mark_bad(&mut self, commit: &str) {
        self.bad.push(commit.to_owned());
        trim(&mut self.bad, KEEP_BAD);
    }

    pub fn record(&mut self, e: &DiagError, time: &str) {
        let f = e.fields();
        self.errors.push(json!({
            "time": time,
            "code": f["code"],
            "what": f["what"],
            "fix": f["fix"],
        }));
        trim(&mut self.errors, KEEP_ERRORS);
    }

    #[must_use]
    pub fn to_json(&self, updated: &str) -> Value {
        json!({
            "component": "hotd",
            "version": crate::VERSION,
            "updated": updated,
            "head": self.head,
            "applied": self.applied,
            "previous": self.previous,
            "applied_at_ms": self.applied_at_ms,
            "applied_pid": self.applied_pid,
            "waiting": self.waiting,
            "deferred": self.deferred,
            "failed": self.failed.iter().map(|(c, r)| json!({"commit": c, "reason": r})).collect::<Vec<_>>(),
            "bad": self.bad,
            "recent_errors": self.errors,
            "fetch_error": self.fetch_error,
        })
    }

    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        let text = |k: &str| v[k].as_str().map(str::to_owned);
        let list = |k: &str| -> Vec<String> {
            v[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            head: text("head"),
            applied: text("applied"),
            previous: text("previous"),
            applied_at_ms: v["applied_at_ms"].as_u64().unwrap_or(0),
            applied_pid: v["applied_pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok()),
            waiting: text("waiting"),
            deferred: list("deferred"),
            failed: v["failed"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            Some((
                                x["commit"].as_str()?.to_owned(),
                                x["reason"].as_str().unwrap_or("").to_owned(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            bad: list("bad"),
            errors: v["recent_errors"].as_array().cloned().unwrap_or_default(),
            fetch_error: text("fetch_error"),
        }
    }

    /// PL: Odczyt; brak pliku = stan pusty (pierwszy start).
    /// EN: Reads the file; a missing file is an empty state (first start).
    ///
    /// # Errors
    /// PL: `HOT-E0207`, gdy plik jest, ale nie jest poprawnym JSON-em.
    /// EN: `HOT-E0207` when the file exists but is not valid JSON.
    pub fn load(path: &Path) -> Result<Self, DiagError> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(state_error(path, "read").caused_by(&e)),
        };
        serde_json::from_str::<Value>(&text)
            .map(|v| Self::from_json(&v))
            .map_err(|e| state_error(path, "parse").caused_by(&e))
    }
}

/// PL: Zapis przez plik tymczasowy i `rename`. EN: Write via a temporary file and `rename`.
///
/// # Errors
/// PL: `HOT-E0207`. EN: `HOT-E0207`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DiagError> {
    let tmp = path.with_extension("tmp");
    path.parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&tmp, bytes))
        .and_then(|()| fs::rename(&tmp, path))
        .map_err(|e| state_error(path, "write").caused_by(&e))
}

fn state_error(path: &Path, what: &str) -> DiagError {
    DiagError::new(
        codes::STATE_FILE,
        format!("hotd cannot {what} its state file {}", path.display()),
        "Check that the service user owns hot-state/ and the disk is not full (df -h); a file that does not parse can be moved aside, hotd then starts from an empty state and reapplies the repository",
    )
    .context("path", path.display().to_string())
    .state("hotd keeps running with the state it has in memory")
}

fn trim<T>(list: &mut Vec<T>, keep: usize) {
    if list.len() > keep {
        list.drain(..list.len() - keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_keeps_everything() {
        let mut s = State {
            head: Some("b".into()),
            applied: Some("a".into()),
            previous: None,
            applied_at_ms: 7,
            applied_pid: Some(42),
            waiting: Some("czekam".into()),
            deferred: vec!["x".into()],
            fetch_error: Some("brak sieci".into()),
            ..State::default()
        };
        s.fail("c", "nie zbudował się");
        s.mark_bad("d");
        assert_eq!(State::from_json(&s.to_json("t")), s);
        assert!(s.settled("a") && s.settled("c") && s.settled("d") && !s.settled("b"));
    }

    #[test]
    fn lists_are_bounded() {
        let mut s = State::default();
        for i in 0..100 {
            s.fail(&i.to_string(), "r");
            s.mark_bad(&i.to_string());
        }
        assert_eq!(s.failed.len(), KEEP_FAILED);
        assert_eq!(s.failed[0].0, "80");
        assert_eq!(s.bad.len(), KEEP_BAD);
    }
}
