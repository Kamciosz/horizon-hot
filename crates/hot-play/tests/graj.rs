//! PL: „Graj” i „Napraw” (D26, M11) na fałszywym świecie: co robi pierwszy raz, co przy
//! kolejnym, co naprawia i kiedy nie puszcza gracza do gry.
//! EN: "Play" and "Repair" (D26, M11) on a fake world: what the first run does, what later runs
//! do, what gets repaired and when the player is kept out of the game.
#![allow(clippy::unwrap_used)] // PL: testy. EN: tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hot_diag::DiagError;
use hot_play::codes;
use hot_play::game::{graj, sha1_hex, Instance, ServerInfo, Setup, State, Versions, World};
use hot_play::servers::servers_dat;

const REV: &str = "66f3981f522b0000000000000000000000000000";
const TAG: &str = "nightly-2026-11-05-66f3981";
const SLOTS: &[u8] = b"slots v1";

struct Fake {
    launcher: bool,
    info: ServerInfo,
    versions: Versions,
    accounts: bool,
    sign_in_works: bool,
    instance: Option<Instance>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    download: Vec<u8>,
    calls: Vec<String>,
    said: Vec<String>,
}

impl Fake {
    fn new() -> Self {
        Self {
            launcher: false,
            info: ServerInfo {
                horizon: Some(REV.into()),
                slots_sha1: Some(sha1_hex(SLOTS)),
            },
            versions: Versions {
                available: vec![TAG.into(), "nightly-2026-11-13-9f42064".into()],
                installed: vec![],
            },
            accounts: false,
            sign_in_works: true,
            instance: None,
            files: BTreeMap::new(),
            download: SLOTS.to_vec(),
            calls: Vec::new(),
            said: Vec::new(),
        }
    }

    fn did(&self, prefix: &str) -> bool {
        self.calls.iter().any(|c| c.starts_with(prefix))
    }

    fn file(&self, name: &str) -> Option<&Vec<u8>> {
        self.files.get(&home().join(name))
    }
}

fn home() -> PathBuf {
    PathBuf::from("/game/horizon-hot")
}

fn ok() -> Result<(), DiagError> {
    Ok(())
}

impl World for Fake {
    fn ensure_launcher(&mut self, _: bool) -> Result<(), DiagError> {
        if !self.launcher {
            self.calls.push("install launcher".into());
            self.launcher = true;
        }
        ok()
    }
    fn server_info(&mut self) -> Result<ServerInfo, DiagError> {
        Ok(self.info.clone())
    }
    fn versions(&mut self) -> Result<Versions, DiagError> {
        Ok(self.versions.clone())
    }
    fn install_version(&mut self, tag: &str) -> Result<(), DiagError> {
        self.calls.push(format!("install {tag}"));
        self.versions.installed.push(tag.into());
        ok()
    }
    fn signed_in(&mut self) -> Result<bool, DiagError> {
        Ok(self.accounts)
    }
    fn sign_in(&mut self) -> Result<(), DiagError> {
        self.calls.push("sign-in".into());
        self.accounts = self.sign_in_works;
        ok()
    }
    fn prepare(&mut self, tag: &str) -> Result<(), DiagError> {
        self.calls.push(format!("prepare {tag}"));
        ok()
    }
    fn instance(&mut self, _: &str) -> Result<Option<Instance>, DiagError> {
        Ok(self.instance.clone())
    }
    fn create_instance(&mut self, name: &str) -> Result<Instance, DiagError> {
        self.calls.push(format!("create {name}"));
        let inst = Instance {
            id: "i1".into(),
            home: home(),
            version: None,
        };
        self.instance = Some(inst.clone());
        Ok(inst)
    }
    fn pin_version(&mut self, id: &str, tag: &str) -> Result<(), DiagError> {
        self.calls.push(format!("pin {id} {tag}"));
        if let Some(i) = &mut self.instance {
            i.version = Some(tag.into());
        }
        ok()
    }
    fn read(&mut self, path: &Path) -> Option<Vec<u8>> {
        self.files.get(path).cloned()
    }
    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), DiagError> {
        self.calls.push(format!("write {}", path.display()));
        self.files.insert(path.to_path_buf(), bytes.to_vec());
        ok()
    }
    fn download_slots(&mut self) -> Result<Vec<u8>, DiagError> {
        self.calls.push("download".into());
        Ok(self.download.clone())
    }
    fn play(&mut self, id: &str, server: &str) -> Result<(), DiagError> {
        self.calls.push(format!("play {id} {server}"));
        ok()
    }
    fn say(&mut self, line: &str) {
        self.said.push(line.into());
    }
}

fn setup() -> Setup {
    Setup {
        server: "gra.example.pl:25565".into(),
        name: "Hot".into(),
    }
}

fn play(world: &mut Fake, state: &mut State) -> Result<(), DiagError> {
    graj(world, &setup(), state, false, true)
}

#[test]
fn the_first_run_sets_everything_up_and_joins() {
    let mut w = Fake::new();
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    for call in [
        "install launcher",
        "install nightly-2026-11-05-66f3981",
        "sign-in",
        "prepare nightly-2026-11-05-66f3981",
        "create horizon-hot",
        "pin i1 nightly-2026-11-05-66f3981",
        "download",
        "play i1 gra.example.pl:25565",
    ] {
        assert!(w.did(call), "{call}: {:?}", w.calls);
    }
    assert_eq!(w.file("horizon-mods/hot_slots.hmod").unwrap(), SLOTS);
    assert_eq!(w.file("horizon-mods.txt").unwrap(), b"mod.hot_slots:true\n");
    assert_eq!(
        w.file("servers.dat").unwrap(),
        &servers_dat("Hot", "gra.example.pl:25565")
    );
    assert_eq!(s.prepared, vec![TAG.to_owned()]);
}

#[test]
fn a_second_run_only_plays() {
    let mut w = Fake::new();
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    w.calls.clear();
    play(&mut w, &mut s).unwrap();
    assert_eq!(w.calls, ["play i1 gra.example.pl:25565"]);
}

#[test]
fn a_damaged_slot_pool_is_downloaded_again() {
    let mut w = Fake::new();
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    w.files.insert(
        home().join("horizon-mods/hot_slots.hmod"),
        b"broken".to_vec(),
    );
    w.calls.clear();
    play(&mut w, &mut s).unwrap();
    assert!(w.did("download"));
    assert_eq!(w.file("horizon-mods/hot_slots.hmod").unwrap(), SLOTS);
}

#[test]
fn a_download_with_the_wrong_checksum_keeps_the_player_out() {
    let mut w = Fake::new();
    w.download = b"tampered".to_vec();
    let mut s = State::default();
    let e = play(&mut w, &mut s).unwrap_err();
    assert_eq!(e.code(), codes::SLOTS);
    assert!(w.file("horizon-mods/hot_slots.hmod").is_none());
    assert!(!w.did("play"));
}

#[test]
fn a_server_version_that_is_gone_falls_back_to_the_newest_nightly() {
    let mut w = Fake::new();
    w.versions.available = vec![
        "nightly-2026-11-08-fdc87ae".into(),
        "nightly-2026-11-13-9f42064".into(),
        "v0.2.99".into(),
    ];
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    assert!(w.did("install nightly-2026-11-13-9f42064"), "{:?}", w.calls);
    assert!(
        w.said.iter().any(|l| l.contains("najnowszej")),
        "{:?}",
        w.said
    );
}

#[test]
fn an_installed_server_version_is_used_without_the_network_list() {
    let mut w = Fake::new();
    w.versions.available.clear();
    w.versions.installed = vec![TAG.into()];
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    assert!(!w.did("install nightly"));
    assert!(w.did("pin i1 nightly-2026-11-05-66f3981"));
}

#[test]
fn repair_prepares_and_downloads_again_without_playing() {
    let mut w = Fake::new();
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    w.calls.clear();
    graj(&mut w, &setup(), &mut s, true, false).unwrap();
    assert!(w.did("prepare") && w.did("download"));
    assert!(!w.did("play") && !w.did("sign-in"));
}

#[test]
fn a_failed_sign_in_stops_before_the_game() {
    let mut w = Fake::new();
    w.sign_in_works = false;
    let mut s = State::default();
    let e = play(&mut w, &mut s).unwrap_err();
    assert_eq!(e.code(), codes::ACCOUNT);
    assert!(!w.did("prepare") && !w.did("play"));
}

#[test]
fn the_mod_list_keeps_other_mods() {
    let mut w = Fake::new();
    w.instance = Some(Instance {
        id: "i1".into(),
        home: home(),
        version: Some(TAG.into()),
    });
    w.files.insert(
        home().join("horizon-mods.txt"),
        b"mod.drony:true\nmod.hot_slots:false\n".to_vec(),
    );
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    assert_eq!(
        w.file("horizon-mods.txt").unwrap(),
        b"mod.drony:true\nmod.hot_slots:true\n"
    );
    assert!(!w.did("create") && !w.did("pin"));
}

#[test]
fn a_server_without_a_slot_pool_is_reported() {
    let mut w = Fake::new();
    w.info.slots_sha1 = None;
    let mut s = State::default();
    let e = play(&mut w, &mut s).unwrap_err();
    assert_eq!(e.code(), codes::SLOTS);
    assert!(!w.did("play"));
}

#[test]
fn a_server_without_a_version_says_so_and_plays_the_newest() {
    let mut w = Fake::new();
    w.info.horizon = None;
    let mut s = State::default();
    play(&mut w, &mut s).unwrap();
    assert!(w.did("install nightly-2026-11-13-9f42064"), "{:?}", w.calls);
    assert!(
        w.said.iter().any(|l| l.contains("nie podał swojej wersji")),
        "{:?}",
        w.said
    );
}
