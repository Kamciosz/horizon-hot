# Nowy kod błędu

Każdy błąd, który zobaczy operator albo gracz, ma stały kod `HOT-EXXXX` i opis w `docs/bledy/`. Zasady: ADR 0032 (D27).

## Kroki

1. **Zakres.** Weź następny wolny numer w zakresie składnika (`ls docs/bledy/`): `E00` diagnostyka, `E01` instalacja, `E02` git, CI, dane, sloty, paczka i świat, `E03` budowa silnika, `E04` mody gorące, `E06` proxy i podmiana, `E07` aktualizacje, `E09` aplikacja gracza.
2. **Stała** w `codes.rs` składnika (`crates/hotd/src/codes.rs`, `crates/hot-core/src/codes.rs`, `crates/hot-proxy/src/codes.rs`, `crates/hot-play/src/codes.rs`, `crates/hot-diag/src/codes.rs`):

   ```rust
   pub const NAZWA: Code = Code::new("HOT-E0213");
   ```

3. **Wpis w logu** przez `DiagError::new(kod, co się stało, co zrobić)`. Bez tych trzech pól kod się nie skompiluje. Dołóż `expected`, `caused_by`, `context` z identyfikatorami potrzebnymi do odtworzenia i `state`: co system zrobił sam. Wzór: `crates/hotd/src/data.rs`. Treść po angielsku, pełnymi zdaniami, bez skrótów projektu. W instalatorze to samo robi `fail HOT-E01xx …` albo `wait_for_operator` (kod wyjścia 3, gdy coś musi zrobić operator).
4. **Opis** `docs/bledy/HOT-E0213.md` z sekcjami: `## Znaczenie`, `## Typowe przyczyny`, `## Diagnostyka` (polecenia do skopiowania), `## Naprawa`, `## Co system robi sam`, `## Kod` (pliki i funkcje). Wzór: `docs/bledy/HOT-E0208.md`.
5. **Test** sytuacji, w której błąd powstaje, z asercją na kod.

## Sprawdzenie

`cargo test -p hot-diag --test catalog` sprawdza w obie strony: kod użyty w `crates/` albo `scripts/` bez pliku w `docs/bledy/` i plik bez użycia w kodzie dają czerwony test, tak samo jak brak którejś sekcji.
