# ADR 0020 — dane modów jako paczka danych świata (D7)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, przeniesiona z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Przepisy, łupy, tagi i funkcje to w grze dane, nie kod. Silnik umie je przeładować komendą `reload` bez restartu (R13), więc nie ma powodu budować ich w CI ani wkładać w bibliotekę moda.

## Decyzja

`hotd` kopiuje `mods/<id>/data/` z repo serwera do `<świat>/datapacks/hot-<id>/`, dopisuje `pack.mcmeta` i wysyła jeden `reload` na commit (`crates/hotd/src/data.rs`). Usunięcie moda usuwa jego paczkę. Zły JSON albo dowiązanie symboliczne zatrzymuje commit, zanim silnik cokolwiek dostanie (`HOT-E0208`). Paczka, której silnik nie włączył, wraca do poprzedniej wersji. Plik pominięty przez silnik jest tylko ostrzeżeniem w `hot status`.

## Konsekwencje

- Mod z samym `data/` wchodzi bez czekania na CI.
- Na czas `reload` świat stoi (pomiar: `docs/sprzet.md`); commit bez zmian w `data/` nie robi `reload`.
- Przepisy wskazujące bloki modów muszą na razie używać nazw z puli, `hot_slots:slot_<numer>` (ADR 0021).
