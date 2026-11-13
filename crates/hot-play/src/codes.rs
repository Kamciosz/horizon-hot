//! PL: Kody błędów aplikacji gracza (zakres `E09xx`). Opisy: `docs/bledy/`.
//! EN: The player app's error codes (the `E09xx` range). Descriptions: `docs/bledy/`.

use hot_diag::Code;

/// PL: Nie ma launchera Horizona i nie da się go zainstalować. EN: No Horizon launcher, and it
/// cannot be installed.
pub const LAUNCHER: Code = Code::new("HOT-E0901");

/// PL: Serwer nie podaje `hot-play.json` (adres, sieć, `hotd serve-pack`). EN: The server does not
/// serve `hot-play.json` (address, network, `hotd serve-pack`).
pub const SERVER: Code = Code::new("HOT-E0902");

/// PL: Nie ma wersji Horizona do gry albo jej instalacja się nie udała. EN: No Horizon version to
/// play, or installing it failed.
pub const VERSION: Code = Code::new("HOT-E0903");

/// PL: Logowanie kontem Microsoft się nie udało. EN: Signing in with a Microsoft account failed.
pub const ACCOUNT: Code = Code::new("HOT-E0904");

/// PL: `prepare` nie przygotował plików gry z Minecrafta gracza. EN: `prepare` did not prepare the
/// game files from the player's Minecraft.
pub const PREPARE: Code = Code::new("HOT-E0905");

/// PL: Instancji albo jej plików nie da się ustawić. EN: The instance or its files cannot be set up.
pub const INSTANCE: Code = Code::new("HOT-E0906");

/// PL: Pula slotów nie pobrała się albo ma inną sumę niż na serwerze. EN: The slot pool did not
/// download, or its checksum differs from the server's.
pub const SLOTS: Code = Code::new("HOT-E0907");

/// PL: Launcher nie uruchomił gry. EN: The launcher did not start the game.
pub const PLAY: Code = Code::new("HOT-E0908");

/// PL: Nowej wersji `hot-play` nie da się sprawdzić, pobrać albo wstawić; gra idzie na obecnej.
/// EN: A new `hot-play` version cannot be checked, downloaded or put in place; play goes on with
/// the current one.
pub const UPDATE: Code = Code::new("HOT-E0909");

/// PL: Zdanie dla gracza do kodu: co się stało i co zrobić. EN: A sentence for the player per code:
/// what happened and what to do.
#[must_use]
pub fn for_player(code: Code) -> &'static str {
    match code.as_str() {
        "HOT-E0901" => "Nie ma launchera Horizona i nie udało się go pobrać. Sprawdź internet i spróbuj jeszcze raz.",
        "HOT-E0902" => "Serwer nie odpowiada. Sprawdź internet; jeśli działa, serwer może być wyłączony: daj znać właścicielowi.",
        "HOT-E0903" => "Nie udało się zainstalować wersji Horizona, na której działa serwer. Sprawdź internet i spróbuj jeszcze raz.",
        "HOT-E0904" => "Logowanie do konta Microsoft się nie udało. Zaloguj się kontem, na którym jest kupiony Minecraft: Java Edition.",
        "HOT-E0905" => "Nie udało się przygotować plików gry z Twojego Minecrafta. Uruchom raz Minecraft 26.3 w oficjalnym launcherze albo zaloguj się kontem z kupioną grą.",
        "HOT-E0906" => "Nie udało się ustawić folderu gry. Wybierz [2] Napraw.",
        "HOT-E0907" => "Nie udało się pobrać bloków serwera. Wybierz [2] Napraw; jeśli to nie pomoże, wyślij plik z logiem właścicielowi serwera.",
        "HOT-E0908" => "Launcher nie uruchomił gry. Wybierz [2] Napraw i wyślij plik z logiem właścicielowi serwera.",
        "HOT-E0909" => "Nie udało się zaktualizować hot-play; gram na obecnej wersji.",
        _ => "Coś poszło nie tak. Wybierz [2] Napraw i wyślij plik z logiem właścicielowi serwera.",
    }
}
