//! PL: Nazwy bloków modów (M5.1). Mod mówi o swoim bloku `<id>:<nazwa>`, a silnik zna go jako
//! `hot_slots:slot_N` z puli. Przydział zapisuje `hotd` w `hot-state/slots.json`; `hot-core` go
//! tylko czyta: przy starcie i po `hot slots` (wysyła ją `hotd` po każdej zmianie przydziału).
//! Tłumaczenie działa w `block`, `set_block`, `give` i w polu `block` zdarzeń.
//!
//! EN: Mod block names (M5.1). A mod calls its block `<id>:<name>` while the engine knows it as
//! `hot_slots:slot_N` from the pool. `hotd` writes the assignment to `hot-state/slots.json`;
//! `hot-core` only reads it, at start and after `hot slots` (sent by `hotd` after every
//! assignment change). Names are translated in `block`, `set_block`, `give` and in events' `block`.

use std::collections::BTreeMap;
use std::path::Path;

use hot_diag::DiagError;
use serde_json::Value;

use crate::codes;

/// PL: Przestrzeń nazw puli. EN: The pool namespace.
const NAMESPACE: &str = "hot_slots";

/// PL: Nazwy w obie strony. EN: Names both ways.
#[derive(Debug, Default)]
pub struct SlotNames {
    to_engine: BTreeMap<String, String>,
    to_mod: BTreeMap<String, String>,
}

impl SlotNames {
    /// PL: Odczyt `slots.json`; brak pliku to brak bloków modów.
    /// EN: Reads `slots.json`; a missing file means no mod blocks.
    ///
    /// # Errors
    /// PL: Plik jest, ale nie da się go odczytać. EN: The file exists but cannot be read.
    pub fn read(path: &Path) -> Result<Self, DiagError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(read_error(path, &e.to_string())),
        };
        let value: Value =
            serde_json::from_str(&text).map_err(|e| read_error(path, &e.to_string()))?;
        let mut names = Self::default();
        for (name, slot) in value["blocks"].as_object().into_iter().flatten() {
            if let Some(slot) = slot.as_u64() {
                names.insert(name, &format!("{NAMESPACE}:slot_{slot}"));
            }
        }
        Ok(names)
    }

    fn insert(&mut self, name: &str, engine: &str) {
        self.to_engine.insert(name.to_owned(), engine.to_owned());
        self.to_mod.insert(engine.to_owned(), name.to_owned());
    }

    /// PL: Liczba bloków modów. EN: The number of mod blocks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.to_engine.len()
    }

    /// PL: Nazwa dla silnika; inne nazwy bez zmian. EN: The engine's name; other names unchanged.
    #[must_use]
    pub fn to_engine<'a>(&'a self, name: &'a str) -> &'a str {
        self.to_engine.get(name).map_or(name, String::as_str)
    }

    /// PL: Nazwa dla moda; inne nazwy bez zmian. EN: The mod's name; other names unchanged.
    #[must_use]
    pub fn to_mod<'a>(&'a self, name: &'a str) -> &'a str {
        self.to_mod.get(name).map_or(name, String::as_str)
    }
}

fn read_error(path: &Path, reason: &str) -> DiagError {
    DiagError::new(
        codes::STATE_FILE,
        format!("hot-core cannot read {}: {reason}", path.display()),
        "hotd writes this file; run hotd once, or restore hot-state/slots.json from the latest backup",
    )
    .context("path", path.display().to_string())
    .state("Mods see their blocks under the engine names hot_slots:slot_N until the file is readable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_go_both_ways_and_others_pass() {
        let dir = std::env::temp_dir().join(format!("hot-core-slots-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("slots.json");
        assert_eq!(SlotNames::read(&file).map(|s| s.len()).ok(), Some(0));
        let _ = std::fs::write(&file, r#"{"blocks": {"sklep:lada": 3}}"#);
        let s = SlotNames::read(&file).unwrap_or_default();
        assert_eq!(s.to_engine("sklep:lada"), "hot_slots:slot_3");
        assert_eq!(s.to_mod("hot_slots:slot_3"), "sklep:lada");
        assert_eq!(s.to_engine("minecraft:stone"), "minecraft:stone");
        let _ = std::fs::write(&file, "{");
        assert!(SlotNames::read(&file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
