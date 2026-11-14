# ADR 0026 — aktualizacja modów i klasyfikacja zmian (D17, D24)

- **Data:** 2026-11-14 (decyzje z 2026-11-12, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Właściciel zmienia serwer commitem w repo serwera. Różne pliki wymagają różnych działań: dane wystarczy przeładować, kod trzeba zbudować w CI, wygląd wymaga nowej paczki zasobów. Serwer nie może wystawiać otwartego portu na webhook.

## Decyzja

- **D17.** `hotd run` co 10 s (`--interval`) pobiera repo serwera gitem, bez webhooka i bez otwartego portu. Liczy zmiany od ostatniego zastosowanego commita i stosuje je według tabeli niżej. Stan: `hot-state/hotd.json`.
- **D24. Klasyfikacja** (`crates/hotd/src/classify.rs`, czysta funkcja z testami):

| Zmiana w repo serwera | Działanie | Co widzi gracz |
|---|---|---|
| `mods/<id>/data/` | kopia do paczki danych świata i `reload` (ADR 0020) | nic; na czas `reload` świat stoi |
| `mods/<id>/` poza `data/` i `assets/` (kod) | podpisany plik z gałęzi `hot-builds` → nowa generacja (ADR 0018, ADR 0024) | nic |
| `mods/<id>/assets/` | przydział slotów i nowa paczka zasobów (ADR 0021) | nowy wygląd po ponownym wejściu |
| nowy albo usunięty mod | `init` albo `deactivate` | nic |
| `native/` | tylko odnotowane w `hot status`; budowy i podmiany jeszcze nie ma | nic |
| `hot.toml` | tylko odnotowane; ustawień z repo `hotd` jeszcze nie czyta | nic |

## Konsekwencje

- Zmiana trafia na serwer najpóźniej ok. 10 s po wyniku CI.
- Commit, który się nie stosuje, nie blokuje następnych: następny liczy zmiany od ostatniego udanego.
- `hot-core` aktualizuje się razem z horizon-hot (ADR 0028), a Horizon osobną drogą (ADR 0027), nie przez repo serwera.
