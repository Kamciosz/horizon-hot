//! PL: Start rdzenia: przywrócenie modów z `hot-state/mods.json`, start po awarii z ostatniej dobrej
//! generacji i tryb bezpieczny po pętli awarii (D13).
//!
//! EN: Core start: restoring mods from `hot-state/mods.json`, a start after a crash from the last good
//! generation and safe mode after a crash loop (D13).

use super::*;

impl Core {
    pub(super) fn restore(&mut self, api: &HostApi) {
        let file = match store::read_mods(&self.paths) {
            Ok(f) => f,
            Err(e) => {
                self.error(&e);
                ModsFile::default()
            }
        };
        let now = now_ms();
        self.started_ms = now;
        self.crash_starts = crash_starts(&file, now);
        if self.crash_starts >= SAFE_MODE_AFTER && !file.mods.is_empty() {
            self.enter_safe_mode(&file);
        } else {
            for record in &file.mods {
                self.restore_one(api, record, file.clean_stop);
            }
        }
        let keep: Vec<PathBuf> = self
            .mods
            .iter()
            .flat_map(|m| {
                [
                    Some(m.gen.file.clone()),
                    m.good.as_ref().map(|(g, _)| g.file.clone()),
                ]
            })
            .chain(self.dormant.iter().flat_map(|r| {
                [
                    Some(r.current.clone()),
                    r.good.as_ref().map(|(g, _)| g.clone()),
                ]
            }))
            .flatten()
            .collect();
        store::prune_generations(&self.paths, &keep);
        // PL: Od teraz każde zatrzymanie bez `save` to awaria. EN: From now on, a stop without
        // `save` counts as a crash.
        self.write_mods(false);
        self.write_status();
        self.log.info(
            "hot-core started",
            json!({ "home": self.paths.home.display().to_string(), "mods": self.mods.iter().map(|m| m.gen.id.clone()).collect::<Vec<_>>(), "after_clean_stop": file.clean_stop, "crash_starts": self.crash_starts }),
        );
    }

    /// PL: Tryb bezpieczny (M7.1): silnik padł drugi raz z rzędu krótko po starcie, mimo
    /// ostatnich dobrych generacji. Żaden mod gorący nie startuje, żeby przerwać pętlę awarii;
    /// wpisy zostają w `mods.json` i wracają po czystym restarcie albo `hot load`.
    /// EN: Safe mode (M7.1): the engine crashed a second time in a row soon after starting, even
    /// with the last good generations. No hot mod starts, to break the crash loop; the records
    /// stay in `mods.json` and return after a clean restart or `hot load`.
    pub(super) fn enter_safe_mode(&mut self, file: &ModsFile) {
        self.dormant.clone_from(&file.mods);
        let ids: Vec<&str> = file.mods.iter().map(|r| r.id.as_str()).collect();
        let e = DiagError::new(
            codes::CRASH_LOOP,
            format!(
                "The server crashed {} times in a row, each within {} min of starting; hot mods {} stay off (safe mode) to stop a crash loop",
                self.crash_starts,
                CRASH_LOOP_WINDOW_MS / 60_000,
                ids.join(", ")
            ),
            "Read logs/latest.log (the crash and the lines before it) and logs/hot-core.jsonl. If a hot mod crashed: fix it and push a commit, hotd loads it; or run hot load <file> for each mod you trust. If the crash is not from a hot mod: fix that first. A clean restart of the server (systemctl restart horizon-hot-server) loads all of these mods again",
        )
        .expected("The server runs longer than the probation after a start")
        .context("mods", ids.join(","))
        .context("crash_starts", self.crash_starts)
        .context("previous_start_ms", file.started_ms)
        .state("hot-core runs without hot mods; the records stay in hot-state/mods.json");
        self.error(&e);
    }

    /// PL: Po czystym zatrzymaniu wraca bieżąca generacja, po awarii ostatnia dobra (D13).
    /// EN: After a clean stop the current generation returns, after a crash the last good one.
    pub(super) fn restore_one(&mut self, api: &HostApi, record: &ModRecord, clean_stop: bool) {
        let (path, generation) = if clean_stop {
            (record.current.clone(), record.generation)
        } else if let Some((good, n)) = &record.good {
            if *good != record.current {
                self.log.warn(
                    &format!(
                        "The server stopped without a clean shutdown; hot mod \"{}\" starts from its last good generation {n} instead of the unproven generation {}",
                        record.id, record.generation
                    ),
                    json!({ "mod": record.id, "skipped": record.current.display().to_string(), "loaded": good.display().to_string() }),
                );
            }
            (good.clone(), *n)
        } else {
            self.log.warn(
                &format!(
                    "The server stopped without a clean shutdown; hot mod \"{}\" has no generation that passed probation and stays off until the next hot load",
                    record.id
                ),
                json!({ "mod": record.id, "skipped": record.current.display().to_string() }),
            );
            return;
        };
        let gen = match loader::open(&path, &record.source) {
            Ok(g) => g,
            Err(e) => return self.error(&e),
        };
        let good = match &record.good {
            Some((g, n)) if *g == path => Some((gen.clone(), *n)),
            Some((g, n)) => match loader::open(g, &record.source) {
                Ok(x) => Some((x, *n)),
                Err(e) => {
                    self.error(&e);
                    None
                }
            },
            None => None,
        };
        let proven = good.as_ref().is_some_and(|(g, _)| g.file == gen.file);
        let state = store::take_saved(&self.paths, &gen.id);
        let mut candidates = vec![(gen, generation)];
        if !proven {
            candidates.extend(good.clone());
        }
        for (k, (gen, generation)) in candidates.iter().enumerate() {
            if let Some(instance) = self.init_gen(api, gen, *generation, &state) {
                let is_good = good.as_ref().is_some_and(|(g, _)| g.file == gen.file);
                self.mods.push(Active {
                    gen: gen.clone(),
                    generation: *generation,
                    instance,
                    previous: if k == 0 && !proven {
                        good.clone()
                    } else {
                        None
                    },
                    good: good.clone(),
                    snapshot: state,
                    ok_ticks: if is_good { GOOD_AFTER_TICKS } else { 0 },
                });
                return;
            }
        }
        if let Err(e) = store::write_atomic(&self.paths.saved(&record.id), &state) {
            self.error(&e);
        }
    }
}
