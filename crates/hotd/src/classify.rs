//! PL: Klasyfikacja zmian w repo serwera (D24) i plan działań. Czysta logika, bez gita i plików:
//! wejście to lista zmienionych ścieżek, mody z kodem w nowym commicie i mody aktywne w silniku.
//! Repo jest źródłem prawdy: mod aktywny w silniku, którego nie ma w repo, zostaje wyłączony.
//!
//! EN: Classifying changes in the server repository (D24) and the action plan. Pure logic, no git
//! and no files: the input is the changed paths, the code mods in the new commit and the mods
//! active in the engine. The repository is the source of truth: an active mod missing from the
//! repository is unloaded.

use std::collections::BTreeSet;

/// PL: Co zmieniło się między commitami. EN: What changed between commits.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Change {
    /// PL: Mody, których kod trzeba zbudować od nowa. EN: Mods whose code needs a new build.
    pub code: BTreeSet<String>,
    /// PL: Zmiana wspólna dla wszystkich modów (`Cargo.lock` w korzeniu itp.).
    /// EN: A change shared by every mod (the root `Cargo.lock` and so on).
    pub all_code: bool,
    /// PL: Zmiana w `native/` (podmiana silnika, M8). EN: A change in `native/` (engine swap, M8).
    pub native: bool,
    /// PL: Zmiana w `hot.toml` (M6). EN: A change in `hot.toml` (M6).
    pub config: bool,
    /// PL: Katalogi w `mods/` z niedozwoloną nazwą. EN: Folders in `mods/` with a forbidden name.
    pub bad_names: BTreeSet<String>,
}

/// PL: Plan dla silnika i to, czego `hotd` jeszcze nie umie. EN: The engine plan and what `hotd`
/// cannot do yet.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    pub load: BTreeSet<String>,
    pub unload: BTreeSet<String>,
    /// PL: Zmiany odłożone, po polsku, do `hot status`. EN: Deferred changes, in Polish.
    pub deferred: Vec<String>,
}

/// PL: Pliki w korzeniu repo, które wpływają na budowę każdego moda.
/// EN: Root files that affect every mod's build.
const SHARED_BUILD_FILES: [&str; 4] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rust-toolchain",
];

/// PL: Id moda trafia do ścieżek i komend, więc tylko te znaki (jak w `hot-core`).
/// EN: The id ends up in paths and commands, so only these characters (as in `hot-core`).
#[must_use]
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// PL: Sortuje zmienione ścieżki według tabeli D24. EN: Sorts changed paths by the D24 table.
#[must_use]
pub fn classify<S: AsRef<str>>(paths: &[S]) -> Change {
    let mut c = Change::default();
    for path in paths {
        let path = path.as_ref();
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["mods", id, rest @ ..] if !rest.is_empty() => {
                if !valid_id(id) {
                    c.bad_names.insert((*id).to_owned());
                    continue;
                }
                let set = match rest[0] {
                    // PL: Paczki danych porównuje `data.rs` ze światem, a paczkę zasobów `pack.rs`
                    // po sha1, nie z listą zmian. EN: `data.rs` compares data packs with the world
                    // and `pack.rs` the resource pack by sha1, not with the change list.
                    "data" | "assets" => continue,
                    // PL: Wszystko inne w katalogu moda (src/, Cargo.toml, mod.toml, build.rs)
                    // wpływa na kod. EN: Everything else in the mod folder affects the code.
                    _ => &mut c.code,
                };
                set.insert((*id).to_owned());
            }
            ["native", ..] => c.native = true,
            ["hot.toml"] => c.config = true,
            [".cargo", ..] => c.all_code = true,
            [file] if SHARED_BUILD_FILES.contains(file) => c.all_code = true,
            _ => {}
        }
    }
    c
}

/// PL: Id modów z kodem w commicie: katalogi `mods/<id>/` z `Cargo.toml`.
/// EN: Code mod ids in a commit: `mods/<id>/` folders with a `Cargo.toml`.
#[must_use]
pub fn code_mods<S: AsRef<str>>(tree: &[S]) -> BTreeSet<String> {
    tree.iter()
        .filter_map(
            |p| match p.as_ref().split('/').collect::<Vec<_>>().as_slice() {
                ["mods", id, "Cargo.toml"] if valid_id(id) => Some((*id).to_owned()),
                _ => None,
            },
        )
        .collect()
}

/// PL: Plan: ładuje mody ze zmienionym kodem i te, których silnik nie ma; wyłącza te, których
/// nie ma w repo.
/// EN: The plan: loads mods with changed code and the ones the engine lacks; unloads the ones
/// absent from the repository.
#[must_use]
pub fn plan(change: &Change, in_repo: &BTreeSet<String>, active: &BTreeSet<String>) -> Plan {
    let load = in_repo
        .iter()
        .filter(|id| change.all_code || change.code.contains(*id) || !active.contains(*id))
        .cloned()
        .collect();
    let unload = active.difference(in_repo).cloned().collect();
    let mut deferred = Vec::new();
    let list = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
    if change.native {
        deferred.push("zmiana native/ — podmiana silnika dojdzie w M8".to_owned());
    }
    if change.config {
        deferred.push("zmiana hot.toml — odczyt ustawień dojdzie w M6".to_owned());
    }
    if !change.bad_names.is_empty() {
        deferred.push(format!(
            "pominięte katalogi w mods/ (dozwolone a-z, 0-9, _ i -): {}",
            list(&change.bad_names)
        ));
    }
    Plan {
        load,
        unload,
        deferred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn source_change_reloads_only_that_mod() {
        let c = classify(&["mods/sklep/src/lib.rs", "README.md"]);
        assert_eq!(c.code, set(&["sklep"]));
        let p = plan(&c, &set(&["sklep", "licznik"]), &set(&["sklep", "licznik"]));
        assert_eq!(p.load, set(&["sklep"]));
        assert!(p.unload.is_empty() && p.deferred.is_empty());
    }

    #[test]
    fn manifest_and_build_script_count_as_code() {
        let c = classify(&["mods/a/Cargo.toml", "mods/b/mod.toml", "mods/c/build.rs"]);
        assert_eq!(c.code, set(&["a", "b", "c"]));
    }

    #[test]
    fn root_lock_file_reloads_every_mod() {
        let c = classify(&["Cargo.lock"]);
        assert!(c.all_code);
        let p = plan(&c, &set(&["a", "b"]), &set(&["a", "b"]));
        assert_eq!(p.load, set(&["a", "b"]));
    }

    #[test]
    fn repo_without_mods_unloads_everything() {
        let c = classify(&["mods/a/src/lib.rs"]);
        let p = plan(&c, &BTreeSet::new(), &set(&["a", "b"]));
        assert!(p.load.is_empty());
        assert_eq!(p.unload, set(&["a", "b"]));
    }

    #[test]
    fn mod_missing_in_engine_is_loaded_without_a_change() {
        let p = plan(&classify::<&str>(&[]), &set(&["a"]), &BTreeSet::new());
        assert_eq!(p.load, set(&["a"]));
    }

    #[test]
    fn readme_only_change_does_nothing() {
        let c = classify(&["README.md", "docs/x.md", ".github/workflows/hot.yml"]);
        assert_eq!(c, Change::default());
        let p = plan(&c, &set(&["a"]), &set(&["a"]));
        assert_eq!(p, Plan::default());
    }

    #[test]
    fn native_and_config_are_deferred_and_data_and_assets_are_not_code() {
        let c = classify(&[
            "mods/a/data/recipe.json",
            "mods/a/assets/model.json",
            "native/x.hnmod",
            "hot.toml",
        ]);
        assert!(c.code.is_empty());
        let p = plan(&c, &set(&["a"]), &set(&["a"]));
        assert!(p.load.is_empty());
        assert_eq!(p.deferred.len(), 2, "{:?}", p.deferred);
    }

    #[test]
    fn forbidden_folder_names_are_skipped() {
        let c = classify(&["mods/Zły Mod/src/lib.rs", "mods/../x"]);
        assert!(c.code.is_empty());
        assert_eq!(c.bad_names, set(&["Zły Mod", ".."]));
        assert_eq!(
            code_mods(&[
                "mods/ok/Cargo.toml",
                "mods/Bad/Cargo.toml",
                "mods/ok/src/lib.rs"
            ]),
            set(&["ok"])
        );
    }
}
