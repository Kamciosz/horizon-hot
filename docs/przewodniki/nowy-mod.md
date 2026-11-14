# Pierwszy mod gorący

Mod gorący to biblioteka Rusta (`cdylib`), którą serwer wczytuje w trakcie gry, bez restartu. Ten przewodnik prowadzi od pustego repo serwera do komendy `/witaj` w grze. Zasady działania: ADR 0017–0019.

## 1. Repo serwera

Repo serwera to zwykłe repo na GitHubie z katalogiem `mods/`. Wzór i pełny opis plików: `templates/server-repo/README.md`. Plik `.github/workflows/hot.yml` instalator zapisał na serwerze w `/srv/horizon/server-repo-files/`; skopiuj go do repo i dodaj w ustawieniach repo sekret `HOT_SIGNING_KEY` (instalator wypisał jego treść).

CI pobiera horizon-hot do katalogu `.hot/` w repo serwera. Do budowy u siebie zrób to samo:

```sh
git clone https://github.com/Kamciosz/horizon-hot .hot
printf '.hot/\ntarget/\n' >> .gitignore
```

## 2. Mod

`mods/witaj/Cargo.toml`:

```toml
[package]
name = "witaj"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
hot-sdk = { path = "../../.hot/crates/hot-sdk" }
```

`mods/witaj/src/lib.rs`:

```rust
use hot_sdk::{Event, Flow, Host, HotMod};

pub struct Witaj;

impl HotMod for Witaj {
    fn init(_host: &mut Host<'_>, _state: &[u8]) -> Self {
        Witaj
    }

    fn on_event(&mut self, host: &mut Host<'_>, event: &Event<'_>) -> Flow {
        match event {
            Event::Command { command, .. } if command.trim() == "witaj" => {
                host.reply("Cześć! Ten mod wszedł na serwer bez restartu.");
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }

    fn save(&mut self, _host: &mut Host<'_>) -> Vec<u8> {
        Vec::new()
    }
}

hot_sdk::hot_mod!(Witaj, id = "witaj", version = env!("CARGO_PKG_VERSION"), commands = "witaj");
```

- Nazwa katalogu `mods/<id>/`: małe litery, cyfry, `_` i `-`, do 64 znaków.
- `commands = "witaj"` deklaruje komendę gracza. Bez tego `tell`, `give` i `players()` nie widzą gracza w trakcie komendy (ADR 0019, D6a).
- `save` oddaje stan nowej wersji moda przy przeładowaniu, a `init` dostaje go w `state` (przykład: `examples/licznik`).
- Panika w modzie nie zabija serwera: wyłącza tę wersję moda i wraca do poprzedniej (ADR 0018).
- Więcej przykładów: `examples/sklep` (komendy, przedmioty), `examples/teleport-blok` (bloki, kliknięcia). Lista zdarzeń i funkcji: `crates/hot-sdk/src/lib.rs` (`Event`, `Host`).

## 3. Sprawdzenie u siebie

```sh
ssh-keygen -q -t ed25519 -N '' -f /tmp/klucz-testowy
HOT_SIGNING_KEY_FILE=/tmp/klucz-testowy bash .hot/scripts/hot-build.sh . /tmp/hot-out
```

Dobry wynik: `hot-build: <commit> built for <platforma>: witaj` i `"ok": true` w `/tmp/hot-out/<commit>/build.json`. Wynik trafia do katalogu nazwanego numerem commita `HEAD`, więc repo musi mieć choć jeden commit. To ten sam skrypt, który uruchamia CI.

## 4. Na serwer

```sh
git add mods/witaj .gitignore && git commit -m "Mod witaj" && git push
```

CI buduje mod na Ubuntu, podpisuje go i wypycha na gałąź `hot-builds`. `hotd` na serwerze sprawdza ją co 10 s, sprawdza podpis i wczytuje mod. Na serwerze:

```sh
sudo -u horizon /srv/horizon/bin/hotd status --home /srv/horizon/server
```

W grze: `/witaj`. Zmień tekst, wypchnij commit, wpisz `/witaj` jeszcze raz: nowa odpowiedź bez wychodzenia z gry.

## Gdy nie działa

- Czerwone CI: błąd kompilacji jest w logu przebiegu i w `hot status` na serwerze (`HOT-E0202`). Serwer zostaje przy poprzedniej wersji moda.
- `hot status` mówi o złym podpisie: sekret `HOT_SIGNING_KEY` nie pasuje do klucza z instalacji (`HOT-E0203`).
- Pozostałe kody: `docs/bledy/`.

## Dane, wygląd i nowe bloki

- Przepisy, łupy, tagi: `mods/<id>/data/<przestrzeń nazw>/…` (ADR 0020). Mod z samym `data/` nie potrzebuje `Cargo.toml`.
- Nowy blok z teksturą: `mods/<id>/assets/blocks/<nazwa>.png` (ADR 0021, `docs/sloty.md`). Nowy wygląd gracz widzi po ponownym wejściu.
