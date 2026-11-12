# ADR 0002 — Wyniki prób S1–S12 i poprawki decyzji D1–D29

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Plan budowy (`PLAN.md`, sekcja 4) opierał się na kilku założeniach, których nie dało się sprawdzić bez prób: czy kod gospodarza da się wkompilować w silnik, czy pula slotów działa, czy podmiana silnika obejdzie się bez ekranu ładowania, ile to kosztuje i jak często Horizon zmienia rzeczy, na których stoimy. Etap M0 sprawdził je próbami S1–S12. Próby S9 i S10 wymagają ThinkCentre i jeszcze się nie odbyły.

Wszystkie próby: Horizon `nightly-2026-11-05-66f3981`, macOS arm64 (Apple M4 Pro, 24 GB), klient i serwery na jednej maszynie. Czasy z Maca są dolną granicą: na ThinkCentre mogą być dłuższe.

## Wyniki

| Próba | Pytanie | Wynik | Dokument |
|---|---|---|---|
| S1 | Czy `hot-abi` wejdzie do paczki moda natywnego jako `[[crate]]`? | Tak. `hot-abi` nie może mieć zależności. Paczkę `hot-host` składa skrypt (kopia crate), bo launcher przyjmuje tylko ścieżki wewnątrz paczki. | `docs/spike/s1-hot-abi-w-silniku.md` |
| S2 | Czy pula slotów w `.hmod` działa na serwerze dedykowanym? | Tak. Gracz bez pliku dostaje czytelny komunikat z id i wersją. | `docs/spike/s2-pula-slotow-hmod.md` |
| S3 | Czy własna komenda przejdzie przez hak `player_command`? | Tak, przez `Flow::Cancel`. Odpowiedź tylko przez `event.replies`, bo `Host::tell` w tym haku zwraca `false`. | `docs/spike/s3-komenda-przez-player-command.md` |
| S4 | Czy `run_command` działa z `server_tick`? | Tak. Komenda wykonuje się w wątku serwera, wewnątrz ticku. | `docs/spike/s4-run-command-z-server-tick.md` |
| S5 | Czy paczka serwera nadaje wygląd slotom? | Tak, jeśli `hot-slots.hmod` nie ma `models/`, `textures/` ani `lang/`. Klient pobiera paczkę ponownie tylko przy nowym sha1. | `docs/spike/s5-wyglad-slotu-z-paczki-serwera.md` |
| S6 | Czy proxy zmieni wygląd w trakcie gry? | Tak, bez rozłączenia, ale przez ok. 1,3 s ekran zasłania nakładka ładowania. Potrzebny wpis serwera z `acceptTextures` i stałe id paczki. | `docs/spike/s6-paczka-w-trakcie-gry.md` |
| S7 | Ile kosztuje pula 4096/8192? | Klient startuje o ok. 1 s wolniej, serwer zajmuje ok. +230 MB, klient ok. +250 MB. Podmiana silnika rośnie do 1,45–1,82 s, z nowym plikiem silnika do ok. 3,9 s. | `docs/spike/s7-rozmiar-puli.md`, `docs/sloty.md` |
| S8 | Czy da się podmienić silnik bez rozłączenia i bez ekranu ładowania? | Tak, przez własne proxy. Pauza bez modów: 0,96–1,07 s. | `docs/spike/s8-podmiana-analiza-klienta.md`, `docs/spike/s8-podmiana-prototyp.md`, `docs/podmiana.md` |
| S9 | Czy `native-build` działa na Ubuntu bez okna? | Nie sprawdzono: czeka na ThinkCentre (M0.10). | — |
| S10 | Jaki sprzęt ma ThinkCentre i ile pamięci dostaje gra? | Nie sprawdzono: czeka na ThinkCentre (M0.11). | — |
| S11 | Co zmienia się w Horizonie między wydaniami? | Haki i pliki, z których korzystamy, są takie same od 11-03 do najnowszego Nightly. Build aktualizacji potrzebuje `--ignore-pins`. | `docs/aktualizacje.md` |
| S12 | Czy dwa silniki mogą pracować na jednym świecie? | Na zmianę tak, naraz nie: silnik trzyma `session.lock`. Świat może leżeć poza katalogiem serwera (dowiązanie). | `docs/spike/s12-dwa-serwery-jeden-swiat.md` |

## Rozważane opcje

Dwa miejsca, w których próby dały wybór:

1. **Podmiana silnika: własne proxy (`hot-proxy`) czy wbudowany w Horizona `[transfer]`.** `[transfer]` przenosi gracza między dwoma działającymi serwerami. Przy jednym świecie oba silniki nie mogą działać naraz (S12), więc eksport i import gracza nie są potrzebne. Wybór: własne proxy. `[transfer]` przyda się dopiero przy kilku światach.
2. **Odpowiedź na komendę: `Host::tell` czy `event.replies`.** `tell` w haku komendy nie działa na serwerze dedykowanym (S3). Wybór: `event.replies` w haku komendy, `tell` tylko poza nim.

## Decyzja

Plan zostaje. Żadna próba nie obaliła wariantu A ani warstwy gorącej. Poprawki w `PLAN.md`:

- **D1, D3:** potwierdzone (S1). Składanie paczki `hot-host` skryptem jest już w kroku M1.3.
- **D6:** wiadomość do gracza w haku komendy idzie przez `event.replies`. Poza tym hakiem `hot-host` używa `Host::tell`, a gdy ten zwróci `false`, wysyła `tellraw` przez `run_command` (działa z `server_tick`, S4). Czy `tell` działa poza hakiem komendy, rozstrzyga test w M4.1.
- **D8:** wersję `hot-slots` podbijamy przy każdej zmianie puli. Serwer ogłasza też SHA-256 pliku, ale nie wiemy, czy inny SHA-256 przy tej samej wersji rozłącza gracza. Ta decyzja czyni to pytanie nieistotnym.
- **D9:** `hot-play` dopisuje serwer z `acceptTextures` = `true` do `servers.dat` w katalogu gry swojej instancji. Bez tego klient pyta o zgodę przy każdej paczce (S6). FAKT: klient czyta plik obok `saves/` (`multiplayer.rs`, `servers_file`), a każda instancja launchera ma własny katalog gry i własną listę serwerów (`horizon-launcher --help`). Globalnego `servers.dat` domyślnej instancji `hot-play` nie rusza. `hot-proxy` wysyła paczkę zawsze z tym samym id.
- **D11:** pauza zmierzona, budżet 1 s nie mieści się przy pełnej puli. Budżet i rozmiar puli wybiera użytkownik (`PLAN.md`, sekcja 8).
- **D16:** silniki nigdy nie działają naraz (S12), więc podmiana nie potrzebuje pamięci na dwa. Pamięć na drugi silnik potrzebna jest tylko przy teście aktualizacji (D18). Po zimnym starcie silnik zajmuje ok. 700 MB (S7, RSS `horizon-server` bez graczy). Wartości do `MemoryLow` daje M0.11, bo pomiar Horizona (R28, 1,4–1,5 GB) dotyczy pełnej gry `horizon-client --dedicated`, a nie silnika serwerowego.
- **D18:** build aktualizacji z `--version <nowe Nightly> --ignore-pins` (S11). Dopóki M9 nie pokaże, że `content_identity` się nie zmienia, każde nowe Nightly traktujemy jako zmianę, której klient potrzebuje (okno zgodności).
- **D24:** zmiana wyglądu i podmiana silnika opisane zmierzonymi wartościami zamiast „mrugnięcia ≤ 1 s”.

## Konsekwencje

Decyzje użytkownika potrzebne przed M5:

1. Rozmiar puli a pauza podmiany. Pula 1024/2048: 1,16–1,34 s, z nowym plikiem silnika ok. 2,2 s. Pula 4096/8192: 1,45–1,82 s, z nowym plikiem silnika ok. 3,9 s.
2. Kiedy nowy wygląd trafia do graczy: od razu (wszyscy widzą ok. 1,3 s nakładki) albo przy następnym wejściu.

Otwarte punkty, każdy z krokiem, który go rozstrzygnie:

| Punkt | Krok |
|---|---|
| `Host::tell` poza hakiem komendy | M4.1 |
| klucz nazwy przedmiotu z puli w `hot-pack` (`block.hot_slots.item_N`) | M5.2 |
| podmiana z inną listą modów na nowym silniku i z paczką zasobów | M8.3 |
| tryb online i Velocity forwarding za proxy | M8.1 |
| wielu graczy, id encji w środku pakietów, tagi (`update_tags`), gracz w oknie, pojeździe albo innym wymiarze | M8.2, M8.3 |
| proxy przekazuje silnikowi ostatnią pozycję klienta (HIPOTEZA: gracz nie cofa się o ok. 1 s) | M8.3 |
| cache treści modów budowany przed podmianą (HIPOTEZA) | M8.3 |
| `content_identity` na różnych Nightly | M9.1 |
| plik pobrany skryptem bez kwarantanny i Mark of the Web (HIPOTEZA) | M11 |
| `native-build` na Ubuntu bez okna (S9) | M0.10 |
| czasy i pamięć na ThinkCentre, duża paczka zasobów (S10) | M0.11 |

Ryzyko do obserwacji: Horizon od 11-05 sam wprowadza mody bez restartu (`docs/aktualizacje.md`, punkt 8). Na razie dotyczy to klienta i serwera zintegrowanego, a nie `horizon-server`.
