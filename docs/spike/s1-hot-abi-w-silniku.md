# S1 — czy mod natywny może używać naszego crate'a `hot-abi`

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, launcher 0.2.49, macOS arm64.
- Krok planu: M0.2. Wynik: **potwierdzone**.

## Problem

`hot-host` to trait mod (paczka z `horizon-mixin.toml` i pakietem `[api]`), który launcher Horizona kopiuje do swojego workspace (`native/stage/native-api/<id>/`) i kompiluje razem z silnikiem. Musi używać typów z `crates/hot-abi`. Zwykła zależność ścieżkowa do naszego repo nie przetrwa kopiowania.

## Co sprawdzono

Paczka `spikes/s1-hot-abi-crate/`:

- `hot-abi` dołączony do paczki jako `[[crate]]` (`id = "hot_abi"`, `path = "hot_abi"`). Launcher umieszcza go w `native-crates/<id moda>/<id crate'a>/` i dodaje do workspace Horizona.
- Pakiet `[api]` zależy od niego ścieżką liczoną od miejsca po skopiowaniu: `hot-abi = { path = "../../native-crates/spike_hot_s1/hot_abi" }`.
- Hak `server_about_to_start` wypisuje `hot_abi::ABI_MAJOR`.

Kopię `crates/hot-abi` do paczki robi się przed buildem (w repo próby jej nie ma, żeby nie dublować kodu).

## Komendy i wynik

```bash
horizon-launcher native-build spike-hot-s1 --server-output s1-server \
  --version nightly-2026-11-05-66f3981 --trust-native-code
# Built in 2 min 16 s (performance, incremental)

horizon-server --dir s1-srv init --eula --world world --port 25611 --offline --yes --no-whitelist
horizon-server --dir s1-srv config set engine.path=<s1-server>/horizon-engine
horizon-server --dir s1-srv start
```

Log serwera:

```
spike-hot-s1: hot_abi::ABI_MAJOR = 1
[15:39:21] [Server thread/INFO]: Done (0.889s)! For help, type "help"
```

## Wnioski

1. `[[crate]]` + ścieżka `../../native-crates/<mod>/<crate>` działa. Plan B (kopiowanie kodu do pakietu `[api]`) niepotrzebny.
2. `[[crate]]` nie może mieć zależności ścieżkowych ani git, a zależności z rejestru tylko z `Cargo.lock` Horizona (`docs/native-mods.md`, „Bundle crates”). `hot-abi` nie ma zależności i musi tak zostać.
3. Workspace Horizona nie ma `version` w `[workspace.package]`, więc `crates/hot-abi/Cargo.toml` ma pola wpisane wprost, bez `*.workspace = true`. `[lints] workspace = true` zostaje: w naszym workspace bierze nasze reguły, u Horizona — jego.
4. Paczkę `hot-host` trzeba składać skryptem (kopia `crates/hot-abi` do paczki), bo launcher przyjmuje tylko ścieżki wewnątrz paczki. Krok dochodzi do M1.3.
5. Ścieżka w `[api]` wskazuje układ katalogów launchera. Jeśli Horizon go zmieni, build `hot-host` przestanie działać — wyłapie to test aktualizacji (D18).
