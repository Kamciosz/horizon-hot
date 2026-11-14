//! PL: Wywołania modów i generacje: zdarzenia do modów, powrót do poprzedniej generacji po panice
//! (D4), próba 6000 ticków (D13), wczytanie w tle i przełączenie na granicy ticku.
//!
//! EN: Mod calls and generations: events to mods, falling back to the previous generation after a
//! panic (D4), the 6000-tick probation (D13), loading in the background and switching at the tick
//! boundary.

use super::*;

impl Core {
    /// PL: Wywołuje mod przez tabelę z jego id; zwraca wynik i opis paniki od `hot-sdk`.
    /// EN: Calls the mod through its own table; returns the result and the `hot-sdk` panic report.
    pub(super) fn with_call<R>(
        &self,
        outer: &HostApi,
        gen: &Gen,
        generation: u64,
        f: impl FnOnce(&HostApi) -> R,
    ) -> (R, Option<String>) {
        let data_dir = self.paths.data(&gen.id);
        let mut call = ModCall {
            outer,
            id: &gen.id,
            generation,
            data_dir: &data_dir,
            log: &self.log,
            panic: None,
            later: &self.later,
            reply_to: self.reply_to,
            slots: &self.slots,
        };
        let api = call.api();
        let result = f(&api);
        (result, call.panic)
    }

    /// PL: Komendy odłożone przez mody w poprzednim ticku. Odmowa silnika trafia do logu z id moda.
    /// EN: Commands the mods put off in the previous tick. An engine refusal is logged with the mod.
    pub(super) fn run_later(&mut self, outer: &HostApi) {
        let queued = self.later.take();
        for later in queued {
            if !(outer.run_command)(outer.ctx, Str::from_str(&later.line)) {
                self.log.warn(
                    &format!(
                        "The engine refused a command hot mod \"{}\" queued for the next tick. Fix: run the same command in the server console to see why",
                        later.id
                    ),
                    json!({ "mod": later.id, "command": later.line }),
                );
            }
        }
    }

    pub(super) fn init_gen(
        &mut self,
        outer: &HostApi,
        gen: &Gen,
        generation: u64,
        state: &[u8],
    ) -> Option<*mut c_void> {
        let (instance, panic) = self.with_call(outer, gen, generation, |api| {
            (gen.vtable.init)(api, Str::from_bytes(state))
        });
        if instance.is_null() {
            self.panicked(gen, generation, "init", panic, "");
            None
        } else {
            Some(instance)
        }
    }

    pub(super) fn deactivate(&mut self, outer: &HostApi, i: usize) -> Vec<u8> {
        let (gen, generation, instance) = {
            let m = &self.mods[i];
            (m.gen.clone(), m.generation, m.instance)
        };
        let (buf, panic) = self.with_call(outer, &gen, generation, |api| {
            (gen.vtable.deactivate)(instance, api)
        });
        let state = take_buf(&gen, buf);
        if panic.is_some() {
            self.panicked(
                &gen,
                generation,
                "deactivate",
                panic,
                "The mod's state from this generation is lost; the next generation starts empty",
            );
        }
        state
    }

    pub(super) fn dispatch(&mut self, outer: &HostApi, event: &hot_abi::Event) -> Flow {
        let mut i = 0;
        while i < self.mods.len() {
            let (result, stayed) = self.call_one(outer, i, event);
            if result == flow::CANCEL {
                self.cancelled_by = self.mods.get(i).map(|m| m.gen.id.clone());
                return Flow::Cancel;
            }
            if stayed {
                i += 1;
            }
        }
        Flow::Continue
    }

    /// PL: Zdarzenie dla jednego moda. Zwraca wynik moda i to, czy mod nadal jest na liście
    /// (po panice może zniknąć). EN: An event for one mod. Returns the mod's result and whether
    /// the mod is still listed (it may be gone after a panic).
    pub(super) fn call_one(
        &mut self,
        outer: &HostApi,
        i: usize,
        event: &hot_abi::Event,
    ) -> (u32, bool) {
        let (gen, generation, instance) = {
            let m = &self.mods[i];
            (m.gen.clone(), m.generation, m.instance)
        };
        let (result, panic) = self.with_call(outer, &gen, generation, |api| {
            (gen.vtable.on_event)(instance, api, event)
        });
        if result == flow::PANICKED {
            let stayed = self.fall_back(outer, i, event.kind, panic);
            return (result, stayed);
        }
        (result, true)
    }

    /// PL: Po panice w `on_event`: poprzednia generacja dostaje stan sprzed bieżącej. Gdy jej nie
    /// ma albo też padnie, mod się wyłącza, a stan trafia do `saved/`. Zwraca, czy mod został.
    /// EN: After a panic in `on_event`: the previous generation gets the state from before the
    /// current one. With no previous generation, or when it fails too, the mod turns off and the
    /// state goes to `saved/`. Returns whether the mod stayed.
    pub(super) fn fall_back(
        &mut self,
        outer: &HostApi,
        i: usize,
        event_kind: u32,
        panic: Option<String>,
    ) -> bool {
        let gen = self.mods[i].gen.clone();
        let generation = self.mods[i].generation;
        let snapshot = std::mem::take(&mut self.mods[i].snapshot);
        let previous = self.mods[i].previous.take();
        let hook = format!("on_event (kind {event_kind})");
        if let Some((prev, prev_generation)) = previous {
            self.panicked(
                &gen,
                generation,
                &hook,
                panic,
                &format!("Generation {prev_generation} of the mod is active again with the state it had before generation {generation}"),
            );
            if let Some(instance) = self.init_gen(outer, &prev, prev_generation, &snapshot) {
                let m = &mut self.mods[i];
                m.ok_ticks = if m.good.as_ref().is_some_and(|(g, _)| g.file == prev.file) {
                    GOOD_AFTER_TICKS
                } else {
                    0
                };
                m.gen = prev;
                m.generation = prev_generation;
                m.instance = instance;
                m.snapshot = snapshot;
                self.write_mods(false);
                self.write_status();
                return true;
            }
        } else {
            self.panicked(
                &gen,
                generation,
                &hook,
                panic,
                "The mod has no earlier generation in this run, so it is off until the next hot load; its state is in hot-state/saved/",
            );
        }
        if let Err(e) = store::write_atomic(&self.paths.saved(&gen.id), &snapshot) {
            self.error(&e);
        }
        self.mods.remove(i);
        self.write_mods(false);
        self.write_status();
        false
    }

    pub(super) fn count_tick(&mut self) {
        let mut promoted = Vec::new();
        for m in &mut self.mods {
            m.ok_ticks = m.ok_ticks.saturating_add(1);
            if m.ok_ticks == GOOD_AFTER_TICKS {
                m.good = Some((m.gen.clone(), m.generation));
                promoted.push((m.gen.id.clone(), m.generation));
            }
        }
        if !promoted.is_empty() {
            for (id, generation) in &promoted {
                self.log.info(
                    &format!("Hot mod \"{id}\" generation {generation} passed probation and is now the last good generation"),
                    json!({ "mod": id, "generation": generation, "ticks": GOOD_AFTER_TICKS }),
                );
            }
            self.write_mods(false);
            self.write_status();
        }
    }

    pub(super) fn start_load(&mut self, source: PathBuf) {
        self.seq += 1;
        self.pending.push(source.clone());
        loader::spawn(source, self.paths.gen_dir(), self.seq, self.tx.clone());
        self.write_status();
    }

    pub(super) fn apply_loaded(&mut self, outer: &HostApi) {
        while let Ok(outcome) = self.rx.try_recv() {
            match outcome {
                Outcome::Ready(gen) => {
                    self.pending.retain(|p| *p != gen.source);
                    self.switch(outer, gen);
                }
                Outcome::Failed(source, e) => {
                    self.pending.retain(|p| *p != source);
                    self.error(&e);
                }
            }
            self.write_status();
        }
    }

    /// PL: Nowa generacja na miejsce starej (albo nowy mod). Stan starej przechodzi do nowej.
    /// EN: A new generation replaces the old one (or a new mod). The old state passes over.
    pub(super) fn switch(&mut self, outer: &HostApi, gen: Gen) {
        self.warn_shared_commands(&gen);
        // PL: `hot load` uśpionego moda kończy dla niego tryb bezpieczny. EN: `hot load` of a
        // sleeping mod ends safe mode for it.
        self.dormant.retain(|r| r.id != gen.id);
        let Some(i) = self.mods.iter().position(|m| m.gen.id == gen.id) else {
            // PL: Stan zostawiony przez `hot unload` albo wyłączenie po panice. Gdy start się nie
            // uda, wraca do `saved/`. EN: The state left by `hot unload` or a turn-off after a
            // panic. If the start fails, it goes back to `saved/`.
            let state = store::take_saved(&self.paths, &gen.id);
            let Some(instance) = self.init_gen(outer, &gen, 1, &state) else {
                if let Err(e) = store::write_atomic(&self.paths.saved(&gen.id), &state) {
                    self.error(&e);
                }
                return;
            };
            self.log.info(
                &format!("Hot mod \"{}\" {} is active as generation 1", gen.id, gen.version),
                json!({ "mod": gen.id, "generation": 1, "file": gen.file.display().to_string(), "source": gen.source.display().to_string(), "state_bytes": state.len() }),
            );
            self.mods.push(Active {
                gen,
                generation: 1,
                instance,
                previous: None,
                good: None,
                snapshot: state,
                ok_ticks: 0,
            });
            self.write_mods(false);
            return;
        };
        let state = self.deactivate(outer, i);
        let old = self.mods[i].gen.clone();
        let old_generation = self.mods[i].generation;
        let generation = old_generation + 1;
        if let Some(instance) = self.init_gen(outer, &gen, generation, &state) {
            self.log.info(
                &format!(
                    "Hot mod \"{}\" {} is active as generation {generation}, replacing generation {old_generation}",
                    gen.id, gen.version
                ),
                json!({ "mod": gen.id, "generation": generation, "file": gen.file.display().to_string(), "state_bytes": state.len() }),
            );
            let m = &mut self.mods[i];
            m.previous = Some((old, old_generation));
            m.gen = gen;
            m.generation = generation;
            m.instance = instance;
            m.snapshot = state;
            m.ok_ticks = 0;
        } else if let Some(instance) = self.init_gen(outer, &old, old_generation, &state) {
            self.mods[i].instance = instance;
            self.mods[i].snapshot = state;
        } else {
            if let Err(e) = store::write_atomic(&self.paths.saved(&old.id), &state) {
                self.error(&e);
            }
            self.mods.remove(i);
        }
        self.write_mods(false);
    }
}
