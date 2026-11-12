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
- Kod prób w `spikes/` (poza workspace, budowany launcherem Horizona albo `cargo horizon-mod`).

### Zmienione
- `PLAN.md`, D18: build aktualizacji z `--version <Nightly> --ignore-pins`, bo `hot-host` przypina wersję Horizona; R23 i R24 poprawione; ryzyko i szansa: Horizon sam wprowadza mody bez restartu; M0.12 odhaczone.
- `PLAN.md`, D11–D13: podmiana bez `player-export` i bez „zapomnij chunki”, kolejność stop → start bez czekania na komendę, zmierzona pauza; M0.9 odhaczone.
- `PLAN.md`, D9: nowy wygląd tylko przez paczkę wysłaną w grze (z nakładką) albo przy ponownym wejściu; decyzja, kiedy ją wysyłać; M0.7 odhaczone.
- `docs/podmiana.md`, S8: podmiana sprawdzona zrzutami klatki, bo `screen` nie widzi nakładki.
- `PLAN.md`, D8, D9 i D11: wyniki S7 (pauza podmiany z pulą, zimny cache przy nowym silniku, klucz nazwy przedmiotu); decyzja o rozmiarze puli przed M5.1; M0.8 odhaczone.
- `PLAN.md`, D8 i D9: `hot-slots.hmod` bez `models/`, `textures/`, `lang/`, bo treść modów zasłania paczkę serwera; `require-resource-pack = true` i zgoda na paczkę ustawiana przez `hot-play`.

### Uwagi
- `Host::tell` w haku `player_command` na serwerze dedykowanym zwraca `false`; odpowiedzi idą przez `event.replies`. Zachowanie `tell` poza hakiem gracza do sprawdzenia w M4.
