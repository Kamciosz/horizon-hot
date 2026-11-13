//! PL: Hamulec MSPT (D16, M6.2). Osobny wątek co sekundę czyta `horizon-server status --json`.
//! Gdy p95 czasu ticku przekroczy 35 ms, zatrzymuje (`SIGSTOP`) ciężkie procesy, które `hotd`
//! uruchamia w tle (dziś `du`, od M9 budowa silnika); gdy p95 jest poniżej 25 ms przez 10 s,
//! wznawia je (`SIGCONT`). Gita, komend dla silnika (`hot load`, `hot slots`) ani samego odczytu
//! stanu nie hamuje: dostarczają mody, a serwer, który nie nadąża stale, nie dostałby ich nigdy.
//! Git i tak działa w `horizon-bg.slice` z najniższym priorytetem. Serwer w pauzie (nikogo nie ma) albo zatrzymany liczy się
//! jako spokojny, bo jego próbki są stare. Nieudany odczyt niczego nie zmienia: pod obciążeniem
//! to właśnie odczyt może nie zdążyć.
//!
//! Każdy proces tła dostaje własną grupę procesów, a jej numer trafia do rejestru, z którego
//! korzysta wątek hamulca. Główny wątek czeka na zatrzymany proces, więc hamulec musi działać
//! w osobnym wątku, inaczej nikt by go nie wznowił. Stan dla `hot status` główny wątek bierze
//! przez [`note`], żeby `hotd.json` pisał tylko on.
//!
//! EN: The MSPT brake (D16, M6.2). A separate thread reads `horizon-server status --json` every
//! second. When the p95 tick time goes above 35 ms it stops (`SIGSTOP`) the heavy processes `hotd`
//! runs in the background (`du` today, the engine build from M9); once p95 stays below 25 ms for
//! 10 s it resumes them (`SIGCONT`). Git, engine commands (`hot load`, `hot slots`) and the status
//! read itself are never braked: they deliver mods, and a server that is always behind would never
//! get them. Git runs in `horizon-bg.slice` at the lowest priority anyway. A paused server (nobody online) or a
//! stopped one counts as calm, as its samples are stale. A failed read changes nothing: under load
//! the read is what may time out.
//!
//! Every background process gets its own process group, whose number goes into a registry the
//! brake thread uses. The main thread waits on a stopped process, so the brake must run on its own
//! thread, or nothing would resume it. The main thread takes the state for `hot status` through
//! [`note`], so only it writes `hotd.json`.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use hot_diag::Logger;
use serde_json::{json, Value};

/// PL: Powyżej tylu ms (p95) hamulec się włącza. EN: Above this p95 (ms) the brake engages.
pub const HIGH_MS: f64 = 35.0;
/// PL: Poniżej tylu ms (p95) liczy się spokój. EN: Below this p95 (ms) counts as calm.
pub const LOW_MS: f64 = 25.0;
/// PL: Tyle spokoju zwalnia hamulec. EN: This much calm releases the brake.
pub const CALM_FOR: Duration = Duration::from_secs(10);

/// PL: Grupy procesów tła i to, czy hamulec jest włączony. EN: Background process groups and
/// whether the brake is on.
struct Registry {
    groups: Vec<u32>,
    engaged: bool,
    note: Option<String>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    groups: Vec::new(),
    engaged: false,
    note: None,
});

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// PL: Wysyła sygnał do grupy procesów wywołaniem systemowym. Nie przez program `kill`: ten z
/// procps 4.0.4 (Ubuntu 24.04) źle czyta ujemny numer grupy i może trafić w grupę 1 (ADR 0009).
/// Grupa 0 i 1 nigdy nie dostaje sygnału.
/// EN: Sends a signal to a process group with the system call. Not through the `kill` program:
/// the one from procps 4.0.4 (Ubuntu 24.04) misreads a negative group number and may hit
/// group 1 (ADR 0009). Groups 0 and 1 never get a signal.
fn signal(group: u32, sig: libc::c_int) {
    let Ok(group) = libc::pid_t::try_from(group) else {
        return;
    };
    if group > 1 {
        // SAFETY: `kill` reads no memory of ours; a group that is gone only gives ESRCH.
        unsafe { libc::kill(-group, sig) };
    }
}

/// PL: Jak [`Command::output`], ale proces jest w tle: własna grupa procesów, którą hamulec może
/// zatrzymać. Uruchomiony przy włączonym hamulcu staje od razu.
/// EN: Like [`Command::output`], but the process is background work: its own process group, which
/// the brake may stop. Started while the brake is on, it stops at once.
///
/// # Errors
/// PL: Gdy procesu nie da się uruchomić. EN: When the process cannot be started.
pub fn output(cmd: &mut Command) -> io::Result<Output> {
    cmd.process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = {
        let mut reg = registry();
        let child = cmd.spawn()?;
        reg.groups.push(child.id());
        if reg.engaged {
            signal(child.id(), libc::SIGSTOP);
        }
        child
    };
    let id = child.id();
    let out = child.wait_with_output();
    registry().groups.retain(|g| *g != id);
    out
}

/// PL: Zdanie dla `hot status`, gdy hamulec trzyma; `None`, gdy nie. EN: The `hot status` line
/// while the brake holds; `None` otherwise.
#[must_use]
pub fn note() -> Option<String> {
    registry().note.clone()
}

/// PL: Włącza albo zwalnia hamulec: zatrzymuje albo wznawia wszystkie grupy tła. Zwraca ich
/// liczbę. EN: Engages or releases the brake: stops or resumes every background group. Returns
/// their number.
pub fn hold(on: bool, note: Option<String>) -> usize {
    let mut reg = registry();
    reg.engaged = on;
    reg.note = note;
    for g in &reg.groups {
        signal(*g, if on { libc::SIGSTOP } else { libc::SIGCONT });
    }
    reg.groups.len()
}

/// PL: Odczyt obciążenia z `status --json`. EN: A load reading from `status --json`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Load {
    /// PL: Silnik zatrzymany, w pauzie albo bez gry: nie ma czego chronić.
    /// EN: The engine is stopped, paused or has no game: nothing to protect.
    Calm,
    /// PL: p95 czasu ticku w ms. EN: The p95 tick time in ms.
    P95(f64),
}

/// PL: Czyta `sample.game.mspt.p95`; `None`, gdy to nie jest JSON `horizon-server`.
/// EN: Reads `sample.game.mspt.p95`; `None` when this is not `horizon-server` JSON.
#[must_use]
pub fn reading(status_json: &str) -> Option<Load> {
    let v: Value = serde_json::from_str(status_json).ok()?;
    let phase = v["phase"].as_str()?;
    let game = &v["sample"]["game"];
    if phase != "running" || game["paused"] == true {
        return Some(Load::Calm);
    }
    Some(game["mspt"]["p95"].as_f64().map_or(Load::Calm, Load::P95))
}

/// PL: Histereza: włącza się powyżej [`HIGH_MS`], zwalnia po [`CALM_FOR`] poniżej [`LOW_MS`].
/// EN: Hysteresis: engages above [`HIGH_MS`], releases after [`CALM_FOR`] below [`LOW_MS`].
#[derive(Debug, Default)]
pub struct Brake {
    engaged: bool,
    calm_since: Option<Instant>,
}

impl Brake {
    #[must_use]
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// PL: Bierze odczyt (albo `None` po nieudanym) i zwraca `Some(true)`, gdy hamulec właśnie się
    /// włączył, `Some(false)`, gdy właśnie się zwolnił. EN: Takes a reading (or `None` after a
    /// failed one) and returns `Some(true)` when the brake just engaged, `Some(false)` when it
    /// just released.
    pub fn observe(&mut self, load: Option<Load>, now: Instant) -> Option<bool> {
        let calm = match load {
            None => {
                self.calm_since = None;
                return None;
            }
            Some(Load::Calm) => true,
            Some(Load::P95(ms)) if ms > HIGH_MS => {
                self.calm_since = None;
                if self.engaged {
                    return None;
                }
                self.engaged = true;
                return Some(true);
            }
            Some(Load::P95(ms)) => ms < LOW_MS,
        };
        if !calm {
            self.calm_since = None;
            return None;
        }
        let since = *self.calm_since.get_or_insert(now);
        if self.engaged && now.duration_since(since) >= CALM_FOR {
            self.engaged = false;
            return Some(false);
        }
        None
    }
}

/// PL: Pętla wątku hamulca: co sekundę `probe()` (wyjście `status --json`), histereza, sygnały
/// i wpis w logu przy każdej zmianie. EN: The brake thread's loop: `probe()` (the
/// `status --json` output) every second, hysteresis, signals and a log entry on every change.
pub fn run(mut probe: impl FnMut() -> Option<String>, log: &Arc<Logger>) {
    let mut brake = Brake::default();
    loop {
        let load = probe().as_deref().and_then(reading);
        match brake.observe(load, Instant::now()) {
            Some(true) => {
                let ms = match load {
                    Some(Load::P95(ms)) => ms,
                    _ => 0.0,
                };
                let text = format!(
                    "serwer nie nadąża (p95 czasu ticku {ms:.1} ms > {HIGH_MS:.0} ms): praca hotd w tle wstrzymana, mody wczytują się dalej"
                );
                let stopped = hold(true, Some(text));
                log.info(
                    "hotd pauses its background work: the server is behind",
                    json!({ "p95_ms": ms, "high_ms": HIGH_MS, "stopped_groups": stopped }),
                );
            }
            Some(false) => {
                let resumed = hold(false, None);
                log.info(
                    "hotd resumes its background work: the server keeps up again",
                    json!({ "low_ms": LOW_MS, "calm_s": CALM_FOR.as_secs(), "resumed_groups": resumed }),
                );
            }
            None => {}
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_p95_is_read_and_a_paused_or_stopped_server_is_calm() {
        let running = r#"{"phase":"running","engine_pid":7,"sample":{"rss_bytes":1000,"cpu_percent":12.5,
            "game":{"tps":20.0,"mspt":{"average":3.5,"p50":3.0,"p95":41.5,"p99":60.0},"paused":false}}}"#;
        assert_eq!(reading(running), Some(Load::P95(41.5)));
        let paused = running.replace("\"paused\":false", "\"paused\":true");
        assert_eq!(reading(&paused), Some(Load::Calm));
        assert_eq!(reading(r#"{"phase":"stopped"}"#), Some(Load::Calm));
        assert_eq!(
            reading(r#"{"phase":"running","sample":{}}"#),
            Some(Load::Calm),
            "no game yet (the world is loading)"
        );
        assert_eq!(reading("error: connection refused"), None);
    }

    #[test]
    fn the_brake_engages_above_35_and_releases_after_10_s_below_25() {
        let t = Instant::now();
        let at = |s: u64| t + Duration::from_secs(s);
        let mut b = Brake::default();
        assert_eq!(b.observe(Some(Load::P95(30.0)), at(0)), None);
        assert!(!b.engaged());
        assert_eq!(b.observe(Some(Load::P95(36.0)), at(1)), Some(true));
        assert_eq!(b.observe(Some(Load::P95(50.0)), at(2)), None);
        // PL: 20 ms przez 9 s, potem 30 ms: zegar spokoju startuje od nowa.
        // EN: 20 ms for 9 s, then 30 ms: the calm clock starts over.
        for s in 3..=12 {
            assert_eq!(b.observe(Some(Load::P95(20.0)), at(s)), None);
        }
        assert_eq!(b.observe(Some(Load::P95(30.0)), at(13)), None);
        assert_eq!(b.observe(Some(Load::P95(20.0)), at(14)), None);
        assert_eq!(b.observe(None, at(20)), None, "a failed read is not calm");
        assert!(b.engaged());
        assert_eq!(b.observe(Some(Load::Calm), at(21)), None);
        assert_eq!(b.observe(Some(Load::P95(10.0)), at(30)), None);
        assert_eq!(b.observe(Some(Load::P95(10.0)), at(31)), Some(false));
        assert!(!b.engaged());
    }

    #[test]
    fn background_work_waits_while_the_brake_holds() -> io::Result<()> {
        hold(true, Some("test".into()));
        let worker = std::thread::spawn(|| {
            let start = Instant::now();
            // PL: Hamulec zatrzymuje proces dopiero po jego starcie. Samo `echo` na obciążonym
            // runnerze CI kończyło się przed sygnałem (dwa czerwone przebiegi), więc proces trwa
            // chwilę, jak git czy cargo w prawdziwej pracy tła.
            // EN: The brake stops a process only after it starts. A bare `echo` on a busy CI
            // runner finished before the signal (two red runs), so the process lasts a moment,
            // like git or cargo in real background work.
            let out = output(Command::new("sh").args(["-c", "sleep 0.5; echo gotowe"]));
            (out, start.elapsed())
        });
        // PL: Czekamy na grupę, a nie stały czas: wolny runner CI potrzebuje więcej niż 300 ms.
        // EN: Wait for the group, not a fixed time: a slow CI runner needs more than 300 ms.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = None;
        while Instant::now() < deadline {
            let group = registry().groups.first().copied();
            state = group.and_then(|g| {
                let out = Command::new("ps")
                    .args(["-o", "stat=", "-p", &g.to_string()])
                    .output()
                    .ok()?;
                Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
            });
            if state.as_deref().is_some_and(|s| s.starts_with('T')) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(note().as_deref(), Some("test"));
        hold(false, None);
        let (out, took) = worker
            .join()
            .map_err(|_| io::Error::other("worker panicked"))?;
        assert_eq!(String::from_utf8_lossy(&out?.stdout), "gotowe\n");
        assert!(
            state.as_deref().is_some_and(|s| s.starts_with('T')),
            "stopped while held: {state:?}"
        );
        assert!(took >= Duration::from_millis(1500), "waited {took:?}");
        assert_eq!(note(), None);
        Ok(())
    }
}
