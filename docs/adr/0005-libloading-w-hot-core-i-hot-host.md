# ADR 0005 — `libloading` w `hot-core` i `hot-host`

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

`hot-core` wczytuje pliki modów gorących (`.so`, `.dylib`) w trakcie gry, a `hot-host` w ten sam sposób wczytuje `hot-core` (D2). Biblioteka standardowa Rusta nie ma `dlopen`. Zasada repo: nowa zależność wymaga ADR. `hot-host` jest paczką moda natywnego, więc może używać tylko zależności z `Cargo.lock` Horizona (S1, R1).

## Rozważane opcje

1. **`libloading` 0.8.9** — ta sama wersja jest w `Cargo.lock` Horizona `66f3981` i we wszystkich sprawdzonych Nightly (S11). Działa na Linuksie, macOS i Windows, a błędy `dlopen` oddaje jako `std::error::Error`.
2. **Własne wywołania `dlopen`/`dlsym` przez `libc`** — mniej kodu z zewnątrz, ale osobny kod dla każdego systemu i ręczne odczytywanie `dlerror`.
3. **Statyczne linkowanie `hot-core` w `hot-host`** — sprzeczne z D2: zmiana rdzenia wymagałaby nowego silnika.

## Decyzja

Wybieramy **opcję 1**: `libloading = "0.8.9"` w `[workspace.dependencies]`, używane w `hot-core`. `hot-host` bierze tę samą wersję z `Cargo.lock` Horizona.

Wczytanej biblioteki nigdy nie zamykamy (`std::mem::forget`). Stara generacja może mieć jeszcze wątki albo dane w pamięci, a mody natywne Horizona też nie są wyładowywane (R4). Pamięć odzyskuje podmiana silnika (D4, D11).

## Konsekwencje

- Każda generacja moda zajmuje pamięć do końca życia procesu silnika. Test `fifty_reloads_in_a_row_keep_the_counter` (`crates/hot-core/tests/reload.rs`) robi 50 przeładowań w jednym procesie.
- Jeśli `libloading` zniknie z `Cargo.lock` Horizona, `hot-host` się nie zbuduje, a serwer zostanie na starej wersji (`PLAN.md`, sekcja 6).
