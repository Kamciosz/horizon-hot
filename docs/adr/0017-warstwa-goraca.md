# ADR 0017 — warstwa gorąca: gospodarz w silniku, `hot-core` i C ABI (D1, D2, D3)

- **Data:** 2026-11-14 (decyzje z 2026-11-12, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Mody mają się zmieniać w trakcie gry, bez restartu serwera i bez rozłączania graczy. Horizon (silnik) to program w Rust, a jego mody natywne wkompilowuje się w silnik przed budową, więc każda ich zmiana oznacza nowy plik silnika. Rust nie ma stabilnego ABI między dwoma kompilacjami: biblioteka zbudowana innym kompilatorem albo z inną wersją zależności może mieć inny układ struktur, a każda strona ma własną stertę i własny hak paniki.

## Decyzja

- **D1.** W silnik wkompilowujemy jeden mały mod natywny, `hot-host` (`crates/hot-host`, paczka `horizon-mixin.toml`). Mody gorące to biblioteki `cdylib` (`.so` na serwerze, `.dylib` na Macu w testach), ładowane w trakcie gry przez `libloading` (ADR 0005). Mod gorący robi tylko to, co dają haki i funkcje gospodarza (ADR 0019). Nowe bloki i przedmioty dostaje przez sloty (ADR 0021).
- **D2.** `hot-host` jest cienki i stały: ładuje `hot-core` (też `cdylib`) i przekazuje mu wszystkie haki. `hot-core` zarządza modami, slotami i komendą `/hot` i sam przeładowuje się jak mod gorący (`hot core load`), więc zmiana logiki gospodarza nie wymaga nowego silnika. `hot-host` nie używa `hot-diag`: paczka moda natywnego nie może mieć zależności ścieżkowych, a z rejestru tylko te, które są w `Cargo.lock` Horizona (próba S1, ADR 0002). Własne błędy (`HOT-E0406` rdzeń się nie wczytał, `HOT-E0407` rdzeń spanikował) `hot-host` pisze przez `log` z celem `horizon_hot` do `logs/latest.log` silnika (ADR 0007).
- **D3.** Granica między gospodarzem a modami to stabilne C ABI w `crates/hot-abi`: struktury `#[repr(C)]`, napisy jako `(ptr, len)` pożyczane na czas wywołania, każda strona zwalnia własną pamięć. Mod eksportuje jedną funkcję, `hot_mod_entry() -> *const ModVTable`, a pierwsze pole tabeli to `abi`. Inne `ABI_MAJOR` niż u gospodarza (dziś 2) oznacza odmowę załadowania. W `PLAN.md` była jeszcze osobna funkcja `hot_mod_abi`; wersja w tabeli wystarcza. Kształt tabel: ADR 0006.

## Konsekwencje

- Mod gorący buduje się dowolnym stabilnym Rustem, niezależnie od kompilatora silnika.
- Każdy hak i każdą funkcję gospodarza trzeba przetłumaczyć ręcznie przez ABI, dlatego lista haków jest krótka. Nowy hak: `docs/przewodniki/nowy-hak.md`.
- Zmiana `hot-abi` wymaga nowego `ABI_MAJOR`, przebudowy wszystkich modów i nowego silnika (bo `hot-host` ma kopię `hot-abi`).
- `hot-host` zależy od API modów natywnych Horizona. Używamy tylko nazwanych haków, nie łatek na dowolne funkcje, bo te zmieniają się z każdym Nightly.
