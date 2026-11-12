# ADR 0007 — `hot-host`: logi, komendy i pauza pustego serwera

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Przy pierwszym uruchomieniu `hot-host` w silniku (M1.3) wyszły trzy rzeczy, których plan nie przewidział:
1. D2 mówiło, że `hot-host` pisze błąd jedną linią na stderr, do `logs/supervisor.log`. Logger silnika (`horizon-client-render/src/logging.rs`, `ours`) zapisuje jednak do `logs/latest.log` każdy poziom ERROR, a INFO i WARN tylko z celów zaczynających się od `horizon`.
2. Hot mod nie może dopisać własnej komendy do drzewa komend silnika, bo drzewo powstaje raz, przy otwarciu świata (`register_commands`).
3. Pusty serwer dedykowany po `pause-when-empty-seconds` (domyślnie 60 s) przestaje tykać i żaden hak ticku nie działa („a paused pass fires nothing”). Działają tylko komendy konsoli. Wczytanie z `hot load` czekało wtedy na pierwszego gracza (w próbie 41 wczytań w kolejce).

## Rozważane opcje

- Logi: stderr (plan) albo `log` z celem `horizon_hot` (ten sam crate co silnik, jest w jego `Cargo.lock`).
- Komendy: osobna komenda w silniku dla każdego moda (wymaga nowego silnika przy każdym nowym modzie) albo jedna komenda `hot` plus hak `player_command`, który widzi każdą komendę gracza.
- Pauza: wyłączyć pauzę w `server.properties`, dodać nowy rodzaj zdarzenia „tick w pauzie” (silnik go nie daje) albo przełączać gotowe wczytania także przy komendzie konsoli.

## Decyzja

- **Logi:** `log::info!/warn!/error!` z celem `horizon_hot`. Wszystko trafia do `logs/latest.log` obok linii silnika. Błędy samego `hot-host` mają kody `HOT-E0406` (rdzeń się nie wczytał) i `HOT-E0407` (rdzeń spanikował).
- **Komendy:** `hot-host` rejestruje jedną komendę `hot [args:text]` dla operatorów poziomu 4. Z konsoli i od operatora idzie do rdzenia jako `CONSOLE_COMMAND`. `hot <mod> …` rdzeń przekazuje tylko temu modowi (D5). Pozostałe komendy graczy (np. `/licznik`) docierają do modów przez hak `player_command` jako `PLAYER_COMMAND`; odpowiedź idzie przez `event.replies`. Komendę `hot …` od gracza `player_command` przepuszcza, żeby silnik sprawdził poziom operatora w `perform_command`.
- **Pauza:** rdzeń przełącza gotowe wczytania przy `SERVER_TICK` i przy każdej komendzie konsoli. Komenda konsoli też wypada między tickami. `hotd` po `hot load` pyta `hot status`, aż zobaczy nową generację. Pauzy pustego serwera nie wyłączamy, bo oszczędza procesor ThinkCentre.
- **Podmiana rdzenia:** `hot core load [plik]` obsługuje `hot-host`. Stary rdzeń oddaje stan (`deactivate`, zapisuje mody w `hot-state/`), nowy startuje z nim. Gdy nowy nie wystartuje, wraca stary z tym samym stanem.

## Konsekwencje

- Diagnostyka `hot-host` jest w `logs/latest.log`, a `hot-core` w `logs/hot-core.jsonl`. `hot report` (M12) zbiera oba.
- Komenda moda gorącego nie ma podpowiedzi w czacie gracza, bo nie jest w drzewie komend silnika. Po wysłaniu działa bez komunikatu „Unknown command” (S3).
- `RANDOM_TICK` nie jest przekazywany w v1: hak wymaga osobnej rejestracji bloków w silniku i wołany jest tysiące razy na tick.
- Gdy serwer stoi w pauzie i nikt nie wyśle komendy konsoli, wczytanie czeka. `hotd` musi wysłać `hot status` (M2).
