//! PL: Start silnika po czystym zatrzymaniu i po awarii (D13): ostatnia dobra generacja, segfault,
//! druga awaria z rzędu → tryb bezpieczny, zepsuta lista modów. Warsztat: `common/mod.rs`.
//!
//! EN: Engine start after a clean stop and after a crash (D13): last good generation, segfault,
//! a second crash in a row → safe mode, a broken mod list. Harness: `common/mod.rs`.

use std::fs;
use std::path::Path;
use std::process::Command;

mod common;

use common::*;

#[test]
fn clean_stop_brings_back_current_generation_and_state() -> TestResult {
    let home = Server::home("clean-stop");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    s.load_and_wait(&mod_file("licznik_v2")?, |st| {
        generation(st, "licznik") == Some(2)
    })?;
    for _ in 0..30 {
        s.tick();
    }
    let before = count_of(&s.chat("ile"), "v2").ok_or("v2 did not answer")?;
    s.stop();
    let mut s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(generation(&status, "licznik"), Some(2), "{status:#}");
    let after = count_of(&s.chat("ile"), "v2").ok_or("v2 did not come back")?;
    assert_eq!(after, before);
    s.stop();
    Ok(())
}

#[test]
fn crash_brings_back_last_good_generation() -> TestResult {
    let home = Server::home("crash");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..hot_core::GOOD_AFTER_TICKS {
        s.tick();
    }
    assert_eq!(s.status()?["mods"][0]["good"], true);
    s.load_and_wait(&mod_file("licznik_v2")?, |st| {
        generation(st, "licznik") == Some(2)
    })?;
    // PL: Awaria: nowy start bez `deactivate`. EN: A crash: a new start without `deactivate`.
    let crashed = s;
    let mut s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(
        version(&status, "licznik").as_deref(),
        Some("v1"),
        "{status:#}"
    );
    assert_eq!(generation(&status, "licznik"), Some(1));
    assert!(count_of(&s.chat("ile"), "v1").is_some());
    s.stop();
    drop(crashed.fake);
    Ok(())
}

/// PL: Część procesu potomnego dla `segfault_brings_back_last_good_generation`: bez zmiennej
/// `HOT_SEGFAULT_HOME` nic nie robi. Wczytuje mod, który po 3 tickach pisze pod adres 8.
/// EN: The child-process half of `segfault_brings_back_last_good_generation`: without the
/// `HOT_SEGFAULT_HOME` variable it does nothing. Loads a mod that writes to address 8 after 3 ticks.
#[test]
fn segfault_child() -> TestResult {
    let Some(home) = std::env::var_os("HOT_SEGFAULT_HOME") else {
        return Ok(());
    };
    let mut s = Server::start(Path::new(&home))?;
    s.load_and_wait(&mod_file("licznik_segfault")?, |st| {
        version(st, "licznik").as_deref() == Some("sf")
    })?;
    for _ in 0..100 {
        s.tick();
    }
    Err("the segfault mod did not crash the process".into())
}

/// PL: M7.1: prawdziwy segfault w modzie gorącym kładzie proces; następny start bierze ostatnią
/// dobrą generację. EN: M7.1: a real segfault in a hot mod kills the process; the next start takes
/// the last good generation.
#[cfg(unix)]
#[test]
fn segfault_brings_back_last_good_generation() -> TestResult {
    use std::os::unix::process::ExitStatusExt;
    let home = Server::home("segfault");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..hot_core::GOOD_AFTER_TICKS {
        s.tick();
    }
    s.stop();
    let out = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "segfault_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HOT_SEGFAULT_HOME", &home)
        .output()?;
    assert_eq!(
        out.status.signal(),
        Some(11),
        "the child did not die from SIGSEGV: {:?}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout)
    );
    let s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(
        version(&status, "licznik").as_deref(),
        Some("v1"),
        "{status:#}"
    );
    assert!(status["safe_mode"].is_null(), "{status:#}");
    s.stop();
    Ok(())
}

/// PL: M7.1: druga awaria z rzędu krótko po starcie → mody śpią (HOT-E0408), serwer działa; czysty
/// restart je przywraca. EN: M7.1: a second crash in a row soon after start → hot mods sleep
/// (HOT-E0408), the server runs; a clean restart brings them back.
#[test]
fn second_crash_in_a_row_puts_hot_mods_to_sleep() -> TestResult {
    let home = Server::home("crash-loop");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..hot_core::GOOD_AFTER_TICKS {
        s.tick();
    }
    s.load_and_wait(&mod_file("licznik_v2")?, |st| {
        generation(st, "licznik") == Some(2)
    })?;
    // PL: Awarie: nowy start bez `deactivate`. EN: Crashes: a new start without `deactivate`.
    let first = s;
    let s = Server::start(&home)?;
    assert_eq!(version(&s.status()?, "licznik").as_deref(), Some("v1"));
    let second = s;
    let mut s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(
        status["mods"].as_array().map(Vec::len),
        Some(0),
        "{status:#}"
    );
    assert_eq!(status["safe_mode"]["dormant"][0], "licznik", "{status:#}");
    assert_eq!(
        status["recent_errors"][0]["code"], "HOT-E0408",
        "{status:#}"
    );
    let (_, list) = s.console("hot list");
    assert!(
        list.iter().any(|l| l.contains("Tryb bezpieczny")),
        "{list:?}"
    );
    s.stop();
    let mut s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(
        version(&status, "licznik").as_deref(),
        Some("v1"),
        "{status:#}"
    );
    assert!(status["safe_mode"].is_null(), "{status:#}");
    assert!(count_of(&s.chat("ile"), "v1").is_some());
    s.stop();
    drop((first.fake, second.fake));
    Ok(())
}

/// PL: `hot load` i `hot unload` kończą tryb bezpieczny dla jednego moda.
/// EN: `hot load` and `hot unload` end safe mode for one mod.
#[test]
fn load_and_unload_end_safe_mode_for_a_mod() -> TestResult {
    let home = Server::home("safe-mode-exit");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    s.load_and_wait(&mod_file("gadula")?, |st| {
        generation(st, "gadula") == Some(1)
    })?;
    for _ in 0..hot_core::GOOD_AFTER_TICKS {
        s.tick();
    }
    let first = s;
    let second = Server::start(&home)?;
    let mut s = Server::start(&home)?;
    assert_eq!(
        s.status()?["safe_mode"]["dormant"].as_array().map(Vec::len),
        Some(2)
    );
    s.load_and_wait(&mod_file("licznik_v2")?, |st| {
        version(st, "licznik").as_deref() == Some("v2")
    })?;
    let (_, out) = s.console("hot unload gadula");
    assert!(
        out.iter().any(|l| l.contains("śpi") || l.contains("spał")),
        "{out:?}"
    );
    let status = s.status()?;
    assert!(status["safe_mode"].is_null(), "{status:#}");
    s.stop();
    let s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(
        version(&status, "licznik").as_deref(),
        Some("v2"),
        "{status:#}"
    );
    assert_eq!(generation(&status, "gadula"), None, "{status:#}");
    s.stop();
    drop((first.fake, second.fake));
    Ok(())
}

#[test]
fn broken_mod_list_starts_empty_and_is_kept_aside() -> TestResult {
    let home = Server::home("broken");
    fs::create_dir_all(home.join("hot-state"))?;
    fs::write(home.join("hot-state/mods.json"), "{\"clean_stop\": tr")?;
    let s = Server::start(&home)?;
    let status = s.status()?;
    assert_eq!(
        status["recent_errors"][0]["code"], "HOT-E0404",
        "{status:#}"
    );
    let aside = fs::read_dir(home.join("hot-state"))?
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("broken-"));
    assert!(aside, "the broken file was not kept");
    s.stop();
    Ok(())
}
