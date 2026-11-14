//! PL: Konsola, `hot status`, pełny dysk, ciepły silnik przy podmianie i liczenie starych generacji
//! do wymiany pamięci. Warsztat: `common/mod.rs`.
//!
//! EN: Console, `hot status`, a full disk, a warm engine during the swap and counting old
//! generations for the memory swap. Harness: `common/mod.rs`.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hot_abi::flow;
use serde_json::Value;

mod common;

use common::*;

#[test]
fn console_commands_list_and_pass_through() -> TestResult {
    let home = Server::home("console");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    let (flow, replies) = s.console("hot list");
    assert_eq!(flow, flow::CANCEL);
    assert!(
        replies.iter().any(|r| r.contains("licznik v1")),
        "{replies:?}"
    );
    let (flow, _) = s.console("say cześć");
    assert_eq!(flow, flow::CONTINUE);
    let (_, replies) = s.console("hot load /nie/ma/takiego.so");
    assert!(
        replies.iter().any(|r| r.contains("Nie ma pliku")),
        "{replies:?}"
    );
    let (flow, replies) = s.console("hot licznik ile");
    assert_eq!(flow, flow::CANCEL);
    assert!(replies.iter().any(|r| r.starts_with("v1:")), "{replies:?}");
    let (_, replies) = s.console("hot licznik cokolwiek");
    assert!(
        replies.iter().any(|r| r.contains("nie obsłużył")),
        "{replies:?}"
    );
    let (_, replies) = s.console("hot unload licznik");
    assert!(
        replies.iter().any(|r| r.contains("wyłączony")),
        "{replies:?}"
    );
    assert!(home.join("hot-state/saved/licznik.bin").exists());
    s.stop();
    Ok(())
}

#[test]
fn hot_status_shows_the_state_of_hotd() -> TestResult {
    // PL: `/hot status` w grze pokazuje też stan `hotd` z `hot-state/hotd.json` (D21).
    // EN: `/hot status` in game also shows the `hotd` state from `hot-state/hotd.json` (D21).
    let home = Server::home("hotd-status");
    let mut s = Server::start(&home)?;
    let (_, replies) = s.console("hot status");
    assert!(
        replies.iter().any(|r| r.contains("hotd: brak stanu")),
        "{replies:?}"
    );
    fs::write(
        home.join("hot-state/hotd.json"),
        r#"{"head":"b64df969dc","applied":"21e1c94aa","waiting":null,
            "failed":[{"commit":"b64df969dc","reason":"CI nie zbudowało moda licznik"}]}"#,
    )?;
    let (_, replies) = s.console("hot status");
    assert!(
        replies
            .iter()
            .any(|r| r == "hotd: repo b64df96, zastosowany 21e1c94"),
        "{replies:?}"
    );
    assert!(
        replies
            .iter()
            .any(|r| r.contains("commit b64df96 nie wszedł: CI nie zbudowało moda licznik")),
        "{replies:?}"
    );
    s.stop();
    Ok(())
}

/// PL: M7.2 na prawdziwie pełnym dysku. Działa tylko ze zmienną `HOT_FULL_DISK_DIR`: katalog na
/// małym, osobnym woluminie (np. obraz `hdiutil` 64 MB), nigdy na dysku systemu. Start `hot-core`
/// bez miejsca na zapis stanu i logów nie może się skończyć błędem ani paniką, mody mają działać,
/// a `mods.json` ma się dalej parsować.
/// EN: M7.2 on a really full disk. Runs only with `HOT_FULL_DISK_DIR`: a folder on a small, separate
/// volume (e.g. a 64 MB `hdiutil` image), never the system disk. Starting `hot-core` with no room
/// for state and logs must not fail or panic, mods must run and `mods.json` must still parse.
#[test]
fn full_disk_start_keeps_hot_mods_running() -> TestResult {
    use std::io::Write;
    let Some(dir) = std::env::var_os("HOT_FULL_DISK_DIR") else {
        return Ok(());
    };
    let dir = PathBuf::from(dir);
    let (home, filler) = (dir.join("core-home"), dir.join("filler-core"));
    let _ = fs::remove_file(&filler);
    let _ = fs::remove_dir_all(&home);
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..hot_core::GOOD_AFTER_TICKS {
        s.tick();
    }
    s.stop();

    // PL: APFS oddaje miejsce po usuniętych plikach z opóźnieniem: pisz, aż przestanie przybywać.
    // EN: APFS frees the space of removed files with a delay: write until no more appears.
    let mut f = fs::File::create(&filler)?;
    let chunk = vec![0u8; 1 << 20];
    loop {
        let len = f.metadata()?.len();
        while f.write_all(&chunk).is_ok() {}
        while f.write_all(&chunk[..4096]).is_ok() {}
        let _ = f.sync_all();
        std::thread::sleep(Duration::from_millis(500));
        if f.metadata()?.len() == len {
            break;
        }
    }
    drop(f);
    let before = fs::read(home.join("hot-state/mods.json"))?;
    assert!(
        fs::write(home.join("hot-state/probe"), vec![1u8; 1 << 16]).is_err(),
        "the disk is not full"
    );
    let mut s = Server::start(&home)?;
    for _ in 0..10 {
        s.tick();
    }
    let (_, replies) = s.console("hot list");
    assert!(
        replies.iter().any(|r| r.contains("licznik v1")),
        "{replies:?}"
    );
    s.stop();
    let after = fs::read(home.join("hot-state/mods.json"))?;
    let mods: Value = serde_json::from_slice(&after)?;
    assert_eq!(mods["mods"][0]["id"], "licznik", "{mods:#}");
    assert_eq!(after, before, "mods.json was written on a full disk");

    fs::remove_file(&filler)?;
    let s = Server::start(&home)?;
    assert_eq!(version(&s.status()?, "licznik").as_deref(), Some("v1"));
    s.stop();
    Ok(())
}

/// PL: Podmiana silnika (D11): rozgrzany silnik startuje, gdy stary jeszcze trzyma świat i jego
/// `mods.json` mówi `clean_stop = false`. Nowy rdzeń nie może tego uznać za awarię (wróciłaby
/// starsza generacja, a po dwóch podmianach tryb bezpieczny), ani nic zapisać, gdy podmiana
/// zostanie przerwana. EN: Engine swap (D11): the warm engine starts while the old one still
/// holds the world and its `mods.json` says `clean_stop = false`. The new core must not take that
/// for a crash, nor write anything when the swap is aborted.
#[test]
fn a_warm_engine_reads_hot_state_only_after_the_old_one_stops() -> TestResult {
    let home = Server::home("warm-swap");
    let mut old = Server::start(&home)?;
    old.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    old.load_and_wait(&mod_file("licznik_v2")?, |st| {
        generation(st, "licznik") == Some(2)
    })?;
    let before = count_of(&old.chat("ile"), "v2").ok_or("v2 did not answer")?;
    // PL: Stary silnik trzyma świat. EN: The old engine holds the world.
    fs::create_dir_all(home.join("world"))?;
    let lock = fs::File::create(home.join("world/session.lock"))?;
    lock.try_lock()?;
    assert!(hot_core::world_in_use(&home));
    let mods = fs::read(home.join("hot-state/mods.json"))?;
    // PL: Przerwana podmiana: nowy silnik staje, zanim zajmie świat. EN: An aborted swap.
    let aborted = Server::start(&home)?;
    aborted.stop();
    assert_eq!(fs::read(home.join("hot-state/mods.json"))?, mods);
    let mut new = Server::start(&home)?;
    assert_eq!(fs::read(home.join("hot-state/mods.json"))?, mods);
    old.stop();
    lock.unlock()?;
    drop(lock);
    assert!(!hot_core::world_in_use(&home));
    // PL: Pierwsze zdarzenie po zajęciu świata wczytuje stan. EN: The first event reads the state.
    let after = count_of(&new.chat("ile"), "v2").ok_or("v2 did not come back")?;
    assert_eq!(after, before);
    let status = new.status()?;
    assert_eq!(generation(&status, "licznik"), Some(2), "{status:#}");
    assert!(status["safe_mode"].is_null(), "{status:#}");
    new.stop();
    Ok(())
}

fn libraries_file(home: &Path) -> Result<serde_json::Map<String, Value>, Box<dyn Error>> {
    let v: Value =
        serde_json::from_str(&fs::read_to_string(home.join("hot-state/libraries.json"))?)?;
    assert_eq!(v["pid"].as_u64(), Some(u64::from(std::process::id())));
    Ok(v["files"].as_object().cloned().unwrap_or_default())
}

/// PL: M8.5: `hot-core` podaje, ile bajtów to nieużywane generacje, z bibliotekami poprzednich
/// kopii `hot-core` w tym samym procesie, ale bez tych z poprzedniego silnika (inny pid).
/// EN: M8.5: `hot-core` reports how many bytes are unused generations, with the libraries of
/// earlier `hot-core` copies in the same process, but not those of an earlier engine (another pid).
#[test]
fn old_generations_are_counted_for_the_memory_swap() -> TestResult {
    let home = Server::home("libraries");
    fs::create_dir_all(home.join("hot-state/gen"))?;
    let earlier_copy = home.join("hot-state/gen/earlier-core-copy.dylib");
    let key = earlier_copy.display().to_string();
    fs::write(
        home.join("hot-state/libraries.json"),
        serde_json::json!({ "pid": std::process::id(), "files": { key.clone(): 1000 } })
            .to_string(),
    )?;
    let mut s = Server::start(&home)?;
    let files = [mod_file("licznik_v1")?, mod_file("licznik_v2")?];
    for n in 1..=5u64 {
        s.load_and_wait(&files[(n % 2) as usize], |st| {
            generation(st, "licznik") == Some(n)
        })?;
    }
    let status = s.status()?;
    let lib = &status["libraries"];
    assert_eq!(lib["pid"].as_u64(), Some(u64::from(std::process::id())));
    let listed = libraries_file(&home)?;
    assert!(listed.contains_key(&key), "{listed:?}");
    let gen_dir = home.join("hot-state/gen").display().to_string();
    let ours: Vec<&String> = listed.keys().filter(|f| f.starts_with(&gen_dir)).collect();
    assert_eq!(ours.len(), 6, "{ours:?}");
    // PL: W użyciu: generacja 5 i 4 (powrót po panice); nieużywane: 1–3 i kopia rdzenia.
    // EN: In use: generations 5 and 4 (the fallback); unused: 1–3 and the core copy.
    let smallest = files
        .iter()
        .map(|f| fs::metadata(f).map(|m| m.len()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .min()
        .unwrap_or(0);
    let stale = lib["stale_bytes"].as_u64().unwrap_or(0);
    let bytes = lib["bytes"].as_u64().unwrap_or(0);
    assert!(stale >= 1000 + 3 * smallest, "{lib:#}");
    assert!(bytes - stale >= 2 * smallest, "{lib:#}");
    s.stop();

    let other = Server::home("libraries-earlier-engine");
    fs::create_dir_all(other.join("hot-state"))?;
    let theirs = other.join("hot-state/gen/earlier-engine.dylib");
    fs::write(
        other.join("hot-state/libraries.json"),
        serde_json::json!({
            "pid": u64::from(std::process::id()) + 1,
            "files": { theirs.display().to_string(): 1000 },
        })
        .to_string(),
    )?;
    let mut s = Server::start(&other)?;
    s.load_and_wait(&files[0], |st| generation(st, "licznik") == Some(1))?;
    let listed = libraries_file(&other)?;
    assert!(
        !listed.contains_key(&theirs.display().to_string()),
        "{listed:?}"
    );
    s.stop();
    Ok(())
}
