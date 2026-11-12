# Repo serwera horizon-hot

Każdy commit na gałęzi serwera trafia na serwer bez restartu. Ten plik opisuje, co gdzie położyć.

```
mods/<id>/Cargo.toml     mod z kodem (Rust, hot-api); buduje go CI (.github/workflows/hot.yml)
mods/<id>/src/…
mods/<id>/data/<przestrzeń nazw>/…   paczka danych: przepisy, łupy, tagi, funkcje, osiągnięcia
.github/workflows/hot.yml   kopia z /srv/horizon/server-repo-files/ na serwerze
```

`<id>`: małe litery, cyfry, `_` i `-`, do 64 znaków. Usunięcie katalogu `mods/<id>/` wyłącza mod i usuwa jego paczkę danych.

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
