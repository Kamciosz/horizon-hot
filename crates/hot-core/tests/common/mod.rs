//! PL: Wspólny warsztat testów przeładowań: buduje małe mody testowe (`cdylib`) w osobnym katalogu
//! docelowym i daje `Server`, udawanego gospodarza z tą samą tabelą ABI, której używa `hot-host`.
//! Każdy plik testów używa tylko części tych funkcji, stąd `allow(dead_code)`.
//!
//! EN: Shared harness for the reload tests: builds small test mods (`cdylib`) in a separate target
//! folder and provides `Server`, a fake host with the same ABI table `hot-host` uses. Each test file
//! uses only part of it, hence `allow(dead_code)`.

#![allow(dead_code)]

use std::error::Error;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use hot_abi::{flow, kind, Event, HostApi, ModVTable, PlayerInfo, Str, Uuid};
use serde_json::Value;

pub type TestResult = Result<(), Box<dyn Error>>;

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
pub fn mods() -> Result<&'static Path, String> {
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
        (
            "licznik_segfault",
            counter(
                "sf",
                "",
                // PL: Prawdziwy SIGSEGV (zapis pod adres 8), którego `hot-sdk` nie złapie.
                // EN: A real SIGSEGV (a write to address 8) that `hot-sdk` cannot catch.
                "if self.ticks >= 3 { unsafe { std::ptr::without_provenance_mut::<u64>(8).write_volatile(1); } }",
            ),
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

pub fn mod_file(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = mods()?;
    Ok(dir.join(format!(
        "{}{name}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )))
}

// ---------- PL: udawany gospodarz / EN: fake host ----------

#[derive(Default)]
pub struct Fake {
    pub home: String,
    pub logs: Vec<(u32, String)>,
    pub replies: Vec<String>,
    /// PL: Komendy, które dostał silnik. EN: Commands the engine got.
    pub commands: Vec<String>,
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
pub struct Server {
    pub fake: Box<Fake>,
    pub vt: &'static ModVTable,
    pub core: *mut c_void,
    pub tick: u64,
}

impl Server {
    pub fn home(name: &str) -> PathBuf {
        let home = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("hot-core-homes")
            .join(name);
        let _ = fs::remove_dir_all(&home);
        home
    }

    pub fn start(home: &Path) -> Result<Self, Box<dyn Error>> {
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

    pub fn api(&mut self) -> HostApi {
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

    pub fn send(&mut self, event: &Event) -> u32 {
        let api = self.api();
        (self.vt.on_event)(self.core, &api, event)
    }

    pub fn console(&mut self, line: &str) -> (u32, Vec<String>) {
        self.fake.replies.clear();
        let mut e = Event::new(kind::CONSOLE_COMMAND);
        e.text = Str::from_str(line);
        let flow = self.send(&e);
        (flow, std::mem::take(&mut self.fake.replies))
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        let mut e = Event::new(kind::SERVER_TICK);
        e.number = self.tick;
        self.send(&e);
    }

    pub fn chat(&mut self, message: &str) -> Vec<String> {
        self.fake.replies.clear();
        let mut e = Event::new(kind::PLAYER_CHAT);
        e.player = [7; 16];
        e.name = Str::from_str("Gracz");
        e.text = Str::from_str(message);
        self.send(&e);
        std::mem::take(&mut self.fake.replies)
    }

    pub fn player_event(&mut self, kind: u32, player: u8, text: &str) -> u32 {
        let mut e = Event::new(kind);
        e.player = [player; 16];
        e.name = Str::from_str("Gracz");
        e.text = Str::from_str(text);
        self.send(&e)
    }

    /// PL: `hot load` i ticki, aż `status.json` pokaże warunek. EN: `hot load`, then tick until.
    pub fn load_and_wait(
        &mut self,
        file: &Path,
        until: impl Fn(&Value) -> bool,
    ) -> Result<Value, Box<dyn Error>> {
        let (flow, replies) = self.console(&format!("hot load {}", file.display()));
        assert_eq!(flow, flow::CANCEL, "{replies:?}");
        assert!(
            replies.iter().any(|r| r.starts_with("Ładuję")),
            "hot load did not start: {replies:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.tick();
            let status = self.status()?;
            if until(&status) && status["pending"].as_array().is_some_and(Vec::is_empty) {
                return Ok(status);
            }
            if Instant::now() > deadline {
                let log = log_entries(Path::new(&self.fake.home))
                    .iter()
                    .rev()
                    .take(6)
                    .map(|e| e["what"].to_string())
                    .collect::<Vec<_>>();
                return Err(format!(
                    "timeout after {replies:?}; last log entries {log:?}; status: {status:#}"
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn status(&self) -> Result<Value, Box<dyn Error>> {
        let path = Path::new(&self.fake.home).join("hot-state/status.json");
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    pub fn stop(mut self) {
        let api = self.api();
        let buf = (self.vt.deactivate)(self.core, &api);
        (self.vt.free_buf)(buf);
    }
}

pub fn generation(status: &Value, id: &str) -> Option<u64> {
    status["mods"].as_array()?.iter().find(|m| m["id"] == id)?["generation"].as_u64()
}

pub fn version(status: &Value, id: &str) -> Option<String> {
    status["mods"]
        .as_array()?
        .iter()
        .find(|m| m["id"] == id)
        .and_then(|m| m["version"].as_str().map(str::to_owned))
}

pub fn log_entries(home: &Path) -> Vec<Value> {
    fs::read_to_string(home.join("logs/hot-core.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn count_of(replies: &[String], tag: &str) -> Option<u64> {
    replies
        .iter()
        .find_map(|r| r.strip_prefix(&format!("{tag}:"))?.parse().ok())
}
