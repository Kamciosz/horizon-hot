# PLAN — horizon-hot (wersja 2)

Serwer Horizon (Minecraft Java 26.3 napisany w Rust), który bierze mody z repozytorium na GitHubie i wczytuje je w trakcie gry, bez restartu. Do tego instalacja jednym poleceniem i prosta aplikacja terminalowa dla graczy. Wszystko aktualizuje się samo i samo naprawia.

- Data: 2026-11-12. Wersja 2 zastępuje wersję 1 (ta sama data) po weryfikacji wykonalności w kodzie Horizona.
- Stan: plan. Repo założone (`f788091`: szablon standardu, CI Rust, pusty `hot-abi`). Budowa rusza na „go”.
- Oznaczenia: **FAKT** = sprawdzone w kodzie Horizona (rewizja `66f3981`, `~/Library/Application Support/horizon-launcher/native/stage`, ścieżki względem `crates/`) albo próbą; **NIEWIADOMA** = rozstrzyga ją wskazany spike z M0. Wyniki spike'ów i poprawki decyzji: `docs/adr/0002-spike.md`.

---

## 1. Wymagania użytkownika

| # | Wymaganie |
|---|---|
| W1 | Serwer Horizon; mody z podpiętego repo na GitHubie; repo może nie mieć modów. |
| W2 | Zmiana modów **bez restartu serwera**, z możliwie najkrótszą pauzą (1,5 s to za długo). Decyzja z 2026-11-12: świat przy podmianie silnika stoi krócej niż 1 s w każdym przypadku. |
| W3 | Działa na współdzielonym ThinkCentre kolegi (Ubuntu, `sudo` jest); słaby sprzęt nie może przerywać gry — ciężka praca idzie w tle. |
| W4 | Wszystko aktualizuje się samo: mody, Horizon, nasz system, aplikacja gracza. |
| W5 | Wszystko samo się naprawia po błędzie; przywrócenie świata z kopii tylko po potwierdzeniu operatora. |
| W6 | Serwer instaluje i konfiguruje jedno polecenie na Ubuntu. |
| W7 | Aplikacja gracza: terminalowa, odporna na pomyłki, instalowana jedną linijką na macOS, Windows i Linuksie. |
| W8 | Bez zewnętrznych powiadomień; stan widać w grze (operatorzy) i w `hot status`. |
| W9 | Gotowy projekt może rozwijać i naprawiać ktoś z zespołu (człowiek albo agent AI) bez historii rozmów i bez tego planu; każdy błąd zostawia log, z którego model bez wiedzy o projekcie umie go naprawić. |

## 2. Kryterium „gotowe” (całość)

Każdy punkt ma obserwację, która by oblała:

1. Repo bez `mods/` → zwykły serwer; gracz wchodzi (W1).
2. Push zmiany w kodzie moda gorącego → nowe zachowanie w ≤ 3 min (z budową w CI), PID silnika ten sam, gracz połączony bez przerwy (W1, W2).
3. Push zmiany w `data/` → nowy przepis działa, PID ten sam (W2).
4. Push nowego bloku z puli slotów → blok w grze bez restartu (W2).
5. Push moda natywnego `.hnmod` → działa po podmianie silnika; nikt nie rozłączony; pauza świata zmierzona i krótsza niż 1 s (D11); gracz bez ekranu ładowania (W2, wariant A).
6. Nowe Nightly Horizona → serwer sam przechodzi na nie jak w punkcie 5; wydanie, którego nie da się zbudować → serwer zostaje na starym, `hot status` mówi dlaczego (W4).
7. Mod z paniką → wyłączony, wraca poprzednia generacja, serwer działa (W5).
8. Mod z segfaultem → silnik wstaje z ostatnią dobrą generacją bez pętli awarii; gracze przytrzymani przez proxy i wprowadzeni z powrotem (W5).
9. Na ThinkCentre z 10 botami, w trakcie budowy silnika i podmiany: p99 MSPT < 50 ms (W3).
10. Czysta maszyna Ubuntu → jedno polecenie → autotest „Działa”; drugie uruchomienie bez zmian (W6).
11. Czyste maszyny macOS, Windows 11, Ubuntu Desktop → jedna linijka → „Graj” → gracz na serwerze; uszkodzony plik launchera lub Horizona naprawiony przy następnym starcie (W7, W5).
12. `quality-audit` ≥ 120/125.
13. Test „nowej osoby w zespole”: świeży agent dostaje tylko repo (bez `PLAN.md`) i wynik `hot report` z celowo wywołanego błędu (po jednym z każdej kategorii D27) i bez innych wskazówek naprawia go albo trafnie wskazuje przyczynę. Obserwacja: transkrypt w `docs/testy/obcy-agent.md` (W9).

---

## 3. Ustalenia z researchu i weryfikacji

| # | Ustalenie | Status i źródło |
|---|---|---|
| R1 | Trait mod może zależeć od `libloading 0.8.9` (jest w `Cargo.lock` Horizona) | FAKT, próba `horizon-mod-lab/projects/probe-hot-host`, build 39 s (cache ciepły) |
| R2 | Taki mod w silniku serwera (`native-build --server-output`) ładuje `.dylib` i woła z niej funkcję | FAKT, log `hot_probe() = 4242` |
| R3 | Pętla serwera: jeden wątek, 20 TPS; generowanie chunków i światło na puli wątków | FAKT, `docs/server.md:28` |
| R4 | Mody natywne ładowane raz (`OnceLock`), bez wyładowania; Horizon planuje dylibs nigdy niewyładowywane | FAKT, `mod_api.rs:594`, issue #7386 |
| R5 | Mod rejestruje komendy gracza i konsoli | FAKT, `docs/native-mods.md:2095-2105`, `native_server.rs:999-1054` |
| R6 | Hak `player_command` dostaje każdą komendę przed obsługą i może ją anulować | FAKT, `horizon-client/src/game/lan_actions.rs:3357-3385` |
| R7 | `horizon-server cmd` przekazuje komendę na stdin silnika | FAKT, `horizon-server/src/cli.rs:150-161`, `supervisor.rs:685` |
| R8 | Silnik bez `panic=abort`; alokator mimalloc bez `override` → cdylib ma własną stertę i własny hook paniki | FAKT, `Cargo.toml` (profile), `third_party/libmimalloc-sys/Cargo.toml:62` |
| R9 | Bloki i przedmioty rejestrują się raz przy starcie, z zamrożonym kształtem, światłem, twardością, stanami | FAKT, `docs/mods.md:23-25,140-260` |
| R10 | Zachowanie bloku można ustalać hakami w locie | FAKT, `native_hooks.rs:6584-6892` |
| R11 | Serwer dedykowany ładuje `.hmod` z `horizon-mods/` + `horizon-mods.txt` (`mod.<id>:true`) | FAKT, `game/mods.rs:78`, `horizon-client-render/src/mod_list.rs:12-61`, `horizon-server/src/engine.rs:270` |
| R12 | Klient Horizon (`brand == "horizon"`) musi mieć te same `.hmod` z `side="both"` i ten sam `content_identity`, inaczej rozłączenie | FAKT, `lan/conn.rs:1003-1076`, `content_identity.rs` |
| R13 | `/reload` na serwerze dedykowanym: przepisy, łupy, tagi, zaawansowania, `mcfunction` + synchronizacja graczy | FAKT, `game.rs:20248-20300`, `functions.rs:716` |
| R14 | Paczka zasobów: wysyłana w konfiguracji; z poprawnym sha1 → cache, nowy sha1 → pobranie; trait API nie ma wysyłki paczki w trakcie gry | FAKT, `server_packs.rs:50,209`, `download.rs:214-262` |
| R15 | `/transfer` na ten sam host:port działa; `accepts-transfers` zmienia się bez restartu | FAKT, `game/remote.rs:2058-2070`, `lan/mod.rs:362`, `management_methods.rs:149` |
| R16 | Forwarding Velocity v1: `online_mode=false`, `forwarding.secret`, adres IP | FAKT, `lan/forwarding.rs`, `docs/velocity-forwarding.md`, `game/dedicated.rs:234` |
| R17 | Klient obsługuje `StartConfiguration`, ale zeruje świat i pokazuje ekran rekonfiguracji | FAKT, `net.rs:891`, `game/remote.rs:9996-10040` |
| R18 | Horizon ma `[transfer]`: `player-expect`, `player-export [freeze]` (zamyka okna, NBT gracza), `player-release`; brak strony importu | FAKT, `docs/server.md:147-254`, `game/dedicated/transfer.rs` |
| R19 | Stan gry: `Game` ~480 pól, `Guest` ~163 pola na gracza; podmiana niewidoczna dla klienta (≤ 50 ms) to miesiące pracy | FAKT, `game.rs:2383-3289`, `lan/guest.rs:85` |
| R20 | Zapis chunka w pamięci (`Chunk::into_nbt`) i zbiór zmienionych chunków istnieją | FAKT, `horizon-world/src/chunk.rs:255`, `game.rs:15895` |
| R21 | Budowa silnika wymaga przygotowanych zasobów gry; pobieranie plików Mojanga w CI zabronione przez politykę Horizona | FAKT, `launcher/src/native_mods.rs:2113`, `docs/microsoft-account.md:104-108` |
| R22 | Pierwszy build silnika ok. 12 min, potem przyrostowo | FAKT, `docs/native-mods.md:1967`; próba: 39 s |
| R23 | Od 2026-10-28 tylko wydania Nightly; ~6 w dwa tygodnie; między nimi 51–6255 commitów; `game.rs` 2391 commitów od 1.11 | FAKT, `gh release list`, `gh api compare`, `docs/aktualizacje.md` |
| R24 | `PROTOCOL_VERSION` stały dla 26.3 (777 w 10 ostatnich wydaniach); `content_identity` liczony ze stanów bloków, wpisów modów i komponentów NBT modów natywnych, nie z wersji Horizona; plik bez zmian od 11-01 | FAKT, `horizon-protocol/src/lib.rs:31`, `content_identity.rs:56-75`, `docs/aktualizacje.md` |
| R25 | `status --json`: MSPT `average/p50/p95/p99` z ostatnich 100 ticków, `rss_bytes`, `cpu_percent`; próbka co 1 s | FAKT, `game/dedicated/status.rs:156-160`, `clock.rs:291`, `supervisor.rs:536` |
| R26 | `horizon-server update` działa tylko dla instalacji z `install.sh` | FAKT, komunikat programu |
| R27 | Katalog budowy `native/stage` i `build.log` są wspólne dla buildów natywnych | FAKT, próba |
| R28 | Pusty serwer Horizon: ok. 1,4–1,5 GB RAM | FAKT, `docs/server.md` (pomiar M4 Pro) |
| R29 | Datowane wydania Nightly Horizona są na GitHubie tylko kilka dni (2026-11-13: są `nightly`, `nightly-2026-11-08`, `nightly-2026-11-07`; `nightly-2026-11-05-66f3981` zniknęło). Tagi `nightly-source/<data>-<skrót>` i commity zostają | FAKT, `gh api …/releases`, `…/git/refs/tags/nightly-source` |

---

## 4. Decyzje architektoniczne

Każda w formie gotowej do `docs/adr/NNNN-*.md` (kontekst → decyzja → konsekwencje). Numery ADR kolejne (0001 to struktura projektu, 0002 wyniki spike'ów), a w tytule ADR numer decyzji (Dn).

### Warstwa gorąca (bez żadnej pauzy)

**D1. Gospodarz w silniku + mody gorące ładowane w locie.**
Kontekst: R1, R2, R4. Decyzja: jeden trait mod `hot-host` wkompilowany w silnik; mody gorące to `cdylib` (`.so` na serwerze, `.dylib` na Macu w testach) ładowane przez `libloading`. Konsekwencje: mod gorący robi tylko to, co dają haki D5 i funkcje D6; nowe bloki — przez sloty (D8).

**D2. `hot-host` jest cienkim, stałym loaderem; logika gospodarza to `hot-core`.**
`hot-host` tylko ładuje `hot-core` (też `cdylib`) i przekazuje mu wszystkie haki. `hot-core` zarządza modami, slotami i komendą `/hot` i przeładowuje się jak mod gorący. Zmiana gospodarza nie wymaga nowego silnika. `hot-host` nie używa `hot-diag`: paczka moda natywnego nie może mieć zależności ścieżkowych, a z rejestru tylko te z `Cargo.lock` Horizona (S1). Błędy, które `hot-host` zgłasza sam (`HOT-E0406` rdzeń się nie wczytał, `HOT-E0407` rdzeń spanikował), idą przez `log` z celem `horizon_hot` do `logs/latest.log` silnika (ADR 0007). Szczegóły w M1.3.

**D3. Granica to stabilne C ABI (`hot-abi`).**
Kontekst: R8 — osobne sterty i hooki paniki. Decyzja: struktury `#[repr(C)]`, napisy `(ptr, len)` pożyczane na czas wywołania, każda strona zwalnia własną pamięć; `hot_mod_abi() -> u32`, `hot_mod_entry() -> *const HotModVTable`; inny `ABI_MAJOR` → odmowa. Konsekwencje: mody budują się dowolnym stabilnym Rustem; każdy hak tłumaczymy ręcznie, dlatego lista D5 jest krótka.

**D4. Cykl życia moda w ABI od wersji 1; starej generacji nie wyładowujemy.**
- `init(ctx, prev_state)` i `dlopen` poza wątkiem serwera; przełączenie generacji na granicy ticku.
- Obowiązkowe `deactivate(ctx) -> state`: zatrzymuje wątki i timery, oddaje stan.
- `ctx` ważny tylko na czas wywołania (`HostCtx<'call>` w `hot-sdk`).
- `catch_unwind` w każdym eksporcie (`hot-sdk`); panika → mod wyłączony, wraca poprzednia generacja.
- Każda generacja to kopia pliku pod unikalną nazwą; stara zostaje w pamięci bez wywołań. Pamięć odzyskuje podmiana silnika (D11) wykonywana automatycznie, gdy wzrost przekroczy próg — bez restartu.
- Trwałe dane moda: `hot-data/<mod>/`.

**D5. Haki v1.**
`server_tick`, `player_logged_in`, `player_logged_out`, `player_chat`, `player_command` (własne komendy najwyższego poziomu, np. `/sklep` — R6), `use_item`, `use_item_on`, `right_click_block`, `left_click_block`, `place_block`, `destroy_block`, `server_data_loaded`. Plus komenda `/hot <mod> …` rejestrowana przez `hot-host`.
- `tick_block` i `random_tick` przechodzą do M5 (2026-11-12). Silnik woła je na najgorętszej ścieżce, dla każdego bloku, z `LevelWriteHost` bez nazwy bloku i bez `Host`. Filtr i nazwę dają dopiero bloki ze slotów (D8), więc bez slotów hak kosztowałby wywołanie przez ABI na każdy blok bez korzyści. `kind::RANDOM_TICK` zostaje zarezerwowany w ABI.

**D6. Funkcje gospodarza v1.**
Wiadomość do gracza i do wszystkich, `run_command`, odczyt i zapis bloku, danie przedmiotu, dane gracza (nazwa, UUID, pozycja), cząsteczki, log. Tabela `HostVTable` ważna tylko w trakcie wywołania.
- Wiadomość do gracza w haku `player_command` idzie przez `event.replies`, bo `Host::tell` zwraca tam `false` (FAKT, S3). Poza tym hakiem `hot-host` używa `Host::tell`, a gdy zwróci `false`, wysyła `tellraw` przez `run_command` (działa z `server_tick`, S4). Test M4.1 (FAKT, 2026-11-12, silnik z `hot-host` na Macu, prawdziwy klient): `tell` dochodzi wprost z haka komendy konsoli, z haka kliknięcia bloku, z `server_tick` i z ticku gracza; zapasowy `tellraw` nie był potrzebny (zero odmów w logu), ale zostaje na wypadek odmowy. Funkcje ABI 2: `set_block` (przez `edit_blocks`, nadpisuje każdy blok, więc mod sam sprawdza miejsce), `give` (zwraca, ile weszło), `particles`, `run_later` (komenda na początku następnego ticku, bo `run_command` nie działa w haku gracza, S4).
- **D6a. Komendy gracza z podstawionym graczem (2026-11-12).** W haku `player_command` silnik nie podstawia gracza: `server_players()` go pomija, a `tell` i `give_stack` do niego nie trafiają (FAKT z testu M4.1: `/teleport-blok ustaw` odpowiedziało „Nie widzę cię na serwerze”; dokumentacja Horizona `docs/native-mods.md`, opis `native_player_command`: „The player's state is not swapped in, and `native_tell` and the other helpers that take a UUID reach nobody inside this hook”). Komendy rejestruje się tylko raz, przy otwarciu świata (`native_register_commands`), więc mod gorący nie doda swojej później. Decyzja: mod deklaruje swoje komendy najwyższego poziomu (`hot_mod!(…, commands = "sklep")`, pole `commands` w `ModVTable`). `hot-core` anuluje taką komendę od razu i odkłada ją do `player_tick` tego gracza (najpóźniej tick później), gdzie silnik podstawia gracza. Właściciela komendy szuka się dopiero przy wywołaniu, wśród modów wczytanych w tej chwili; gdy go nie ma, gracz dostaje wiadomość. Deklaracja oznacza własność: `Continue` w odłożonej komendzie nie oddaje jej silnikowi. Odpowiedzi (`reply`) idą wtedy przez `tell`. Kolejka: do 16 komend na gracza, czyszczona przy wyjściu gracza; martwy gracz nie tickuje, więc jego komendy czekają do odrodzenia. Komendy niezadeklarowane idą jak dotąd, wprost w haku `player_command` (odpowiedź przez `event.replies`).

**D7. Dane modów jako paczka danych świata + `/reload`.**
`mods/<id>/data/` → `<świat>/datapacks/hot-<id>/` → `reload` (R13). Bez budowania.

**D8. Pula slotów w `hot-slots.hmod` (`side="both"`), gracz instaluje ją raz (robi to aplikacja gracza).**
- Kontekst: R9, R11, R12. **`hot-host` i `hot-core` nie rejestrują żadnej treści** — inaczej klienci dostaliby rozłączenie z niejasną przyczyną.
- Pula od pierwszej wersji duża: 4096 bloków (pełna kostka, bez stanów) i 8192 przedmiotów. Zmiana puli zmienia `content_identity` → wymaga nowego pliku u graczy; dlatego pula jest duża i stała. S7: klient z pełną pulą startuje o ok. 1 s wolniej (kryterium spełnione), ale podmiana silnika (D11) wydłuża się do 1,45–1,82 s. **Rozmiar do decyzji użytkownika przed M5.1** (sekcja 8). Liczby: `docs/sloty.md`.
- Przydział slotów tylko dopisywany, w `hot-state/slots.json` (objęty kopią zapasową); slot nigdy nie wraca do innego moda bez jawnej komendy operatora.
- Każda zmiana puli to nowa wersja `hot-slots`. Serwer ogłasza też SHA-256 pliku, ale czy inny SHA-256 przy tej samej wersji rozłącza gracza, nie sprawdzono (S2). Nowa wersja przy każdej zmianie czyni to pytanie nieistotnym.
- Slot daje: nazwę i wygląd (paczka zasobów) + zachowanie (haki D5). Model może mieć dowolny kształt, ale tylko wizualnie: kolizja, światło i twardość zostają z `template` (R9).
- `hot-slots.hmod` zawiera tylko `data/`, `blockstates/` i `items/`; **bez `models/`, `textures/`, `lang/`** — treść modów leży nad paczką serwera, więc każdy taki plik w `.hmod` zasłoniłby wygląd z `hot-pack` (FAKT, S5: `docs/spike/s5-wyglad-slotu-z-paczki-serwera.md`).

**D9. Wygląd: paczka zasobów serwowana przez `hot-pack` pod stałym adresem, z prawdziwym sha1.**
`hot-pack` (osobny mały proces, poza hamulcem D16) serwuje `http://<host>:8131/pack.zip`; adres ustawiony raz przy instalacji. Nowa paczka = nowy sha1 → klient pobiera tylko zmienioną (R14, potwierdzone w S5). Na serwerze `require-resource-pack = true`; `hot-play` zgadza się na paczkę naszego serwera z góry: dopisuje go do `servers.dat` w katalogu gry swojej instancji z `acceptTextures` = `true`. FAKT: klient czyta `servers.dat` obok `saves/` w katalogu gry (`horizon-client/src/multiplayer.rs`, `servers_file`), a każda instancja launchera ma własny katalog gry i własną listę serwerów (`horizon-launcher --help`: „Instances: separate game folders”, „multiplayer: … an instance's Multiplayer list (servers.dat)”). Globalny `~/Library/Application Support/horizon/servers.dat` należy do domyślnej instancji Horizona i `hot-play` go nie rusza. Bez tego klient czeka na „Joining world…” na decyzję gracza i pyta ponownie przy każdej paczce wysłanej w grze (S6). Klucz nazwy przedmiotu z puli to `block.hot_slots.item_N` (S7), nie `item.…`; `hot-pack` używa klucza, który pokazuje gra. Nowy wygląd dociera do graczy tylko na dwa sposoby: `hot-proxy` wysyła w grze `ResourcePackPush` ze stałym id (zastępuje poprzednią paczkę) albo gracz wchodzi ponownie. Podmiana silnika wyglądu nie zmienia, bo proxy połyka konfigurację nowego silnika. S6: wysłanie w grze działa bez rozłączenia, ale przez ok. 1,3 s cały ekran zasłania nakładka ładowania (`docs/spike/s6-paczka-w-trakcie-gry.md`). Kiedy wysyłać: decyzja użytkownika (sekcja 8).

### Silnik i proxy (wariant A)

**D10. `hot-proxy` przed silnikiem.**
Gracze łączą się z `hot-proxy` (Rust, na `horizon-protocol`) na porcie 25565; silnik słucha na `127.0.0.1:25566` w trybie forwardingu Velocity (R16). Proxy kończy szyfrowanie, kompresję, keep-alive i teleporty po stronie klienta i mapuje identyfikatory encji między klientem a silnikiem. Logowanie Microsoft weryfikuje proxy (tryb online), więc serwer zostaje premium.

**D11. Podmiana silnika — wariant A: gracz zostaje, świat „mruga”, bez ekranu ładowania.**
- Kontekst: R17 — `StartConfiguration` pokazuje ekran; R19 — podmiana całkiem niewidoczna to miesiące pracy.
- Kroki (rozgrzany silnik, S13):
  1. Nowy silnik buduje się w tle (D15) do nowego katalogu. Każdy silnik ma własną ścieżkę i nigdy nie jest nadpisany w miejscu: macOS zabija następne uruchomienie nadpisanego pliku (kod 137), a Linux nie pozwala go nadpisać, dopóki działa (S13).
  2. `hotd` wstrzymuje zmiany datapacków i uruchamia `horizon-server start` nowej strony w tle, bez czekania na koniec komendy (wraca dopiero w fazie `running`). Nowy silnik wczytuje zasoby, buduje cache w swoim katalogu, otwiera port i czeka przed zajęciem świata (mixin `wait_for_world` w `hot-host`). Gotowość: w `logs/latest.log` linia „horizon-hot: waiting for the world”. `start` z kodem różnym od 0 to błąd przed zamrożeniem: `hotd` zgłasza go od razu, gracze zostają na starym silniku. Przerwana podmiana: `hotd` sam zatrzymuje czekający silnik, bo po 120 s wpadłby w pętlę restartów z S12. Robi to w tej kolejności: `horizon-server stop` w tle, potem jeden SIGTERM do procesu silnika. Czekający silnik nie czyta konsoli, więc samo `stop` czeka 120 s, a z SIGTERM kończy się po ok. 0,5 s (M0.20). Pid silnika `hotd` bierze z pola `engine_pid` w `horizon-server --dir <strona> status --json` (jest już w fazie `starting`), nigdy ze ścieżki programu: przy podmianie na ten sam silnik obie strony działają z tego samego pliku i SIGTERM trafiłby w grę. Odwrotnej kolejności (SIGTERM przed `stop`) nie sprawdzano.
  3. Proxy przestaje przekazywać ruch; stary silnik dostaje `save-all flush`, potem `stop` (zapisuje świat i graczy, zwalnia `session.lock`). Nowy silnik zajmuje świat w ciągu 2 ms od zwolnienia. `player-export` niepotrzebny: oba silniki czytają tych samych graczy ze wspólnego świata (S8). Proxy loguje się, gdy nowy silnik przyjmie logowanie.
  4. Proxy loguje się do nowego silnika jako każdy gracz, odtwarza jego konfigurację, połyka nowy `Login`, wysyła klientowi „usuń encje” i zamienia id gracza (id u klienta ↔ id w silniku). „Zapomnij chunki” niepotrzebne: nowe chunki zastępują stare w miejscu (S8). Klient widzi mrugnięcie encji, bez ekranu.
  5. Błąd nowego silnika → proxy trzyma graczy, `hotd` uruchamia z powrotem stary silnik tą samą drogą (ok. 1 s więcej).
- Budżet: pauza świata **krótsza niż 1 s w każdym przypadku**, czyli dla tego samego silnika i dla nowego pliku silnika (decyzja użytkownika 2026-11-12). Kolejność „stop, potem start” go nie spełniała: 0,96–1,07 s bez puli (S8), 1,16–1,34 s z pulą 1024/2048 i ok. 2,2 s z nowym plikiem silnika (S7). **Rozgrzany silnik spełnia go na Macu (S13, pula 1024/2048, 18 podmian z graczem):** ten sam silnik 470–541 ms z `save-all flush` (501–703 ms bez), nowy plik silnika z pustym cache 509–531 ms. Pula 4096/8192 (M0.20): 480–549 ms i 505–573 ms. Pauzę wypełnia prawie w całości `stop` starego silnika (0,4–0,6 s, w tym stałe 100 ms w `stop_dedicated`); od zwolnienia świata do „Done” ok. 0,1 s, proxy < 30 ms. Na ThinkCentre do zmierzenia (M0.11), bo `stop` zależy od dysku. Opis: `docs/podmiana.md`, `docs/spike/s13-rozgrzany-silnik.md`.
- Mixin `wait_for_world` (`crates/hot-host/wait_for_world.rs`): hak HEAD w `WorldRequest::prepare_world`, tylko w serwerze dedykowanym; czeka, dopóki `SessionLock::is_locked`, maksymalnie 120 s albo do pierwszego SIGTERM (`horizon_threads::stop_requests`). Blokada to `File::try_lock`, więc znika z procesem, także po `kill -9` (D13 bez czekania). Każde nowe Nightly może przesunąć to miejsce: test D18 sprawdza linię „waiting for the world” w logu.
- Wyjątek: zmiana rejestrów (nowe bloki w `.hnmod`, nowa pula) → klient potrzebuje nowych plików; wtedy aplikacja gracza je pobiera, a gracz wchodzi ponownie. Dlatego treść idzie przez sloty.
- To samo przełączenie służy do: aktualizacji Horizona (D18), odzyskania pamięci po generacjach (D4), wznowienia po awarii silnika (D13).

**D12. Dwa katalogi silnika (niebieski/zielony) pod nadzorem `hotd`.**
`/srv/horizon/engine-a` i `engine-b`, wspólny świat w `/srv/horizon/world` (przenoszony między nimi tylko w kroku 3 D11). Każdy pod własnym `horizon-server --dir` z własnym portem. Świat spoza katalogu (dowiązanie) i drugi port działają (S12); panel webowy tylko po jednej stronie albo na różnych portach.

**D13. Ochrona przed pętlą awarii i wznowienie po awarii.**
- Przy starcie `hot-core` ładuje tylko generacje oznaczone jako ostatnie dobre (przeżyły 5 min).
- `hotd` oznacza commit jako zły, gdy silnik padł ≤ 60 s od jego załadowania; nie aplikuje go ponownie.
- Pętla awarii (M7.1, 2026-11-13): druga awaria z rzędu, każda w ciągu 5 min od startu `hot-core`, włącza tryb bezpieczny: żaden mod gorący nie startuje, wpisy zostają w `mods.json` i wracają po czystym restarcie albo `hot load` (`HOT-E0408`). FAKT (`horizon-server/src/supervisor.rs`): supervisor Horizona wznawia grę po awarii z przerwą 5, 10, 20… s i poddaje się po 5 awariach w 10 min; tryb bezpieczny zatrzymuje pętlę wcześniej, a serwer działa dalej bez modów gorących. SIGTERM (systemd) zatrzymuje grę czysto (`signals.rs`).
- Po awarii silnika proxy trzyma połączenia graczy (świat stoi, bez ekranu ładowania), potem wprowadza ich jak w D11. Cel: nikt nie traci połączenia, jeśli silnik wstanie w < 30 s (limit ciszy klienta `READ_TIMEOUT` = 30 s, potwierdzone w kodzie w S8; proxy wysyła w tym czasie keep-alive, więc limit dotyczy tylko proxy).

### Budowa i zasoby

**D14. Mody gorące budowane w GitHub Actions; serwer tylko pobiera.**
`cdylib` modu nie zawiera plików Mojanga, więc CI jest dozwolone (inaczej niż silnik, R21). Workflow w repo serwera buduje `.so` dla architektury ThinkCentre, podpisuje (klucz CI) i wypycha wynik na gałąź `hot-builds` repo serwera (nie artefakt Actions: ten wymaga tokenu API i wygasa; ADR 0008). `hotd` pobiera ją gitem, sprawdza podpis (`ssh-keygen -Y verify`) i ładuje. Zastępuje „budowę ze źródeł na serwerze” z wersji 1.

**D15. Silnik budowany na ThinkCentre, w tle.**
Kontekst: R21 — silnik wymaga przygotowanych zasobów, a ThinkCentre i tak je ma. `native-build --server-output` w klasie „tło” (D16), z blokadą pliku (R27). Silnik budujemy rzadko: nowy `.hnmod`, nowe Nightly, zmiana `hot-host`. Pierwszy build ok. 12 min, potem przyrostowo (R22).

### Sprzęt (W3)

**D16. Gra ma pierwszeństwo; tło pracuje z resztek.**
- Gra (silnik, `hot-proxy`, `hot-pack`): slice systemd `horizon-game.slice`, `CPUWeight=10000`, `MemoryLow=` (z S10), `AllowedCPUs=` na poziomie procesu.
- Tło (`hotd`: git, pobieranie, budowa silnika, kopie zapasowe): `horizon-bg.slice`, `CPUWeight=100`, `MemoryMax=`, `IOWeight=1`, `nice 19`, `ionice -c3`; `SCHED_IDLE` tylko dla kopii `hot-state`. Zmiana w M6.2 (pomiar w maszynie wirtualnej): przy `CPUWeight=1` i grze na wszystkich rdzeniach `hotd` nie robił ani kroku przez 25 s, czyli w praktyce stał; przy 100 robi kroki co 10 s, a gra dostaje ok. 99% CPU. Z tego samego powodu `hotd` nie ma `SCHED_IDLE`. Myślnik w nazwie buduje drzewo: oba slice'y leżą w `horizon.slice` (`CPUWeight=1000`, więcej niż `system.slice`).
- Hamulec MSPT: `hotd` co sekundę czyta `status --json` (R25). p95 > 35 ms → zatrzymuje swoje ciężkie procesy potomne (`SIGSTOP` na ich grupach: dziś `du`, od M9 budowa silnika); p95 < 25 ms przez 10 s → wznawia. Sam `hotd`, `hot-pack` i hamulec nigdy nie są zatrzymywane. Git i komendy dla silnika też nie: dostarczają mody, a serwer, który stale nie nadąża, nie dostałby ich nigdy (git ma i tak najniższy priorytet w `horizon-bg.slice`). Kopie świata robi sam `horizon-server` w swoim procesie (`supervisor.rs`, `back_up`), więc są w grupie gry; ani slice, ani hamulec ich nie spowalniają. Opóźnienie reakcji 1–5 s (okno 100 ticków, R25).
- Dwa silniki nigdy nie trzymają świata naraz (S12). Rozgrzany silnik (D11) to jednak drugi proces, który przez czas przygotowania podmiany działa bez świata: 363 MB RSS przez 0,5 s z ciepłym cache albo ok. 3 s z zimnym przy puli 1024/2048, ok. 515 MB przez 0,8–0,9 s albo 4,1–4,4 s przy puli 4096/8192 (S13, M0.20, Mac); czy ThinkCentre to mieści, powie M0.11. Podmiana rusza tylko wtedy, gdy wolna pamięć to mieści; inaczej idzie bez rozgrzania i trwa dłużej (zapis w logu z kodem błędu). Drugi silnik działa równolegle tylko przy teście aktualizacji (D18). Ten test rusza tylko wtedy, gdy wolna pamięć mieści drugi silnik z zapasem ponad `MemoryLow` gry; inaczej czeka. Po zimnym starcie silnik zajmuje ok. 700 MB, po ciepłym 290–540 MB zależnie od puli (S7: RSS procesu `horizon-server`, pusty świat bez graczy, 10 s po starcie, Mac). R28 (1,4–1,5 GB) mierzy co innego: pełną grę `horizon-client --dedicated` z zasięgiem 10 (`docs/server.md` Horizona). Z graczami i wczytanymi chunkami będzie więcej niż w S7; wartości do `MemoryLow` i `MemoryMax` daje dopiero M0.11 (S10).

### Aktualizacje i naprawy (W4, W5, W8)

**D17. Aktualizacja modów:** push → CI (D14) → `hotd` (polling co 10 s, bez webhooka, bez otwartego portu) → klasyfikacja (tabela D24) → zastosowanie.

**D18. Aktualizacja Horizona — kanał Nightly.**
- Kontekst: R23, R26. `hotd` raz na godzinę pyta API GitHuba o nowe Nightly.
- Kroki: przygotowanie wersji (`horizon-launcher install` + `prepare` z zasobów serwera) → build silnika z `hot-host` (D15) z `--version <nowe Nightly> --ignore-pins`, bo `hot-host` przypina wersję Horizona (`horizon_revision`), a bez tej flagi build zostaje na starej (`docs/aktualizacje.md`, punkt 7) → test: silnik na kopii świata z ostatniej kopii, 10 botów przez 5 minut, mody gorące ładują się, log bez błędów, MSPT nie gorszy o > 20% → podmiana (D11) → obserwacja 10 min → jeśli gorzej, powrót tą samą drogą i wersja oznaczona jako zła.
- Okno zgodności klientów: tylko gdy zmienia się coś, czego klient potrzebuje (R24 — protokół stały; kanały `horizon:*` się dopisuje; nie wiadomo, czy `content_identity` jest takie samo na różnych Nightly, więc decyzja: każde nowe Nightly traktujemy jako zmianę, której klient potrzebuje, dopóki test w M9 tego nie obali: klient na jednym Nightly, serwer na drugim, ta sama pula). Aplikacja gracza aktualizuje Horizona sama przy starcie, więc okno = czas do najbliższego startu; serwer czeka maks. 24 h i ogłasza termin na czacie.
- Ryzyko: `libloading` może zniknąć z `Cargo.lock` Horizona → build nie przejdzie → serwer zostaje na starej wersji, `hot status` mówi dlaczego. Mało prawdopodobne: 0.8.9 we wszystkich 10 ostatnich wydaniach, ciągnie ją grafika (`wgpu-hal`, `ash`).
- Skąd brać Horizona (R29, 2026-11-13): binaria serwera budujemy ze źródeł w pełnej rewizji commita (instalator krok 4, `Dockerfile` etap `horizon`), nigdy z datowanego wydania, bo wydania znikają po kilku dniach. Nowe Nightly w M9 = nowa rewizja w `HORIZON_REV` (instalator i `Dockerfile`, test `installer_and_image_build_the_same_horizon_revision_from_source`). `horizon-launcher install <tag>` w kroku silnika z `hot-host` ma ten sam problem (tag znika); do rozwiązania w M9, do tego czasu ścieżka niesprawdzona.
- `hot-host` używa tylko nazwanych haków i `[transfer]`, nie łatek na dowolne funkcje (R23 — duży churn); dlatego aktualizacje rzadko będą się zatrzymywać.

**D19. Aktualizacja naszego systemu.**
`hotd`, `hot-proxy`, `hot-pack`, `hot-core` mają wydania w repo `horizon-hot`; `hotd` sprawdza je raz na dobę. `hot-core` → przeładowanie (D2). `hotd`, `hot-pack` → restart usługi, gra nie zauważa. `hot-proxy` → przekazanie gniazd i stanu szyfrowania do nowego procesu (`SCM_RIGHTS`); do czasu M13 aktualizacja proxy czeka na moment bez graczy online.

**D20. Samonaprawa — tabela.**

| Błąd | Reakcja automatyczna |
|---|---|
| panika w modzie | wyłączenie, poprzednia generacja (D4) |
| commit się nie buduje (CI) | nic nie zmienia się na serwerze; `hot status` |
| segfault silnika | supervisor wstaje, ostatnie dobre generacje, gracze przytrzymani (D13) |
| zły commit (awaria ≤ 60 s) | oznaczony, pominięty (D13) |
| zła aktualizacja Horizona | powrót (D18) |
| `hotd`/`hot-pack`/`hot-proxy` pada | systemd `Restart=always` |
| uszkodzony świat | **bez automatu**: `hot status` pokazuje problem i polecenie przywrócenia z kopii; operator potwierdza |
| pełny dysk | usuwanie najstarszych kopii ponad retencję i buildów; potem `hot status` |

**D21. Stan i komunikaty bez zewnętrznych powiadomień.**
`hot status` (terminal na serwerze) i `/hot status` (w grze) pokazują: wersję Horizona, commit modów, listę modów i generacji, ostatnie błędy, oczekujące aktualizacje. Operatorzy w grze dostają wiadomość na czacie, gdy coś wymaga ich decyzji.

### Diagnostyka i przejmowanie projektu (W9)

**D27. Logi pisane dla modelu, który nie zna projektu.**
- Kontekst: projekt będą kontynuować i naprawiać agenci bez historii rozmów; błąd ma być naprawialny z samego logu.
- Wspólny crate `hot-diag`, używany przez wszystkie składniki (`hotd`, `hot-core`, `hot-sdk`, `hot-proxy`, `hot-pack`, `hot-play`, skrypty instalacyjne przez `hot-diag` CLI). Nikt nie loguje z pominięciem go.
- Format: JSON Lines, jeden wpis na linię, klucze i treść po angielsku (spójnie z logami Horizona; komunikaty dla gracza osobno, po polsku). Każdy wpis o błędzie ma pola:

| Pole | Treść | Przykład |
|---|---|---|
| `code` | stały kod błędu | `HOT-E0412` |
| `component`, `version` | składnik i commit, z którego zbudowany | `hotd`, `a1b2c3d` |
| `what` | co się stało, jednym zdaniem, bez skrótów projektowych | `Hot mod "sklep" panicked in server_tick and was disabled` |
| `expected` | co powinno się stać | `server_tick returns without panic` |
| `context` | wszystkie identyfikatory potrzebne do odtworzenia: mod, generacja, commit repo serwera, wersja Horizona, ścieżki plików | `{"mod":"sklep","generation":7,...}` |
| `cause` | najbardziej prawdopodobna przyczyna (jeśli znana) | `index out of bounds at mods/sklep/src/lib.rs:42` |
| `fix` | konkretny następny krok: komenda albo plik do zmiany | `fix the panic, push; previous generation 6 is active meanwhile` |
| `doc` | ścieżka do wpisu w katalogu błędów | `docs/bledy/HOT-E0412.md` |
| `source` | miejsce w kodzie, które zgłosiło błąd | `crates/hot-core/src/generation.rs:118` |
| `chain` | pełny łańcuch przyczyn (`anyhow`/`std::error::Error::source`), backtrace przy panice | `[...]` |
| `state` | co system zrobił sam i w jakim stanie zostawił serwer | `rolled back to generation 6; server running` |

- Zakaz: komunikatów bez kontekstu („failed”, „error: invalid data”), `unwrap()` poza testami, połykania błędów. Clippy `unwrap_used`, `expect_used` jako błąd.
- Katalog błędów `docs/bledy/HOT-EXXXX.md`: znaczenie, typowe przyczyny, komendy diagnostyczne, naprawy, pliki kodu. Test w CI: każdy kod użyty w kodzie ma wpis i odwrotnie; każdy wpis ma wszystkie sekcje.
- Kategorie kodów: `E00xx` sama diagnostyka (`hot-diag`), `E01xx` instalacja, `E02xx` git i CI, `E03xx` budowa silnika, `E04xx` mody gorące, `E05xx` dane i paczka zasobów, `E06xx` proxy i podmiana silnika, `E07xx` aktualizacje, `E08xx` sprzęt i hamulec, `E09xx` aplikacja gracza, `E10xx` świat i kopie.
- Rotacja: `/srv/horizon/logs/<składnik>.jsonl`, 50 MB × 5 plików; u gracza w katalogu danych aplikacji.

**D28. `hot report` — samowystarczalny pakiet dla obcego modelu.**
- Na serwerze `hot report`, u gracza „Napraw” w `hot-play`. Wynik: jeden plik tekstowy (Markdown, < 200 KB, do wklejenia w czat z dowolnym modelem).
- Zawartość, w tej kolejności:
  1. Nagłówek „Czym jest ten system” — 20 linii: składniki, przepływ, gdzie co leży (stały tekst z `hot-diag`).
  2. Wersje: nasze składniki, Horizon, system, sprzęt.
  3. Ostatnie błędy z pełnymi wpisami D27 i treścią ich wpisów z `docs/bledy/`.
  4. `hot status`, ostatnie 200 linii logów każdego składnika i silnika, konfiguracja.
  5. Instrukcja dla modelu: gdzie jest repo, jak odtworzyć, czego nie wolno (lista z `AGENTS.md`).
- Sekrety (`forwarding.secret`, klucze, tokeny, adresy IP graczy) zamaskowane; test w CI sprawdza, że żaden znany sekret nie trafia do raportu.

**D29. Gotowy projekt tłumaczy się sam, bez tego planu.**
- `PLAN.md` to plan budowy, nie dokumentacja. Po M12 wszystko, co zespół musi wiedzieć, jest w repo poza nim: decyzje w `docs/adr/` (D1–D29 przeniesione), działanie w `docs/ARCHITECTURE.md`, obsługa w `docs/operator.md`, błędy w `docs/bledy/`.
- `AGENTS.md`: czym jest projekt, słownik, mapa repo, komendy, zasady nienaruszalne, jak dodać mod, jak dodać składnik, jak debugować z `hot report`.
- Przewodniki „jak zrobić typową rzecz”: nowy mod gorący, nowy hak w ABI, nowy kod błędu, wydanie wersji.
- Każdy crate ma na górze `lib.rs`/`main.rs` komentarz: do czego służy, kto go używa, z czym rozmawia.
- Decyzje zmieniane później → nowy ADR, stary oznaczony „zastąpiony przez”.
- Dokumentacja zmienia się w tym samym commicie co kod (sprawdza to recenzja PR).

### Repozytoria, instalacja, aplikacja gracza

**D22. Format repo serwera (na GitHubie).**
```
repo-serwera/
  hot.toml            # porty, kanał aktualizacji, operatorzy, okno zgodności
  mods/               # może nie istnieć → czysty serwer
    <id>/
      mod.toml        # id, wersja, sloty
      Cargo.toml src/ # opcjonalnie: logika (cdylib, zależy od hot-sdk)
      data/           # opcjonalnie: paczka danych
      assets/         # opcjonalnie: modele i tekstury slotów
  native/             # opcjonalnie: mody natywne .hnmod (podmiana silnika)
  .github/workflows/hot.yml   # dodawany przez instalator: budowa modów (D14)
```
Prywatne repo → klucz tylko do odczytu (deploy key) generowany przez instalator.

**D23. Workspace `horizon-hot`.**
`crates/hot-abi`, `crates/hot-diag`, `crates/hot-sdk`, `crates/hot-core`, `crates/hotd`, `crates/hot-proxy`, `crates/hot-pack`, `crates/hot-play` (aplikacja gracza), `bundle/hot-host` (`horizon-mixin.toml`), `content/hot-slots` (`.hmod`), `examples/`, `scripts/server-install.sh`, `scripts/install.sh`, `scripts/install.ps1`, `.github/workflows/` (CI + wydania). Conventional Commits, forma bezosobowa, autor Szymon Sosnowski.

**D24. Klasyfikacja zmian (serce `hotd`).**

| Zmiana | Działanie | Co widzi gracz |
|---|---|---|
| `mods/*/data/` | kopia + `reload` | nic |
| `mods/*/src/` | artefakt z CI → nowa generacja | nic |
| `mods/*/assets/`, sloty w puli | nowa paczka (nowy sha1) | nowy wygląd od razu z nakładką ładowania ok. 1,3 s albo przy następnym wejściu (D9, decyzja użytkownika) |
| nowy / usunięty mod gorący | `init` / `deactivate` | nic |
| `hot-core` | przeładowanie | nic |
| `native/*.hnmod`, nowe Nightly, `hot-host` | build w tle + podmiana silnika (D11) | świat stoi bez ekranu ładowania krócej niż 1 s (D11): z rozgrzanym silnikiem 470–541 ms, nowy plik silnika 509–531 ms (Mac, S13, pula 1024/2048); z pulą 4096/8192 480–573 ms (M0.20); ThinkCentre do zmierzenia (M0.11) |
| nowe bloki w `.hnmod`, zmiana puli | jak wyżej + nowe pliki u gracza | aplikacja pobiera pliki, gracz wchodzi ponownie |

**D25. Instalacja serwera jednym poleceniem (Ubuntu na ThinkCentre, `sudo` jest).**
`curl -fsSL <adres>/server-install.sh | sudo bash -s -- --repo <github-url> --eula` (EULA akceptuje właściciel, nie skrypt):
1. Sprawdzenie: Ubuntu, architektura, RAM, dysk, wolne porty 25565/8131.
2. Użytkownik `horizon`, katalogi `/srv/horizon`.
3. `horizon-server` i `horizon-engine` zbudowane ze źródeł Horizona w przypiętej rewizji (od 2026-11-13: datowane wydania Nightly znikają z GitHuba po kilku dniach); `horizon-launcher`; git.
4. Zasoby gry: logowanie Microsoft kodem urządzenia (jedyny krok ręczny) albo `import-resources` z folderu skopiowanego z Maca.
5. `init` świata, sekrety: `forwarding.secret`, klucz deploy, klucz weryfikacji artefaktów CI; whitelist, operatorzy.
6. Jednostki systemd: `hotd`, `hot-proxy`, `hot-pack`, silniki a/b; slice'y D16.
7. `ufw`: 25565 i 8131; 25566 tylko lokalnie.
8. Kopie: co noc, retencja 7 dni, razem ze `hot-state/` i `hot-data/`.
9. Pierwszy build silnika w tle; do tego czasu działa silnik bez modów natywnych.
10. Autotest: bot wchodzi, testowy mod gorący ładuje się i odpowiada, `status` OK → „Działa: <adres>”.
- Idempotentny; `--uninstall` usuwa wszystko oprócz świata i kopii.

**D26. Aplikacja gracza `hot-play` — terminalowa, jedna linijka.**
- Instalacja: macOS/Linux `curl -fsSL <adres>/install.sh | sh`, Windows `irm <adres>/install.ps1 | iex`. Bez uprawnień administratora. Skrypt sprawdza SHA-256 i tworzy skrót „Graj” na pulpicie/w menu.
- Menu: `[1] Graj  [2] Napraw  [3] Wyjdź`, po polsku, bez innych pytań.
- „Graj” przy każdym starcie: samoaktualizacja `hot-play` → sprawdzenie i naprawa: `horizon-launcher` (instalacja/aktualizacja), wersja zgodna z serwerem, `prepare`, instancja, `hot-slots.hmod` + `horizon-mods.txt`, adres serwera → logowanie Microsoft tylko za pierwszym razem → `play --server`.
- „Napraw”: pełna weryfikacja i ponowne pobranie wszystkiego, potem plik z logiem do wysłania operatorowi.
- Działa na kupionym Minecrafcie; bez obchodzenia sprawdzenia zakupu; bez plików Mojanga w aplikacji; bez logo Minecrafta, Mojanga i Horizona.
- Budowa w CI: macOS arm64 + x86_64, Windows x86_64, Linux x86_64 (dozwolone — brak plików Mojanga).
- HIPOTEZA: plik pobrany skryptem nie dostaje znacznika kwarantanny / Mark of the Web, więc bez podpisu nie ma ostrzeżeń. Sprawdzenie w M11; jeśli obleje — decyzja o płatnym podpisie.

---

## 5. Kroki

Każdy krok: konkretna zmiana + sprawdzenie, które obleje, jeśli zmiana nie działa. Odhaczaj `[x]`.

### M0 — Spike'i (rozstrzygają niewiadome)
- [x] M0.1 Repo: `repo-init --stack rust`, `verify-init`, CI Rust (`fmt`, `clippy -D warnings`, `test`). Sprawdzenie: hook odrzucił commit „test”; `cargo test` zielone (`f788091`).
- [x] M0.2 S1: `hot-abi` jako crate ścieżkowy (`[[crate]]`) w bundlu `hot-host`. Sprawdzenie: `native-build --server-output` przechodzi. Gdy nie: `xtask` kopiuje moduł do bundla.
- [x] M0.3 S2: serwer dedykowany ładuje testowy `.hmod` (R11) i klient bez niego dostaje rozłączenie, a z nim wchodzi. Sprawdzenie: `setblock` bloku z moda + oba wejścia klienta.
- [x] M0.4 S3: `/sklep` przez anulowany `player_command` bez „Unknown command”. Sprawdzenie: odpowiedź na czacie klienta.
- [x] M0.5 S4: `Host::run_command` z `server_tick`. Sprawdzenie: `say` w logu.
- [x] M0.6 S5: blok z puli z modelem z paczki serwera (sha1 ustawiony). Sprawdzenie: zrzut ekranu z poprawnym modelem.
- [x] M0.7 S6: proxy-prototyp wysyła `ResourcePackPush` w trakcie gry. Sprawdzenie: zmiana tekstury bez mrugnięcia. Wynik opcjonalny (D9). Wynik: zmiana działa, ale z nakładką ładowania ok. 1,3 s.
- [x] M0.8 S7: rozmiar puli 4096/8192: czas startu klienta i serwera, RAM. Sprawdzenie: liczby w `docs/sloty.md`; start klienta wolniejszy o < 2 s, inaczej mniejsza pula.
- [x] M0.9 S8: prototyp podmiany wariantu A: proxy + dwa silniki na jednym świecie, „usuń encje / zapomnij chunki”, mapowanie id gracza; pomiar pauzy; czy da się rozgrzać nowy silnik przed zwolnieniem świata; ile czasu klient czeka bez pakietów, zanim się rozłączy. Sprawdzenie: prawdziwy klient zostaje połączony, bez ekranu ładowania, pauza zmierzona (`docs/podmiana.md`). **Blokuje D11 i D13.** Jeśli pauza > 1 s — decyzja użytkownika przed M8.
- [ ] M0.10 S9: `native-build` na Ubuntu x86_64 bez okna, z zasobami z `prepare`. Sprawdzenie: eksport startuje.
- [ ] M0.11 S10: ThinkCentre: model, CPU, RAM, wersja Ubuntu, architektura; pomiar pustego serwera i 10 botów. Sprawdzenie: `docs/sprzet.md` z wartościami `AllowedCPUs`, `MemoryLow`, `MemoryMax`.
- [x] M0.12 S11: ostatnie 10 Nightly — co zmieniło `content_identity`, kanały `horizon:*`, API haków, `Cargo.lock` (`libloading`). Sprawdzenie: `docs/aktualizacje.md`. Wynik: używane haki i pliki bez zmian od 11-03; D18 potrzebuje `--ignore-pins`.
- [x] M0.13 S12: `horizon-server` ze światem poza swoim katalogiem i na porcie 25566. Sprawdzenie: dwa supervisory naprzemiennie na tym samym świecie.
- [x] M0.14 Wyniki S1–S12 jako `docs/adr/0002-spike.md`; korekta D1–D29, jeśli trzeba. Sprawdzenie: każda NIEWIADOMA w planie zamieniona na FAKT albo nową decyzję. Wynik: poprawione D6, D8, D9, D16, D18, D24; w planie nie ma już NIEWIADOMEJ poza definicją; S9 i S10 czekają na ThinkCentre (M0.10, M0.11).
- [x] M0.19 S13: rozgrzany silnik (D11): rozgrzanie cache przed podmianą, mixin wstrzymujący start przed zajęciem świata, `save-all flush` przed zamrożeniem. Sprawdzenie: prototyp z S8 z pulą 1024/2048, po 6 podmian na ten sam silnik i na nowy plik silnika, każda pauza krótsza niż 1 s na Macu; pamięć czekającego silnika zmierzona. Gdy nie: mniejsza pula, potem decyzja użytkownika z liczbami. Na ThinkCentre powtórka w M0.11. Decyzja: `docs/adr/0004-pauza-podmiany-ponizej-1-s.md`.
  - Wynik (Mac, gracz w grze przez cały czas): ten sam silnik 501–703 ms bez `save-all flush`, 470–541 ms z nim; nowy plik silnika z pustym cache 509–531 ms (6/6). Czekający silnik: 363 MB RSS. Blok postawiony tuż przed podmianą jest na nowym silniku i u klienta. Mixin `wait_for_world` w `crates/hot-host/`, flagi `--warm`/`--flush` w proxy S8, skrypty `spikes/s13-rozgrzany-silnik/`. Szczegóły i wymagania dla `hotd`: `docs/spike/s13-rozgrzany-silnik.md`. Haki `tick_block`/`random_tick` przechodzą do M0.20, bo zależą od kształtu puli.
- [x] M0.20 Zamrożenie puli: rozmiar i bloki z tickiem. Sprawdzenie: (1) S13 z pulą 4096/8192 (`spikes/s13-rozgrzany-silnik/`), 6 podmian na ten sam silnik i 6 na nowy plik silnika; pula zostaje największa, przy której każda pauza na Macu jest krótsza niż 1 s; (2) decyzja o zakresie slotów z tickiem zapisana w `docs/sloty.md`. Zmiana puli zmienia `content_identity`, więc obie rzeczy rozstrzygają się razem, zanim pula trafi do graczy.
  - Wynik (Mac, gracz w grze, `--warm --flush`, 2026-11-13): pula 4096/8192 — ten sam silnik 480–549 ms (6/6), nowy plik silnika z pustym cache 505–573 ms (6/6); rozgrzanie 0,8–0,9 s z ciepłym cache i 4,1–4,4 s z pustym; czekający silnik ok. 515 MB. Pula zostaje 4096/8192 (`hotd::slots`, wersja `4096.8192.0`); większej nie mierzono. Slotów z tickiem nie ma, powody i warunek powrotu w `docs/sloty.md`.
  - Przy okazji: czekającego silnika nie zatrzymuje samo `horizon-server stop` (supervisor pisze `stop` na stdin, którego silnik nie czyta; czekało do limitu 120 s). Mixin wychodzi teraz z czekania po pierwszym SIGTERM: `stop` + SIGTERM → `Stopped` po 0,5 s, drugi serwer bez zmian.

### M0b — Diagnostyka (przed każdym innym kodem, D27–D29)
- [x] M0.15 `AGENTS.md` z opisem projektu, słownikiem i zasadami (D29); uzupełniany w każdym kolejnym kroku. Sprawdzenie końcowe w M12. Wynik: `AGENTS.md` z mapą repo, zasadami pracy na maszynie z grą i odesłaniem do wyników M0.
- [x] M0.16 `hot-diag`: typ błędu z polami D27, zapis JSONL, rotacja, maskowanie sekretów, przechwytywanie paniki z backtrace. Sprawdzenie: testy jednostkowe — wpis bez `code`, `what` lub `fix` nie kompiluje się (builder z wymaganymi polami); znany sekret w kontekście zamaskowany. Wynik: `crates/hot-diag`, zależność `serde_json` (ADR 0003), własne kody `E00xx`.
- [x] M0.17 Katalog `docs/bledy/` + test zgodności kodów w CI. Sprawdzenie: kod bez wpisu → CI czerwone. Wynik: `crates/hot-diag/tests/catalog.rs`; obserwacja: przykład `HOT-E0401` w komentarzu dał czerwony test.
- [x] M0.18 Szkielet `hot report`. Sprawdzenie: raport ze sztucznego błędu zawiera wszystkie 5 części D28 i < 200 KB. Wynik: `hot-diag report`; test z logiem 5 MB, sekretem i adresem IP gracza.

Zasada dla kroków M1–M13: każdy nowy błąd dostaje kod i wpis w katalogu, a sprawdzenie kroku obejmuje wywołanie co najmniej jednego błędu i obejrzenie wpisu w logu.

### M1 — Rdzeń przeładowania (`hot-abi`, `hot-sdk`, `hot-host`, `hot-core`)
- [x] M1.1 `hot-abi`: VTable z wersją, `init`, `deactivate`, `server_tick`, `player_chat`; testy `size_of`/`align_of`.
- [x] M1.2 `hot-sdk`: makro `hot_mod!`, `HostCtx<'call>`, `catch_unwind` w każdym eksporcie, stan między generacjami.
- [x] M1.3 `hot-host`: tylko ładowanie `hot-core` i przekazywanie haków (D2). Paczka składana skryptem: kopia `crates/hot-abi` jako `[[crate]]` (wynik S1, `docs/spike/s1-hot-abi-w-silniku.md`).
- [x] M1.4 `hot-core`: `hot load <ścieżka>`, ładowanie poza wątkiem serwera, przełączenie na granicy ticku, unikalne nazwy plików, ostatnie dobre generacje (D13).
- [x] M1.5 `examples/licznik`: liczy ticki, odpowiada na czacie, stan przechodzi przez przeładowanie.
- Sprawdzenie M1:
  - przeładowanie zmienia odpowiedź, PID silnika ten sam, licznik zachowany;
  - 100 przeładowań przy 10 botach: p95 MSPT rośnie < 2 ms; pamięć rośnie ≤ rozmiar `.so` na generację;
  - `panic!()` w `server_tick` → mod wyłączony, poprzednia generacja, serwer działa;
  - przeładowanie samego `hot-core` → PID ten sam, mody działają dalej.

### M2 — CI modów i `hotd` (git → artefakt → załadowanie)
- [ ] M2.1 Workflow `hot.yml` dla repo serwera: budowa `.so`, podpis, artefakt (D14). Zrobione: `templates/server-repo/.github/workflows/hot.yml`, `scripts/hot-build.sh`, `scripts/hot-publish.sh` (ADR 0008); skrypty sprawdzone lokalnie. Nie sprawdzone: przebieg w GitHub Actions — brak repo serwera (założenie publicznego repo to publikacja, decyzja właściciela).
- [x] M2.2 `hotd`: polling repo co 10 s, pobranie artefaktu, weryfikacja podpisu (`crates/hotd`).
- [x] M2.3 Klasyfikacja zmian D24 (testy jednostkowe na przykładowych diffach: `crates/hotd/src/classify.rs`).
- [x] M2.4 `hot apply` przez `horizon-server cmd`, odczyt `hot-state/status.json`, oznaczanie złych commitów (D13).
- [x] M2.5 `hot status` (terminal: `hotd status`) i `/hot status` (gra) (D21).
- Sprawdzenie: push → zmiana na serwerze ≤ 3 min; repo bez `mods/` → czysty serwer; artefakt z błędnym podpisem odrzucony; commit niekompilujący się → serwer bez zmian, `hot status` mówi dlaczego.

### M3 — Dane (D7)
- [x] M3.1 Kopia `data/` + `reload`; usunięcie moda usuwa paczkę (`crates/hotd/src/data.rs`). Sprawdzone na serwerze testowym (Mac) z graczem online (klient Horizon, tryb offline): przed commitem `recipe give` → „Unknown recipe”, po commicie z `mods/przepis/data/hottest/recipe/…` → „Unlocked 1 recipe(s)”, po commicie bez moda znów „Unknown recipe”; gracz cały czas połączony, PID ten sam; `reload` z graczem 0,25–0,55 s bez „Can't keep up”. Bez gracza: tabela łupów i funkcja z paczki działają. Nie sprawdzone: pomiar na ThinkCentre — brak maszyny.
- Sprawdzenie: nowy przepis działa, PID ten sam; zmierzony skok ticka przy `reload` zapisany w `docs/sprzet.md`.

### M4 — Haki i funkcje v1 (D5, D6)
- [x] M4.1 Pozostałe haki D5 i funkcje D6. `tick_block` i `random_tick` przeniesione do M5 (D5); komendy gracza przez tick gracza (D6a).
- [x] M4.2 `examples/teleport-blok`, `examples/sklep` (komenda `/sklep`).
- Sprawdzenie: test integracyjny z botem dla każdego haka (zdarzenie → widoczny efekt). Wykonane prawdziwym klientem przez MCP zamiast bota (wynik w sekcji 8) i testami `hot-core` na udawanym gospodarzu.

### M5 — Sloty i wygląd (D8, D9)
- [x] M5.1 `hot-slots.hmod` w rozmiarze z S7; przydział tylko dopisywany.
  - Źródło puli generuje `hotd slots-source <katalog>` (stałe `hotd::slots::BLOCKS`/`ITEMS`, wersja z nich wyliczona), plik buduje `cargo horizon-mod build`. FAKT (Mac, 2026-11-13): dwa buildy tego samego źródła z różnych katalogów dają ten sam SHA-256, więc serwer i gracze budują go sami i dostają te same bajty.
  - Blok moda deklaruje plik `mods/<id>/assets/blocks/<nazwa>.png` (tekstura na wszystkie ściany); nazwa w grze z `mods/<id>/assets/lang.json` (`{"<nazwa>": "…"}`). Przydział w `hot-state/slots.json` robi `hotd` (jedyny zapisujący), tylko dopisywany; pełna pula → `HOT-E0209`.
  - Mod z kodem używa nazwy `<id>:<nazwa>`; `hot-core` tłumaczy ją na slot w `block`, `set_block` i `give` (bez zmiany ABI). Znana luka: przepisy i inne pliki `data/` muszą na razie wskazywać `hot_slots:slot_N`.
- [x] M5.2 `hot-pack`: budowa paczki z `assets/`, sha1, serwowanie na 8131.
  - Paczka powtarzalna (posortowane wpisy, stała data), `hot-state/pack/pack.zip` i `pack.sha1`; serwuje ją `hotd serve-pack` (osobny proces, D9).
  - FAKT (źródła Horizona, `lan/conn.rs`, `NetConfig` z `server.toml` przy starcie): silnik czyta adres i sha1 paczki tylko przy starcie. Dlatego `server.toml` ma adres bez sha1: klient bez sha1 pobiera paczkę przy każdym wejściu (`server_packs/download.rs`), więc nowy wygląd dochodzi przy następnym wejściu bez restartu silnika. Sha1 przyda się `hot-proxy` (M8), które poda aktualny przy logowaniu.
  - Bloki ze slotów: haki `tick_block`/`random_tick` przeniesione do M0.19 (2026-11-13). FAKT (źródła Horizona 66f3981, `crates/horizon-client/src/mod_api.rs`): (1) hak dostaje `LevelWriteHost`, który nie zmienia całego bloku („Whole-block writes wait for block entity support”, l. 715), tylko właściwość (`set_block_property`), a bloki ze slotów nie mają właściwości; (2) silnik woła `random_tick` tylko dla bloków z losowym tickiem, więc pula musiałaby mieć `"random_ticks": true` (`horizon-mods/src/content.rs`), co zmienia `content_identity` i wymaga nowej puli u graczy; (3) `LevelHost` (l. 685) nie zamienia `BlockStateId` na nazwę, więc filtr slotów nie ma z czego zbudować tablicy. Hak w tej chwili dałby modom tylko cząsteczki i dźwięk, za cenę wywołania przez ABI dla każdego tykającego bloku z puli.
  - `serve-pack` podaje też `/hot_slots.hmod` (dla graczy do czasu `hot-play`), a przed pierwszą paczką paczkę bez bloków, bo przy `require-resource-pack = true` brak paczki wyrzuca gracza.
  - Instalator, krok 8: `cargo-horizon-mod` z rewizji Horizona serwera (FAKT, Mac: `cargo install --locked --git … --rev 66f3981` w 30 s, ten sam SHA-256 puli), pula w `horizon-mods/`, `--address` do adresu paczki, usługa `horizon-hot-pack`. Nie sprawdzone: brak maszyny x86_64.
  - Znana luka: aktualizacja `hotd`, która zmienia format paczki, nie przebudowuje istniejącego `pack.zip` (odbudowa tylko, gdy pliku brak); do czasu poprawki: usuń `hot-state/pack/pack.zip`, `hotd` zbuduje go w następnym kroku.
- Sprawdzenie: nowy blok z repo stawiany bez restartu; po następnym wejściu ma swój model (zrzut); podmiana silnika nie zmienia przydziału.
  - Wynik na Macu (2026-11-13), silnik z nowym `hot-core`, pula 1024/2048 w serwerze i w instancji `hot-spike`: commit z `mods/wyglad/assets/blocks/lada.png` → `hotd once` (bez CI, 0,3 s) przydziela `slot_0`, wysyła `hot slots` (odpowiedź „Bloki modów: 1”), `serve-pack` daje paczkę (200). Mod próbny: `set_block(pos, "wyglad:lada")` stawia `hot_slots:slot_0` (`execute if block` → „Test passed”), `block(pos)` zwraca `wyglad:lada`. Gracz przyjmuje paczkę w grze i widzi teksturę oraz nazwę „Lada próbna”. Nowa tekstura i nazwa w repo → `hotd once` → ponowne wejście: nowy wygląd i „Lada po zmianie”, ten sam PID silnika. Nie sprawdzone: przydział po podmianie silnika (M8) i ThinkCentre.

### M6 — Serwer na ThinkCentre: instalator i priorytety (D16, D25)
- [ ] M6.1 `server-install.sh` kroki 1–8 i 10 (bez proxy i podmiany — dochodzą w M8). Zrobione: `scripts/server-install.sh`, kody `HOT-E0101`–`HOT-E0111`; krok 9 (silnik z `hot-host`) tylko po `--trust-native-code` od właściciela. Sprawdzone w maszynie wirtualnej Ubuntu 22.04 aarch64 z `--test-no-horizon`. Nie sprawdzone: brak maszyny x86_64 (pobranie Horizona, pliki gry, świat, usługa serwera, silnik z `hot-host`, autotest z `hot-core`).
- [x] M6.2 Slice'y i hamulec MSPT w `hotd`.
  - Instalator zapisuje `horizon.slice` (`CPUWeight=1000`), `horizon-game.slice` (`CPUWeight=10000`, `IOWeight=10000`) z silnikiem i `hot-pack` oraz `horizon-bg.slice` (`CPUWeight=100`, `IOWeight=1`) z `hotd` (`Nice=19`, `IOSchedulingClass=idle`) i kopią `hot-state` (do tego `CPUSchedulingPolicy=idle`). `MemoryLow`/`MemoryMax` czekają na S10 (M0.11).
  - Hamulec (`crates/hotd/src/brake.rs`) działa we własnym wątku, bo główny wątek czeka na procesy, które on zatrzymuje. Procesy tła startują we własnej grupie procesów (`brake::output`), sygnał idzie wywołaniem `kill(-grupa)` z `libc` (ADR 0009; program `kill` z Ubuntu 24.04 źle czyta ujemny numer grupy). Odczyt, który się nie udał, niczego nie zmienia; serwer w pauzie albo zatrzymany liczy się jako spokojny. `hot status` pokazuje wiersz „hamulec”.
  - Testy: odczyt `sample.game.mspt.p95` z JSON-a w kształcie z `admin.rs`, histereza 35/25/10 s na jawnych `Instant`, prawdziwy proces zatrzymany (`T` w `ps`) i wznowiony po zwolnieniu. Mutacja (`signal` nic nie robi) czerwieni test.
  - Na żywo (Mac, 2026-11-13, prawdziwy silnik z `hot-host`): mod gorący, który śpi 50 ms w każdym ticku → p95 55 ms, hamulec włączony po ok. 2 s (`p95_ms: 53.9` w `hotd.jsonl`), w `hot status` wiersz „hamulec: serwer nie nadąża…”; `hot unload` → p95 0,6 ms, zwolnienie 11 s później, wiersz znika. Boty się nie połączyły, ale serwer nie był w pauzie, więc ticki szły.
  - Maszyna wirtualna Ubuntu 22.04 aarch64 (lima, 2 CPU), `--test-no-horizon`: usługi w `/horizon.slice/horizon-game.slice/…` i `/horizon.slice/horizon-bg.slice/…`, wagi w `cpu.weight` zgodne; 2 rdzenie zajęte w grze i 2 w tle przez 20 s → gra 40,2 s CPU, tło 0 ms; drugi przebieg instalatora niczego nie restartuje (ten sam PID `hotd`), zmiana pliku slice'a → `daemon-reload` i nowa waga.
  - Znalezione w tej maszynie i poprawione w instalatorze (oba weszły z pulą slotów i nie były sprawdzane w maszynie wirtualnej): `cargo horizon-mod` padał, bo rustup był instalowany bez domyślnego toolchainu, a pula buduje się poza repo (teraz `rustup default stable`, gdy brak domyślnego); ponowne uruchomienie odrzucało port 8131 zajęty przez naszą własną usługę `horizon-hot-pack` (teraz każdy port ma swoją usługę).
  - NIEWIADOMA: priorytet dysku. W maszynie wirtualnej planista to `mq-deadline`, który nie bierze pod uwagę `IOWeight` ani klasy `idle` (działa z BFQ); na NVMe bywa `none`. Na ThinkCentre do sprawdzenia: `cat /sys/block/*/queue/scheduler`.
  - Nie sprawdzone: `stress-ng` + 10 botów z p99 MSPT < 50 ms — wymaga silnika na Linuksie (ThinkCentre, M0.10).
- [x] M6.3 Idempotencja i `--uninstall`.
  - Idempotencja: sprawdzona w M6.1 i M6.2 (drugi przebieg nie buduje ani nie restartuje niczego, klucze i pliki jednostek bez zmian).
  - `server-install.sh --uninstall` wypisuje, co usunie i co zostawi, i kończy się kodem 2 bez zmian. Z `--yes`: kopia `hot-state` i `hot-keys` w `/srv/horizon/backups/hot-state-przed-odinstalowaniem-<data>.tar.gz`, zatrzymanie i usunięcie usług, minutnika i slice'ów, usunięcie reguł zapory dla portu gry i paczki (SSH i sama zapora zostają), usunięcie `bin`, `horizon`, `src`, `tmp`, `engines`, kluczy, toolchainu, `hot-core`, `hot-state`, `hot-keys`, `hot_slots.hmod`, linii `mod.hot_slots` i adresu naszej paczki zasobów w `server.toml`. Zostają świat z ustawieniami, kopie, logi i użytkownik `horizon`. Pakietów systemowych nie rusza. Porażka: `HOT-E0113`.
  - Maszyna wirtualna (Ubuntu 22.04 aarch64, `--test-no-horizon`, podstawione `world/level.dat`, `backups/a.zip`, `server.toml` z adresem paczki i `horizon-mods.txt` z dwoma modami): bez `--yes` kod 2 i 6 jednostek dalej działa; z `--yes` kod 0, 0 jednostek `horizon*`, w zaporze tylko 22, zapora aktywna, brak procesów użytkownika `horizon`, świat i kopia na miejscu, w `server.toml` zostały `port`, `motd` i `view-distance` (właściciel `horizon`), w `horizon-mods.txt` tylko `mod.inny:true`, archiwum ma `hot-state/hotd.json`. Drugie `--uninstall --yes`: kod 0, nic do usunięcia. Ponowna instalacja na tym samym świecie: „Test bez Horizona przeszedł”, `mod.hot_slots` wraca, `level.dat` ten sam; kolejne odinstalowanie: 0 jednostek.
  - Znalezione w trakcie i poprawione: slice'y bez plików zostawały aktywne do restartu; teraz są zatrzymywane.
  - Nie sprawdzone: odinstalowanie z prawdziwym Horizonem (brak maszyny x86_64).
- Sprawdzenie: czysta maszyna wirtualna Ubuntu (multipass, wersja jak na ThinkCentre) → jedno polecenie → „Działa”; drugie uruchomienie bez zmian; `stress-ng` w `horizon-bg.slice` + 10 botów → p99 MSPT < 50 ms; potem to samo na ThinkCentre.

### M7 — Samonaprawa (D13, D20)
- [x] M7.1 Segfault w module testowym → ostatnie dobre generacje, brak pętli.
  - Test `segfault_brings_back_last_good_generation`: proces potomny wczytuje mod, który po 3 tickach pisze pod adres 8; proces kończy się sygnałem 11, a następny start bierze ostatnią dobrą generację.
  - Test `second_crash_in_a_row_puts_hot_mods_to_sleep`: druga awaria z rzędu → brak modów, `safe_mode` w `status.json`, `HOT-E0408`, linia w `hot list`; czysty restart przywraca mody. `load_and_unload_end_safe_mode_for_a_mod`: `hot load` i `hot unload` kończą tryb bezpieczny dla jednego moda. Oba padają, gdy tryb bezpieczny jest wyłączony (sprawdzone mutacją `SAFE_MODE_AFTER = 99`).
  - `hot status` w terminalu pokazuje „TRYB BEZPIECZNY” z listą uśpionych modów.
  - Prawdziwy silnik z `hot-host` na Macu (2026-11-13, serwer testowy, Horizon `nightly-2026-11-05-66f3981`): licznik v1 przeszedł próbę („dobra”); `hot load` moda z segfaultem → silnik padł sygnałem 11, supervisor wznowił go po 5 s, `hot list` pokazał v1 („dobra”), `hot-core started` z `crash_starts: 1`. Druga awaria 50 s po starcie (`kill -9`) → po 10 s `hot list`: „Tryb bezpieczny… mody śpią: licznik”, `safe_mode: {crash_starts: 2, dormant: [licznik]}` w `status.json`, `HOT-E0408` w `hot-core.jsonl`. `horizon-server stop` i `start` → licznik v1 wrócił, `safe_mode` puste.
  - FAKT: pierwszy `kill -SEGV` wysłany z zewnątrz nie położył silnika (najpewniej łapie go handler przepełnienia stosu z biblioteki standardowej Rusta, który dla sygnału spoza strażnika stosu tylko przywraca domyślną obsługę). Do symulacji awarii służy `kill -9` albo prawdziwy błąd pamięci w modzie.
  - Nie sprawdzone: ThinkCentre (brak maszyny); `hot-proxy` trzymające graczy przy awarii to M8.4.
- [x] M7.2 Pełny dysk (sztucznie zapełniony) → sprzątanie i komunikat.
  - `hotd` mierzy wolne miejsce (`df -Pk`) przed każdym krokiem. Poniżej `min_free_bytes` (512 MB) usuwa swoje stare artefakty i kopie `hot-core` (zostaje najnowsza); gdy nadal brakuje, nie pobiera i nie stosuje commitów, `HOT-E0211` raz, powód w `hot status`; po zwolnieniu miejsca wraca sam. Test `low_disk_pauses_hotd_and_cleans_its_own_files` (mutacja bez `return` go czerwieni).
  - Prawdziwie pełny dysk (obraz `hdiutil` APFS 64 MB, zmienna `HOT_FULL_DISK_DIR`, testy `full_disk_leaves_state_and_pack_whole` w `hotd` i `full_disk_start_keeps_hot_mods_running` w `hot-core`): przy zerowym wolnym miejscu pada pobieranie (`HOT-E0201`) i zapis stanu (`HOT-E0207`), a przy 6 MB wolnego zapis paczki; stan, `slots.json` i `pack.zip` zostają całe, bez paniki, a po zwolnieniu miejsca commit wchodzi i idzie `hot slots`. `hot-core` startuje na pełnym dysku, mod działa, `mods.json` bez zmian.
  - Znalezione na pełnym dysku i poprawione: brak miejsca przy zapisie paczki oznaczał dobry commit jako nieudany (`HOT-E0210`) na zawsze; `slots.json` zapisany przed paczką gubił `hot slots` przy ponowieniu (mutacja kolejności czerwieni test); ucięte pliki `.tmp` zostawały na dysku. Teraz: `HOT-E0211` z `failed_code`, ponowienie, gdy silnik nic jeszcze nie dostał; git z `LC_ALL=C`.
  - FAKT (APFS): miejsce po usuniętym pliku wraca z opóźnieniem, a pełny dysk nie pozwala skrócić pliku, tylko go usunąć; testy zapełniają dysk, aż miejsca przestanie przybywać.
  - Instalator usuwa po sobie `/srv/horizon/tmp/hot-slots` i `hot-host-bundle`. Kopie świata ogranicza minutnik kopii (krok 9, `backups.keep=7`). Stare silniki w `/srv/horizon/engines` sprząta M9.
  - Nie sprawdzone: ThinkCentre (ext4, brak maszyny).
- [x] M7.3 Uszkodzony świat → komunikat i polecenie przywrócenia; przywrócenie po potwierdzeniu.
  - FAKT (`horizon-client/src/game/world_loading.rs`, próba na kopii świata): serwer dedykowany sam bierze `level.dat_old`, gdy `level.dat` się nie wczyta. Gdy oba są złe, kończy pracę kodem 1 i pisze w `crash-reports/crash-<data>-server.txt` linię `Failed to load world "<folder>": its level data does not read (…)`. Supervisor po 5 awariach w 10 minut przestaje wznawiać silnik (`status --json` → `stopped`). `restore <kopia>` bez `--yes` odmawia i niczego nie zmienia, z `--yes` przenosi stary świat do `backups/replaced-<data>/world`.
  - `hotd` (`world.rs`, `check_world`): gdy silnik nie działa, czyta najnowszy raport nowszy niż ostatni raz, gdy silnik działał; jeśli to błąd wczytania świata, `hot status` pokazuje wiersz „ŚWIAT” z powodem i poleceniem `horizon-server --dir … restore <najnowsza kopia> --yes`, a log dostaje `HOT-E0212` raz na raport. Sam niczego nie przywraca. Gdy silnik znów działa, komunikat znika.
  - Test `damaged_world_shows_the_restore_command_and_restores_nothing`: 3 kroki bez silnika → żadnej komendy, świat i kopie bez zmian, w statusie powód i `restore nowa.zip --yes`, jeden wpis `HOT-E0212`; po starcie silnika komunikat znika, a stary raport nie wraca. Mutacja (`since` → `None`) go czerwieni.
  - Na żywo (Mac, 2026-11-13, kopia świata testowego z uciętym `level.dat` i `level.dat_old`): silnik padał 5 razy i supervisor się poddał; `hotd once` z prawdziwym raportem pokazał w `hot status` „ŚWIAT: silnik nie wczytuje świata: its level data does not read (NBT at byte 3: truncated input)” i polecenie przywrócenia kopii `…-przed-psuciem.zip`. To polecenie przywróciło świat, a serwer wystartował.
  - Znane: jednostka `horizon-hot-server` ma `Restart=on-failure`, więc po poddaniu się supervisora systemd uruchamia go znowu co 5 s i dopisuje kolejne raporty. Komunikat się nie zmienia (ta sama przyczyna), a wpis w logu przybywa raz na raport. Gdy brakuje `level.dat`, Horizon tworzy nowy świat bez błędu; tego `hotd` nie wykrywa.
  - Nie sprawdzone: ThinkCentre (brak maszyny).
- Sprawdzenie: każdy wiersz tabeli D20 ma test, który wywołuje błąd i obserwuje reakcję.

### M8 — `hot-proxy` i podmiana silnika (D10–D12)
Projekt (2026-11-13, FAKT ze źródeł Horizona 66f3981, licencja MIT):
- Kontrolę zakupu robi `hot-proxy`, a silniki jej nie omijają. Proxy loguje gracza online jak serwer vanilla: prośba o szyfrowanie, AES/CFB8 i `hasJoined` w usłudze sesji. Adres usługi bierze z dokumentu discovery, tak jak Horizon (`horizon-client-render/src/auth.rs`).
- Silniki działają za proxy bez logowania online (`online_mode = false`), za to z przekazywaniem Velocity w wersji „modern” (`forwarding.secret`, `docs/velocity-forwarding.md` Horizona). Proxy podpisuje HMAC-SHA256 UUID, nazwę, adres i właściwości profilu (skórkę). Silnik odrzuca każde wejście bez podpisu tym sekretem („This server is behind a proxy”), więc bezpośrednie połączenie z silnikiem nie omija kontroli zakupu. Dodatkowo silniki słuchają tylko na 127.0.0.1 (`[server] bind`). Gracz ma w świecie swoje prawdziwe UUID, a nie offline, więc zmiana nazwy konta nie gubi ekwipunku.
- Tryb bez logowania online (boty w M8.2, testy) proxy włącza tylko na adresie pętli zwrotnej. Instalator go nie ustawia, a test to sprawdza.
- `[transfer]` Horizona (`player-export`, `player-expect`) nie jest potrzebny. Służy do przenoszenia gracza między serwerami z różnymi światami, a u nas świat jest jeden.
- Kod: `crates/hot-proxy`. Pakiety, szyfrowanie, kompresja, RSA i hash serwera pochodzą z `horizon-protocol` (git, rewizja serwera), HMAC z `sha2`, a zapytanie HTTPS z `ureq`, czyli tak samo jak w Horizonie (ADR 0010). Kod próby S8 przechodzi tu po kawałku: M8.1 przekazuje ruch, M8.2 dokłada zamianę id i podmianę.
- Test bez Mojanga: usługa sesji podstawiona lokalnym serwerem HTTP (zgoda i odmowa). CI nigdy nie łączy się z Mojangiem. Na tej maszynie jest prawdziwe konto (`kamciosz`, instancja `hot-spike`), więc logowanie online da się sprawdzić ręcznie.
- [x] M8.1 Proxy przezroczyste: logowanie online, szyfrowanie, kompresja, forwarding. Sprawdzenie: gra przez proxy; ping i MSPT bez istotnej różnicy.
  - Wynik (2026-11-13, Mac): `crates/hot-proxy`, 13 testów bez Mojanga (podpis zgodny bajt w bajt z nagraniem Velocity, odmowa przy złym sekrecie, braku potwierdzenia, braku usługi sesji i silniku bez `forwarding.secret`). Na prawdziwym koncie (`kamciosz`, instancja `hot-spike`) przez prawdziwą usługę sesji: gracz wchodzi przez proxy, silnik ma jego prawdziwe UUID (`list uuids`), czat dochodzi do silnika i wraca, status i ping listy serwerów przechodzą. Ta sama gra połączona z silnikiem z pominięciem proxy dostaje „This server is behind a proxy: join through the proxy”. MSPT z graczem 2,2 ms. Opóźnienia przez prawdziwą sieć nie mierzono (wszystko na 127.0.0.1).
  - Bezczynność: gracz stoi 2 min 40 s w menu pauzy i nadal jest w grze (limit 30 s dotyczy tylko logowania). Po wyjściu log proxy podaje przyczynę i czas gry („the player connection ended…”, `seconds`).
  - Znane: czat ma znacznik „Not Secure”, bo silnik bez logowania online nie sprawdza podpisów czatu. Gra działa normalnie. Do rozstrzygnięcia przy M8.2 (podpisy przekazuje klient w fazie gry, proxy ich nie zmienia).
- [x] M8.1b Instalator: silnik na 127.0.0.1 z `online_mode = false` i `forwarding.secret`, `hot-proxy` jako usługa na porcie graczy, test, że proxy nigdy nie dostaje `--offline`. Sprawdzenie: instalacja na czystej maszynie wirtualnej Ubuntu, wejście przez proxy, odmowa bezpośrednio na silnik.
  - Wynik (2026-11-13): krok 8/13 `proxy_setup` w `scripts/server-install.sh` (sekret 32 losowe bajty w base64, 0600, właściciel `horizon`; potem `server.bind=127.0.0.1`, `server.port=$ENGINE_PORT`, kontrola, na końcu `online_mode=false`), usługa `horizon-hot-proxy`, opcja `--engine-port` (domyślnie 25566), kod `HOT-E0114`, `hot-proxy ping` w autoteście, przywrócenie sprawdzania kont przy odinstalowaniu. Testy `crates/hot-proxy/tests/installer.rs` (mutacje: `online_mode=false` przed `bind` → pada; `--offline` w jednostce → pada) i `ping_reaches_the_engine_through_the_proxy`.
  - Maszyna wirtualna Ubuntu 22.04 aarch64, `--test-no-horizon`, horizon-hot z lokalnego repo: kod 0; `hot-proxy` słucha na 0.0.0.0:25565, w jednostce `--engine 127.0.0.1:25566`, bez `--offline`; sekret 41 B, `-rw------- horizon`; zapora: 22, 25565, 8131. Drugi przebieg: kod 0, ten sam PID proxy, ten sam sekret. `ping` bez silnika → kod 1. Odinstalowanie: 0 jednostek `horizon*`, brak reguły 25565, sekret usunięty, port wolny.
  - Na Macu (`horizon-server` z Nightly, kopia `server.toml`): polecenia kroku 8 dają `server.bind = "127.0.0.1"`, `server.port = 25566`, `server.online_mode = false`, a polecenia odinstalowania przywracają `""`, 25565 i `true`. `hot-proxy ping` przez proxy do prawdziwego silnika zwraca jego status.
  - Biała lista za proxy: silnik z `online_mode = false` zapisuje prawdziwe UUID (`whitelist add kamciosz` → `004f2aec-…`; gracz nigdy niewidziany `Notch` → `069a79f4-…`, czyli UUID z konta, nie offline). `init --whitelist --player --op` też zapisuje prawdziwe. Instalacja z `--op` działa więc za proxy bez obejść.
  - Do sprawdzenia później: proxy słucha tylko na IPv4 (`0.0.0.0`); silnik z `bind = ""` przyjmował też IPv6.
  - Nie sprawdzone: brak maszyny x86_64, więc pełna instalacja z Horizonem (krok 8 na prawdziwym `server.toml`, autotest z `ping`, wejście gracza) czeka na serwer kolegi. Wejście przez proxy i odmowę bezpośrednio na silnik sprawdzono w M8.1 na Macu.
- [ ] M8.1c Docker (prośba właściciela, 2026-11-13): serwer da się uruchomić jako kontener, obok instalatora na Ubuntu. Obraz bez plików Mojanga: pliki gry przygotowane z Minecrafta właściciela i świat są w wolumenie (`/srv/horizon/server`), obraz ma `hotd`, `hot-proxy`, `hot-core`, `horizon-server` i `horizon-engine` zbudowane ze źródeł Horizona w przypiętej rewizji (R29). W kontenerze nie ma systemd ani ufw: jeden proces nadrzędny uruchamia silnik (127.0.0.1), `hot-proxy` (port graczy), `hotd` i serwer paczki (8131) i wznawia je po awarii. Te same zasady co w instalatorze: sekret przed `online_mode = false`, proxy nigdy offline, kod natywny (`--trust-native-code`) tylko za zgodą właściciela. Obraz tylko linux/amd64 (Horizon nie ma serwera na ARM). Decyzja w ADR. Sprawdzenie: `docker compose up` na czystej maszynie, wejście przez proxy, port silnika niewidoczny z zewnątrz, restart kontenera nie gubi świata ani stanu modów.
  - Wynik (2026-11-13): `Dockerfile` (etapy `rust`, `horizon`, `build`, obraz 101 MB), `compose.yaml`, `scripts/container-entrypoint.sh`, ADR 0011, `docs/docker.md`, kod `HOT-E0115`; test `crates/hot-proxy/tests/installer.rs` obejmuje kontener (brak `--offline`, kolejność sekret → `127.0.0.1` → `online_mode=false`, tylko porty 25565 i 8131, `HOT_TRUST_NATIVE_CODE` zakomentowane, ta sama rewizja Horizona w instalatorze i obrazie). Mutacje: port 25566 w `compose.yaml`, odkomentowana zgoda, `--offline` w skrypcie, inna rewizja w `Dockerfile` → test pada.
  - Sprawdzone na Macu (colima `hot-docker`, vz + Rosetta, linux/amd64): w obrazie nie ma plików gry (`*.jar`, `horizon-resources.json`, zasoby); bez `HOT_REPO` → `HOT-E0115`, kod 3; bez plików gry → `HOT-E0107`, kod 3; z atrapą folderu gry (sam `{}` w `horizon-resources.json`, nic od Mojanga): klucz CI, świat, pula slotów, sekret 42 B `-rw------- horizon`, `server.bind = "127.0.0.1"`, `server.port = 25566`, `server.online_mode = false`, adres paczki w `[properties]`; z hosta otwarte 25565 i 8131, 25566 zamknięty; `pack.zip` i `hot_slots.hmod` HTTP 200; silnik odrzuca atrapę („prepared game files … incomplete”) i wraca co 5 s; `hot-proxy ping` bez silnika → kod 1; sekret ten sam po odtworzeniu kontenera; `docker compose stop` → 3 s, kod 0, bez SIGKILL.
  - Znalezione i poprawione w trakcie: (1) wydanie `nightly-2026-11-05-66f3981` zniknęło z GitHuba (R29), więc instalator na nowej maszynie padał w kroku 4 (M8.1b tego nie złapał, bo szedł z `--test-no-horizon`); teraz instalator i obraz budują Horizona ze źródeł w pełnej rewizji. (2) Skrypt startowy zapisywał PID podpowłoki basha zamiast programu, więc SIGTERM nie doszedłby do silnika i świat nie zapisałby się przy `docker stop`; teraz `exec`, PID-y procesów potwierdzone w `ps`. (3) `hot-diag` czyta przy budowie `docs/bledy/` i `AGENTS.md`: dodane do `.dockerignore`.
  - Nie sprawdzone (pliki gry montuje tylko właściciel): silnik w kontenerze na prawdziwych plikach, wejście gracza przez proxy, zapis świata przy `docker compose stop`, zachowanie świata po restarcie, silnik z `hot-host` (`HOT_TRUST_NATIVE_CODE`, tag launchera z R29). Instalator po zmianie kroku 4 nie był uruchomiony na x86_64 (czas kompilacji Horizona: w emulacji Rosetta na 4 CPU klient ok. 39 min, całość ponad 2 h; na ThinkCentre do zmierzenia). Kryterium „wejście przez proxy” czeka na właściciela, dlatego punkt zostaje otwarty.
- [x] M8.2 Mapowanie id encji i trzymanie keep-alive. Sprawdzenie: 50 sztucznych przełączeń na ten sam silnik bez rozłączenia (10 botów + prawdziwy klient).
  - Wynik (2026-11-13): `crates/hot-proxy/src/remap.rs` (zamiana id w obu kierunkach, testy na prawdziwym kodowaniu pakietów Horizona), `link.rs` (gracz z wymiennym silnikiem: zapis i odtworzenie konfiguracji, połknięcie `Login`, `RemoveEntities`, przesunięcie `global_index` czatu, keep-alive od proxy w przerwie, przepuszczanie tylko odpowiedzi na keep-alive aktywnego silnika, zamknięcie paczki pakietów, ukrycie „joined/left” i `game_event` 13 przez 5 s), `control.rs` (gniazdo `<HOT_HOME>/hot-proxy.sock`, 0600, `hot-proxy switch [ADRES]` i `status`), kod `HOT-E0605`. Test `tests/switch.rs` z atrapą silnika; 9 mutacji (bez zapisu id z `Login`, bez `RemoveEntities`, bez przesunięcia czatu, bez czyszczenia starych keep-alive, bez zamiany id, bez keep-alive w przerwie, bez odtworzenia marki, bez filtra po podmianie) → test pada.
  - Na Macu: silnik `horizon-server` (Nightly 66f3981) za `hot-proxy` offline na 127.0.0.1, 10 botów `horizon-bots` (mixed) i prawdziwy klient `horizon-client` z `--control`. 50 × `hot-proxy switch` na ten sam silnik, przed każdą wiadomość czatu i `setblock`: 50/50 kod 0, za każdym razem 11 graczy przeniesionych, 0 nieudanych; pauza po stronie proxy min 28, mediana 67, p95 107, max 118 ms (bez restartu silnika, więc to sam koszt ponownego logowania). Po każdej podmianie klient widzi postawiony blok i swoją wiadomość, ekran gry (`playing`), zdrowie 20; boty online 10, `failed` 0. Zrzuty po rundach 10–50: obraz gry, czat ciągły, bez ekranu ładowania. W rundach 25–27 ekran pauzy: Escape naciśnięty przez osobę przy komputerze (`input-log`: `origin: user`), nie skutek podmiany.
  - Znalezione w trakcie: gniazdo Unix ma limit długości ścieżki (ok. 104 bajtów); przy długim `HOT_HOME` proxy nie startuje (`HOT-E0604` z tą przyczyną w opisie). `/srv/horizon` mieści się z zapasem.
  - Nie sprawdzone: ThinkCentre (brak maszyny); podpisany czat (silniki za proxy są offline, czat „Not Secure”, opis w `docs/podmiana.md`).
- [ ] M8.3 Podmiana na nowy silnik według D11, z powrotem przy błędzie. Sprawdzenie: dodanie `.hnmod` w trakcie gry → mod działa, nikt nie rozłączony, brak ekranu ładowania (zrzuty co 100 ms), pauza krótsza niż 1 s (D11); celowo zepsuty silnik → gracze zostają na starym.
- [x] M8.4 Wznowienie po awarii silnika z przytrzymaniem graczy (D13). Sprawdzenie: `kill -9` silnika → gracze połączeni, wracają do gry po starcie.
  - Wynik (2026-11-13): zerwanie połączenia przez silnik bez `Disconnect` → proxy trzyma gracza do 180 s, wysyła mu własne keep-alive (pierwszy po 5 s, potem co 10 s) i co 20 ms próbuje logowania pod aktywnym adresem; po czasie komunikat dla gracza i `HOT-E0605`. Test z atrapą, która wstaje po 6 s, i test podmiany na martwy silnik.
  - Na Macu (to samo stanowisko co M8.2, 11 graczy): `kill -9` procesu silnika → supervisor `horizon-server` wznowił go, wszyscy wrócili do gry po 5,6 s, nikt nie został rozłączony, boty online 10, klient w `playing`, czat działa. Zmiany świata od ostatniego zapisu przepadły (blok z ostatniej rundy): silnik zabity bez zapisu, tego proxy nie naprawi.
  - Nie sprawdzone: ThinkCentre (brak maszyny); przerwa dłuższa niż 30 s z prawdziwym klientem (atrapa sprawdza keep-alive, prawdziwy silnik wstał po 5,6 s).
- [ ] M8.5 Odzyskanie pamięci generacji przez podmianę (D4). Sprawdzenie: po 200 przeładowaniach RSS spada do poziomu startowego bez restartu.

### M9 — Aktualizacja Horizona (D18)
- [ ] M9.1 Wykrycie Nightly, przygotowanie wersji, build w tle, test na kopii świata, podmiana, obserwacja, powrót.
- Sprawdzenie: przejście ze starszego Nightly na nowsze na działającym serwerze z graczami; Nightly z celowo usuniętym `libloading` (lokalna kopia) → serwer zostaje, `hot status` mówi dlaczego.

### M10 — Aktualizacja naszego systemu (D19)
- [ ] M10.1 Wydania `horizon-hot`, sprawdzanie raz na dobę, aktualizacja `hot-core`, `hotd`, `hot-pack`; `hot-proxy` przy braku graczy.
- Sprawdzenie: nowa wersja każdego składnika wchodzi sama; gracze w grze nie zauważają (poza proxy, które czeka).

### M11 — Aplikacja gracza `hot-play` (D26)
- [ ] M11.1 Menu terminalowe, „Graj” z pełnym sprawdzeniem i naprawą, logowanie raz, `play --server`.
- [ ] M11.2 „Napraw” i plik z logiem.
- [ ] M11.3 `install.sh`, `install.ps1`, build w CI, samoaktualizacja, skrót „Graj”.
- Sprawdzenie: czyste maszyny macOS, Windows 11, Ubuntu Desktop → jedna linijka → skrót → wejście na serwer bez ostrzeżeń systemu; usunięty `hot-slots.hmod` lub uszkodzony plik launchera → naprawione przy starcie; przerwana instalacja + ponowna → działa.

### M12 — Dokumentacja i jakość
- [ ] M12.1 Przeniesienie D1–D29 do `docs/adr/`, `docs/operator.md`, przewodniki z D29.
- [ ] M12.2 Test „nowej osoby w zespole” (kryterium 13): repo bez `PLAN.md`; świeży agent (a) odpowiada na pytania: co to za projekt, jak dodać mod, jak wydać wersję, czego nie wolno; (b) dodaje prosty mod gorący; (c) naprawia po jednym błędzie z każdej kategorii D27, mając tylko `hot report`. Transkrypt w `docs/testy/nowa-osoba.md`; każde potknięcie = poprawka dokumentacji i powtórka.
- [ ] M12.3 README, ARCHITECTURE, ADR 0002–0030 (D1–D29), przewodnik „pierwszy mod gorący”, instrukcja operatora (`hot status`, przywracanie z kopii).
- Sprawdzenie: `quality-audit` ≥ 120/125; każdy ADR ma kontekst, decyzję, konsekwencje (skrypt w CI sprawdza nagłówki).

### M13 — Aktualizacja proxy bez rozłączeń (D19)
- [ ] M13.1 Przekazanie gniazd i stanu szyfrowania (`SCM_RIGHTS`).
- Sprawdzenie: nowa wersja proxy w trakcie gry, zero rozłączeń.

---

## 6. Przewidywane problemy

| Problem | Co wtedy |
|---|---|
| S8: pauza podmiany > 1 s | użytkownik nie akceptuje (2026-11-12); rozgrzany silnik (D11) daje na Macu 470–541 ms (S13); gdy ThinkCentre (M0.11) nie zmieści się w 1 s: mixin bez stałych 100 ms w `stop_dedicated`, mniejsza pula, potem decyzja użytkownika z liczbami |
| S8: klient rozłącza się przy długim braku pakietów | proxy wysyła klientowi keep-alive i własne pakiety podtrzymujące w czasie podmiany |
| S2: klient bez `hot-slots.hmod` jest rozłączany | to oczekiwane; `hot-play` instaluje plik automatycznie |
| Nightly często psuje build `hot-host` | serwer zostaje na starej wersji (D18); `hot-host` tylko na nazwanych hakach |
| `libloading` znika z `Cargo.lock` Horizona | serwer zostaje na starej wersji; zgłoszenie do Horizona |
| Wspólny `native/stage` (R27) | blokada pliku; na serwerze nie ma innych buildów |
| Mod trzyma wątek po `deactivate` | `hot-sdk` daje `HotThread`, który kończy się przy `deactivate` |
| Wyciek pamięci generacji | podmiana silnika po przekroczeniu progu (D4, M8.5) |
| Nowy mod zmienia format danych bloku/encji | podmiana używa konwersji, którą silnik ma dla starych światów; bez niej — odmowa i komunikat |
| ThinkCentre ma mało RAM-u na dwa silniki | podmiana czeka (D16); `hot status` pokazuje, ile brakuje |
| `/reload` daje skok ticka na słabym CPU | Horizon nie pozwala przeładować jednej paczki (pełne `reload`); `hotd` wysyła jedno `reload` na commit i żadnego bez zmian w `data/`. Mac: 0,2–0,75 s, przy dużym obciążeniu Maca pierwszy po bezczynności 3,9–8,2 s (`docs/sprzet.md`); ThinkCentre do zmierzenia |
| Ostrzeżenie systemu mimo instalacji skryptem | decyzja o płatnym podpisie (D26) |

## 7. Poza zakresem
- Podmiana silnika niewidoczna dla klienta w ≤ 50 ms (R19: miesiące pracy, kruche przy każdym Nightly).
- Fork klienta Horizon dobudowujący rejestry w trakcie gry (bardzo duży koszt); nowa treść idzie przez sloty.
- Konwersja cudzych `.hmod` na sloty (pasuje tylko do prostych bloków; wraca, jeśli będzie potrzebna).
- Wielowątkowy tick — robi to Horizon (projekt W3 w `docs/server-next.md`); ewentualnie pomagamy PR-ami.
- Piaskownica dla kodu modów (zabezpieczeniem jest podpis artefaktów CI i dostęp do repo).
- Gra bez kupionego Minecrafta, konta offline omijające sprawdzenie zakupu.
- Kilka serwerów za jednym proxy.
- Zewnętrzne powiadomienia (Discord, mail).

## 8. Stan prac

**DECYZJA (2026-11-12): bez danych o ThinkCentre.** Użytkownik nie poda ani sprzętu, ani dostępu. Dlatego:
- M0.10 i M0.11 przechodzą do instalatora. `server-install.sh` czyta `uname -m`, `nproc` i `/proc/meminfo` i z nich liczy `AllowedCPUs`, `MemoryLow` i `MemoryMax` według wzoru w D16. Architektura nieznana: CI buduje mody gorące na x86_64 i aarch64, a `hotd` wybiera plik według `uname -m`.
- S9 (`native-build` na Linuksie) nie da się sprawdzić z tej maszyny, bo wymaga `prepare` z kontem gracza. Krok budowy silnika w instalatorze kończy się przy błędzie kodem `HOT-E…`, wpisem w katalogu i wskazówką `hot report`.
- Rozgrzany silnik (S13) działa tylko wtedy, gdy pamięć pozwala; inaczej podmiana idzie na zimno z wpisem w logu (D16).
- Oczekujące wybory przyjmuję jako decyzje: pula 1024/2048, paczka wysyłana od razu (D9, wariant a).
- Raz przy instalacji osoba z kupionym Minecraftem loguje się kontem Microsoft (instalator pokazuje kod). Tego nie da się obejść.
- Kolejność pracy: M1 (przeładowanie bez pauzy), M2 (git → CI → załadowanie), cienki M6.1 (instalator), potem M3–M5, M7, M0.19 i M8, na końcu M9–M13. Sprawdzenia, których nie da się zrobić bez ThinkCentre ani czystych maszyn graczy, oznaczam „nie sprawdzone: brak maszyny” i nie odhaczam.
- Narzędzia: `horizon-bots` (crate w repo Horizona) do prób z botami, `limactl` lub `docker` do czystego Ubuntu bez logowania.

**2026-11-12 — M1.1, M1.2, M1.4 zrobione (bez silnika):** `hot-abi` (ABI v1), `hot-sdk` (`HotMod`, `hot_mod!`, panika z miejscem w logu) i `hot-core` (`hot load/unload/list`, przełączenie na granicy ticku, powrót po panice, próba 6000 ticków, start po awarii z ostatniej dobrej generacji). 11 testów w `crates/hot-core/tests/reload.rs` na prawdziwych `cdylib`; mutacje „bez przekazania stanu” i „bez powrotu do poprzedniej generacji” je czerwienią. Odstępstwa od D4 i M1.1–M1.2 (jedno `on_event`, `init` w wątku serwera, `Host` zamiast `HostCtx`) w ADR 0006. Dalej: M1.3 `hot-host` i M1.5 `examples/licznik`, potem sprawdzenia M1 w silniku.

**2026-11-12 — M1 zrobione, sprawdzone w silniku na Macu:** `hot-host` (ładuje `hot-core`, przekazuje haki, `hot core load`) i `examples/licznik`. Wyniki: przeładowanie zmienia odpowiedź, licznik ciągły (164 → 186), PID silnika ten sam; 100 przeładowań przy 10 botach — p95 MSPT 13,4 → 13,4 ms (ruchome boty) i 13,8 → 15,1 ms (stojące boty), czyli < 2 ms; pamięć +30 MB na 100 generacji (~300 KB na generację, plik 412 KB); panika w `server_tick` i w komendzie → wraca poprzednia generacja, serwer działa, 0 awarii; podmiana samego `hot-core` → ten sam PID, mody działają, licznik zachowany. Nowe: logi `hot-host` w `latest.log`, `hot <mod> …`, przełączenie także przy komendzie konsoli, bo pusty serwer w pauzie nie tyka (ADR 0007). Na ThinkCentre nie sprawdzone: brak maszyny. Dalej: M2.

**OTWARTE (D28, maskowanie w raporcie):** `hot report` maskuje znane sekrety i adresy IPv4. Nie maskuje jeszcze adresów IPv6 graczy (dojdzie w M8, razem z logami `hot-proxy`) ani ścieżek z katalogiem domowym i nazwą użytkownika w raporcie gracza z „Napraw” (M11; Horizon w `share-log` je usuwa).

**2026-11-12 — M0b (M0.15–M0.18) zrobione:** `AGENTS.md` uzupełniony o zasady pracy z grą. `crates/hot-diag`: `DiagError::new(code, what, fix)` (brak pola = błąd kompilacji, testy `compile_fail` z kodem E0061), zapis JSONL z rotacją, maskowanie znanych sekretów w każdym polu, panika jako `HOT-E0002` z backtrace, katalog błędów wklejany do programu przy budowie. Test katalogu w CI w obie strony. `hot-diag report`: pięć części D28, < 200 KB, sekrety i adresy IPv4 graczy zamaskowane. Testy maskowania sprawdzone mutacją (wyłączone maskowanie → testy czerwone). Etap M0 czeka tylko na ThinkCentre (M0.10, M0.11). Następny krok: M1.

**2026-11-12 — M0.14 zrobione:** wyniki S1–S12 w `docs/adr/0002-spike.md` (tabela prób, otwarte punkty z krokiem, który je rozstrzygnie). Poprawki: D6 (`event.replies` w haku komendy, poza nim `tell` albo `tellraw`), D8 (nowa wersja `hot-slots` przy każdej zmianie puli), D9 (`servers.dat` instancji z `acceptTextures`), D16 (silniki nie działają naraz, pamięć na drugi tylko przy teście D18), D18 (każde Nightly traktowane jako zmiana dla klienta do testu w M9), D24 (zmierzone czasy zamiast „mrugnięcia ≤ 1 s”). Etap M0 kończą M0.10 i M0.11 na ThinkCentre. Następny krok: M0b (M0.15).

**2026-11-12 — M0.12 (S11) zrobione:** 6 Nightly (wszystkie istniejące) i 4 ostatnie wydania stabilne. Haki i funkcje `Host`, których używa `hot-host`, oraz pliki `content_identity`, `mod_list`, `mod_net` i numery pakietów są takie same od 11-03 do najnowszego Nightly; `libloading` 0.8.9 wszędzie; kanały `horizon:*` tylko się dopisuje. Poprawka D18: `hot-host` przypina wersję Horizona, więc build aktualizacji musi mieć `--ignore-pins`. Od 11-07 cache treści modów ma osobny katalog na zestaw modów (wniosek S7 bez zmian). Szczegóły: `docs/aktualizacje.md`.

**RYZYKO / SZANSA (HIPOTEZA, do obserwacji):** Horizon od 11-05 sam wprowadza mody bez restartu (`docs/design/mod-hotload.md` w repo Horizona): przełączanie modów treści w grze jednoosobowej i LAN, wyłączanie trait modów (`set_mod_on`), w planach mody wasm z wyładowaniem. Serwer dedykowany jeszcze tego nie ma. Jeśli dostanie, część D11 może być zbędna, a mody wasm mogłyby zastąpić nasze `cdylib` (D2). Sprawdzać przy każdym przeglądzie aktualizacji.

**2026-11-12 — M0.9 (S8) zrobione:** prototyp proxy (`spikes/s8-podmiana/proxy/`) przeniósł prawdziwego klienta 10 razy między dwoma silnikami na jednym świecie: ekran `playing` przez cały czas, połączenie zostało, chunki zostały, moby mrugnęły (ok. 0,8 s). Zamiana id gracza konieczna (próba kontrolna bez niej oblała). Pauza 0,96–1,07 s: prawie cała to stop + start silnika. Chunki od B zastępują kopię klienta (brak bloków-duchów); ruch i akcje gracza z przerwy przepadają. Niesprawdzone, do M8: mody i paczka zasobów w odtwarzanej konfiguracji, tryb online, wielu graczy. Szczegóły: `docs/spike/s8-podmiana-prototyp.md`, `docs/podmiana.md`.

**2026-11-12 — M0.8 (S7) zrobione:** pełna pula 4096/8192 działa (ostatni slot i przedmiot w grze), klient startuje o ok. 1 s wolniej, serwer o ok. 0,35 s, pamięć ok. +230 MB serwer, ok. +250 MB klient, plik 7 MB. Podmiana silnika z pulą trwa dłużej: tabela w `docs/sloty.md`. Cache treści modów zależy od pliku programu, więc podmiana z nowym silnikiem startuje go na zimno. Proxy odtwarza listę modów wobec B bez błędu (30 podmian). Szczegóły: `docs/spike/s7-rozmiar-puli.md`.

**2026-11-12 — M0.7 (S6) zrobione:** proxy wysłało klientowi w grze dwie paczki: wygląd zmienił się bez rozłączenia, przeładowanie trwało 1,39 s i przez ten czas cały ekran zasłaniała nakładka ładowania (zrzuty klatki). Klient pyta o zgodę przy każdej paczce, jeśli serwera nie ma na liście z `accept_textures`. `horizon-ctl status` nie widzi nakładki; podmiana silnika z S8 sprawdzona zrzutami: bez nakładki. Szczegóły: `docs/spike/s6-paczka-w-trakcie-gry.md`.

**DECYZJA UŻYTKOWNIKA (przed M5): kiedy nowy wygląd trafia do graczy.** (a) od razu: `hot-proxy` wysyła paczkę w grze, wszyscy gracze widzą ok. 1,3 s nakładki ładowania; (b) przy następnym wejściu: bez przerwy, ale do tego czasu nowe bloki mają wygląd zastępczy. Mój wybór: (a), bo nowy wygląd pojawia się tylko z nowym modem, czyli rzadko, a twórca moda od razu widzi efekt. Zastrzeżenie: 1,3 s zmierzono na Macu z paczką 2 KB; czas dla pełnej paczki i na ThinkCentre nieznany (M0.11).

**DECYZJA UŻYTKOWNIKA (2026-11-12): świat przy podmianie silnika stoi krócej niż 1 s w każdym przypadku** (ten sam silnik i nowy plik silnika). Dziś na Macu, bez rozgrzania: bez puli 0,96–1,07 s; pula 1024/2048 → 1,16–1,34 s, ok. 2,2 s z nowym silnikiem; pula 4096/8192 → 1,45–1,82 s, ok. 3,9 s z nowym silnikiem. Obecna kolejność tego nie spełnia. Droga: rozgrzany silnik (D11), próba S13 w M0.19, ADR 0004. Rozmiar puli: największa, przy której S13 mieści się poniżej 1 s; do tego czasu 1024/2048. Pamięć na czekający silnik czyni pytanie o RAM ThinkCentre (M0.11) pilnym. **Wynik S13 (2026-11-13): spełnione na Macu**, 470–541 ms na tym samym silniku i 509–531 ms na nowym pliku silnika przy puli 1024/2048. **M0.20:** pula 4096/8192 też mieści się w budżecie (480–573 ms), więc zostaje.

**2026-11-12 — M0.9 (S8) część 1, analiza kodu klienta:** `StartConfiguration`, drugi `Login` i `Respawn` zawsze pokazują ekran ładowania, więc proxy po pierwszym wejściu nie wysyła żadnego z nich. Jedyna droga: proxy samo loguje się i konfiguruje z B, połyka `Login` B, usuwa encje A i zamienia id gracza pA ↔ pB we wszystkich pakietach. Klient rozłącza się po 30 s ciszy. Bazą proxy będzie `horizon-protocol` (MIT, bez plików gry). Szczegóły: `docs/spike/s8-podmiana-analiza-klienta.md`. Dalej: prototyp proxy (część 2).

**2026-11-12 — M0.13 (S12) zrobione:** świat przez dowiązanie `world -> ../shared/world`; drugi silnik nie otworzy zajętego świata (`session.lock`), ale jego supervisor wpada w pętlę restartów 5→40 s, więc `hotd` musi najpierw zatrzymać stary; przekazanie świata ok. 1 s na Macu. Odkryta sekcja `[transfer]` w `server.toml` (proxy + zamrożenie gracza) — sprawdzić w M0.9. Szczegóły: `docs/spike/s12-dwa-serwery-jeden-swiat.md`.

**2026-11-12 — M0.3–M0.6 (S2–S5) zrobione:** pula slotów w `.hmod` działa, klient bez niej dostaje „Missing: hot_slots 0.1.0”; `/sklep` przez `Flow::Cancel` bez „Unknown command”, odpowiedź tylko przez `event.replies` (`Host::tell` w tym haku = `false`); `run_command` z `server_tick` działa; wygląd, model i nazwa slotu z paczki serwera działają tylko wtedy, gdy `.hmod` nie ma `models/`, `textures/`, `lang/` (D8 poprawione). Szczegóły: `docs/spike/s2-…` – `s5-…`. Dalej: M0.13, M0.9, M0.8, M0.7, M0.12, M0.14, M0b. M0.10 i M0.11 czekają na ThinkCentre.
- Zasoby prób (moje). Stan 2026-11-12 po M0b: usunięte silniki `s1-server` i `s2-server` (2,2 GB) i zatrzymany `http.server` 8137. Zostają do M8, bo przydadzą się przy `hot-proxy`: `/private/tmp/claude-501/hot-spike/` (ok. 1 GB: serwery S2, S7, S12, zrzuty S6), instancja Horizona `hot-spike` (`~/Library/Application Support/horizon/instances/hot-spike`, 111 MB), projekty `~/Developer/horizon-mod-lab/projects/spike-hot-*` i `probe-hot-host`. Klon `hz.git` (91 MB) leży w katalogu tymczasowym sesji. W katalogu launchera `native/stage/native-api/` zostały `spike_hot_s1`, `spike_hot_s2` i `probe_hot_host`, a `spike_hot_s2` jest w `members` w `native/stage/Cargo.toml`. Tego katalogu używa też „Drony Test”. Launcher sam przepisuje `members` przy każdym buildzie (obserwacja: `probe_hot_host` z listy już zniknął), więc plików launchera nie edytuję. Katalogi usunąć po M8, przed tym sprawdzić, że żaden build ich nie używa.

**2026-11-12 — M0.2 (S1) zrobione:** `hot-abi` jako `[[crate]]` w paczce trait moda buduje się z silnikiem serwera; log `hot_abi::ABI_MAJOR = 1`. Szczegóły: `docs/spike/s1-hot-abi-w-silniku.md`. Dalej: M0.3–M0.6.

**2026-11-12 — plan v2 + diagnostyka i przekazanie zespołowi (D27–D29). Implementacja nie zaczęta: czeka na „go”.**
- Repo: `~/Developer/horizon-hot`, commit `f788091` (M0.1).
- Próba z researchu: `~/Developer/horizon-mod-lab/projects/probe-hot-host` — baza dla M0.2 i M1.3.
- W `native/stage/Cargo.toml` (members) zostało `native-api/probe_hot_host`; launcher przepisuje listę przy buildzie — sprawdzić przy najbliższym buildzie „Drony Test”.
- Następny krok: M0.2, po „go” użytkownika.

**2026-11-12 — M2 zrobione poza przebiegiem w Actions:** `crates/hotd` (lustro gita, klasyfikacja D24, podpis `ssh-keygen`, `hot load`/`hot unload` z potwierdzeniem z `status.json`, zły commit wg D13, `hot-state/hotd.json`), skrypty CI i szablon `hot.yml`; wyniki CI na gałęzi `hot-builds` zamiast artefaktów Actions (ADR 0008), kody `HOT-E0201`–`HOT-E0207`. Testy bez sieci (`crates/hotd/tests/flow.rs`, 7): dobry commit czeka na CI i się ładuje; repo bez `mods/` → czysty serwer bez CI; podmieniony plik → odrzucony i usunięty (mutacja: test pada bez sprawdzania podpisu); commit, który się nie kompiluje → serwer bez zmian, `hot status` mówi dlaczego, poprawka wchodzi; inny PID silnika w 60 s → zły commit. Na prawdziwym silniku (Mac, `m1/srv`) z `hot-build.sh` i `hot-publish.sh` na lokalnym repo: commit → załadowany w 0,63 s, licznik ciągły (11326 → 11662) i nowa odpowiedź po drugim commicie, repo bez `mods/` → 0 modów, commit z błędem kompilacji → serwer bez zmian i powód w `hot status` oraz w `/hot status`, PID silnika cały czas ten sam. Nie sprawdzone: push → serwer ≤ 3 min (brak repo serwera na GitHubie i brak ThinkCentre). Dalej: cienki M6.1.

**2026-11-12 — M6.1 zrobione bez maszyny x86_64:** `scripts/server-install.sh` (D25 kroki 1–8 i 10, krok 9 za zgodą właściciela), kody `HOT-E0101`–`HOT-E0111`, test katalogu błędów sprawdza też `scripts/`; `hotd.json` ma pole `fetch_error` (stan pobierania repo teraz, nie historia), `hotd status` pokazuje „repo teraz niedostępne”. Dwa kroki kończą skrypt kodem 3 i czekają na właściciela: pliki gry z jego Minecrafta (`--resources DIR`) i `--trust-native-code` dla silnika z `hot-host` (Horizon zabrania agentom dawać tę zgodę). `hot.yml` dla repo serwera instalator zapisuje w `/srv/horizon/server-repo-files/` z `ref` przypiętym do zbudowanego commita horizon-hot; skopiować go i ustawić sekret `HOT_SIGNING_KEY` musi właściciel (instalator nie ma dostępu do repo). Maszyna wirtualna Ubuntu 22.04 aarch64 (lima, 2 CPU, 4 GB), `--test-no-horizon`, horizon-hot z GitHuba (`spike/m0`): bez `sudo` → kod 2; aarch64 bez flagi testowej → `HOT-E0101` w `install.jsonl` ze wszystkimi polami D27; pierwszy przebieg 1 min 24 s (pakiety, rustup, budowa `hotd`/`hot-core`/`hot-diag`, klucz CI i `allowed_signers`, jednostki systemd, `ufw` z SSH, minutnik kopii; kopia ręczna dała `hot-state-2026-11-12.tar.gz`); drugi przebieg: klucze i pliki jednostek bez zmian, nic nie budowane od nowa; prywatne repo `git@…` → klucz tylko do odczytu i `GIT_SSH_COMMAND` w jednostce, niedostępne repo → `HOT-E0111` z poleceniem dodania klucza; dobre repo → przechodzi. Znalezione i poprawione w trakcie: autotest przepuszczał niedostępne repo, bo w `hotd.json` został `head` z poprzedniego repo (stąd `fetch_error`); po przebudowie `hotd` usługa działała na starym pliku (teraz restart). Nie sprawdzone: brak maszyny x86_64 — pobranie Horizona, `import-resources`, `init`, usługa `horizon-hot-server`, `native-build` na Linuksie (to też M0.10), autotest z `hot-core`. Jedno polecenie z adresu `main` zadziała dopiero po scaleniu `spike/m0` do `main`; do tego czasu `--hot-ref spike/m0` i skrypt z gałęzi `spike/m0`. Po recenzji: `hotd` nie wysyła komend do silnika, który nie jest w fazie `running` (start, wstawanie po awarii) — commit czeka zamiast przepaść jako nieudany (test `commit_waits_while_the_engine_is_not_running`, mutacja: pada bez sprawdzenia); instalator włącza `hotd` dopiero po fazie `running`; polecenie do powtórki ma wszystkie podane opcje i adres skryptu z `--hot-ref`; zapora bierze port SSH także z `sshd -T` (Ubuntu 24.04 z `ssh.socket`). Na lokalnym `horizon-server` (Mac, kopia folderu) sprawdzone formaty, od których zależy instalator: `status --json` daje `"phase":"running"`, `config set engine.path=…` zapisuje linię `path = "…"` w `[engine]`, `config set backups.every=1d backups.keep=7` jest przyjęte, `init --eula --world --port --yes --whitelist --player --op` tworzy `server.toml`, `ops.json` i `whitelist.json`. Dalej: M3.

**2026-11-12 — M3.1 zrobione:** `hotd` kopiuje `mods/<id>/data/` z commita do `<świat>/datapacks/hot-<id>/data/` z wygenerowanym `pack.mcmeta` i plikiem `hot-tree` (hash drzewa gita), potem wysyła jedno `reload` i sprawdza `datapack list`. Stanem jest sam świat: paczki porównywane po hashu, więc ręcznie skasowany katalog wraca, a katalogi `hot-*` bez `hot-tree` nie są ruszane. Świat bierze z `[world] name` w `server.toml` katalogu `--engine-dir`. Mod z samym `data/` (bez `Cargo.toml`) wchodzi bez CI. Przed wysłaniem czegokolwiek do silnika `hotd` sprawdza, czy każdy `.json` się parsuje i czy w `data/` nie ma dowiązań — Horizon pomija zły plik po cichu, więc commit z błędem dostaje `HOT-E0208` i niczego nie zmienia. Gdy silnik nie pokaże paczki po `reload` (np. wyłączona w `datapacks.json`), stare paczki wracają i idzie drugie `reload`. Próby w źródłach i na serwerze testowym: Horizon włącza nową paczkę z `datapacks/` sam przy `reload`, usunięty katalog znika, ponowne dodanie znów jest włączone; zły JSON w łupach lub przepisach jest pomijany bez błędu komendy. Testy przepływu (6 nowych, 4 mutacje padają: bez sprawdzania JSON, bez przywracania, bez warunku pustej różnicy danych, bez wymogu `hot-tree`). Prawdziwy `hotd once` na serwerze testowym: commit z `data/` → `data_written: [lupy]`, łup z paczki; commit bez moda → `data_removed: [lupy]`, łup vanilla; PID ten sam. Pomiar `reload`: `docs/sprzet.md`. Dalej: M4.

**2026-11-12 — M3.1 po recenzji:** (1) `datapack list` pokazuje tylko wykryte paczki, nie wczytane tabele, więc `hotd` czyta też to, co silnik dopisał do `logs/supervisor.log` w czasie `reload`: „… are not applied: …” = porażka (stare paczki wracają, drugie `reload`, `HOT-E0208` z tą linią w `chain`), „… not loaded: …” i „failed to load function …” = ostrzeżenie w `hotd.jsonl` i linia „plik pominięty przez silnik” w `hotd status` (pole `data_warnings` w `hotd.json`). (2) Po złym commicie (D13) paczki danych wracają do ostatniego dobrego commita, a działający silnik dostaje `reload` — inaczej silnik wstawałby w kółko na złej paczce. (3) CI (`hot-build.sh`) bierze tylko `mods/*/Cargo.toml`, więc mod z samym `data/` go nie psuje. (4) Próba z graczem online: przepis z commita działa, usunięcie go zabiera, gracz połączony, PID ten sam. (5) `templates/server-repo/README.md`: układ repo serwera dla autorów modów. Testy: 3 nowe w przepływie, każdy pada po mutacji (bez porażek z logu, bez przywracania po złym commicie, bez ostrzeżeń w stanie). Serwer testowy `m1/srv` zatrzymany. Dalej: M4.

**2026-11-12 — M4 zrobione (ABI 2):** `HostApi` ma `set_block`, `give`, `particles`, `run_later`; `ModVTable` ma `commands`; `kind::PLAYER_TICK` idzie tylko do `hot-core`. Próba w silniku z `hot-host` (serwer testowy na Macu, klient `hot-spike`, nie „Drony Test”), wszystkie wyniki obserwowane w czacie, ekwipunku i stanie gracza: `/sklep` pokazuje konto i ofertę; `/sklep kup chleb` daje 4 chleby i pobiera 1 monetę; `hot sklep daj` z konsoli odpowiada „wiadomość: wysłana” i gracz ją dostaje; wypłata z `server_tick` przy 10 monetach dochodzi do gracza; `/teleport-blok ustaw` widzi gracza (przed D6a: „Nie widzę cię na serwerze”); `/teleport-blok postaw` stawia magnetyt, a na zajętym miejscu odmawia; prawy klik na magnetycie daje 40 cząsteczek `portal` w kliencie i przenosi gracza do celu w następnym ticku; komenda wysłana przez martwego gracza wykonała się po odrodzeniu (D6a). Zmiana wersji moda `teleport-blok` przez `hot load` w trakcie gry: generacja 2 bez restartu. Poprawka przy okazji: `hot load` moda, który nie jest aktywny, bierze stan z `hot-state/saved/` (dotąd go pomijał, choć `hot unload` obiecuje „stan zapisany na następne ładowanie”). Testy: `declared_commands_wait_for_the_players_tick`, `unload_keeps_the_state_for_the_next_load`, `commands_from_player_events_run_on_the_next_tick`; każdy pada po celowym zepsuciu kodu. Następny krok: M5 (pula slotów 1024/2048 według decyzji z tej sekcji).

**2026-11-13 — M5.1 i M5.2 zrobione (na Macu):** blok moda to plik `mods/<id>/assets/blocks/<nazwa>.png`; `hotd` przydziela slot (tylko dopisywany, `hot-state/slots.json`), buduje powtarzalną paczkę zasobów bez CI, a `hotd serve-pack` ją serwuje. `hot-core` tłumaczy `<id>:<nazwa>` ↔ `hot_slots:slot_N` w `block`, `set_block`, `give` i w zdarzeniach. Sprawdzone w grze (szczegóły w M5): nowy wygląd po ponownym wejściu bez restartu silnika. **Budżet podmiany nadal niespełniony:** pula 1024/2048 wydłuża podmianę silnika do 1,16–1,34 s (S7), więc wymóg „krócej niż 1 s” (ADR 0004) czeka na M0.19; nie odhaczam go. Instalator ma krok 8 (pula, `--address`, `horizon-hot-pack`), nie sprawdzony: brak maszyny x86_64. Dalej: M7.

**2026-11-13 — `tick_block`/`random_tick` przeniesione do M0.19:** w obecnym API haki nie zmienią bloku ze slotu, wymagają zmiany puli u graczy i nie mają filtra po nazwie (FAKT, szczegóły w M5.2). Decyzja przy zamrożeniu puli. Dalej: M7.

**2026-11-13 — M6.3 zrobione (maszyna wirtualna):** `server-install.sh --uninstall [--yes]` usuwa horizon-hot i Horizona, a zostawia świat, kopie, logi i użytkownika `horizon`; przedtem pakuje stan modów do `backups/`. Szczegóły w M6.3. Dalej: M0.19.

**2026-11-13 — M0.20 zrobione na Macu: pula 4096/8192.** Z rozgrzanym silnikiem pula prawie nie zmienia pauzy: 480–549 ms na tym samym silniku, 505–573 ms na nowym pliku silnika z pustym cache; rośnie rozgrzanie (do 4,4 s, przed zamrożeniem) i pamięć czekającego silnika (ok. 515 MB). `hotd::slots` ma 4096/8192, bez slotów z tickiem (`docs/sloty.md`). Mixin `wait_for_world` kończy czekanie po SIGTERM, bo samo `stop` czekało 120 s; `hotd` przerywa podmianę przez `stop` + SIGTERM. Nie sprawdzone: ThinkCentre (M0.11). Dalej: M8.

**2026-11-13 — M8.2 i M8.4: podmiana silnika pod graczem.** `hot-proxy` przenosi graczy na silnik bez ekranu ładowania (`hot-proxy switch`) i trzyma ich, gdy silnik padnie. Na Macu 50 podmian z 10 botami i prawdziwym klientem bez rozłączenia (pauza proxy ≤ 118 ms) i `kill -9` silnika z powrotem wszystkich po 5,6 s. Przy okazji: numer czatu po podmianie trzeba przesuwać, inaczej klient rozłącza się przy pierwszej wiadomości. Dalej: M8.3 (`hotd` uruchamia nowy silnik i woła `switch`).

**2026-11-13 — M8.1c: serwer w Dockerze; Horizon ze źródeł.** Obraz i `compose.yaml` działają do miejsca, w którym potrzebne są pliki gry właściciela (szczegóły w M8.1c). Przy okazji wyszło, że datowane wydania Horizona znikają z GitHuba po kilku dniach (R29): instalator na czystej maszynie padał, więc instalator i obraz kompilują teraz Horizona ze źródeł w pełnej rewizji. Poprawione też przekazywanie SIGTERM do silnika w kontenerze. Otwarte: test na prawdziwych plikach gry. Dalej: M8.2.

**2026-11-13 — M8.1b: instalator stawia silnik za `hot-proxy`.** Krok 8/13: sekret, adres lokalny, dopiero potem tryb offline (test kolejności). Sprawdzone w maszynie wirtualnej bez Horizona i na lokalnym `horizon-server`; pełna instalacja czeka na x86_64. Właściciel prosi o uruchomienie w Dockerze: krok M8.1c. Dalej: M8.1c, potem M8.2.

**2026-11-13 — M8.1 zrobione na Macu: `hot-proxy` z logowaniem online.** Proxy loguje gracza przez usługę sesji, podpisuje silnikowi jego dane (Velocity) i przesyła ruch. Sprawdzone na prawdziwym koncie: wejście przez proxy z prawdziwym UUID; bezpośrednie wejście na silnik odrzucone. Dalej: M8.1b (instalator), potem M8.2.

**2026-11-13 — M0.19 (S13) zrobione na Macu: budżet „krócej niż 1 s” spełniony.** Nowy silnik startuje przed `stop` starego i czeka przed zajęciem świata (mixin `wait_for_world` w `hot-host`). Pula 1024/2048, gracz w grze: ten sam silnik 470–541 ms z `save-all flush`, nowy plik silnika z pustym cache 509–531 ms; wcześniej 1,16–1,34 s i ok. 2,2 s. Czekający silnik: 363 MB. Zmiany w D11 (kroki), D16 (pamięć), S12 wniosek 3 (dotyczy już tylko przerwanej podmiany i silnika bez `hot-host`). Nowe wymagania dla `hotd` w M8: błąd `start` przed zamrożeniem po kodzie wyjścia, zatrzymanie czekającego silnika przy przerwanej podmianie, każdy silnik pod nową ścieżką, wstrzymanie zmian datapacków w czasie podmiany. Nie sprawdzone: ThinkCentre (M0.11), pula 4096/8192 (M0.20). Szczegóły: `docs/spike/s13-rozgrzany-silnik.md`. Dalej: M0.20.

**2026-11-13 — M6.2 zrobione (Mac + maszyna wirtualna):** slice'y systemd dla gry i tła oraz hamulec MSPT w `hotd` (zatrzymuje ciężkie procesy tła, nie blokuje modów). Pomiar zmienił D16: tło ma `CPUWeight=100`, bo przy 1 `hotd` stał. Poprawione dwa błędy instalatora (brak domyślnego toolchainu, port 8131 przy ponownym uruchomieniu). Szczegóły w M6.2. Dalej: M6.3.

**2026-11-13 — M7.3 zrobione (Mac):** świat, którego silnik nie wczyta → `hot status` podaje powód i polecenie przywrócenia najnowszej kopii, `HOT-E0212`; `hotd` niczego nie przywraca sam. Sprawdzone testem i na prawdziwym raporcie Horizona. Szczegóły w M7.3. Dalej: M6.2.

**2026-11-13 — M7.2 zrobione (Mac):** wstrzymanie i sprzątanie przy małej ilości miejsca, testy na prawdziwie pełnym woluminie APFS; poprawiony błąd, przez który brak miejsca odrzucał dobry commit na zawsze. Szczegóły w M7.2. Dalej: M7.3 (uszkodzony świat).

**2026-11-13 — M7.1 sprawdzone na prawdziwym silniku (Mac):** segfault moda → restart przez supervisor i v1; druga awaria (`kill -9`) → tryb bezpieczny; czysty restart → mody wracają. Szczegóły w M7.1. Nie sprawdzone: ThinkCentre.

**2026-11-13 — M7.1 zrobione (bez silnika):** prawdziwy segfault moda gorącego w procesie potomnym → następny start z ostatniej dobrej generacji; druga awaria z rzędu w ciągu 5 min od startu → tryb bezpieczny bez modów gorących (`HOT-E0408`), wyjście przez `hot load`, `hot unload` albo czysty restart. Testy w `crates/hot-core/tests/reload.rs`, mutacja je czerwieni. Dalej: M7.2 (pełny dysk).
