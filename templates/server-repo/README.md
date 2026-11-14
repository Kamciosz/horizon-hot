# Repo serwera horizon-hot

Każdy commit na gałęzi serwera trafia na serwer bez restartu. Ten plik opisuje, co gdzie położyć.

```
mods/<id>/Cargo.toml     mod z kodem (Rust, hot-sdk); buduje go CI (.github/workflows/hot.yml)
mods/<id>/src/…
mods/<id>/data/<przestrzeń nazw>/…   paczka danych: przepisy, łupy, tagi, funkcje, osiągnięcia
mods/<id>/assets/blocks/<nazwa>.png  nowy blok z tą teksturą
mods/<id>/assets/lang.json           nazwy bloków w grze: {"<nazwa>": "…"}
.github/workflows/hot.yml   kopia z /srv/horizon/server-repo-files/ na serwerze
```

`<id>`: małe litery, cyfry, `_` i `-`, do 64 znaków. Usunięcie katalogu `mods/<id>/` wyłącza mod i usuwa jego paczkę danych.

## Mod z kodem

CI pobiera horizon-hot do katalogu `.hot/` w tym repo, więc mod zależy od `hot-sdk` tak:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
hot-sdk = { path = "../../.hot/crates/hot-sdk" }
```

Do budowy u siebie: `git clone https://github.com/Kamciosz/horizon-hot .hot` i `.hot/` w `.gitignore`. Cały przykład krok po kroku: `docs/przewodniki/nowy-mod.md` w horizon-hot.

## Komendy modów z kodem

- Komendę gracza mod deklaruje w `hot_mod!(..., commands = "sklep")`. Wtedy działa w ticku gracza: `players()`, `tell` i `give` widzą gracza.
- Komenda bez deklaracji dochodzi tam, gdzie silnik gracza nie widzi; `hot-core` raz zapisze wtedy w logu ostrzeżenie z poprawką.
- Serwer nie sprawdza uprawnień do komend modów. Komendę tylko dla operatorów mod blokuje sam, po polu `permission` gracza.
- Dwa mody z tą samą komendą: dostaje ją ten, który był na serwerze wcześniej, a w logu jest ostrzeżenie.

## Paczka danych (`data/`)

- Pliki idą w `mods/<id>/data/<przestrzeń nazw>/…`, tak jak w `data/` zwykłej paczki danych, np. `mods/sklep/data/sklep/recipe/lada.json`.
- Bez `pack.mcmeta`: serwer dopisuje go sam.
- Mod z samym `data/` nie potrzebuje `Cargo.toml` i wchodzi bez czekania na CI.
- Każdy plik `.json` musi być poprawnym JSON-em, a `data/` nie może mieć dowiązań symbolicznych; inaczej commit nie wchodzi (`HOT-E0208`, powód w `hot status`).
- Wszystkie zmiany w `data/` z jednego commita wchodzą jednym `reload`. Na czas `reload` świat stoi (pomiar: `docs/sprzet.md` w horizon-hot); commit bez zmian w `data/` nie robi `reload`.
- Plik, który serwer pominął (np. nieznany przedmiot w przepisie), widać w `hot status` jako „plik pominięty przez silnik”.

## Nowe bloki (`assets/`)

- Plik `mods/<id>/assets/blocks/<nazwa>.png` (PNG, najlepiej 16×16) dodaje blok. Serwer daje mu wolne miejsce z puli i buduje paczkę zasobów, bez czekania na CI.
- Gracz widzi nowy wygląd po ponownym wejściu na serwer. Gra pyta wtedy o paczkę zasobów serwera; trzeba ją przyjąć.
- W kodzie moda blok nazywa się `<id>:<nazwa>`, np. `set_block(pos, "sklep:lada")`, `give(gracz, "sklep:lada", 1)`, a `block(pos)` zwraca tę samą nazwę.
- W plikach `data/` (przepisy, tagi, łupy) trzeba na razie podać nazwę z puli, `hot_slots:slot_<numer>`. Numer jest w `hot status` i w `hot-state/slots.json` na serwerze.
- Blok ma kolizję, światło i twardość kamienia. Kształt inny niż sześcian i własne zachowanie przy ticku jeszcze nie działają.
- Usunięcie pliku nie zwalnia miejsca w puli: gdy blok wróci, dostaje to samo. Plik, który nie jest PNG, albo zły `lang.json` zatrzymuje commit (`HOT-E0210`).
