//! PL: Komendy: odłożone komendy graczy wykonywane w ticku gracza (D6a) i komenda `hot` w konsoli
//! i w grze (`load`, `unload`, `slots`, `list`, `status`).
//!
//! EN: Commands: deferred player commands run in the player's tick (D6a) and the `hot` command in the
//! console and in game (`load`, `unload`, `slots`, `list`, `status`).

use super::*;

impl Core {
    /// PL: Indeks moda, który deklaruje pierwszy wyraz komendy. EN: The mod declaring the command.
    pub(super) fn owner(&self, command: &str) -> Option<usize> {
        let word = command.split_whitespace().next()?;
        self.mods
            .iter()
            .position(|m| m.gen.commands.iter().any(|c| c == word))
    }

    /// PL: Komenda gracza zadeklarowana przez mod trafia do kolejki na jego tick; zwraca, czy
    /// została odłożona. W haku komendy silnik nie widzi gracza (D6a), więc mod nie dostaje jej tu.
    /// EN: A player command a mod declared is queued for the player's tick; returns whether it was
    /// deferred. In the command hook the engine does not see the player (D6a), so the mod does not
    /// get it here.
    pub(super) fn defer(&mut self, outer: &HostApi, event: &Event) -> bool {
        // SAFETY: napisy zdarzenia ważne w wywołaniu. EN: the event's strings are valid for the call.
        let command = unsafe { event.text.as_str() }.unwrap_or("").to_owned();
        if self.owner(&command).is_none() {
            return false;
        }
        let waiting = self
            .deferred
            .iter()
            .filter(|d| d.player == event.player)
            .count();
        if waiting >= DEFERRED_MAX {
            (outer.reply)(
                outer.ctx,
                Str::from_str("Za dużo komend naraz; poczekaj chwilę i spróbuj ponownie."),
            );
            return true;
        }
        self.deferred.push(Deferred {
            player: event.player,
            // SAFETY: jak wyżej. EN: as above.
            name: unsafe { event.name.as_str() }.unwrap_or("").to_owned(),
            command,
        });
        true
    }

    /// PL: Mod obsłużył komendę gracza w haku komendy, gdzie silnik nie widzi gracza (D6a). Jedno
    /// ostrzeżenie na parę (mod, komenda), bo blokada komendy silnika też jest poprawna.
    /// EN: A mod handled a player command in the command hook, where the engine does not see the
    /// player (D6a). One warning per (mod, command) pair, as blocking an engine command is valid.
    pub(super) fn warn_undeclared(&mut self, event: &Event) {
        let Some(id) = self.cancelled_by.take() else {
            return;
        };
        // SAFETY: napis zdarzenia ważny w wywołaniu. EN: the event's string is valid for the call.
        let line = unsafe { event.text.as_str() }.unwrap_or("");
        let word = line.split_whitespace().next().unwrap_or("").to_owned();
        if !self.warned.insert((id.clone(), word.clone())) {
            return;
        }
        self.log.warn(
            &format!(
                "Hot mod \"{id}\" answered the player command /{word} in the command hook, where the engine does not see the player: players() leaves them out and tell() and give() do not reach them. Fix: if the mod needs the player, declare the command with hot_mod!(..., commands = \"{word}\") so it runs in the player's tick"
            ),
            json!({ "mod": id, "command": word }),
        );
    }

    /// PL: Ostrzeżenie, gdy nowy mod deklaruje komendę innego aktywnego moda; wygrywa ten, który
    /// był wcześniej na liście. EN: A warning when a new mod declares another active mod's
    /// command; the one listed earlier wins.
    pub(super) fn warn_shared_commands(&self, gen: &Gen) {
        for m in self.mods.iter().filter(|m| m.gen.id != gen.id) {
            for c in gen.commands.iter().filter(|c| m.gen.commands.contains(c)) {
                self.log.warn(
                    &format!(
                        "Hot mods \"{}\" and \"{}\" both declare the command /{c}; \"{}\" gets it. Fix: remove /{c} from commands = \"...\" in one of them",
                        m.gen.id, gen.id, m.gen.id
                    ),
                    json!({ "command": c, "owner": m.gen.id, "ignored": gen.id }),
                );
            }
        }
    }

    /// PL: Odłożone komendy gracza, w jego ticku. Właściciela szuka się teraz, bo mod mógł zostać
    /// przeładowany albo usunięty. EN: The player's deferred commands, in their tick. The owner is
    /// looked up now, as the mod may have been reloaded or removed meanwhile.
    pub(super) fn run_deferred(&mut self, outer: &HostApi, player: Uuid) {
        if !self.deferred.iter().any(|d| d.player == player) {
            return;
        }
        let (mine, rest): (Vec<Deferred>, Vec<Deferred>) = std::mem::take(&mut self.deferred)
            .into_iter()
            .partition(|d| d.player == player);
        self.deferred = rest;
        for d in mine {
            let Some(i) = self.owner(&d.command) else {
                let line = format!("Komendy /{} nie obsługuje już żaden mod.", d.command);
                (outer.tell)(outer.ctx, &player, Str::from_str(&line));
                continue;
            };
            let mut event = Event::new(kind::PLAYER_COMMAND);
            event.player = player;
            event.name = Str::from_str(&d.name);
            event.text = Str::from_str(&d.command);
            self.reply_to = Some(player);
            self.call_one(outer, i, &event);
            self.reply_to = None;
        }
    }

    /// PL: `hot …` z konsoli albo od operatora; `from` to zdarzenie z komendą (gracz, jeśli był).
    /// EN: `hot …` from the console or an op; `from` is the command event (with its player, if any).
    pub(super) fn command(&mut self, api: &HostApi, args: &str, from: &hot_abi::Event) {
        let (verb, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
        let rest = rest.trim();
        let mut out: Vec<String> = Vec::new();
        match verb {
            "load" if !rest.is_empty() => {
                let mut path = PathBuf::from(rest);
                if path.is_relative() {
                    path = self.paths.home.join(path);
                }
                if path.is_file() {
                    out.push(format!(
                        "Ładuję {} w tle; przełączenie przy najbliższym ticku.",
                        path.display()
                    ));
                    self.start_load(path);
                } else {
                    out.push(format!("Nie ma pliku {}.", path.display()));
                }
            }
            "unload" if !rest.is_empty() => {
                if let Some(i) = self.mods.iter().position(|m| m.gen.id == rest) {
                    let state = self.deactivate(api, i);
                    if let Err(e) = store::write_atomic(&self.paths.saved(rest), &state) {
                        self.error(&e);
                    }
                    self.mods.remove(i);
                    self.write_mods(false);
                    self.write_status();
                    self.log.info(
                        &format!("Hot mod \"{rest}\" was unloaded by the hot unload command"),
                        json!({ "mod": rest }),
                    );
                    out.push(format!(
                        "Mod {rest} wyłączony; stan zapisany na następne ładowanie."
                    ));
                } else if self.dormant.iter().any(|r| r.id == rest) {
                    self.dormant.retain(|r| r.id != rest);
                    self.write_mods(false);
                    self.write_status();
                    self.log.info(
                        &format!("Hot mod \"{rest}\", asleep in safe mode, was removed by the hot unload command"),
                        json!({ "mod": rest }),
                    );
                    out.push(format!(
                        "Mod {rest} spał w trybie bezpiecznym; usunięty z listy, nie wróci po restarcie."
                    ));
                } else {
                    out.push(format!("Nie ma aktywnego moda {rest}."));
                }
            }
            "slots" => {
                self.read_slots();
                out.push(format!(
                    "Bloki modów: {} (hot-state/slots.json).",
                    self.slots.len()
                ));
            }
            "list" | "status" => {
                out.push(format!(
                    "hot-core {VERSION}, ABI {}, mody: {}, w kolejce: {}",
                    hot_abi::ABI_MAJOR,
                    self.mods.len(),
                    self.pending.len()
                ));
                for m in &self.mods {
                    let probation = if m.ok_ticks >= GOOD_AFTER_TICKS {
                        "dobra".to_owned()
                    } else {
                        format!("próba {}/{GOOD_AFTER_TICKS}", m.ok_ticks)
                    };
                    out.push(format!(
                        "  {} {} — generacja {}, {probation}",
                        m.gen.id, m.gen.version, m.generation
                    ));
                }
                if !self.dormant.is_empty() {
                    let ids: Vec<&str> = self.dormant.iter().map(|r| r.id.as_str()).collect();
                    out.push(format!(
                        "Tryb bezpieczny: serwer padł {} razy z rzędu krótko po starcie, więc te mody śpią: {}. Wrócą po hot load albo po czystym restarcie serwera; opis: HOT-E0408.",
                        self.crash_starts,
                        ids.join(", ")
                    ));
                }
                out.extend(hotd_lines(&self.paths.state().join("hotd.json")));
            }
            _ if self.mods.iter().any(|m| m.gen.id == verb) => {
                // PL: `hot <mod> …` trafia tylko do tego moda jako komenda konsoli `<mod> …` (D5).
                // EN: `hot <mod> …` goes only to that mod as the console command `<mod> …` (D5).
                let i = self.mods.iter().position(|m| m.gen.id == verb).unwrap_or(0);
                let mut event = *from;
                event.text = Str::from_str(args);
                match self.call_one(api, i, &event).0 {
                    flow::CANCEL => {}
                    flow::PANICKED => out.push(format!(
                        "Mod {verb} spanikował przy tej komendzie; szczegóły w logs/hot-core.jsonl (HOT-E0403)."
                    )),
                    _ => out.push(format!("Mod {verb} nie obsłużył komendy „{args}”.")),
                }
            }
            _ => {
                out.push(
                    "Użycie: hot load <plik> | hot unload <mod> | hot list | hot slots | hot <mod> [argumenty]"
                        .to_owned(),
                );
            }
        }
        let mut host = Host::new(api);
        for line in out {
            host.reply(&line);
        }
    }
}
