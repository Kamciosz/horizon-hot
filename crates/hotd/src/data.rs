//! PL: Paczki danych modów (D7, M3). `mods/<id>/data/` z commita trafia do
//! `<świat>/datapacks/hot-<id>/data/` z wygenerowanym `pack.mcmeta`, potem silnik dostaje jedną
//! komendę `reload` na wszystkie zmiany. Horizon włącza nową paczkę sam (nikt jej nie wyłączył
//! w `datapacks.json`), a usunięty katalog znika z silnika przy tym samym `reload`.
//!
//! Stan to sam katalog świata: każda nasza paczka ma plik `hot-tree` z hashem drzewa gita, z
//! którego powstała. Porównanie z commitem daje listę paczek do zapisu i do usunięcia, więc
//! skasowany ręcznie katalog wraca przy następnym commicie. Katalogów `hot-*` bez tego pliku
//! `hotd` nie rusza: nie są jego.
//!
//! Przygotowanie idzie do `<świat>/.hot-staging/`, poza `datapacks/`, bo Horizon włącza każdy
//! katalog z `pack.mcmeta` w `datapacks/`. Stare kopie czekają tam do potwierdzenia `reload`.
//!
//! EN: Mod data packs (D7, M3). A commit's `mods/<id>/data/` goes to
//! `<world>/datapacks/hot-<id>/data/` with a generated `pack.mcmeta`, then the engine gets one
//! `reload` for every change. Horizon enables a new pack on its own (unless `datapacks.json`
//! disabled it) and a removed folder leaves the engine on the same `reload`. The state is the
//! world folder itself: each of our packs has a `hot-tree` file with the git tree hash it came
//! from; folders named `hot-*` without it are not ours and stay untouched. Preparation goes to
//! `<world>/.hot-staging/`, outside `datapacks/`, because Horizon enables every folder with a
//! `pack.mcmeta` there. Old copies wait in staging until the `reload` is confirmed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::DiagError;

use crate::classify::valid_id;
use crate::codes;

/// PL: Plik z hashem drzewa w każdej naszej paczce. EN: The tree hash file in each of our packs.
pub const MARKER: &str = "hot-tree";

/// PL: Katalog roboczy w świecie, poza `datapacks/`. EN: The work folder in the world.
const STAGING: &str = ".hot-staging";

/// PL: Format paczki danych Horizona `nightly-2026-11-05` (`DATA_PACK_FORMAT`, gra 26.3). Górna
/// granica jest szeroka, żeby aktualizacja Horizona nie oznaczała paczki jako niezgodnej; serwer
/// i tak wczytuje każdą paczkę z `pack.mcmeta`.
/// EN: Horizon's data pack format (`DATA_PACK_FORMAT`, game 26.3). The upper bound is wide so a
/// Horizon update does not flag the pack as incompatible; the server loads any pack with a
/// `pack.mcmeta` anyway.
const PACK_FORMAT: (u32, u32) = (121, 999);

/// PL: Katalog paczki moda w świecie. EN: A mod's pack folder in the world.
#[must_use]
pub fn pack_dir(world: &Path, id: &str) -> PathBuf {
    world.join("datapacks").join(format!("hot-{id}"))
}

/// PL: Katalog świata serwera: `name` z sekcji `[world]` w `server.toml`, inaczej `world`.
/// EN: The server's world folder: `name` from the `[world]` section of `server.toml`, else `world`.
#[must_use]
pub fn world_dir(engine_dir: &Path) -> PathBuf {
    let name = fs::read_to_string(engine_dir.join("server.toml"))
        .ok()
        .and_then(|text| world_name(&text))
        .unwrap_or_else(|| "world".to_owned());
    engine_dir.join(name)
}

fn world_name(toml: &str) -> Option<String> {
    let mut in_world = false;
    for line in toml.lines().map(str::trim) {
        if line.starts_with('[') {
            in_world = line.split('#').next().map(str::trim) == Some("[world]");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !in_world || key.trim() != "name" {
            continue;
        }
        // PL: Wartość to napis w cudzysłowie; po nim może być komentarz `# …`.
        // EN: The value is a quoted string; a `# …` comment may follow it.
        let name = value.trim().strip_prefix('"')?.split('"').next()?;
        return (!name.is_empty() && !name.split('/').any(|p| p == "..")).then(|| name.to_owned());
    }
    None
}

/// PL: Id modów z plikami w `mods/<id>/data/`. EN: Ids of mods with files in `mods/<id>/data/`.
#[must_use]
pub fn data_mods<S: AsRef<str>>(tree: &[S]) -> BTreeSet<String> {
    tree.iter()
        .filter_map(
            |p| match p.as_ref().split('/').collect::<Vec<_>>().as_slice() {
                ["mods", id, "data", _, ..] if valid_id(id) => Some((*id).to_owned()),
                _ => None,
            },
        )
        .collect()
}

/// PL: Nasze paczki w świecie: id → hash drzewa z pliku `hot-tree`.
/// EN: Our packs in the world: id → tree hash from the `hot-tree` file.
#[must_use]
pub fn installed(world: &Path) -> BTreeMap<String, String> {
    let Ok(entries) = fs::read_dir(world.join("datapacks")) else {
        return BTreeMap::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let id = name.strip_prefix("hot-").filter(|id| valid_id(id))?;
            let tree = fs::read_to_string(e.path().join(MARKER)).ok()?;
            Some((id.to_owned(), tree.trim().to_owned()))
        })
        .collect()
}

/// PL: Co zmienić w świecie. EN: What to change in the world.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Diff {
    /// PL: id → hash drzewa do zapisu. EN: id → tree hash to write.
    pub write: BTreeMap<String, String>,
    pub remove: BTreeSet<String>,
}

impl Diff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.write.is_empty() && self.remove.is_empty()
    }

    fn touched(&self) -> impl Iterator<Item = &String> {
        self.write.keys().chain(&self.remove)
    }
}

/// PL: Różnica między paczkami z commita a tymi w świecie.
/// EN: The difference between the commit's packs and the world's.
#[must_use]
pub fn diff(wanted: &BTreeMap<String, String>, have: &BTreeMap<String, String>) -> Diff {
    Diff {
        write: wanted
            .iter()
            .filter(|(id, tree)| have.get(*id) != Some(*tree))
            .map(|(id, tree)| (id.clone(), tree.clone()))
            .collect(),
        remove: have
            .keys()
            .filter(|id| !wanted.contains_key(*id))
            .cloned()
            .collect(),
    }
}

/// PL: Id naszych paczek włączonych w silniku, z odpowiedzi na `datapack list`, np.
/// `There are 2 data pack(s) enabled: [vanilla (built-in)], [file/hot-sklep (world)]`.
/// EN: Ids of our packs enabled in the engine, from the `datapack list` answer.
#[must_use]
pub fn enabled_in(answer: &str) -> BTreeSet<String> {
    answer
        .lines()
        .filter(|l| l.contains(" enabled: "))
        .flat_map(|l| l.split("[file/hot-").skip(1))
        .filter_map(|rest| rest.split([' ', ']']).next())
        .filter(|id| valid_id(id))
        .map(str::to_owned)
        .collect()
}

/// PL: Przygotowane paczki i miejsce na stare kopie. EN: Prepared packs and room for old copies.
pub struct Staged {
    world: PathBuf,
    staging: PathBuf,
    diff: Diff,
}

impl Staged {
    /// PL: Wypakowuje każdą paczkę z `write` do katalogu roboczego (`extract(drzewo, cel)`),
    /// sprawdza ją i dopisuje `pack.mcmeta` oraz `hot-tree`. Świat jeszcze się nie zmienia.
    /// EN: Unpacks each pack in `write` into the work folder (`extract(tree, target)`), checks it
    /// and adds `pack.mcmeta` and `hot-tree`. The world does not change yet.
    ///
    /// # Errors
    /// PL: `HOT-E0208`: zapis się nie udał albo plik `.json` jest uszkodzony.
    /// EN: `HOT-E0208`: a write failed or a `.json` file is malformed.
    pub fn prepare(
        world: &Path,
        diff: Diff,
        mut extract: impl FnMut(&str, &Path) -> Result<(), String>,
    ) -> Result<Self, DiagError> {
        let staging = world.join(STAGING);
        let _ = fs::remove_dir_all(&staging);
        for sub in ["new", "old"] {
            let dir = staging.join(sub);
            fs::create_dir_all(&dir).map_err(|e| io_error(&dir, "create", &e))?;
        }
        for (id, tree) in &diff.write {
            let pack = staging.join("new").join(format!("hot-{id}"));
            let data = pack.join("data");
            fs::create_dir_all(&data).map_err(|e| io_error(&data, "create", &e))?;
            extract(tree, &data).map_err(|output| {
                pack_error(id, "git cannot unpack the mod's data/ folder")
                    .chain_line(output)
                    .context("tree", tree.clone())
            })?;
            check_tree(&data, &data).map_err(|(file, why)| {
                pack_error(id, &format!("mods/{id}/data/{file} {why}"))
                    .cause(why)
                    .context("file", format!("mods/{id}/data/{file}"))
            })?;
            write(&pack.join(MARKER), &format!("{tree}\n"))?;
            // PL: `pack.mcmeta` na końcu: dopiero on robi z katalogu paczkę.
            // EN: `pack.mcmeta` last: only it makes the folder a pack.
            let mcmeta = serde_json::json!({"pack": {
                "description": format!("horizon-hot: mods/{id}/data"),
                "min_format": PACK_FORMAT.0,
                "max_format": PACK_FORMAT.1,
            }});
            write(&pack.join("pack.mcmeta"), &format!("{mcmeta:#}\n"))?;
        }
        Ok(Self {
            world: world.to_owned(),
            staging,
            diff,
        })
    }

    #[must_use]
    pub fn diff(&self) -> &Diff {
        &self.diff
    }

    /// PL: Zamienia paczki w `datapacks/`; stare kopie idą do katalogu roboczego.
    /// EN: Swaps the packs in `datapacks/`; old copies go to the work folder.
    ///
    /// # Errors
    /// PL: `HOT-E0208`. EN: `HOT-E0208`.
    pub fn swap_in(&self) -> Result<(), DiagError> {
        let datapacks = self.world.join("datapacks");
        fs::create_dir_all(&datapacks).map_err(|e| io_error(&datapacks, "create", &e))?;
        for id in self.diff.touched() {
            let live = pack_dir(&self.world, id);
            if live.exists() {
                let old = self.staging.join("old").join(format!("hot-{id}"));
                fs::rename(&live, &old).map_err(|e| io_error(&live, "move", &e))?;
            }
        }
        for id in self.diff.write.keys() {
            let new = self.staging.join("new").join(format!("hot-{id}"));
            let live = pack_dir(&self.world, id);
            fs::rename(&new, &live).map_err(|e| io_error(&new, "move", &e))?;
        }
        Ok(())
    }

    /// PL: Przywraca stan sprzed `swap_in`. EN: Restores the state from before `swap_in`.
    pub fn restore(&self) {
        for id in self.diff.touched() {
            let live = pack_dir(&self.world, id);
            let old = self.staging.join("old").join(format!("hot-{id}"));
            let _ = fs::remove_dir_all(&live);
            if old.exists() {
                let _ = fs::rename(&old, &live);
            }
        }
    }

    /// PL: Usuwa katalog roboczy ze starymi kopiami. EN: Removes the work folder with old copies.
    pub fn finish(self) {
        let _ = fs::remove_dir_all(&self.staging);
    }
}

/// PL: Sprawdza wypakowane `data/`: bez dowiązań (mogłyby wskazać plik spoza paczki) i każdy
/// `.json` musi się parsować. Horizon pomija zły plik po cichu, więc łapiemy to tutaj.
/// EN: Checks the unpacked `data/`: no symlinks (they could point outside the pack) and every
/// `.json` must parse. Horizon skips a bad file silently, so it is caught here.
fn check_tree(root: &Path, dir: &Path) -> Result<(), (String, String)> {
    let rel = |p: &Path| {
        p.strip_prefix(root)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    };
    let entries = fs::read_dir(dir).map_err(|e| (rel(dir), format!("cannot be read: {e}")))?;
    for e in entries.flatten() {
        let path = e.path();
        let kind = e
            .file_type()
            .map_err(|err| (rel(&path), format!("cannot be read: {err}")))?;
        if kind.is_symlink() {
            return Err((
                rel(&path),
                "is a symbolic link; data/ takes plain files only".into(),
            ));
        }
        if kind.is_dir() {
            check_tree(root, &path)?;
        } else if path.extension().is_some_and(|x| x == "json") {
            let bytes =
                fs::read(&path).map_err(|err| (rel(&path), format!("cannot be read: {err}")))?;
            if let Err(err) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                return Err((rel(&path), format!("is not valid JSON: {err}")));
            }
        }
    }
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<(), DiagError> {
    fs::write(path, text).map_err(|e| io_error(path, "write", &e))
}

fn io_error(path: &Path, action: &str, e: &std::io::Error) -> DiagError {
    DiagError::new(
        codes::DATA_PACK_FAILED,
        format!("hotd cannot {action} {}", path.display()),
        "Check the free space and that the world folder belongs to the hotd service user (ls -l <world>/datapacks), then push a new commit or restart hotd",
    )
    .expected("hotd writes mod data packs into the world's datapacks/ folder")
    .caused_by(e)
    .context("path", path.display().to_string())
    .state("The world keeps the data packs it had; the commit is not applied")
}

fn pack_error(id: &str, what: &str) -> DiagError {
    DiagError::new(
        codes::DATA_PACK_FAILED,
        format!("The data pack of mod \"{id}\" cannot be prepared: {what}"),
        "Fix the file named in context.file in the server repository (a JSON validator shows the exact spot) and push a new commit",
    )
    .expected("Every .json file in mods/<id>/data/ is valid JSON and data/ holds no symbolic links")
    .context("mod", id.to_owned())
    .state("Nothing changed on the server; the world keeps the data packs it had")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_name_comes_from_the_world_section_only() {
        let toml = "[server]\nname = \"x\"\n\n[world]\nname = \"swiat\"               # the world folder\nseed = \"\"\n";
        assert_eq!(world_name(toml).as_deref(), Some("swiat"));
        assert_eq!(world_name("[server]\nname = \"x\"\n"), None);
        assert_eq!(world_name("[world]\nname = \"../etc\"\n"), None);
        assert_eq!(world_name("[world]\nnamex = \"a\"\n"), None);
    }

    #[test]
    fn data_mods_need_a_file_under_data() {
        let ids = data_mods(&[
            "mods/a/data/x/recipe/r.json",
            "mods/b/data",
            "mods/C/data/x/y.json",
            "mods/d/src/lib.rs",
        ]);
        assert_eq!(ids, BTreeSet::from(["a".to_owned()]));
    }

    #[test]
    fn diff_writes_changed_and_removes_missing() {
        let m = |p: &[(&str, &str)]| -> BTreeMap<String, String> {
            p.iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect()
        };
        let d = diff(
            &m(&[("a", "1"), ("b", "2")]),
            &m(&[("a", "1"), ("b", "0"), ("c", "3")]),
        );
        assert_eq!(d.write, m(&[("b", "2")]));
        assert_eq!(d.remove, BTreeSet::from(["c".to_owned()]));
        assert!(diff(&m(&[("a", "1")]), &m(&[("a", "1")])).is_empty());
    }

    #[test]
    fn enabled_packs_are_read_from_the_datapack_list_answer() {
        let answer = "There are 3 data pack(s) enabled: [vanilla (built-in)], [file/hot-sklep (world)], [file/inna (world)]\nThere are 1 data pack(s) available: [file/hot-wylaczony (world)]";
        assert_eq!(enabled_in(answer), BTreeSet::from(["sklep".to_owned()]));
        assert!(enabled_in("There are no more data packs available").is_empty());
    }
}
