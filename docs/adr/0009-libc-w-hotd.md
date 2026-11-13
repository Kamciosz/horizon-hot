# ADR 0009 — `libc` w `hotd`

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Hamulec MSPT (M6.2, `crates/hotd/src/brake.rs`) zatrzymuje ciężkie procesy tła sygnałem `SIGSTOP` wysłanym do ich grupy procesów i wznawia je `SIGCONT`. Biblioteka standardowa Rusta umie tylko zabić dziecko (`Child::kill`), nie wysłać dowolny sygnał do grupy. Pierwsza wersja wołała program `kill -s STOP -- -<grupa>`. Na Ubuntu 22.04 (procps 3.3.17) to działało, ale test w CI na `ubuntu-latest` (Ubuntu 24.04, procps 4.0.4) padł: proces nie stanął. W procps 4.0.x jest znany błąd czytania ujemnego numeru grupy ([procps#354](https://gitlab.com/procps-ng/procps/-/issues/354)): bierze tylko pierwszą cyfrę, więc grupa `-1234` staje się `-1`, czyli wszystkie procesy użytkownika, łącznie z grą. Nie wiadomo, które Ubuntu będzie na serwerze.

## Rozważane opcje

1. **`libc` 0.2 i `libc::kill(-grupa, sygnał)`** — jedno wywołanie systemowe, stałe sygnałów właściwe dla systemu (na macOS i Linuksie mają inne numery), bez parsowania tekstu.
2. **Wbudowane `kill` powłoki (`sh -c`)** — zachowanie zależy od powłoki (`dash`, `bash`) i jej wersji; to ten sam rodzaj ryzyka.
3. **Własna deklaracja `extern "C" fn kill`** — bez zależności, ale numery sygnałów trzeba by wpisać ręcznie dla każdego systemu.

## Decyzja

Wybieramy **opcję 1**: `libc = "0.2.191"` w `[workspace.dependencies]`, używane tylko w `hotd`. `signal` nie wysyła niczego do grupy 0 ani 1.

## Konsekwencje

- Jeden blok `unsafe` w `brake.rs`; `kill` nie czyta naszej pamięci, a zniknięta grupa daje tylko `ESRCH`.
- Test `background_work_waits_while_the_brake_holds` sprawdza w CI na Linuksie, że proces naprawdę stoi (`T` w `ps`).
