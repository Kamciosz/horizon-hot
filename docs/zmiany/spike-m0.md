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
- `crates/hotd`: demon, który co 10 s pobiera repo serwera, liczy zmiany od ostatniego zastosowanego commita (D24), pobiera podpisane biblioteki modów z gałęzi `hot-builds`, ładuje je w silniku i zapisuje stan w `hot-state/hotd.json`; `hotd status` w terminalu. Kody `HOT-E0201`–`HOT-E0207`.
- `scripts/hot-build.sh`, `scripts/hot-publish.sh` i szablon `templates/server-repo/.github/workflows/hot.yml`: budowa modów w CI, podpis `ssh-keygen`, wynik na gałęzi `hot-builds`.
- ADR 0008: wyniki CI na gałęzi `hot-builds` i podpis `ssh-keygen` zamiast artefaktów Actions.
- `scripts/server-install.sh`: instalator serwera na Ubuntu jednym poleceniem (D25, kroki 1–8 i 10): sprawdzenie maszyny, użytkownik `horizon` i `/srv/horizon`, Horizon z oficjalnego `install.sh`, `hotd` i `hot-core` budowane ze źródeł, klucz podpisu CI i `allowed_signers`, klucz tylko do odczytu dla prywatnego repo, `hot.yml` z przypiętą wersją horizon-hot, świat, usługi systemd, `ufw`, kopie co noc, autotest. Silnik z `hot-host` budowany dopiero po zgodzie właściciela (`--trust-native-code`). Ponowne uruchomienie robi tylko to, czego brakuje. Kody `HOT-E0101`–`HOT-E0111`.
- Test katalogu błędów sprawdza też kody w `scripts/`.
- `hotd`: pole `fetch_error` w `hot-state/hotd.json` (czy repo da się pobrać teraz) i linia „repo teraz niedostępne” w `hotd status`; commit czeka, gdy silnik nie działa, zamiast trafić na listę nieudanych.
- `hotd`: paczki danych modów (M3, D7). `mods/<id>/data/` trafia do `<świat>/datapacks/hot-<id>/` i wchodzi jednym `reload` bez restartu; usunięcie moda usuwa paczkę. Zły JSON zatrzymuje commit, zanim silnik coś dostanie; paczka niewłączona po `reload` albo z tabelami odrzuconymi przez silnik wraca do poprzedniej; pliki pominięte przez silnik widać w `hotd status`; po złym commicie paczki wracają do ostatniego dobrego. Kod `HOT-E0208`.
- `templates/server-repo/README.md`: układ repo serwera (mody z kodem, paczki danych).
- `docs/sprzet.md`: zmierzony czas `reload` na Macu dewelopera.
- `/hot status` w grze pokazuje też stan `hotd`: commit repo, zastosowany commit, na co czeka i dlaczego ostatni commit nie wszedł.

- ABI 2 (M4): funkcje gospodarza `set_block`, `give` (zwraca, ile weszło), `particles` i `run_later` (komenda wykonana na początku następnego ticku). `tell` odrzucony przez silnik idzie w następnym ticku jako `tellraw`.
- Komendy gracza z podstawionym graczem (D6a): mod deklaruje je w `hot_mod!(…, commands = "…")`, a `hot-core` wykonuje je w ticku gracza, bo w haku komendy silnik gracza nie widzi. Odpowiedź moda dochodzi wtedy przez `tell`.
- `examples/sklep` (monety za czas gry, `/sklep kup`, `hot sklep daj`) i `examples/teleport-blok` (magnetyt przenosi gracza do celu).
- Ostrzeżenia `hot-core` w logu: mod odpowiada na komendę gracza bez deklaracji w `commands` (raz na mod i komendę) oraz dwa mody deklarują tę samą komendę. Każde z poprawką dla agenta.
- `templates/server-repo/README.md` i dokumentacja `hot_mod!`: serwer nie sprawdza uprawnień do komend modów, mod robi to sam po `permission`.
- Bloki modów (M5): plik `mods/<id>/assets/blocks/<nazwa>.png` dostaje slot z puli, a `hotd` zapisuje przydział w `hot-state/slots.json` (tylko dopisywany). Mod z kodem używa nazwy `<id>:<nazwa>`, a `hot-core` tłumaczy ją na `hot_slots:slot_N` i z powrotem; komenda `hot slots` wczytuje przydział ponownie.
- Paczka zasobów z repo serwera: `hotd` buduje powtarzalny `hot-state/pack/pack.zip` (tekstura, model, nazwa z `lang.json`) bez czekania na CI, a `hotd serve-pack` serwuje go na porcie 8131. `hotd slots-source <katalog>` tworzy źródło `hot-slots.hmod`. Kody `HOT-E0209` (pełna pula) i `HOT-E0210` (zły obraz albo `lang.json`).
- `hotd` sprawdza odpowiedź na `hot slots`; `hot-core` starszy niż `hotd` to błąd `HOT-E0205`, a nie cisza. `hot status` pokazuje bloki modów i ich sloty.
- `hotd serve-pack` podaje też `/hot_slots.hmod`, a przed pierwszą paczką paczkę bez bloków.
- Instalator, krok 8: `cargo-horizon-mod` z wersji Horizona serwera, budowa i instalacja `hot_slots.hmod`, adres paczki w `server.toml` (nowa opcja `--address`), usługa `horizon-hot-pack`; kod `HOT-E0112`.
- Tryb bezpieczny `hot-core` (M7.1): po drugiej awarii serwera z rzędu, każdej w ciągu 5 minut od startu, mody gorące nie startują, a serwer działa dalej; `hot load`, `hot unload` albo czysty restart go kończą. Kod `HOT-E0408`, pola `started_ms` i `crash_starts` w `hot-state/mods.json`, `safe_mode` w `status.json`, linia w `hot status`.
- Test z prawdziwym segfaultem moda gorącego w procesie potomnym.
- `hotd` sprawdza wolne miejsce na dysku przed każdym krokiem (M7.2). Gdy jest go mniej niż 512 MB, usuwa swoje stare artefakty i stare kopie `hot-core`. Jeśli to nie wystarczy, nie pobiera i nie stosuje commitów, zapisuje `HOT-E0211` raz i pokazuje powód w `hot status`. Gdy miejsce wróci, wznawia pracę sam.
- Testy `hotd` i `hot-core` na prawdziwie pełnym dysku: działają tylko ze zmienną `HOT_FULL_DISK_DIR`, czyli na osobnym, małym woluminie.

### Zmienione
- Instalator usuwa po sobie źródła puli slotów i pakiet `hot-host` z `/srv/horizon/tmp`.
- `hotd`: brak miejsca na dysku w trakcie commita, zanim silnik coś dostał, nie odrzuca już commita na zawsze (wcześniej `HOT-E0210` przy zapisie paczki). Commit czeka i jest ponawiany, a log dostaje `HOT-E0211` z kodem kroku, który padł. Paczka zasobów jest zapisywana przed `slots.json`, ucięte pliki tymczasowe znikają, a git działa z `LC_ALL=C`.
- CI uruchamia się też po pushu na gałęzie `spike/**`, nie tylko na `main` i w PR-ach.
- `tick_block` i `random_tick` przeniesione do M5: bez slotów hak na każdym bloku kosztowałby wywołanie przez ABI bez korzyści (D5).
- `hot load` moda, który nie jest aktywny, bierze stan zostawiony przez `hot unload` albo wyłączenie po panice w `hot-state/saved/`.
- `players()` w `hot-host` zawiera gracza podstawionego w haku, nawet gdy silnik nie ma go na liście.
- `hotd`: zmiana w `mods/<id>/data/` nie jest już odkładana do M3; `Engine::command` zwraca odpowiedź silnika.
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
- Haki `tick_block` i `random_tick` dla bloków ze slotów przeniesione z M5 do M0.19: obecne API nie zmienia w nich całego bloku i nie podaje nazwy bloku, a włączenie losowego ticku zmienia pulę u graczy (`PLAN.md`, M5.2).

### Uwagi
- `Host::tell` w haku `player_command` na serwerze dedykowanym zwraca `false`; odpowiedzi idą przez `event.replies`. Zachowanie `tell` poza hakiem gracza do sprawdzenia w M4.
