# ADR 0001 — Struktura projektu: workspace Rust z osobnymi crate na składnik

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Projekt ma kilka programów działających w różnych miejscach: w silniku Horizon, jako usługi na serwerze i na komputerach graczy. Część z nich (mody, `hot-core`) ładuje się dynamicznie i musi mieć stabilną granicę. Projekt będą rozwijać różne osoby i agenci AI, często równolegle.

## Rozważane opcje

1. **Jeden workspace Cargo, jeden crate na składnik** — wspólne zależności i jedna bramka CI; równoległa praca bez konfliktów, bo każdy rusza swój crate.
2. **Osobne repozytoria na składnik** — niezależne wydania, ale zmiana ABI wymaga zsynchronizowanych PR-ów w kilku repo.
3. **Jeden duży crate** — najprościej na start, ale moduły dynamiczne i tak muszą być osobnymi `cdylib`.

## Decyzja

Wybieramy **opcję 1**. Rust (jak Horizon), edycja 2021, wersja stabilna z `rust-toolchain.toml`. Granica między gospodarzem a modami to osobny crate `hot-abi`. Wspólne reguły jakości w `[workspace.lints]`.

## Konsekwencje

- Jedno `cargo test --workspace` sprawdza wszystko.
- Zmiana `hot-abi` od razu psuje build modów, które jej nie uwzględniają — to zamierzone.
- `hot-host` (mod natywny) nie jest członkiem workspace: buduje go launcher Horizona razem z silnikiem.
- Warunek zmiany decyzji: gdy wydania składników muszą się rozjechać (np. aplikacja gracza częściej niż serwer).
