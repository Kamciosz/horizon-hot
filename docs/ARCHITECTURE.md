# Architektura

## Po co ten dokument
Żeby nowa osoba (albo Ty po miesiącu) wiedziała, gdzie co leży i **dlaczego** tak, bez czytania całego kodu.

## Moduły
| Moduł | Odpowiada za | Zależy od | Uwagi |
|---|---|---|---|
| `src/.../modul-a` | ... | ... | ... |

## Przepływ danych
```
[wejście] → [moduł A] → [moduł B] → [wyjście]
```
Opisz 3–5 najważniejszych ścieżek (np. „żądanie HTTP → walidacja → serwis → baza").

## Decyzje i kompromisy
- Co wybraliśmy i dlaczego (albo link: `docs/adr/0001-*.md`).
- Czego świadomie **nie** robimy (i kiedy warto do tego wrócić).

## Rozwój
- Gdzie dodać nową funkcję (który katalog, jaki wzorzec).
- Co jest kruche / wymaga ostrożności przy zmianach.
- Znane długi techniczne (`TODO` z uzasadnieniem).
