# Katalog błędów

Każdy błąd, który może zobaczyć operator albo gracz, ma tu plik `HOT-EXXXX.md`. Kod błędu stoi w logu (pole `code`) i w raporcie z `hot report`. Pole `doc` w logu wskazuje ten plik.

Cel: osoba albo model bez wiedzy o projekcie naprawia błąd z samego wpisu w logu i pliku stąd.

## Zakresy kodów

| Zakres | Obszar |
|---|---|
| `E00xx` | sama diagnostyka (`hot-diag`): zapis logu, panika |
| `E01xx` | instalator serwera i kontener |
| `E02xx` | `hotd` i repo serwera: git, CI, podpisy, paczka danych, sloty, paczka zasobów, dysk, świat |
| `E03xx` | budowa silnika z `hot-host` |
| `E04xx` | mody gorące, `hot-core`, `hot-host` |
| `E06xx` | `hot-proxy` i podmiana silnika |
| `E07xx` | aktualizacje Horizona i horizon-hot |
| `E09xx` | aplikacja gracza `hot-play` |

Zakresy `E05xx`, `E08xx` i `E10xx` są wolne. Nowy kod dostaje następny numer w zakresie składnika, który go zgłasza (`docs/przewodniki/nowy-kod-bledu.md`).

## Pola wpisu w logu

| Pole | Treść |
|---|---|
| `code` | stały kod błędu |
| `component`, `version` | składnik i commit, z którego go zbudowano |
| `what` | co się stało, pełnym zdaniem |
| `expected` | co powinno się stać |
| `context` | identyfikatory do odtworzenia: mod, generacja, commit repo serwera, wersja Horizona, ścieżki |
| `cause` | najbardziej prawdopodobna przyczyna, jeśli znana |
| `fix` | konkretny następny krok: komenda albo plik do zmiany |
| `doc` | ścieżka do pliku w tym katalogu |
| `source` | plik i linia w kodzie, które zgłosiły błąd |
| `chain` | łańcuch przyczyn, przy panice backtrace |
| `state` | co system zrobił sam i w jakim stanie zostawił serwer |

## Wzór pliku `HOT-EXXXX.md`

```markdown
# HOT-EXXXX — krótka nazwa

## Znaczenie
Co się stało, w jednym akapicie, bez skrótów projektowych.

## Typowe przyczyny
1. ...

## Diagnostyka
Komendy, które potwierdzą przyczynę, z oczekiwanym wynikiem.

## Naprawa
Kroki dla każdej przyczyny.

## Co system robi sam
Reakcja automatyczna i stan, w jakim zostaje serwer.

## Kod
Pliki, które zgłaszają ten błąd.
```

Kod błędu raz nadany nie zmienia znaczenia. Usunięty błąd zostawia plik z dopiskiem „nieużywany od wersji X”.
