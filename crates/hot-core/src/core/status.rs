//! PL: Pliki stanu: `hot-state/mods.json`, biblioteki starych generacji i `hot-state/status.json`
//! dla `hot status` (D21).
//!
//! EN: State files: `hot-state/mods.json`, the libraries of old generations and
//! `hot-state/status.json` for `hot status` (D21).

use super::*;

impl Core {
    pub(super) fn write_mods(&mut self, clean_stop: bool) {
        let file = ModsFile {
            clean_stop,
            mods: self
                .mods
                .iter()
                .map(|m| ModRecord {
                    id: m.gen.id.clone(),
                    version: m.gen.version.clone(),
                    generation: m.generation,
                    source: m.gen.source.clone(),
                    current: m.gen.file.clone(),
                    good: m.good.as_ref().map(|(g, n)| (g.file.clone(), *n)),
                })
                .chain(self.dormant.iter().cloned())
                .collect(),
            started_ms: self.started_ms,
            crash_starts: self.crash_starts,
        };
        if let Err(e) = store::write_mods(&self.paths, &file) {
            self.error(&e);
        }
    }

    /// PL: Biblioteki, które poprzednie kopie `hot-core` otworzyły w tym procesie (ten sam pid).
    /// Plik z innym pidem jest z poprzedniego silnika: jego pamięć już oddano.
    /// EN: Libraries earlier `hot-core` copies opened in this process (the same pid). A file with
    /// another pid is from an earlier engine: its memory is already gone.
    pub(super) fn inherit_libraries(&mut self) {
        let Some(v) = std::fs::read(self.paths.libraries())
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        else {
            return;
        };
        if v["pid"].as_u64() != Some(u64::from(std::process::id())) {
            return;
        }
        if let Some(files) = v["files"].as_object() {
            loader::note_mapped(
                files
                    .iter()
                    .map(|(f, n)| (PathBuf::from(f), n.as_u64().unwrap_or(0))),
            );
        }
    }

    /// PL: Pamięć generacji dla `hotd engine` (D4, M8.5): ile bibliotek trzyma ten proces i ile
    /// bajtów plików to już nieużywane generacje. Bieżąca, poprzednia i ostatnia dobra generacja
    /// moda są w użyciu (cel powrotu po panice). Pełna lista idzie do `libraries.json` tylko,
    /// gdy przybyła biblioteka.
    /// EN: Generation memory for `hotd engine` (D4, M8.5): how many libraries this process holds
    /// and how many file bytes are unused generations. A mod's current, previous and last good
    /// generation are in use (fallbacks after a panic). The full list goes to `libraries.json`
    /// only when a library was added.
    pub(super) fn libraries_status(&mut self) -> Value {
        let mapped = loader::mapped();
        let pid = std::process::id();
        if mapped.len() != self.libraries_written {
            let files: serde_json::Map<String, Value> = mapped
                .iter()
                .map(|(f, n)| (f.display().to_string(), json!(n)))
                .collect();
            let v = json!({ "pid": pid, "files": files });
            match store::write_atomic(&self.paths.libraries(), format!("{v}\n").as_bytes()) {
                Ok(()) => self.libraries_written = mapped.len(),
                Err(e) => self.log.error(&e),
            }
        }
        let in_use: BTreeSet<&PathBuf> = self
            .mods
            .iter()
            .flat_map(|m| {
                std::iter::once(&m.gen.file)
                    .chain(m.previous.as_ref().map(|(g, _)| &g.file))
                    .chain(m.good.as_ref().map(|(g, _)| &g.file))
            })
            .collect();
        let bytes: u64 = mapped.values().sum();
        let stale_bytes: u64 = mapped
            .iter()
            .filter(|(f, _)| !in_use.contains(f))
            .map(|(_, n)| n)
            .sum();
        json!({
            "pid": pid,
            "count": mapped.len(),
            "bytes": bytes,
            "stale_bytes": stale_bytes,
        })
    }

    pub(super) fn write_status(&mut self) {
        let libraries = self.libraries_status();
        let mods: Vec<Value> = self
            .mods
            .iter()
            .map(|m| {
                json!({
                    "id": m.gen.id,
                    "version": m.gen.version,
                    "generation": m.generation,
                    "good": m.ok_ticks >= GOOD_AFTER_TICKS,
                    "probation_ticks": m.ok_ticks.min(GOOD_AFTER_TICKS),
                    "file": m.gen.file.display().to_string(),
                    "source": m.gen.source.display().to_string(),
                })
            })
            .collect();
        let v = json!({
            "component": "hot-core",
            "version": VERSION,
            "abi": hot_abi::ABI_MAJOR,
            "updated": hot_diag::utc_timestamp(std::time::SystemTime::now()),
            "tick": self.tick,
            "mods": mods,
            "pending": self.pending.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "safe_mode": (!self.dormant.is_empty()).then(|| json!({
                "crash_starts": self.crash_starts,
                "dormant": self.dormant.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            })),
            "recent_errors": self.errors,
            "libraries": libraries,
        });
        if let Err(e) = store::write_atomic(&self.paths.status(), format!("{v:#}\n").as_bytes()) {
            // PL: Bez `error`, żeby nie zapętlić zapisu statusu. EN: Not `error`, to avoid a loop.
            self.log.error(&e);
        }
    }
}
