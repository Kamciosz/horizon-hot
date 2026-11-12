//! PL: Przykładowy mod gorący (M4.2): magnetyt (`minecraft:lodestone`) przenosi gracza w miejsce
//! ustawione komendą. Pokazuje haki kliknięcia bloku i funkcje gospodarza z D6:
//! - `/teleport-blok ustaw` zapamiętuje miejsce, w którym stoi gracz;
//! - `/teleport-blok postaw` stawia magnetyt dwa bloki na wschód od gracza, gdy jest tam powietrze
//!   (`set_block`);
//! - prawy klik na magnetycie: cząsteczki (`particles`) i teleport w następnym ticku (`run_later`,
//!   bo w haku gracza `run_command` nie działa, S4), potem wiadomość (`tell`).
//!
//! Cel przeżywa `hot load` (stan generacji). Budowa: `cargo build --release -p teleport-blok`.
//!
//! EN: An example hot mod (M4.2): a lodestone takes the player to a place set by a command. It
//! shows the block click hooks and the D6 host functions: `/teleport-blok ustaw` remembers where
//! the player stands, `/teleport-blok postaw` puts a lodestone two blocks east (`set_block`), and
//! a right click on a lodestone gives particles and a teleport on the next tick (`run_later`, as
//! `run_command` does not work in a player hook, S4), then a message (`tell`). The target survives
//! `hot load` (generation state).

use std::collections::BTreeMap;

use hot_sdk::{Event, Flow, Host, HotMod, PlayerId};

/// PL: Blok, który przenosi. EN: The block that teleports.
pub const BLOCK: &str = "minecraft:lodestone";

/// PL: Ile ticków po teleporcie klik jest ignorowany (silnik woła hak raz na rękę).
/// EN: How many ticks after a teleport a click is ignored (the engine calls the hook once per hand).
const COOLDOWN_TICKS: u64 = 10;

pub struct TeleportBlok {
    target: Option<[f64; 3]>,
    tick: u64,
    last_use: BTreeMap<PlayerId, u64>,
}

/// PL: Cel zapisany jako trzy liczby `f64` little-endian; inna długość daje brak celu.
/// EN: The target saved as three little-endian `f64`; any other length means no target.
#[must_use]
pub fn decode(state: &[u8]) -> Option<[f64; 3]> {
    if state.len() != 24 {
        return None;
    }
    let mut out = [0.0; 3];
    for (slot, chunk) in out.iter_mut().zip(state.as_chunks::<8>().0) {
        *slot = f64::from_le_bytes(*chunk);
    }
    Some(out)
}

#[must_use]
pub fn encode(target: Option<[f64; 3]>) -> Vec<u8> {
    target.map_or_else(Vec::new, |t| {
        t.iter().flat_map(|v| v.to_le_bytes()).collect()
    })
}

/// PL: Komenda `tp` dla gracza o nazwie `name`. EN: The `tp` command for the player `name`.
#[must_use]
pub fn tp_command(name: &str, target: [f64; 3]) -> String {
    format!(
        "tp {name} {:.2} {:.2} {:.2}",
        target[0], target[1], target[2]
    )
}

/// PL: Argumenty po słowie `teleport-blok`. EN: The arguments after the `teleport-blok` word.
#[must_use]
pub fn args(command: &str) -> Option<&str> {
    let rest = command.trim().strip_prefix("teleport-blok")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

impl TeleportBlok {
    fn command(&mut self, host: &mut Host<'_>, player: PlayerId, args: &str) -> Flow {
        let Some(me) = host.players().into_iter().find(|p| p.id == player) else {
            host.reply("Nie widzę cię na serwerze.");
            return Flow::Cancel;
        };
        match args {
            "ustaw" => {
                self.target = Some(me.position);
                host.reply(&format!(
                    "Cel ustawiony: {:.0} {:.0} {:.0}. Kliknij magnetyt, żeby tu wrócić.",
                    me.position[0], me.position[1], me.position[2]
                ));
            }
            "postaw" => {
                let pos = [
                    me.position[0].floor() as i32 + 2,
                    me.position[1].floor() as i32,
                    me.position[2].floor() as i32,
                ];
                // PL: `set_block` nadpisuje każdy blok, więc mod sam pilnuje wolnego miejsca.
                // EN: `set_block` overwrites any block, so the mod checks for free space itself.
                if host.block(pos) != "minecraft:air" {
                    host.reply(
                        "Miejsce dwa bloki na wschód jest zajęte; stań tak, żeby było tam powietrze.",
                    );
                } else if host.set_block(pos, BLOCK) {
                    host.reply(&format!(
                        "Magnetyt stoi na {} {} {}.",
                        pos[0], pos[1], pos[2]
                    ));
                } else {
                    host.reply("Serwer nie postawił magnetytu; powód jest w logu serwera.");
                }
            }
            _ => host.reply("Użycie: /teleport-blok ustaw | postaw"),
        }
        Flow::Cancel
    }

    fn click(&mut self, host: &mut Host<'_>, player: PlayerId, pos: [i32; 3]) -> Flow {
        if host.block(pos) != BLOCK {
            return Flow::Continue;
        }
        if self
            .last_use
            .get(&player)
            .is_some_and(|&t| self.tick < t + COOLDOWN_TICKS)
        {
            return Flow::Cancel;
        }
        self.last_use.insert(player, self.tick);
        let Some(target) = self.target else {
            host.tell(player, "Magnetyt nie ma celu: /teleport-blok ustaw");
            return Flow::Cancel;
        };
        let Some(me) = host.players().into_iter().find(|p| p.id == player) else {
            return Flow::Continue;
        };
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 1.2,
            f64::from(pos[2]) + 0.5,
        ];
        host.particles("minecraft:portal", at, [0.3, 0.6, 0.3], 0.5, 40);
        host.run_later(&tp_command(&me.name, target));
        host.tell(player, "Teleport!");
        Flow::Cancel
    }
}

impl HotMod for TeleportBlok {
    fn init(_host: &mut Host<'_>, state: &[u8]) -> Self {
        TeleportBlok {
            target: decode(state),
            tick: 0,
            last_use: BTreeMap::new(),
        }
    }

    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        match event {
            Event::ServerTick { tick } => {
                self.tick = *tick;
                Flow::Continue
            }
            Event::Command {
                player, command, ..
            } => match args(command) {
                Some(a) => self.command(host, *player, a),
                None => Flow::Continue,
            },
            Event::RightClickBlock { player, pos, .. } => self.click(host, *player, *pos),
            _ => Flow::Continue,
        }
    }

    fn save(&mut self, _host: &mut Host<'_>) -> Vec<u8> {
        encode(self.target)
    }
}

hot_sdk::hot_mod!(
    TeleportBlok,
    id = "teleport-blok",
    version = env!("CARGO_PKG_VERSION"),
    commands = "teleport-blok"
);

#[cfg(test)]
mod tests {
    use super::{args, decode, encode, tp_command};

    #[test]
    fn target_survives_a_reload() {
        let t = Some([10.5, 64.0, -3.25]);
        assert_eq!(decode(&encode(t)), t);
        assert_eq!(decode(&encode(None)), None);
        assert_eq!(decode(&[1, 2, 3]), None);
    }

    #[test]
    fn tp_and_arguments() {
        assert_eq!(
            tp_command("Gracz", [1.0, 2.5, -3.0]),
            "tp Gracz 1.00 2.50 -3.00"
        );
        assert_eq!(args("teleport-blok ustaw"), Some("ustaw"));
        assert_eq!(args("teleport-blokada"), None);
    }
}
