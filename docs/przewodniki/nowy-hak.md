# Nowy hak w ABI

Hak to zdarzenie z silnika, które dostają mody gorące (np. zniszczenie bloku). Droga zdarzenia: Horizon → `hot-host` (mod natywny w silniku) → `hot-core` → mody. Zasady: ADR 0017, ADR 0019, kształt ABI: ADR 0006.

## Zanim zaczniesz

- Sprawdź, czy Horizon ma taki hak w przypiętej rewizji (`HORIZON_REV` w `scripts/server-install.sh`): lista haków to cecha `Hooks` w API modów natywnych Horizona (`docs/native-mods.md` w źródłach Horizona tej rewizji). Haka, którego Horizon nie ma, nie da się dodać po naszej stronie.
- Zmiana `hot-host` wymaga przebudowy silnika, a na to zgodę daje tylko właściciel (`--trust-native-code`). Agent jej nie daje. Serwer nie zmieni `hot-host` sam przy aktualizacji (ADR 0028).

## Kroki

1. **`crates/hot-abi/src/lib.rs`**: nowa stała w module `kind` z następnym wolnym numerem i opisem pól `Event`, których używa. Nowy rodzaj zdarzenia jest zgodny wstecz: stary mod dostaje go jako `Event::Unknown` i go pomija, więc `ABI_MAJOR` zostaje. Podbij `ABI_MAJOR` tylko wtedy, gdy zmieniasz układ `Event`, `ModVTable` albo `HostApi`; wtedy popraw też `crates/hot-abi/tests/layout.rs` i `abi_version.rs`.
2. **`crates/hot-host/api/src/lib.rs`**: metoda haka w `impl Hooks for HotHost`. Zdarzenia z blokiem i przedmiotem idą przez `world_event`, inne przez `Event::new(kind::…)` i `send` (wzór: `destroy_block`, `player_logged_in`). Gdy hak może anulować akcję, zwróć `Flow` z wyniku.
3. **`crates/hot-sdk/src/lib.rs`**: wariant w `enum Event` (jest `#[non_exhaustive]`, więc mody muszą mieć gałąź `_`) i jego zamiana z `hot_abi::Event` obok `kind::DESTROY_BLOCK`.
4. **`crates/hot-core/src/core.rs`**: zwykle nic, bo `hot-core` przekazuje każde zdarzenie wszystkim modom. Zmiana jest potrzebna tylko wtedy, gdy rdzeń ma coś zrobić sam (jak z `PLAYER_TICK`).
5. **Test** w `crates/hot-core/tests/reload.rs`: mod testowy dostaje nowe zdarzenie, a anulowanie wraca do gospodarza.
6. **Dokumentacja**: lista haków w ADR 0019 (nowy ADR, jeśli zmienia decyzję), komentarz w `hot-abi`, wpis w `docs/zmiany/<gałąź>.md`.

## Sprawdzenie

- Bez Horizona: bramka repo (`cargo fmt`, `cargo clippy`, `cargo test`) i `rustfmt --edition 2021 --check crates/hot-host/api/src/lib.rs`. `hot-host` nie należy do workspace (zależy od API Horizona), więc `cargo` go nie kompiluje.
- Z Horizonem, na komputerze właściciela: `crates/hot-host/build-bundle.sh <katalog>`, potem `horizon-launcher native-build <katalog> --server-output <silnik> --version <Nightly> --trust-native-code`, start silnika i mod testowy, który loguje nowe zdarzenie. Bez tego kroku hak jest niesprawdzony; zapisz to w opisie zmiany.
