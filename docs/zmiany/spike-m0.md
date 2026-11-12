## Changelog

### Dodane
- Wyniki prób S1–S5 w `docs/spike/`: `hot-abi` w paczce moda natywnego, pula slotów w `.hmod`, komenda przez `player_command`, `run_command` z `server_tick`, wygląd slotu z paczki serwera.
- S12: dwa supervisory na jednym świecie, pomiar przekazania (`docs/spike/s12-dwa-serwery-jeden-swiat.md`).
- S8, część 1: analiza kodu klienta — które pakiety pokazują ekran ładowania i jak podmienić silnik bez niego (`docs/spike/s8-podmiana-analiza-klienta.md`).
- S8, część 2: prototyp proxy podmiany silnika (`spikes/s8-podmiana/proxy/`), pomiar pauzy i próba kontrolna bez zamiany id (`docs/spike/s8-podmiana-prototyp.md`). Trzeci przebieg: nowy silnik startuje po zniknięciu procesu starego (pauza 0,96–1,07 s), testy chunków, ruchu w przerwie i komunikatu „joined”.
- `docs/podmiana.md`: jak `hot-proxy` i `hotd` przenoszą graczy na nowy silnik.
- S7: koszt puli slotów 4096/8192 (start klienta i serwera, pamięć, pauza podmiany), generator puli i skrypty pomiaru (`spikes/s7-rozmiar-puli/`, `docs/spike/s7-rozmiar-puli.md`).
- `docs/sloty.md`: czym jest pula slotów, ile kosztuje i jak ją sprawdzić.
- S6: proxy wysyła paczkę zasobów w trakcie gry (`--pack-file` w `spikes/s8-podmiana/proxy/`), pomiar przeładowania i zrzuty nakładki (`docs/spike/s6-paczka-w-trakcie-gry.md`).
- S11: przegląd 10 ostatnich wydań Horizona: API haków, `content_identity`, kanały `horizon:*`, protokół, `libloading`, cache treści modów, przypięcie modów natywnych do wersji (`docs/aktualizacje.md`).
- `docs/adr/0002-spike.md`: wyniki prób S1–S12, poprawki decyzji i otwarte punkty z krokiem, który je rozstrzygnie.
- `crates/hot-diag`: błąd z polami D27, zapis JSONL z rotacją, maskowanie sekretów, panika w logu, szkielet `hot report` (`hot-diag report`). Zależność `serde_json` (`docs/adr/0003-serde-json-w-hot-diag.md`).
- `docs/bledy/HOT-E0001.md`, `HOT-E0002.md` i zakres `E00xx`; test zgodności kodów z katalogiem w CI.
- Kod prób w `spikes/` (poza workspace, budowany launcherem Horizona albo `cargo horizon-mod`).
- `crates/hot-abi`: ABI v1 (`ModVTable`, `HostApi`, `Event` z `kind`, `PlayerInfo`), test układu struktur w pamięci; ADR 0006 opisuje kształt ABI.
- `crates/hot-sdk`: cecha `HotMod`, makro `hot_mod!`, `Host<'call>` z funkcjami gospodarza, łapanie paniki w każdym eksporcie z miejscem i backtrace w logu.
- `crates/hot-core`: `hot load`, `hot unload`, `hot list`; wczytanie pliku w tle, przełączenie generacji na granicy ticku, stan przekazywany między generacjami, powrót do poprzedniej generacji po panice, próba 6000 ticków (D13), `hot-state/mods.json` i `status.json` zapisywane atomowo, log `logs/hot-core.jsonl`. Zależność `libloading` (ADR 0005). Kody `HOT-E0401`–`HOT-E0405` w `docs/bledy/`.
- `crates/hot-core/tests/reload.rs`: 11 testów na prawdziwych modach `cdylib` z udawanym gospodarzem, w tym 50 przeładowań z rzędu.
- `hot-diag`: wpisy `info` i `warn` (`Logger::info`, `Logger::warn`).

- `crates/hot-host`: mod natywny, który ładuje `hot-core` przy starcie serwera, przekazuje mu haki Horizona i podmienia rdzeń komendą `hot core load` bez restartu; paczkę składa `build-bundle.sh`.
- `examples/licznik`: przykładowy mod gorący; licznik ticków przechodzi przez przeładowanie, `licznik panika` pokazuje powrót do poprzedniej generacji.
- `hot <mod> …` przekazuje komendę konsoli tylko temu modowi.
- Kody `HOT-E0406` (rdzeń się nie wczytał) i `HOT-E0407` (rdzeń spanikował) w `docs/bledy/`.
- ADR 0007: logi `hot-host`, komendy modów i pauza pustego serwera.

### Zmienione
- `hot-core` przełącza gotowe wczytania także przy komendzie konsoli, bo pusty serwer w pauzie nie wywołuje ticku.
- `AGENTS.md`: zasady pracy na maszynie z grą (bez `prepare` przez agenta, bez zrzutów gry w repo, bez ruszania cudzych instancji), `spikes/` i `docs/spike/` w mapie repo, zmierzona pauza podmiany zamiast „mrugnięcia”; to samo w `docs/ARCHITECTURE.md`. `PLAN.md`, D2: `hot-host` nie używa `hot-diag`; D9 i D16 doprecyzowane.
- `PLAN.md`, D6, D8, D9, D16, D18 i D24 według ADR 0002: odpowiedź na komendę przez `event.replies`, wersja `hot-slots` przy każdej zmianie puli, `servers.dat` instancji z `acceptTextures`, pamięć na drugi silnik tylko przy teście aktualizacji, każde Nightly jako zmiana dla klienta do testu w M9, zmierzone czasy w tabeli D24; M0.14 odhaczone.
- `PLAN.md`, D18: build aktualizacji z `--version <Nightly> --ignore-pins`, bo `hot-host` przypina wersję Horizona; R23 i R24 poprawione; ryzyko i szansa: Horizon sam wprowadza mody bez restartu; M0.12 odhaczone.
- `PLAN.md`, D11–D13: podmiana bez `player-export` i bez „zapomnij chunki”, kolejność stop → start bez czekania na komendę, zmierzona pauza; M0.9 odhaczone.
- `PLAN.md`, D9: nowy wygląd tylko przez paczkę wysłaną w grze (z nakładką) albo przy ponownym wejściu; decyzja, kiedy ją wysyłać; M0.7 odhaczone.
- `docs/podmiana.md`, S8: podmiana sprawdzona zrzutami klatki, bo `screen` nie widzi nakładki.
- `PLAN.md`, D8, D9 i D11: wyniki S7 (pauza podmiany z pulą, zimny cache przy nowym silniku, klucz nazwy przedmiotu); decyzja o rozmiarze puli przed M5.1; M0.8 odhaczone.
- `PLAN.md`, D8 i D9: `hot-slots.hmod` bez `models/`, `textures/`, `lang/`, bo treść modów zasłania paczkę serwera; `require-resource-pack = true` i zgoda na paczkę ustawiana przez `hot-play`.
- `PLAN.md`, sekcja 8: dwa budżety pauzy (ten sam silnik, nowy plik silnika), stan zasobów prób po M0, otwarte luki maskowania w raporcie (IPv6, ścieżki domowe).
- Budżet pauzy podmiany silnika: krócej niż 1 s w każdym przypadku (decyzja użytkownika, ADR 0004); rozgrzany silnik w D11 i nowy krok M0.19 (S13).

### Uwagi
- `Host::tell` w haku `player_command` na serwerze dedykowanym zwraca `false`; odpowiedzi idą przez `event.replies`. Zachowanie `tell` poza hakiem gracza do sprawdzenia w M4.
