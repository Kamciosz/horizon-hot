//! PL: Odzyskanie pamięci generacji (D4, M8.5). `hot-core` nie zamyka bibliotek starych generacji
//! modów, więc każde przeładowanie zostawia w procesie silnika kopię biblioteki (na Macu ok. 220 KB
//! RSS na generację moda z pliku 574 KB). `hotd engine` co minutę czyta `hot-state/status.json`
//! (pole `libraries`) i gdy nieużywane generacje przekroczą próg (`--reclaim-mb`), robi podmianę
//! silnika na ten sam plik (D11): nowy proces ma tylko biblioteki w użyciu. Próg liczy bajty
//! plików, nie RSS: RSS rośnie też od świata i graczy, a tego podmiana nie odda.
//!
//! EN: Reclaiming generation memory (D4, M8.5). `hot-core` never closes the libraries of old mod
//! generations, so every reload leaves a library copy in the engine process (on a Mac about 220 KB
//! of RSS per generation of a 574 KB mod file). `hotd engine` reads `hot-state/status.json` (the
//! `libraries` field) every minute and when unused generations pass the limit (`--reclaim-mb`), it
//! swaps the engine for the same file (D11): the new process holds only the libraries in use. The
//! limit counts file bytes, not RSS: RSS also grows with the world and the players, and a swap does
//! not give that back.

use std::time::{Duration, Instant};

use hot_diag::Code;
use serde_json::Value;

use crate::codes;

/// PL: Domyślny próg w MiB plików nieużywanych generacji. EN: The default limit in MiB.
pub const DEFAULT_MB: u64 = 256;
/// PL: Jak często sprawdzać. EN: How often to check.
pub const EVERY: Duration = Duration::from_secs(60);
/// PL: Przerwa po podmianie, której gracze nie odczuli (`HOT-E0606`: zostali na starym silniku).
/// EN: The pause after a swap the players did not feel (`HOT-E0606`: they stayed on the old one).
pub const AFTER_REFUSAL: Duration = Duration::from_secs(30 * 60);
/// PL: Przerwa po `HOT-E0607`: gracze czekali kilka sekund na powrót starego silnika.
/// EN: The pause after `HOT-E0607`: the players waited seconds for the old engine to come back.
pub const AFTER_FAILURE: Duration = Duration::from_secs(6 * 60 * 60);

/// PL: Kiedy wolno spróbować. EN: When a try is allowed.
#[derive(Debug, Default)]
pub struct Reclaim {
    /// PL: Próg w bajtach; 0 = wyłączone. EN: The limit in bytes; 0 = off.
    pub limit: u64,
    pub not_before: Option<Instant>,
}

impl Reclaim {
    #[must_use]
    pub fn new(limit_mb: u64) -> Self {
        Self {
            limit: limit_mb.saturating_mul(1024 * 1024),
            not_before: None,
        }
    }

    /// PL: Bajty nieużywanych generacji, gdy trzeba podmienić silnik; `None`, gdy nie trzeba.
    /// `status` to `hot-state/status.json`, `engine_pid` to pid aktywnego silnika: status z innym
    /// pidem napisał poprzedni silnik albo druga strona i nic nie mówi o tym procesie.
    /// EN: The unused generation bytes when the engine needs a swap; `None` when not. `status` is
    /// `hot-state/status.json`, `engine_pid` the active engine's pid: a status with another pid was
    /// written by an earlier engine or the other side and says nothing about this process.
    #[must_use]
    pub fn due(&self, status: &Value, engine_pid: Option<u64>, now: Instant) -> Option<u64> {
        if self.limit == 0 || self.not_before.is_some_and(|t| now < t) {
            return None;
        }
        let lib = &status["libraries"];
        if engine_pid.is_none() || lib["pid"].as_u64() != engine_pid {
            return None;
        }
        lib["stale_bytes"]
            .as_u64()
            .filter(|stale| *stale >= self.limit)
    }

    /// PL: Zapisuje wynik podmiany: kod błędu albo `None` po sukcesie.
    /// EN: Records a swap result: the error code, or `None` after a success.
    pub fn done(&mut self, error_code: Option<Code>, now: Instant) {
        let pause = match error_code {
            None => EVERY,
            Some(codes::SWAP_FAILED) => AFTER_FAILURE,
            Some(_) => AFTER_REFUSAL,
        };
        self.not_before = Some(now + pause);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(pid: u64, stale_mb: u64) -> Value {
        json!({ "libraries": { "pid": pid, "count": 300, "stale_bytes": stale_mb * 1024 * 1024 } })
    }

    #[test]
    fn a_swap_is_due_only_for_this_engine_above_the_limit() {
        let now = Instant::now();
        let r = Reclaim::new(100);
        assert_eq!(
            r.due(&status(7, 100), Some(7), now),
            Some(100 * 1024 * 1024)
        );
        assert_eq!(r.due(&status(7, 99), Some(7), now), None, "below the limit");
        assert_eq!(
            r.due(&status(8, 500), Some(7), now),
            None,
            "another process"
        );
        assert_eq!(
            r.due(&status(7, 500), None, now),
            None,
            "the engine is not running"
        );
        assert_eq!(
            r.due(&json!({}), Some(7), now),
            None,
            "hot-core without the field"
        );
        assert_eq!(
            Reclaim::new(0).due(&status(7, 500), Some(7), now),
            None,
            "off"
        );
    }

    #[test]
    fn a_failed_swap_waits_before_the_next_try() {
        let now = Instant::now();
        let mut r = Reclaim::new(100);
        r.done(Some(codes::SWAP_NOT_STARTED), now);
        assert_eq!(r.due(&status(7, 500), Some(7), now + EVERY), None);
        assert!(r
            .due(&status(7, 500), Some(7), now + AFTER_REFUSAL)
            .is_some());
        r.done(Some(codes::SWAP_FAILED), now);
        assert_eq!(r.due(&status(7, 500), Some(7), now + AFTER_REFUSAL), None);
        assert!(r
            .due(&status(7, 500), Some(7), now + AFTER_FAILURE)
            .is_some());
        r.done(None, now);
        assert!(r.due(&status(7, 500), Some(7), now + EVERY).is_some());
    }
}
