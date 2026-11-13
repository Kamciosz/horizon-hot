//! PL: „Graj” i „Napraw” (D26) jako czysta sekwencja kroków nad cechą [`World`]: launcher,
//! wersja zgodna z serwerem, logowanie raz, `prepare`, instancja, pula slotów, lista modów,
//! `servers.dat`, `play --server`. Prawdziwy świat jest w `real.rs`, testy podstawiają fałszywy.
//! EN: "Play" and "Repair" (D26) as a pure sequence of steps over the [`World`] trait: launcher,
//! a version matching the server, sign-in once, `prepare`, instance, slot pool, mod list,
//! `servers.dat`, `play --server`. The real world is in `real.rs`, tests use a fake one.

use std::path::{Path, PathBuf};

use hot_diag::DiagError;
use sha1::{Digest, Sha1};

use crate::codes;
use crate::servers::servers_dat;

/// PL: Nazwa instancji launchera, którą `hot-play` zakłada i której pilnuje. Innych nie rusza.
/// EN: The launcher instance `hot-play` creates and looks after. It touches no other.
pub const INSTANCE: &str = "horizon-hot";

/// PL: Linia włączająca pulę slotów w `horizon-mods.txt`. EN: The line enabling the slot pool in
/// `horizon-mods.txt`.
pub const MOD_LINE: &str = "mod.hot_slots:true";

/// PL: Co serwer mówi o sobie (`/hot-play.json` z `hotd serve-pack`). EN: What the server says
/// about itself (`/hot-play.json` from `hotd serve-pack`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// PL: Pełny hash rewizji Horizona silnika. EN: The engine's full Horizon revision hash.
    pub horizon: Option<String>,
    /// PL: SHA-1 `hot_slots.hmod`. EN: The SHA-1 of `hot_slots.hmod`.
    pub slots_sha1: Option<String>,
}

/// PL: Wersje Horizona: do pobrania i zainstalowane. EN: Horizon versions: downloadable and
/// installed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Versions {
    pub available: Vec<String>,
    pub installed: Vec<String>,
}

/// PL: Instancja launchera. EN: A launcher instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub id: String,
    /// PL: Folder gry instancji (`servers.dat`, `horizon-mods/`). EN: The instance's game folder.
    pub home: PathBuf,
    /// PL: Przypięta wersja (`None` = najnowsza). EN: The pinned version (`None` = the newest).
    pub version: Option<String>,
}

/// PL: Ustawienia gracza. EN: The player's settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// PL: Adres gry, np. `serwer.pl:25565`. EN: The game address, e.g. `serwer.pl:25565`.
    pub server: String,
    /// PL: Nazwa na liście serwerów. EN: The name on the server list.
    pub name: String,
}

/// PL: Co `hot-play` już zrobił na tym komputerze (`state.json`). EN: What `hot-play` has already
/// done on this computer (`state.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// PL: Wersje, dla których `prepare` się udał. EN: Versions `prepare` succeeded for.
    pub prepared: Vec<String>,
}

/// PL: Wszystko, czego „Graj” potrzebuje od komputera, launchera i serwera.
/// EN: Everything "Play" needs from the computer, the launcher and the server.
pub trait World {
    /// PL: Launcher jest i działa; brak = instalacja. EN: The launcher is there and runs; missing =
    /// install it.
    fn ensure_launcher(&mut self, repair: bool) -> Result<(), DiagError>;
    fn server_info(&mut self) -> Result<ServerInfo, DiagError>;
    fn versions(&mut self) -> Result<Versions, DiagError>;
    fn install_version(&mut self, tag: &str) -> Result<(), DiagError>;
    /// PL: Jest konto Microsoft w launcherze. EN: There is a Microsoft account in the launcher.
    fn signed_in(&mut self) -> Result<bool, DiagError>;
    /// PL: Logowanie kodem urządzenia; gracz wpisuje kod w przeglądarce. EN: Device-code sign-in;
    /// the player types the code in a browser.
    fn sign_in(&mut self) -> Result<(), DiagError>;
    fn prepare(&mut self, tag: &str) -> Result<(), DiagError>;
    fn instance(&mut self, name: &str) -> Result<Option<Instance>, DiagError>;
    fn create_instance(&mut self, name: &str) -> Result<Instance, DiagError>;
    fn pin_version(&mut self, id: &str, tag: &str) -> Result<(), DiagError>;
    fn read(&mut self, path: &Path) -> Option<Vec<u8>>;
    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), DiagError>;
    fn download_slots(&mut self) -> Result<Vec<u8>, DiagError>;
    fn play(&mut self, id: &str, server: &str) -> Result<(), DiagError>;
    /// PL: Krótki komunikat dla gracza. EN: A short message for the player.
    fn say(&mut self, line: &str);
}

/// PL: Wersja do gry: nightly z tym samym skrótem rewizji co serwer (najpierw zainstalowana),
/// inaczej najnowszy nightly. Drugie pole: `true`, gdy to wersja serwera.
/// EN: The version to play: the nightly with the server's short revision (installed first),
/// otherwise the newest nightly. The second field: `true` when it is the server's version.
#[must_use]
pub fn pick_version(rev: Option<&str>, versions: &Versions) -> Option<(String, bool)> {
    let nightly = |t: &&String| t.starts_with("nightly-");
    if let Some(short) = rev.and_then(|r| r.get(..7)) {
        let exact = versions
            .installed
            .iter()
            .chain(&versions.available)
            .find(|t| nightly(t) && t.ends_with(short));
        if let Some(tag) = exact {
            return Some((tag.clone(), true));
        }
    }
    let newest = |list: &Vec<String>| list.iter().filter(nightly).max().cloned();
    newest(&versions.available)
        .or_else(|| newest(&versions.installed))
        .map(|tag| (tag, false))
}

/// PL: `horizon-mods.txt` z włączoną pulą; inne linie zostają. `None` = plik już jest dobry.
/// EN: `horizon-mods.txt` with the pool enabled; other lines stay. `None` = the file is fine.
#[must_use]
pub fn mods_txt(current: Option<&str>) -> Option<String> {
    let text = current.unwrap_or("");
    if text.lines().any(|l| l.trim() == MOD_LINE) {
        return None;
    }
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|l| l.trim().split(':').next() != Some("mod.hot_slots"))
        .collect();
    lines.push(MOD_LINE);
    Some(lines.join("\n") + "\n")
}

/// PL: SHA-1 jako 40 znaków hex. EN: SHA-1 as 40 hex characters.
#[must_use]
pub fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// PL: Sprawdza i naprawia wszystko, potem uruchamia grę. `repair` („Napraw”) robi `prepare` i
/// pobiera pulę od nowa nawet wtedy, gdy wyglądają dobrze. `play` = `false` kończy przed grą.
/// EN: Checks and repairs everything, then starts the game. `repair` ("Repair") redoes `prepare`
/// and downloads the pool again even when they look fine. `play` = `false` stops before the game.
pub fn graj(
    world: &mut impl World,
    setup: &Setup,
    state: &mut State,
    repair: bool,
    play: bool,
) -> Result<(), DiagError> {
    world.say("Sprawdzam launcher Horizona…");
    world.ensure_launcher(repair)?;

    world.say("Pytam serwer o wersję…");
    let info = world.server_info()?;

    let versions = world.versions()?;
    let Some((tag, exact)) = pick_version(info.horizon.as_deref(), &versions) else {
        return Err(DiagError::new(
            codes::VERSION,
            "no Horizon nightly is available or installed",
            "check the network; the launcher lists releases from GitHub",
        )
        .context("server_rev", info.horizon.clone().unwrap_or_default()));
    };
    if !exact {
        world.say(&format!(
            "Wersji serwera nie da się już pobrać; gram na najnowszej ({tag})."
        ));
    }
    if !versions.installed.contains(&tag) {
        world.say(&format!("Instaluję Horizon {tag}…"));
        world.install_version(&tag)?;
    }

    if !world.signed_in()? {
        world.say("Pierwsze uruchomienie: zaloguj się kontem Microsoft z kupionym Minecraftem.");
        world.sign_in()?;
        if !world.signed_in()? {
            return Err(DiagError::new(
                codes::ACCOUNT,
                "no account after sign-in",
                "sign in with the account that owns Minecraft: Java Edition",
            ));
        }
    }

    if repair || !state.prepared.contains(&tag) {
        world.say("Przygotowuję pliki gry z Twojego Minecrafta…");
        world.prepare(&tag)?;
        if !state.prepared.contains(&tag) {
            state.prepared.push(tag.clone());
        }
    }

    let inst = match world.instance(INSTANCE)? {
        Some(inst) => inst,
        None => {
            world.say("Zakładam osobny folder gry dla serwera…");
            world.create_instance(INSTANCE)?
        }
    };
    if inst.version.as_deref() != Some(tag.as_str()) {
        world.pin_version(&inst.id, &tag)?;
    }

    slots(world, &inst, &info, repair)?;

    let mods = inst.home.join("horizon-mods.txt");
    let current = world
        .read(&mods)
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(text) = mods_txt(current.as_deref()) {
        world.write(&mods, text.as_bytes())?;
    }

    let dat = inst.home.join("servers.dat");
    let want = servers_dat(&setup.name, &setup.server);
    if world.read(&dat).as_deref() != Some(want.as_slice()) {
        world.write(&dat, &want)?;
    }

    if play {
        world.say(&format!("Uruchamiam grę i łączę z {}…", setup.server));
        world.play(&inst.id, &setup.server)?;
    }
    Ok(())
}

fn slots(
    world: &mut impl World,
    inst: &Instance,
    info: &ServerInfo,
    repair: bool,
) -> Result<(), DiagError> {
    let Some(want) = info.slots_sha1.as_deref() else {
        return Err(DiagError::new(
            codes::SLOTS,
            "the server has no slot pool (hot_slots.hmod)",
            "the server owner runs the installer again (step 9)",
        ));
    };
    let path = inst.home.join("horizon-mods").join("hot_slots.hmod");
    let have = world.read(&path).map(|b| sha1_hex(&b));
    if !repair && have.as_deref() == Some(want) {
        return Ok(());
    }
    world.say("Pobieram bloki serwera…");
    let bytes = world.download_slots()?;
    let got = sha1_hex(&bytes);
    if got != want {
        return Err(DiagError::new(
            codes::SLOTS,
            "the downloaded slot pool has a different SHA-1 than the server announced",
            "try again; if it repeats, the server changed the pool during the download or the network alters files",
        )
        .context("expected", want)
        .context("got", got));
    }
    world.write(&path, &bytes)
}
