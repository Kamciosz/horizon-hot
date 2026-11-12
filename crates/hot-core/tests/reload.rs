//! PL: Prawdziwe przeładowania bez silnika: test buduje małe mody (`cdylib`) w osobnym katalogu
//! docelowym, a `hot-core` dostaje udawanego gospodarza przez tę samą tabelę ABI, której używa
//! `hot-host`. Sprawdza M1: zmiana kodu przy zachowanym stanie, panika → poprzednia generacja,
//! zła wersja ABI, start po czystym zatrzymaniu i po awarii (D13), 50 przeładowań z rzędu.
//!
//! EN: Real reloads without the engine: the test builds small mods (`cdylib`) in a separate target
//! folder and `hot-core` gets a fake host through the same ABI table `hot-host` uses. Checks M1:
//! new code with the state kept, panic → previous generation, a wrong ABI version, start after a
//! clean stop and after a crash (D13), 50 reloads in a row.

use std::error::Error;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use hot_abi::{flow, kind, Event, HostApi, ModVTable, PlayerInfo, Str, Uuid};
use serde_json::Value;

type TestResult = Result<(), Box<dyn Error>>;

// ---------- PL: mody testowe / EN: test mods ----------

fn counter(tag: &str, on_init: &str, on_tick: &str) -> String {
    format!(
        r#"
use hot_sdk::{{Event, Flow, Host, HotMod}};
struct Licznik {{ ticks: u64 }}
impl HotMod for Licznik {{
    fn init(_host: &mut Host<'_>, state: &[u8]) -> Self {{
        {on_init}
        Licznik {{ ticks: hot_sdk::state::u64_from(state) }}
    }}
    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {{
        match event {{
            Event::ServerTick {{ .. }} => {{ {on_tick} self.ticks += 1; Flow::Continue }}
            Event::Chat {{ text, .. }} | Event::ConsoleCommand {{ command: text }}
                if *text == "ile" || *text == "licznik ile" => {{
                host.reply(&format!("{tag}:{{}}", self.ticks));
                Flow::Cancel
            }}
            _ => Flow::Continue,
        }}
    }}
    fn save(&mut self, _host: &mut Host<'_>) -> Vec<u8> {{ self.ticks.to_le_bytes().to_vec() }}
}}
hot_sdk::hot_mod!(Licznik, id = "licznik", version = "{tag}");
"#
    )
}

const GADULA: &str = r#"
use hot_sdk::{Event, Flow, Host, HotMod};
struct Gadula;
impl HotMod for Gadula {
    fn init(host: &mut Host<'_>, _state: &[u8]) -> Self {
        let dir = host.data_dir();
        let _ = std::fs::write(dir.join("ok.txt"), "zapis");
        host.info("gadula started");
        Gadula
    }
    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        if let Event::ServerTick { .. } = event {
            host.error("database file is missing");
        }
        Flow::Continue
    }
}
hot_sdk::hot_mod!(Gadula, id = "gadula", version = "1");
"#;

const KOLEJKA: &str = r#"
use hot_sdk::{Event, Flow, Host, HotMod};
struct Kolejka;
impl HotMod for Kolejka {
    fn init(_host: &mut Host<'_>, _state: &[u8]) -> Self {
        Kolejka
    }
    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        match event {
            Event::Chat { player, text, .. } => {
                host.run_later(&format!("say {text}"));
                host.tell(*player, "Masz \"cytat\"");
                Flow::Continue
            }
            Event::Command { command, .. } if command.starts_with("kolejka") => {
                host.reply(&format!("ok {command}"));
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }
}
hot_sdk::hot_mod!(Kolejka, id = "kolejka", version = "1", commands = "kolejka");
"#;

const BLOKI: &str = r#"
use hot_sdk::{Event, Flow, Host, HotMod};
struct Bloki;
impl HotMod for Bloki {
    fn init(_host: &mut Host<'_>, _state: &[u8]) -> Self {
        Bloki
    }
    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        match event {
            Event::Chat { player, text, .. } if *text == "postaw" => {
                host.set_block([0, 0, 0], "bloki:lada");
                host.give(*player, "bloki:lada", 1);
                let seen = host.block([3, 3, 3]);
                host.reply(&format!("widzę {seen}"));
                Flow::Cancel
            }
            Event::DestroyBlock { block, .. } => {
                host.reply(&format!("niszczę {block}"));
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }
}
hot_sdk::hot_mod!(Bloki, id = "bloki", version = "1");
"#;

const BAD_ABI: &str = r#"
use hot_abi::{Buf, HostApi, ModVTable, Str};
extern "C" fn init(_: &HostApi, _: Str) -> *mut std::ffi::c_void { std::ptr::null_mut() }
extern "C" fn on_event(_: *mut std::ffi::c_void, _: &HostApi, _: &hot_abi::Event) -> u32 { 0 }
extern "C" fn deactivate(_: *mut std::ffi::c_void, _: &HostApi) -> Buf { Buf::EMPTY }
extern "C" fn free_buf(_: Buf) {}
static TABLE: ModVTable = ModVTable {
    abi: 99, id: Str::from_str("stary"), version: Str::from_str("0"), commands: Str::from_str(""),
    init, on_event, deactivate, free_buf,
};
#[no_mangle]
pub extern "C" fn hot_mod_entry() -> *const ModVTable { &TABLE }
"#;

/// PL: Buduje wszystkie mody testowe raz na proces. EN: Builds every test mod once per process.
fn mods() -> Result<&'static Path, String> {
    static BUILT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BUILT
        .get_or_init(|| build_mods().map_err(|e| format!("building the test mods failed: {e}")))
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(Clone::clone)
}

fn build_mods() -> Result<PathBuf, Box<dyn Error>> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()?;
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("hot-core-mods");
    let sources = [
        ("licznik_v1", counter("v1", "", ""), "hot-sdk"),
        ("licznik_v2", counter("v2", "", ""), "hot-sdk"),
        (
            "licznik_panika_tick",
            counter(
                "pt",
                "",
                "if self.ticks < u64::MAX { panic!(\"tick test\"); }",
            ),
            "hot-sdk",
        ),
        (
            "licznik_panika_init",
            counter("pi", "panic!(\"init test\");", ""),
            "hot-sdk",
        ),
        ("gadula", GADULA.to_owned(), "hot-sdk"),
        ("kolejka", KOLEJKA.to_owned(), "hot-sdk"),
        ("bloki", BLOKI.to_owned(), "hot-sdk"),
        ("zle_abi", BAD_ABI.to_owned(), "hot-abi"),
    ];
    let mut members = Vec::new();
    for (name, src, dep) in &sources {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n{dep} = {{ path = {:?} }}\n",
                crates.join(dep).display().to_string()
            ),
        )?;
        fs::write(dir.join("src/lib.rs"), src)?;
        members.push(format!("{name:?}"));
    }
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
            members.join(", ")
        ),
    )?;
    let out = Command::new(env!("CARGO"))
        .args(["build", "--offline", "--quiet", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("target"))
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(root.join("target").join("debug"))
}

fn mod_file(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = mods()?;
    Ok(dir.join(format!(
        "{}{name}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )))
}

// ---------- PL: udawany gospodarz / EN: fake host ----------

#[derive(Default)]
struct Fake {
    home: String,
    logs: Vec<(u32, String)>,
    replies: Vec<String>,
    /// PL: Komendy, które dostał silnik. EN: Commands the engine got.
    commands: Vec<String>,
}

fn fake<'a>(ctx: *mut c_void) -> &'a mut Fake {
    // SAFETY: `ctx` to `Box<Fake>` z `Server`, który żyje dłużej niż wywołanie.
    // EN: `ctx` is the `Box<Fake>` owned by `Server`, which outlives the call.
    unsafe { &mut *ctx.cast::<Fake>() }
}

fn text(s: Str) -> String {
    // SAFETY: napis ważny w wywołaniu. EN: valid for the call.
    String::from_utf8_lossy(unsafe { s.bytes() }).into_owned()
}

extern "C" fn f_log(ctx: *mut c_void, level: u32, message: Str) {
    fake(ctx).logs.push((level, text(message)));
}
/// PL: Jak silnik poza hakiem komendy (D6): `tell` odmawia. EN: As the engine outside the command
/// hook (D6): `tell` refuses.
extern "C" fn f_tell(_: *mut c_void, _: &Uuid, _: Str) -> bool {
    false
}
extern "C" fn f_broadcast(_: *mut c_void, _: Str) {}
extern "C" fn f_reply(ctx: *mut c_void, message: Str) {
    fake(ctx).replies.push(text(message));
}
extern "C" fn f_run_command(ctx: *mut c_void, line: Str) -> bool {
    fake(ctx).commands.push(text(line));
    true
}
extern "C" fn f_players(
    _: *mut c_void,
    each: extern "C" fn(*mut c_void, &PlayerInfo),
    user: *mut c_void,
) {
    let gracz = PlayerInfo {
        uuid: [7; 16],
        name: Str::from_str("Gracz"),
        dimension: Str::from_str("minecraft:overworld"),
        position: [0.0; 3],
        permission: 0,
        game_mode: 0,
    };
    each(user, &gracz);
}
extern "C" fn f_set_block(ctx: *mut c_void, _: &[i32; 3], block: Str) -> bool {
    fake(ctx)
        .commands
        .push(format!("set_block {}", text(block)));
    true
}
extern "C" fn f_give(ctx: *mut c_void, _: &Uuid, item: Str, count: u32) -> u32 {
    fake(ctx).commands.push(format!("give {}", text(item)));
    count
}
extern "C" fn f_particles(
    _: *mut c_void,
    _: Str,
    _: &[f64; 3],
    _: &[f64; 3],
    _: f64,
    _: i32,
) -> bool {
    true
}
/// PL: Gospodarz nie powinien dostać `run_later`: kolejkę trzyma `hot-core`.
/// EN: The host should never get `run_later`: `hot-core` keeps the queue.
extern "C" fn f_run_later(ctx: *mut c_void, line: Str) {
    fake(ctx)
        .commands
        .push(format!("HOST GOT run_later {}", text(line)));
}
/// PL: Na `[3, 3, 3]` stoi blok ze slotu 3. EN: Slot 3's block stands at `[3, 3, 3]`.
extern "C" fn f_block(
    _: *mut c_void,
    pos: &[i32; 3],
    out: extern "C" fn(*mut c_void, Str),
    user: *mut c_void,
) {
    let name = if *pos == [3, 3, 3] {
        "hot_slots:slot_3"
    } else {
        "minecraft:air"
    };
    out(user, Str::from_str(name));
}
extern "C" fn f_data_dir(
    ctx: *mut c_void,
    out: extern "C" fn(*mut c_void, Str),
    user: *mut c_void,
) {
    let home = fake(ctx).home.clone();
    out(user, Str::from_str(&home));
}

/// PL: Silnik w miniaturze: `hot-core` przez swoją tabelę, ticki, konsola, czat.
/// EN: A miniature engine: `hot-core` through its table, ticks, console, chat.
struct Server {
    fake: Box<Fake>,
    vt: &'static ModVTable,
    core: *mut c_void,
    tick: u64,
}

impl Server {
    fn home(name: &str) -> PathBuf {
        let home = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("hot-core-homes")
            .join(name);
        let _ = fs::remove_dir_all(&home);
        home
    }

    fn start(home: &Path) -> Result<Self, Box<dyn Error>> {
        // SAFETY: tabela statyczna z `hot_mod!`. EN: a static table from `hot_mod!`.
        let vt = unsafe { &*hot_core::hot_mod_entry() };
        let mut s = Server {
            fake: Box::new(Fake {
                home: home.display().to_string(),
                ..Fake::default()
            }),
            vt,
            core: std::ptr::null_mut(),
            tick: 0,
        };
        let api = s.api();
        s.core = (vt.init)(&api, Str::EMPTY);
        if s.core.is_null() {
            return Err(format!("hot-core init failed: {:?}", s.fake.logs).into());
        }
        Ok(s)
    }

    fn api(&mut self) -> HostApi {
        HostApi {
            ctx: (&raw mut *self.fake).cast(),
            log: f_log,
            tell: f_tell,
            broadcast: f_broadcast,
            reply: f_reply,
            run_command: f_run_command,
            players: f_players,
            block: f_block,
            data_dir: f_data_dir,
            set_block: f_set_block,
            give: f_give,
            particles: f_particles,
            run_later: f_run_later,
        }
    }

    fn send(&mut self, event: &Event) -> u32 {
        let api = self.api();
        (self.vt.on_event)(self.core, &api, event)
    }

    fn console(&mut self, line: &str) -> (u32, Vec<String>) {
        self.fake.replies.clear();
        let mut e = Event::new(kind::CONSOLE_COMMAND);
        e.text = Str::from_str(line);
        let flow = self.send(&e);
        (flow, std::mem::take(&mut self.fake.replies))
    }

    fn tick(&mut self) {
        self.tick += 1;
        let mut e = Event::new(kind::SERVER_TICK);
        e.number = self.tick;
        self.send(&e);
    }

    fn chat(&mut self, message: &str) -> Vec<String> {
        self.fake.replies.clear();
        let mut e = Event::new(kind::PLAYER_CHAT);
        e.player = [7; 16];
        e.name = Str::from_str("Gracz");
        e.text = Str::from_str(message);
        self.send(&e);
        std::mem::take(&mut self.fake.replies)
    }

    fn player_event(&mut self, kind: u32, player: u8, text: &str) -> u32 {
        let mut e = Event::new(kind);
        e.player = [player; 16];
        e.name = Str::from_str("Gracz");
        e.text = Str::from_str(text);
        self.send(&e)
    }

    /// PL: `hot load` i ticki, aż `status.json` pokaże warunek. EN: `hot load`, then tick until.
    fn load_and_wait(
        &mut self,
        file: &Path,
        until: impl Fn(&Value) -> bool,
    ) -> Result<Value, Box<dyn Error>> {
        let (flow, replies) = self.console(&format!("hot load {}", file.display()));
        assert_eq!(flow, flow::CANCEL, "{replies:?}");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.tick();
            let status = self.status()?;
            if until(&status) && status["pending"].as_array().is_some_and(Vec::is_empty) {
                return Ok(status);
            }
            if Instant::now() > deadline {
                return Err(format!("timeout; status: {status:#}").into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn status(&self) -> Result<Value, Box<dyn Error>> {
        let path = Path::new(&self.fake.home).join("hot-state/status.json");
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    fn stop(mut self) {
        let api = self.api();
        let buf = (self.vt.deactivate)(self.core, &api);
        (self.vt.free_buf)(buf);
    }
}

fn generation(status: &Value, id: &str) -> Option<u64> {
    status["mods"].as_array()?.iter().find(|m| m["id"] == id)?["generation"].as_u64()
}

fn version(status: &Value, id: &str) -> Option<String> {
    status["mods"]
        .as_array()?
        .iter()
        .find(|m| m["id"] == id)
        .and_then(|m| m["version"].as_str().map(str::to_owned))
}

fn log_entries(home: &Path) -> Vec<Value> {
    fs::read_to_string(home.join("logs/hot-core.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn count_of(replies: &[String], tag: &str) -> Option<u64> {
    replies
        .iter()
        .find_map(|r| r.strip_prefix(&format!("{tag}:"))?.parse().ok())
}

// ---------- PL: testy / EN: tests ----------

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
