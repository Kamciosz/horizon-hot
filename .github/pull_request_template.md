## Co zmienia

<!-- Krótko: co robi zmiana i którego składnika dotyczy. -->

## Po co

<!-- Problem albo zgłoszenie (#numer). Dlaczego tak, a nie inaczej. -->

## Jak sprawdzone

<!-- Komendy i wynik. Test, który obleje bez tej zmiany. -->

## Ryzyka

<!-- Co może się zepsuć w innych składnikach. Czy zmienia ABI, format danych albo zachowanie przy aktualizacji. -->

## Lista

- [ ] `cargo fmt`, `clippy` i `test` przechodzą (komendy w `AGENTS.md`)
- [ ] nowe błędy mają kod `HOT-EXXXX` i wpis w `docs/bledy/`
- [ ] plik `docs/zmiany/<gałąź>.md` z wpisem do changelogu
- [ ] dokumentacja zmieniona w tym samym PR (README, ARCHITECTURE, ADR, przewodniki)
- [ ] brak martwego kodu, `println!` do debugowania i plików tymczasowych
