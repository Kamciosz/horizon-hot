//! PL: Przykładowy mod gorący (M4.2): sklep pod komendą `/sklep` (własna komenda najwyższego
//! poziomu, R6). Gracz zarabia monetę za każdą minutę na serwerze i kupuje za nie przedmioty:
//! - `/sklep` — stan konta i oferta;
//! - `/sklep kup <towar>` — zakup; przedmioty daje `give`, płaci się tylko za to, co weszło;
//! - konsola: `hot sklep daj <gracz> <ile>` — monety od operatora, gracz dostaje wiadomość.
//!
//! Wiadomości spoza komendy gracza idą przez `tell` (D6); gdy silnik odmówi, `hot-core` wysyła je
//! w następnym ticku jako `tellraw`. Konta przeżywają `hot load` i zatrzymanie serwera (stan
//! generacji, `hot-state/saved/`). Budowa: `cargo build --release -p sklep`.
//!
//! EN: An example hot mod (M4.2): a shop under `/sklep` (its own top-level command, R6). A player
//! earns a coin for every minute on the server and buys items with them: `/sklep` shows the
//! balance and offer, `/sklep kup <item>` buys (items come from `give`, only what fits is paid
//! for), and the console's `hot sklep daj <player> <n>` grants coins with a message to the player.
//! Messages outside a player command go through `tell` (D6); when the engine refuses, `hot-core`
//! sends them as `tellraw` on the next tick. Balances survive `hot load` and a server stop.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hot_sdk::{Event, Flow, Host, HotMod, PlayerId};

/// PL: Ticki na jedną monetę (minuta przy 20 TPS). EN: Ticks per coin (a minute at 20 TPS).
pub const TICKS_PER_COIN: u64 = 1200;

/// PL: Towar: nazwa w komendzie, przedmiot, liczba sztuk, cena w monetach.
/// EN: An offer: the name in the command, the item, how many, the price in coins.
pub struct Offer {
    pub name: &'static str,
    pub item: &'static str,
    pub count: u32,
    pub price: u64,
}

pub const OFFERS: &[Offer] = &[
    Offer {
        name: "chleb",
        item: "minecraft:bread",
        count: 4,
        price: 1,
    },
    Offer {
        name: "pochodnie",
        item: "minecraft:torch",
        count: 16,
        price: 2,
    },
    Offer {
        name: "zelazo",
        item: "minecraft:iron_ingot",
        count: 3,
        price: 5,
    },
    Offer {
        name: "diament",
        item: "minecraft:diamond",
        count: 1,
        price: 20,
    },
];

/// PL: Cena za `given` z `count` sztuk, w górę. EN: The price for `given` of `count`, rounded up.
#[must_use]
pub fn cost(offer: &Offer, given: u32) -> u64 {
    let count = u64::from(offer.count.max(1));
    (offer.price * u64::from(given)).div_ceil(count)
}

/// PL: Konta jako linie `uuid-hex=monety`. EN: Balances as `uuid-hex=coins` lines.
#[must_use]
pub fn encode(accounts: &BTreeMap<PlayerId, u64>) -> Vec<u8> {
    let mut out = String::new();
    for (id, coins) in accounts {
        for b in id.0 {
            let _ = write!(out, "{b:02x}");
        }
        let _ = writeln!(out, "={coins}");
    }
    out.into_bytes()
}

/// PL: Odczyt kont; złe linie pomija. EN: Reads balances; bad lines are skipped.
#[must_use]
pub fn decode(state: &[u8]) -> BTreeMap<PlayerId, u64> {
    let mut out = BTreeMap::new();
    for line in String::from_utf8_lossy(state).lines() {
        let Some((hex, coins)) = line.split_once('=') else {
            continue;
        };
        let (Ok(coins), 32) = (coins.parse::<u64>(), hex.len()) else {
            continue;
        };
        let mut id = [0u8; 16];
        let parsed = id.iter_mut().enumerate().all(|(i, b)| {
            hex.get(i * 2..i * 2 + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .map(|v| *b = v)
                .is_some()
        });
        if parsed {
            out.insert(PlayerId(id), coins);
        }
    }
    out
}

/// PL: Argumenty po słowie `sklep`. EN: The arguments after the `sklep` word.
#[must_use]
pub fn args(command: &str) -> Option<&str> {
    let rest = command.trim().strip_prefix("sklep")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

pub struct Sklep {
    accounts: BTreeMap<PlayerId, u64>,
}

impl Sklep {
    fn menu(&self, host: &mut Host<'_>, player: PlayerId) {
        let coins = self.accounts.get(&player).copied().unwrap_or(0);
        host.reply(&format!("Masz {coins} monet (1 moneta za minutę gry)."));
        for o in OFFERS {
            host.reply(&format!(
                "/sklep kup {} — {} × {} za {} mon.",
                o.name, o.count, o.item, o.price
            ));
        }
    }

    fn buy(&mut self, host: &mut Host<'_>, player: PlayerId, name: &str) {
        let Some(offer) = OFFERS.iter().find(|o| o.name == name) else {
            host.reply(&format!("Nie ma towaru „{name}”. Lista: /sklep"));
            return;
        };
        let coins = self.accounts.get(&player).copied().unwrap_or(0);
        if coins < offer.price {
            host.reply(&format!(
                "Brakuje {} monet (masz {coins}).",
                offer.price - coins
            ));
            return;
        }
        let given = host.give(player, offer.item, offer.count);
        if given == 0 {
            host.reply("Ekwipunek pełny albo przedmiot nieznany serwerowi; nic nie pobrano.");
            return;
        }
        let paid = cost(offer, given);
        let left = coins - paid;
        self.accounts.insert(player, left);
        host.reply(&format!(
            "Kupione: {given} × {} za {paid} mon. Zostało {left}.",
            offer.item
        ));
    }

    fn grant(&mut self, host: &mut Host<'_>, args: &str) {
        let mut words = args.split_whitespace();
        let (Some(name), Some(Ok(n))) = (words.next(), words.next().map(str::parse::<u64>)) else {
            host.reply("Użycie: hot sklep daj <gracz> <ile>");
            return;
        };
        let Some(p) = host.players().into_iter().find(|p| p.name == name) else {
            host.reply(&format!("Gracza {name} nie ma na serwerze."));
            return;
        };
        let coins = self.accounts.entry(p.id).or_insert(0);
        *coins += n;
        let total = *coins;
        let told = host.tell(p.id, &format!("Operator dał ci {n} monet; masz {total}."));
        host.reply(&format!(
            "{name} ma teraz {total} monet (wiadomość: {}).",
            if told { "wysłana" } else { "nie doszła" }
        ));
    }

    fn pay_wages(&mut self, host: &mut Host<'_>) {
        for p in host.players() {
            let coins = self.accounts.entry(p.id).or_insert(0);
            *coins += 1;
            if coins.is_multiple_of(5) {
                let total = *coins;
                host.tell(p.id, &format!("Masz już {total} monet. Wydaj je: /sklep"));
            }
        }
    }
}

impl HotMod for Sklep {
    fn init(_host: &mut Host<'_>, state: &[u8]) -> Self {
        Sklep {
            accounts: decode(state),
        }
    }

    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        match event {
            Event::ServerTick { tick } => {
                if tick % TICKS_PER_COIN == 0 {
                    self.pay_wages(host);
                }
                Flow::Continue
            }
            Event::Command {
                player, command, ..
            } => {
                let Some(a) = args(command) else {
                    return Flow::Continue;
                };
                match a.strip_prefix("kup") {
                    Some(name) => self.buy(host, *player, name.trim()),
                    None => self.menu(host, *player),
                }
                Flow::Cancel
            }
            Event::ConsoleCommand { command } => {
                let Some(a) = args(command) else {
                    return Flow::Continue;
                };
                match a.strip_prefix("daj") {
                    Some(rest) => self.grant(host, rest.trim()),
                    None => host.reply("Użycie: hot sklep daj <gracz> <ile>"),
                }
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }

    fn save(&mut self, _host: &mut Host<'_>) -> Vec<u8> {
        encode(&self.accounts)
    }
}

hot_sdk::hot_mod!(
    Sklep,
    id = "sklep",
    version = env!("CARGO_PKG_VERSION"),
    commands = "sklep"
);

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use hot_sdk::PlayerId;

    use super::{args, cost, decode, encode, OFFERS};

    #[test]
    fn accounts_survive_a_reload() {
        let mut a = BTreeMap::new();
        a.insert(PlayerId([0xab; 16]), 7);
        a.insert(PlayerId([1; 16]), 0);
        assert_eq!(decode(&encode(&a)), a);
        assert!(decode(b"zle\n=5\n00=1\n").is_empty());
    }

    #[test]
    fn only_what_fits_is_paid_for() {
        let torches = &OFFERS[1];
        assert_eq!(cost(torches, 16), 2);
        assert_eq!(cost(torches, 9), 2);
        assert_eq!(cost(torches, 8), 1);
        assert_eq!(cost(torches, 1), 1);
    }

    #[test]
    fn only_sklep_commands_are_taken() {
        assert_eq!(args("sklep kup chleb"), Some("kup chleb"));
        assert_eq!(args("sklepik"), None);
    }
}
