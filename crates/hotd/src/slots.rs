//! PL: Pula slotów (D8, M5.1). Bloki modów gorących nie mogą dopisać się do rejestrów gry w
//! trakcie działania, więc `hot-slots.hmod` ma stałą pulę pustych bloków i przedmiotów, a mody
//! tylko je zajmują. Ten moduł:
//! - generuje źródło `hot-slots.hmod` (`hotd slots-source`), z rozmiarem puli w stałych niżej;
//! - czyta z drzewa commita bloki zadeklarowane przez mody (`mods/<id>/assets/blocks/<nazwa>.png`);
//! - przydziela im sloty w `hot-state/slots.json`. Przydział jest tylko dopisywany: slot usuniętego
//!   moda zostaje zarezerwowany, a mod, który wraca, dostaje swój dawny slot.
//!
//! `hotd` jest jedynym zapisującym `slots.json`; `hot-core` tylko go czyta.
//!
//! EN: The slot pool (D8, M5.1). Hot mod blocks cannot join the game registries while it runs,
//! so `hot-slots.hmod` holds a fixed pool of empty blocks and items that mods only occupy. This
//! module generates the `hot-slots.hmod` source (`hotd slots-source`) with the pool size in the
//! constants below, reads the blocks mods declare in a commit tree
//! (`mods/<id>/assets/blocks/<name>.png`) and assigns them slots in `hot-state/slots.json`. The
//! assignment only grows: a removed mod's slot stays reserved and a returning mod gets its old
//! slot back. `hotd` is the only writer of `slots.json`; `hot-core` only reads it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use hot_diag::DiagError;
use serde_json::{json, Value};

use crate::classify::valid_id;
use crate::codes;

/// PL: Bloki w puli. Tymczasowo 1024: największa pula, przy której podmiana silnika zmieści się
/// w 1 s, wyjdzie z M0.19 (PLAN, sekcja 8). Zmiana rozmiaru zmienia wersję puli (`version`), więc
/// każdy gracz potrzebuje nowego pliku.
/// EN: Blocks in the pool. 1024 for now: M0.19 decides the largest pool that keeps the engine swap
/// under 1 s. A new size changes the pool version, so every player needs the new file.
pub const BLOCKS: u32 = 1024;

/// PL: Przedmioty w puli. EN: Items in the pool.
pub const ITEMS: u32 = 2048;

/// PL: Wersja układu plików w slocie; podbijana, gdy zmienia się treść slotu przy tym samym
/// rozmiarze. EN: The slot file layout version; bumped when a slot's content changes at the same
/// size.
const LAYOUT: u32 = 0;

/// PL: Przestrzeń nazw puli w grze. EN: The pool's namespace in the game.
pub const NAMESPACE: &str = "hot_slots";

/// PL: Wersja `hot-slots`: serwer porównuje ją u gracza przy wejściu (S2).
/// EN: The `hot-slots` version: the server compares it with the player's at join (S2).
#[must_use]
pub fn version() -> String {
    format!("{BLOCKS}.{ITEMS}.{LAYOUT}")
}

/// PL: Nazwa bloku w grze dla slotu. EN: The in-game block name of a slot.
#[must_use]
pub fn block_id(slot: u32) -> String {
    format!("{NAMESPACE}:slot_{slot}")
}

/// PL: Bloki zadeklarowane w drzewie commita, jako `<id>:<nazwa>`.
/// EN: The blocks declared in a commit tree, as `<id>:<name>`.
#[must_use]
pub fn declared_blocks<S: AsRef<str>>(tree: &[S]) -> BTreeSet<String> {
    tree.iter()
        .filter_map(
            |p| match p.as_ref().split('/').collect::<Vec<_>>().as_slice() {
                ["mods", id, "assets", "blocks", file] => {
                    let name = file.strip_suffix(".png")?;
                    (valid_id(id) && valid_id(name)).then(|| format!("{id}:{name}"))
                }
                _ => None,
            },
        )
        .collect()
}

/// PL: Przydział slotów: `<id>:<nazwa>` → numer slotu. EN: Slot assignment: `<id>:<name>` → slot.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Slots {
    pub blocks: BTreeMap<String, u32>,
}

impl Slots {
    /// PL: Odczyt `slots.json`; brak pliku to pusty przydział. EN: Reads `slots.json`; a missing
    /// file is an empty assignment.
    ///
    /// # Errors
    /// PL: Plik jest, ale nie da się go odczytać — nie wolno wtedy przydzielać od zera, bo sloty
    /// zmieniłyby właściciela. EN: The file exists but cannot be read; starting from zero would
    /// hand slots to new owners.
    pub fn read(path: &Path) -> Result<Self, DiagError> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(state_error(path, &e.to_string())),
        };
        let value: Value =
            serde_json::from_str(&text).map_err(|e| state_error(path, &e.to_string()))?;
        let mut blocks = BTreeMap::new();
        for (name, slot) in value["blocks"].as_object().into_iter().flatten() {
            let slot = slot
                .as_u64()
                .and_then(|s| u32::try_from(s).ok())
                .ok_or_else(|| state_error(path, &format!("slot of {name} is not a number")))?;
            blocks.insert(name.clone(), slot);
        }
        Ok(Self { blocks })
    }

    /// PL: Zapis przez plik tymczasowy, żeby przerwany zapis nie zostawił połowy pliku.
    /// EN: Writes through a temporary file so an interrupted write leaves no half file.
    ///
    /// # Errors
    /// PL: Zapis się nie udał. EN: The write failed.
    pub fn write(&self, path: &Path) -> Result<(), DiagError> {
        let text = serde_json::to_string_pretty(&json!({
            "pool": { "version": version(), "blocks": BLOCKS, "items": ITEMS },
            "blocks": self.blocks,
        }))
        .map_err(|e| state_error(path, &e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text + "\n")
            .and_then(|()| fs::rename(&tmp, path))
            .map_err(|e| state_error(path, &e.to_string()))
    }

    /// PL: Dopisuje sloty dla nowych bloków; zwraca te, które dostały slot teraz. Usunięte bloki
    /// zostają w przydziale, a nowe dostają następny wolny numer, nigdy zwolniony.
    /// EN: Adds slots for new blocks and returns those assigned now. Removed blocks stay in the
    /// assignment and new ones get the next free number, never a freed one.
    ///
    /// # Errors
    /// PL: Pula się skończyła; przydział zostaje bez zmian. EN: The pool ran out; the assignment
    /// stays unchanged.
    pub fn assign(&mut self, declared: &BTreeSet<String>) -> Result<Vec<String>, DiagError> {
        let new: Vec<String> = declared
            .iter()
            .filter(|b| !self.blocks.contains_key(*b))
            .cloned()
            .collect();
        let next = self.blocks.values().max().map_or(0, |m| m + 1);
        let free = BLOCKS.saturating_sub(next);
        if new.len() > free as usize {
            return Err(DiagError::new(
                codes::POOL_FULL,
                format!(
                    "The slot pool has {free} free blocks left, the commit declares {} new ones",
                    new.len()
                ),
                "Remove blocks from mods/*/assets/blocks/ in the server repository, or raise hotd::slots::BLOCKS (every player then needs the new hot-slots file, docs/sloty.md)",
            )
            .expected(format!("at most {BLOCKS} blocks in all"))
            .context("new", new.join(" "))
            .state("Nothing changed: the old look stays and no block got a slot"));
        }
        for (i, name) in (next..).zip(&new) {
            self.blocks.insert(name.clone(), i);
        }
        Ok(new)
    }
}

fn state_error(path: &Path, reason: &str) -> DiagError {
    DiagError::new(
        codes::STATE_FILE,
        format!("hotd cannot use {}: {reason}", path.display()),
        "Restore hot-state/slots.json from the latest backup (hot-state is in every backup); never delete it, or blocks change owners",
    )
    .context("file", path.display().to_string())
    .state("Slots and the resource pack stay as they were")
}

/// PL: Zapisuje źródło `hot-slots.hmod` do pustego katalogu; potem `cargo horizon-mod build`.
/// Bez `models/`, `textures/` i `lang/`: wygląd daje paczka zasobów serwera (D8, S5).
/// EN: Writes the `hot-slots.hmod` source into an empty folder, then `cargo horizon-mod build`.
/// No `models/`, `textures/` or `lang/`: the look comes from the server's resource pack.
///
/// # Errors
/// PL: Katalog istnieje albo zapis się nie udał. EN: The folder exists or a write failed.
pub fn write_pool_source(dir: &Path) -> std::io::Result<()> {
    fs::create_dir(dir)?;
    fs::write(
        dir.join("horizon-mod.toml"),
        format!(
            "id = \"{NAMESPACE}\"\nname = \"Hot Slots\"\nversion = \"{}\"\n\
             description = \"horizon-hot slot pool: {BLOCKS} blocks, {ITEMS} items. Looks come from the server's resource pack.\"\n\
             authors = [\"Szymon Sosnowski\"]\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n",
            version()
        ),
    )?;
    let (assets, data) = (
        dir.join("assets").join(NAMESPACE),
        dir.join("data").join(NAMESPACE),
    );
    let put = |path: std::path::PathBuf, v: Value| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, v.to_string())
    };
    for i in 0..BLOCKS {
        let name = format!("slot_{i}");
        let model = format!("{NAMESPACE}:block/{name}");
        let file = format!("{name}.json");
        put(
            data.join("horizon/block").join(&file),
            json!({ "template": "minecraft:stone", "properties": { "hardness": 1.5 } }),
        )?;
        put(
            data.join("loot_table/blocks").join(&file),
            json!({ "type": "minecraft:block", "pools": [{ "rolls": 1, "entries": [
                { "type": "minecraft:item", "name": format!("{NAMESPACE}:{name}") }] }] }),
        )?;
        put(
            assets.join("blockstates").join(&file),
            json!({ "variants": { "": { "model": model } } }),
        )?;
        put(
            assets.join("items").join(&file),
            json!({ "model": { "type": "minecraft:model", "model": model } }),
        )?;
    }
    for i in 0..ITEMS {
        let file = format!("item_{i}.json");
        put(
            data.join("horizon/item").join(&file),
            json!({ "template": "minecraft:stick" }),
        )?;
        put(
            assets.join("items").join(&file),
            json!({ "model": { "type": "minecraft:model", "model": format!("{NAMESPACE}:item/item_{i}") } }),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn blocks_are_declared_by_png_files() {
        let tree = [
            "mods/sklep/assets/blocks/lada.png",
            "mods/sklep/assets/blocks/Zla.png",
            "mods/sklep/assets/blocks/notatka.txt",
            "mods/sklep/assets/lang.json",
            "mods/Zly/assets/blocks/x.png",
            "mods/sklep/assets/blocks/sub/x.png",
        ];
        assert_eq!(declared_blocks(&tree), set(&["sklep:lada"]));
    }

    #[test]
    fn assignment_only_grows() {
        let mut s = Slots::default();
        assert_eq!(
            s.assign(&set(&["a:x", "b:y"])).ok(),
            Some(vec!["a:x".into(), "b:y".into()])
        );
        // PL: Mod `a` znika z repo: jego slot zostaje. EN: Mod `a` leaves: its slot stays.
        assert_eq!(s.assign(&set(&["b:y"])).ok(), Some(vec![]));
        // PL: Nowy blok nie dostaje zwolnionego slotu 0. EN: A new block does not get freed slot 0.
        assert_eq!(
            s.assign(&set(&["b:y", "c:z"])).ok(),
            Some(vec!["c:z".into()])
        );
        assert_eq!(s.blocks["c:z"], 2);
        // PL: Mod wraca i dostaje swój dawny slot. EN: The mod returns and gets its old slot.
        assert_eq!(s.assign(&set(&["a:x"])).ok(), Some(vec![]));
        assert_eq!(s.blocks["a:x"], 0);
    }

    #[test]
    fn a_full_pool_changes_nothing() {
        let mut s = Slots::default();
        s.blocks.insert("a:last".into(), BLOCKS - 1);
        let before = s.clone();
        let e = s.assign(&set(&["b:new"])).err().map(|e| e.fields());
        assert_eq!(
            e.as_ref().map(|f| f["code"].clone()),
            Some(json!("HOT-E0209"))
        );
        assert_eq!(s, before);
    }

    #[test]
    fn slots_survive_a_write_and_a_bad_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("hotd-slots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).ok();
        let file = dir.join("slots.json");
        assert_eq!(Slots::read(&file).ok(), Some(Slots::default()));
        let mut s = Slots::default();
        s.assign(&set(&["a:x", "a:y"])).ok();
        s.write(&file).ok();
        assert_eq!(Slots::read(&file).ok(), Some(s));
        fs::write(&file, "{ zepsuty").ok();
        assert!(Slots::read(&file).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pool_source_has_no_look_files() {
        let dir = std::env::temp_dir().join(format!("hotd-pool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(write_pool_source(&dir).is_ok());
        let toml = fs::read_to_string(dir.join("horizon-mod.toml")).unwrap_or_default();
        assert!(toml.contains(&format!("version = \"{}\"", version())));
        let count = |sub: &str| {
            fs::read_dir(dir.join(sub))
                .map(Iterator::count)
                .unwrap_or(0)
        };
        assert_eq!(count("data/hot_slots/horizon/block"), BLOCKS as usize);
        assert_eq!(count("data/hot_slots/horizon/item"), ITEMS as usize);
        assert_eq!(count("assets/hot_slots/items"), (BLOCKS + ITEMS) as usize);
        for look in ["models", "textures", "lang"] {
            assert!(!dir.join("assets/hot_slots").join(look).exists(), "{look}");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
