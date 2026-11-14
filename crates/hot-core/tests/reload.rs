//! PL: Przeładowania bez silnika (M1): zmiana kodu przy zachowanym stanie, panika → poprzednia
//! generacja, zła wersja ABI, wiadomości i komendy modów, bloki modów, 50 przeładowań z rzędu.
//! Warsztat: `common/mod.rs`.
//!
//! EN: Reloads without the engine (M1): new code with the state kept, panic → previous generation,
//! a wrong ABI version, mod messages and commands, mod blocks, 50 reloads in a row.
//! Harness: `common/mod.rs`.

use std::fs;
use std::time::{Duration, Instant};

use hot_abi::{flow, kind, Event, Str};

mod common;

use common::*;

#[test]
fn reload_switches_code_and_keeps_state() -> TestResult {
    let home = Server::home("reload");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..20 {
        s.tick();
    }
    let before = count_of(&s.chat("ile"), "v1").ok_or("v1 did not answer")?;
    s.load_and_wait(&mod_file("licznik_v2")?, |st| {
        generation(st, "licznik") == Some(2)
    })?;
    let after = count_of(&s.chat("ile"), "v2").ok_or("v2 did not answer")?;
    assert!(
        after >= before,
        "the counter went back: {before} -> {after}"
    );
    s.stop();
    Ok(())
}

#[test]
fn panic_in_tick_falls_back_to_previous_generation() -> TestResult {
    let home = Server::home("panic-tick");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..5 {
        s.tick();
    }
    s.load_and_wait(&mod_file("licznik_panika_tick")?, |st| {
        version(st, "licznik").as_deref() == Some("v1")
            && st["recent_errors"]
                .as_array()
                .is_some_and(|e| !e.is_empty())
    })?;
    let ticks = count_of(&s.chat("ile"), "v1").ok_or("v1 is not back")?;
    assert!(ticks >= 5, "state lost on fallback: {ticks}");
    let entry = log_entries(&home)
        .into_iter()
        .find(|e| e["code"] == "HOT-E0403")
        .ok_or("no HOT-E0403 entry")?;
    assert!(
        entry["cause"].as_str().unwrap_or("").contains("tick test"),
        "{entry:#}"
    );
    assert!(
        entry["source"].as_str().unwrap_or("").contains("lib.rs"),
        "{entry:#}"
    );
    assert!(
        entry["what"].as_str().unwrap_or("").contains("on_event"),
        "{entry:#}"
    );
    s.stop();
    Ok(())
}

#[test]
fn panic_in_init_keeps_running_generation() -> TestResult {
    let home = Server::home("panic-init");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    s.load_and_wait(&mod_file("licznik_panika_init")?, |st| {
        st["recent_errors"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    })?;
    let status = s.status()?;
    assert_eq!(
        version(&status, "licznik").as_deref(),
        Some("v1"),
        "{status:#}"
    );
    assert!(count_of(&s.chat("ile"), "v1").is_some());
    let entry = log_entries(&home)
        .into_iter()
        .find(|e| e["code"] == "HOT-E0403")
        .ok_or("no HOT-E0403 entry")?;
    assert!(
        entry["what"].as_str().unwrap_or("").contains("init"),
        "{entry:#}"
    );
    s.stop();
    Ok(())
}

#[test]
fn first_generation_panic_turns_the_mod_off() -> TestResult {
    let home = Server::home("panic-first");
    let mut s = Server::start(&home)?;
    let status = s.load_and_wait(&mod_file("licznik_panika_tick")?, |st| {
        st["recent_errors"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    })?;
    assert_eq!(generation(&status, "licznik"), None, "{status:#}");
    assert!(home.join("hot-state/saved/licznik.bin").exists());
    // PL: Serwer działa dalej: tick i czat bez paniki. EN: The server keeps going.
    s.tick();
    assert!(s.chat("ile").is_empty());
    s.stop();
    Ok(())
}

#[test]
fn unload_keeps_the_state_for_the_next_load() -> TestResult {
    let home = Server::home("unload-load");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("licznik_v1")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    for _ in 0..5 {
        s.tick();
    }
    let before = count_of(&s.console("licznik ile").1, "v1").ok_or("no v1 count")?;
    s.console("hot unload licznik");
    s.load_and_wait(&mod_file("licznik_v2")?, |st| {
        generation(st, "licznik") == Some(1)
    })?;
    let after = count_of(&s.console("licznik ile").1, "v2").ok_or("no v2 count")?;
    assert!(after >= before, "the state came back: {before} -> {after}");
    assert!(!home.join("hot-state/saved/licznik.bin").exists());
    s.stop();
    Ok(())
}

#[test]
fn wrong_abi_version_is_refused() -> TestResult {
    let home = Server::home("abi");
    let mut s = Server::start(&home)?;
    let status = s.load_and_wait(&mod_file("zle_abi")?, |st| {
        st["recent_errors"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    })?;
    assert_eq!(
        status["recent_errors"][0]["code"], "HOT-E0402",
        "{status:#}"
    );
    assert!(status["mods"].as_array().is_some_and(Vec::is_empty));
    s.stop();
    Ok(())
}

#[test]
fn mod_messages_reach_the_log_with_mod_id_and_data_dir_works() -> TestResult {
    let home = Server::home("gadula");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("gadula")?, |st| {
        generation(st, "gadula") == Some(1)
    })?;
    s.tick();
    assert_eq!(
        fs::read_to_string(home.join("hot-data/gadula/ok.txt"))?,
        "zapis"
    );
    let entries = log_entries(&home);
    let error = entries
        .iter()
        .find(|e| e["code"] == "HOT-E0405")
        .ok_or("no HOT-E0405 entry")?;
    assert_eq!(error["context"]["mod"], "gadula");
    assert!(entries
        .iter()
        .any(|e| e["level"] == "info" && e["what"] == "gadula started"));
    assert!(s
        .fake
        .logs
        .iter()
        .any(|(_, m)| m == "[gadula] database file is missing"));
    s.stop();
    Ok(())
}

#[test]
fn commands_from_player_events_run_on_the_next_tick() -> TestResult {
    let home = Server::home("kolejka");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("kolejka")?, |st| {
        generation(st, "kolejka") == Some(1)
    })?;
    s.fake.commands.clear();
    s.chat("hej");
    assert!(s.fake.commands.is_empty(), "{:?}", s.fake.commands);
    s.tick();
    assert_eq!(
        s.fake.commands,
        [
            "say hej".to_owned(),
            r#"tellraw Gracz {"text":"Masz \"cytat\""}"#.to_owned()
        ]
    );
    s.tick();
    assert_eq!(s.fake.commands.len(), 2, "the queue runs once");
    s.stop();
    Ok(())
}

#[test]
fn mod_blocks_go_by_their_own_names() -> TestResult {
    let home = Server::home("bloki");
    fs::create_dir_all(home.join("hot-state"))?;
    fs::write(
        home.join("hot-state/slots.json"),
        r#"{"blocks": {"bloki:lada": 3}}"#,
    )?;
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("bloki")?, |st| generation(st, "bloki") == Some(1))?;
    s.fake.commands.clear();
    assert_eq!(s.chat("postaw"), ["widzę bloki:lada".to_owned()]);
    assert_eq!(
        s.fake.commands,
        [
            "set_block hot_slots:slot_3".to_owned(),
            "give hot_slots:slot_3".to_owned()
        ]
    );
    s.fake.replies.clear();
    let mut e = Event::new(kind::DESTROY_BLOCK);
    e.player = [7; 16];
    e.block = Str::from_str("hot_slots:slot_3");
    s.send(&e);
    assert_eq!(s.fake.replies, ["niszczę bloki:lada".to_owned()]);

    // PL: Nowy przydział wchodzi po `hot slots`. EN: A new assignment applies after `hot slots`.
    fs::write(home.join("hot-state/slots.json"), r#"{"blocks": {}}"#)?;
    let (_, replies) = s.console("hot slots");
    assert_eq!(
        replies,
        ["Bloki modów: 0 (hot-state/slots.json).".to_owned()]
    );
    s.fake.commands.clear();
    s.chat("postaw");
    assert_eq!(s.fake.commands[0], "set_block bloki:lada");
    s.stop();
    Ok(())
}

#[test]
fn declared_commands_wait_for_the_players_tick() -> TestResult {
    let home = Server::home("odlozone");
    let mut s = Server::start(&home)?;
    s.load_and_wait(&mod_file("kolejka")?, |st| {
        generation(st, "kolejka") == Some(1)
    })?;
    s.fake.commands.clear();
    s.fake.replies.clear();
    let flow = s.player_event(kind::PLAYER_COMMAND, 7, "kolejka raz");
    assert_eq!(
        flow,
        flow::CANCEL,
        "a declared command is cancelled at once"
    );
    assert_eq!(
        s.player_event(kind::PLAYER_COMMAND, 7, "inna"),
        flow::CONTINUE,
        "an undeclared command goes on to the engine"
    );
    s.player_event(kind::PLAYER_TICK, 8, "");
    s.tick();
    assert!(s.fake.commands.is_empty(), "{:?}", s.fake.commands);
    assert!(s.fake.replies.is_empty(), "{:?}", s.fake.replies);

    s.player_event(kind::PLAYER_TICK, 7, "");
    s.player_event(kind::PLAYER_TICK, 7, "");
    s.tick();
    assert_eq!(
        s.fake.commands,
        [r#"tellraw Gracz {"text":"ok kolejka raz"}"#.to_owned()],
        "the mod runs once, in the player's tick, and its reply goes through tell"
    );
    assert!(s.fake.replies.is_empty(), "{:?}", s.fake.replies);

    s.fake.commands.clear();
    s.player_event(kind::PLAYER_COMMAND, 7, "kolejka dwa");
    s.player_event(kind::PLAYER_LEFT, 7, "");
    s.player_event(kind::PLAYER_TICK, 7, "");
    s.tick();
    assert!(
        s.fake.commands.is_empty(),
        "leaving drops the queue: {:?}",
        s.fake.commands
    );

    // PL: Komenda bez deklaracji obsłużona w haku: jedno ostrzeżenie z poprawką.
    // EN: An undeclared command handled in the hook: one warning with the fix.
    assert_eq!(
        s.player_event(kind::PLAYER_COMMAND, 7, "kolejkax"),
        flow::CANCEL
    );
    s.player_event(kind::PLAYER_COMMAND, 7, "kolejkax");
    let warned = log_entries(&home)
        .iter()
        .filter(|e| {
            e["what"]
                .as_str()
                .is_some_and(|w| w.contains(r#"commands = "kolejkax""#))
        })
        .count();
    assert_eq!(warned, 1, "one warning per mod and command");
    s.stop();
    Ok(())
}

#[test]
fn fifty_reloads_in_a_row_keep_the_counter() -> TestResult {
    let home = Server::home("fifty");
    let mut s = Server::start(&home)?;
    let files = [mod_file("licznik_v1")?, mod_file("licznik_v2")?];
    let mut last = 0;
    for n in 1..=50u64 {
        let file = &files[(n % 2) as usize];
        s.load_and_wait(file, |st| generation(st, "licznik") == Some(n))?;
        let tag = if n % 2 == 0 { "v1" } else { "v2" };
        let now = count_of(&s.chat("ile"), tag).ok_or("no answer")?;
        assert!(
            now >= last,
            "counter went back at reload {n}: {last} -> {now}"
        );
        last = now;
    }
    s.stop();
    Ok(())
}

#[test]
fn a_paused_server_switches_on_a_console_command() -> TestResult {
    // PL: Pusty serwer stoi w pauzie bez ticków; `hot status` z konsoli domyka wczytanie.
    // EN: An empty server is paused with no ticks; `hot status` on the console completes a load.
    let home = Server::home("paused");
    let mut s = Server::start(&home)?;
    let (flow, _) = s.console(&format!("hot load {}", mod_file("licznik_v1")?.display()));
    assert_eq!(flow, flow::CANCEL);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (_, replies) = s.console("hot status");
        if replies.iter().any(|r| r.contains("licznik v1")) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no switch without ticks: {replies:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    s.stop();
    Ok(())
}
