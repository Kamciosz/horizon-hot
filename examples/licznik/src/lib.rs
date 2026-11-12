//! PL: Przykładowy mod gorący (M1.5). Liczy ticki serwera i oddaje licznik nowej generacji przy
//! `hot load`, więc przeładowanie nie zeruje wyniku. Odpowiada na `/licznik` (gracz) i
//! `hot licznik` (konsola). `licznik panika` celowo panikuje: pokazuje, że panika moda wraca do
//! poprzedniej generacji zamiast zabić serwer (D4).
//!
//! Budowa: `cargo build --release -p licznik`, plik: `target/release/<lib>licznik.<so|dylib>`.
//! Wczytanie na serwerze: `hot load <ścieżka do pliku>`.
//!
//! EN: An example hot mod (M1.5). It counts server ticks and hands the counter to the new
//! generation on `hot load`, so a reload does not reset it. It answers `/licznik` (player) and
//! `hot licznik` (console). `licznik panika` panics on purpose: it shows that a mod panic falls
//! back to the previous generation instead of killing the server (D4).

use hot_sdk::{Event, Flow, Host, HotMod};

/// PL: Tekst odpowiedzi; zmiana tej linii i `hot load` to najprostszy test przeładowania.
/// EN: The reply text; changing this line and running `hot load` is the simplest reload test.
const GREETING: &str = "Licznik";

pub struct Licznik {
    ticks: u64,
}

impl Licznik {
    fn answer(&self, host: &mut Host<'_>, args: &str) -> Flow {
        if args == "panika" {
            panic!("licznik: panika na życzenie (test powrotu do poprzedniej generacji)");
        }
        host.reply(&format!(
            "{GREETING} {}: {} ticków od pierwszego wczytania.",
            env!("CARGO_PKG_VERSION"),
            self.ticks
        ));
        Flow::Cancel
    }
}

/// PL: Argumenty po słowie `licznik` albo `None`, gdy komenda jest cudza.
/// EN: The arguments after the `licznik` word, or `None` for someone else's command.
fn licznik_args(command: &str) -> Option<&str> {
    let rest = command.trim().strip_prefix("licznik")?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

impl HotMod for Licznik {
    fn init(host: &mut Host<'_>, state: &[u8]) -> Self {
        let ticks = hot_sdk::state::u64_from(state);
        host.info(&format!("licznik startuje od {ticks}"));
        Licznik { ticks }
    }

    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        match event {
            Event::ServerTick { .. } => {
                self.ticks += 1;
                Flow::Continue
            }
            Event::Command { command, .. } | Event::ConsoleCommand { command } => {
                match licznik_args(command) {
                    Some(args) => self.answer(host, args),
                    None => Flow::Continue,
                }
            }
            _ => Flow::Continue,
        }
    }

    fn save(&mut self, _host: &mut Host<'_>) -> Vec<u8> {
        self.ticks.to_le_bytes().to_vec()
    }
}

hot_sdk::hot_mod!(Licznik, id = "licznik", version = env!("CARGO_PKG_VERSION"));

#[cfg(test)]
mod tests {
    use super::licznik_args;

    #[test]
    fn only_licznik_commands_are_taken() {
        assert_eq!(licznik_args("licznik"), Some(""));
        assert_eq!(licznik_args(" licznik panika "), Some("panika"));
        assert_eq!(licznik_args("licznikowy"), None);
        assert_eq!(licznik_args("sklep"), None);
    }
}
