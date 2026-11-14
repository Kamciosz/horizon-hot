# ADR 0019 — haki i funkcje gospodarza v1, komendy gracza (D5, D6, D6a)

- **Data:** 2026-11-14 (decyzje z 2026-11-12, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Mod gorący robi tylko to, na co pozwala ABI (ADR 0017), a każdy hak trzeba ręcznie przetłumaczyć. Potrzebny jest zestaw, który wystarczy na typowe mody serwera (sklep, licznik, teleport) i nie kosztuje nic na najgorętszych ścieżkach silnika.

## Decyzja

- **D5. Haki** (`hot_abi::kind`): `SERVER_TICK`, `PLAYER_JOINED`, `PLAYER_LEFT`, `PLAYER_CHAT`, `PLAYER_COMMAND`, `CONSOLE_COMMAND`, `USE_ITEM`, `USE_ITEM_ON`, `RIGHT_CLICK_BLOCK`, `LEFT_CLICK_BLOCK`, `PLACE_BLOCK`, `DESTROY_BLOCK`, `DATA_LOADED`, `PLAYER_TICK`. Do tego komenda `/hot` dla operatora. `RANDOM_TICK` jest zarezerwowany, ale nie działa: silnik woła go dla każdego bloku z `LevelWriteHost`, który nie zmienia całego bloku, a bloki ze slotów nie mają właściwości (M5.2, źródła Horizona `mod_api.rs`).
- **D6. Funkcje gospodarza** (`hot-sdk`): `tell`, `broadcast`, `run_command`, `run_later` (komenda na początku następnego ticku, bo `run_command` nie działa w haku gracza, próba S4), `block`, `set_block` (nadpisuje każdy blok, więc mod sam sprawdza miejsce), `give` (zwraca, ile weszło), `players` (nazwa, UUID, pozycja), `particles` i log. Tabela funkcji jest ważna tylko w trakcie wywołania. Gdy `tell` zwróci `false`, `hot-host` wysyła `tellraw` przez `run_command`.
- **D6a. Komendy gracza.** W haku `player_command` silnik nie podstawia gracza: `players()` go pomija, a `tell` i `give` do niego nie trafiają (test M4.1, dokumentacja Horizona `docs/native-mods.md`). Komendy rejestruje się tylko raz, przy otwarciu świata. Dlatego mod deklaruje swoje komendy najwyższego poziomu (`hot_mod!(…, commands = "sklep")`, pole `commands` w `ModVTable`). `hot-core` anuluje taką komendę od razu i odkłada ją do `PLAYER_TICK` tego gracza (najpóźniej tick później), gdzie silnik gracza podstawia. Właściciela komendy szuka dopiero przy wywołaniu. Deklaracja oznacza własność: `Continue` nie oddaje komendy silnikowi, a odpowiedzi idą przez `tell`. Kolejka: do 16 komend na gracza (`DEFERRED_MAX`), czyszczona przy wyjściu. Komendy niezadeklarowane idą wprost w haku `player_command`, a odpowiedź przez `event.replies`.

## Konsekwencje

- Mod nie doda nowej komendy do podpowiedzi klienta: rejestracja jest jednorazowa przy otwarciu świata.
- Komenda zadeklarowana odpowiada najpóźniej tick później. Martwy gracz nie tickuje, więc jego komendy czekają do odrodzenia.
- Serwer nie sprawdza uprawnień do komend modów; mod sprawdza je sam (`templates/server-repo/README.md`).
- Nowy hak albo funkcja to zmiana ABI: `docs/przewodniki/nowy-hak.md`.
