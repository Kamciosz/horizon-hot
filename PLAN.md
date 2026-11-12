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
| W2 | Zmiana modów **bez restartu serwera**, z możliwie najkrótszą pauzą (1,5 s to za długo). |
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
5. Push moda natywnego `.hnmod` → działa po podmianie silnika; nikt nie rozłączony; pauza świata zmierzona i w budżecie z sekcji 8 (dziś 1 s, do decyzji użytkownika); gracz bez ekranu ładowania (W2, wariant A).
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

---

## 4. Decyzje architektoniczne

Każda w formie gotowej do `docs/adr/NNNN-*.md` (kontekst → decyzja → konsekwencje). Numery ADR kolejne (0001 to struktura projektu, 0002 wyniki spike'ów), a w tytule ADR numer decyzji (Dn).

### Warstwa gorąca (bez żadnej pauzy)

**D1. Gospodarz w silniku + mody gorące ładowane w locie.**
Kontekst: R1, R2, R4. Decyzja: jeden trait mod `hot-host` wkompilowany w silnik; mody gorące to `cdylib` (`.so` na serwerze, `.dylib` na Macu w testach) ładowane przez `libloading`. Konsekwencje: mod gorący robi tylko to, co dają haki D5 i funkcje D6; nowe bloki — przez sloty (D8).

**D2. `hot-host` jest cienkim, stałym loaderem; logika gospodarza to `hot-core`.**
`hot-host` tylko ładuje `hot-core` (też `cdylib`) i przekazuje mu wszystkie haki. `hot-core` zarządza modami, slotami i komendą `/hot` i przeładowuje się jak mod gorący. Zmiana gospodarza nie wymaga nowego silnika.

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
`server_tick`, `player_logged_in`, `player_logged_out`, `player_chat`, `player_command` (własne komendy najwyższego poziomu, np. `/sklep` — R6), `use_item`, `use_item_on`, `right_click_block`, `left_click_block`, `place_block`, `destroy_block`, `tick_block`, `random_tick`, `server_data_loaded`. Plus komenda `/hot <mod> …` rejestrowana przez `hot-host`.

**D6. Funkcje gospodarza v1.**
Wiadomość do gracza i do wszystkich, `run_command`, odczyt i zapis bloku, danie przedmiotu, dane gracza (nazwa, UUID, pozycja), cząsteczki, log. Tabela `HostVTable` ważna tylko w trakcie wywołania.
- Wiadomość do gracza w haku `player_command` idzie przez `event.replies`, bo `Host::tell` zwraca tam `false` (FAKT, S3). Poza tym hakiem `hot-host` używa `Host::tell`, a gdy zwróci `false`, wysyła `tellraw` przez `run_command` (działa z `server_tick`, S4). Który wariant działa poza hakiem komendy, pokaże test w M4.1.

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
`hot-pack` (osobny mały proces, poza hamulcem D16) serwuje `http://<host>:8131/pack.zip`; adres ustawiony raz przy instalacji. Nowa paczka = nowy sha1 → klient pobiera tylko zmienioną (R14, potwierdzone w S5). Na serwerze `require-resource-pack = true`; `hot-play` zgadza się na paczkę naszego serwera z góry: dopisuje go do `servers.dat` w katalogu gry swojej instancji (obok `saves/`, FAKT z kodu: `multiplayer.rs`, `servers_file`) z `acceptTextures` = `true`. Bez tego klient czeka na „Joining world…” na decyzję gracza i pyta ponownie przy każdej paczce wysłanej w grze (S6). Klucz nazwy przedmiotu z puli to `block.hot_slots.item_N` (S7), nie `item.…`; `hot-pack` używa klucza, który pokazuje gra. Nowy wygląd dociera do graczy tylko na dwa sposoby: `hot-proxy` wysyła w grze `ResourcePackPush` ze stałym id (zastępuje poprzednią paczkę) albo gracz wchodzi ponownie. Podmiana silnika wyglądu nie zmienia, bo proxy połyka konfigurację nowego silnika. S6: wysłanie w grze działa bez rozłączenia, ale przez ok. 1,3 s cały ekran zasłania nakładka ładowania (`docs/spike/s6-paczka-w-trakcie-gry.md`). Kiedy wysyłać: decyzja użytkownika (sekcja 8).

### Silnik i proxy (wariant A)

**D10. `hot-proxy` przed silnikiem.**
Gracze łączą się z `hot-proxy` (Rust, na `horizon-protocol`) na porcie 25565; silnik słucha na `127.0.0.1:25566` w trybie forwardingu Velocity (R16). Proxy kończy szyfrowanie, kompresję, keep-alive i teleporty po stronie klienta i mapuje identyfikatory encji między klientem a silnikiem. Logowanie Microsoft weryfikuje proxy (tryb online), więc serwer zostaje premium.

**D11. Podmiana silnika — wariant A: gracz zostaje, świat „mruga”, bez ekranu ładowania.**
- Kontekst: R17 — `StartConfiguration` pokazuje ekran; R19 — podmiana całkiem niewidoczna to miesiące pracy.
- Kroki:
  1. Nowy silnik buduje się w tle (D15) do nowego katalogu.
  2. Proxy przestaje przekazywać ruch; stary silnik dostaje `stop` (zapisuje świat i graczy, zwalnia `session.lock`). `player-export` niepotrzebny: oba silniki czytają tych samych graczy ze wspólnego świata (S8).
  3. Nowy silnik startuje na tym samym świecie dopiero po zatrzymaniu starego (S12); `hotd` nie czeka na koniec komendy `start`, proxy loguje się, gdy port przyjmie połączenie.
  4. Proxy loguje się do nowego silnika jako każdy gracz, odtwarza jego konfigurację, połyka nowy `Login`, wysyła klientowi „usuń encje” i zamienia id gracza (id u klienta ↔ id w silniku). „Zapomnij chunki” niepotrzebne: nowe chunki zastępują stare w miejscu (S8). Klient widzi mrugnięcie encji, bez ekranu.
  5. Błąd nowego silnika → proxy trzyma graczy, `hotd` uruchamia z powrotem stary silnik tą samą drogą (ok. 1 s więcej).
- Budżet: pauza świata ≤ 1 s. Zmierzone w S8 na Macu: 0,96–1,07 s (stop ok. 0,4 s + start ok. 0,55 s, proxy < 20 ms), gdy `hotd` czeka na zniknięcie procesu silnika, a nie na komendy. Rozgrzanie przed zwolnieniem świata niemożliwe (S12). Z pulą slotów (S7): 1,16–1,34 s przy 1024/2048, 1,45–1,82 s przy 4096/8192. Nowy plik silnika (mod natywny, `hot-host`, Horizon) unieważnia cache treści modów, więc taka podmiana trwa z pełną pulą ok. 3,9 s. Budżet i rozmiar puli: decyzja użytkownika (sekcja 8). Opis: `docs/podmiana.md`.
- Wyjątek: zmiana rejestrów (nowe bloki w `.hnmod`, nowa pula) → klient potrzebuje nowych plików; wtedy aplikacja gracza je pobiera, a gracz wchodzi ponownie. Dlatego treść idzie przez sloty.
- To samo przełączenie służy do: aktualizacji Horizona (D18), odzyskania pamięci po generacjach (D4), wznowienia po awarii silnika (D13).

**D12. Dwa katalogi silnika (niebieski/zielony) pod nadzorem `hotd`.**
`/srv/horizon/engine-a` i `engine-b`, wspólny świat w `/srv/horizon/world` (przenoszony między nimi tylko w kroku 3 D11). Każdy pod własnym `horizon-server --dir` z własnym portem. Świat spoza katalogu (dowiązanie) i drugi port działają (S12); panel webowy tylko po jednej stronie albo na różnych portach.

**D13. Ochrona przed pętlą awarii i wznowienie po awarii.**
- Przy starcie `hot-core` ładuje tylko generacje oznaczone jako ostatnie dobre (przeżyły 5 min).
- `hotd` oznacza commit jako zły, gdy silnik padł ≤ 60 s od jego załadowania; nie aplikuje go ponownie.
- Po awarii silnika proxy trzyma połączenia graczy (świat stoi, bez ekranu ładowania), potem wprowadza ich jak w D11. Cel: nikt nie traci połączenia, jeśli silnik wstanie w < 30 s (limit ciszy klienta `READ_TIMEOUT` = 30 s, potwierdzone w kodzie w S8; proxy wysyła w tym czasie keep-alive, więc limit dotyczy tylko proxy).

### Budowa i zasoby

**D14. Mody gorące budowane w GitHub Actions; serwer tylko pobiera.**
`cdylib` modu nie zawiera plików Mojanga, więc CI jest dozwolone (inaczej niż silnik, R21). Workflow w repo serwera buduje `.so` dla architektury ThinkCentre, podpisuje (klucz CI), publikuje jako artefakt commita. `hotd` pobiera, sprawdza podpis i ładuje. Zastępuje „budowę ze źródeł na serwerze” z wersji 1.

**D15. Silnik budowany na ThinkCentre, w tle.**
Kontekst: R21 — silnik wymaga przygotowanych zasobów, a ThinkCentre i tak je ma. `native-build --server-output` w klasie „tło” (D16), z blokadą pliku (R27). Silnik budujemy rzadko: nowy `.hnmod`, nowe Nightly, zmiana `hot-host`. Pierwszy build ok. 12 min, potem przyrostowo (R22).

### Sprzęt (W3)

**D16. Gra ma pierwszeństwo; tło pracuje z resztek.**
- Gra (silnik, `hot-proxy`, `hot-pack`): slice systemd `horizon-game.slice`, `CPUWeight=10000`, `MemoryLow=` (z S10), `AllowedCPUs=` na poziomie procesu.
- Tło (`hotd`: git, pobieranie, budowa silnika, kopie zapasowe): `horizon-bg.slice`, `CPUWeight=1`, `MemoryMax=`, `IOWeight=1`, `nice 19`, `ionice -c3`, `SCHED_IDLE`.
- Hamulec MSPT: `hotd` co sekundę czyta `status --json` (R25). p95 > 35 ms → zatrzymuje swoje procesy potomne (`SIGSTOP` na grupie budowy, wstrzymanie kopii); p95 < 25 ms przez 10 s → wznawia. Sam `hotd`, `hot-pack` i hamulec nigdy nie są zatrzymywane. Opóźnienie reakcji 1–5 s (okno 100 ticków, R25).
- Podmiana silnika (D11) nie potrzebuje pamięci na dwa silniki: stary kończy pracę, zanim wystartuje nowy (S12). Drugi silnik działa równolegle tylko przy teście aktualizacji (D18). Ten test rusza tylko wtedy, gdy wolna pamięć mieści drugi silnik z zapasem ponad `MemoryLow` gry; inaczej czeka. Po zimnym starcie silnik zajmuje ok. 700 MB, po ciepłym 290–540 MB zależnie od puli (S7, RSS na Macu).

### Aktualizacje i naprawy (W4, W5, W8)

**D17. Aktualizacja modów:** push → CI (D14) → `hotd` (polling co 10 s, bez webhooka, bez otwartego portu) → klasyfikacja (tabela D24) → zastosowanie.

**D18. Aktualizacja Horizona — kanał Nightly.**
- Kontekst: R23, R26. `hotd` raz na godzinę pyta API GitHuba o nowe Nightly.
- Kroki: przygotowanie wersji (`horizon-launcher install` + `prepare` z zasobów serwera) → build silnika z `hot-host` (D15) z `--version <nowe Nightly> --ignore-pins`, bo `hot-host` przypina wersję Horizona (`horizon_revision`), a bez tej flagi build zostaje na starej (`docs/aktualizacje.md`, punkt 7) → test: silnik na kopii świata z ostatniej kopii, 10 botów przez 5 minut, mody gorące ładują się, log bez błędów, MSPT nie gorszy o > 20% → podmiana (D11) → obserwacja 10 min → jeśli gorzej, powrót tą samą drogą i wersja oznaczona jako zła.
- Okno zgodności klientów: tylko gdy zmienia się coś, czego klient potrzebuje (R24 — protokół stały; kanały `horizon:*` się dopisuje; nie wiadomo, czy `content_identity` jest takie samo na różnych Nightly, więc decyzja: każde nowe Nightly traktujemy jako zmianę, której klient potrzebuje, dopóki test w M9 tego nie obali: klient na jednym Nightly, serwer na drugim, ta sama pula). Aplikacja gracza aktualizuje Horizona sama przy starcie, więc okno = czas do najbliższego startu; serwer czeka maks. 24 h i ogłasza termin na czacie.
- Ryzyko: `libloading` może zniknąć z `Cargo.lock` Horizona → build nie przejdzie → serwer zostaje na starej wersji, `hot status` mówi dlaczego. Mało prawdopodobne: 0.8.9 we wszystkich 10 ostatnich wydaniach, ciągnie ją grafika (`wgpu-hal`, `ash`).
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
- Kategorie kodów: `E01xx` instalacja, `E02xx` git i CI, `E03xx` budowa silnika, `E04xx` mody gorące, `E05xx` dane i paczka zasobów, `E06xx` proxy i podmiana silnika, `E07xx` aktualizacje, `E08xx` sprzęt i hamulec, `E09xx` aplikacja gracza, `E10xx` świat i kopie.
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
| `native/*.hnmod`, nowe Nightly, `hot-host` | build w tle + podmiana silnika (D11) | świat stoi bez ekranu ładowania: ok. 2,2 s przy puli 1024/2048, ok. 3,9 s przy 4096/8192 (Mac, S7) |
| nowe bloki w `.hnmod`, zmiana puli | jak wyżej + nowe pliki u gracza | aplikacja pobiera pliki, gracz wchodzi ponownie |

**D25. Instalacja serwera jednym poleceniem (Ubuntu na ThinkCentre, `sudo` jest).**
`curl -fsSL <adres>/server-install.sh | sudo bash -s -- --repo <github-url>`:
1. Sprawdzenie: Ubuntu, architektura, RAM, dysk, wolne porty 25565/8131.
2. Użytkownik `horizon`, katalogi `/srv/horizon`.
3. `horizon-server` oficjalnym `install.sh`; `horizon-launcher`; git.
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

### M0b — Diagnostyka (przed każdym innym kodem, D27–D29)
- [ ] M0.15 `AGENTS.md` z opisem projektu, słownikiem i zasadami (D29); uzupełniany w każdym kolejnym kroku. Sprawdzenie końcowe w M12.
- [ ] M0.16 `hot-diag`: typ błędu z polami D27, zapis JSONL, rotacja, maskowanie sekretów, przechwytywanie paniki z backtrace. Sprawdzenie: testy jednostkowe — wpis bez `code`, `what` lub `fix` nie kompiluje się (builder z wymaganymi polami); znany sekret w kontekście zamaskowany.
- [ ] M0.17 Katalog `docs/bledy/` + test zgodności kodów w CI. Sprawdzenie: kod bez wpisu → CI czerwone.
- [ ] M0.18 Szkielet `hot report`. Sprawdzenie: raport ze sztucznego błędu zawiera wszystkie 5 części D28 i < 200 KB.

Zasada dla kroków M1–M13: każdy nowy błąd dostaje kod i wpis w katalogu, a sprawdzenie kroku obejmuje wywołanie co najmniej jednego błędu i obejrzenie wpisu w logu.

### M1 — Rdzeń przeładowania (`hot-abi`, `hot-sdk`, `hot-host`, `hot-core`)
- [ ] M1.1 `hot-abi`: VTable z wersją, `init`, `deactivate`, `server_tick`, `player_chat`; testy `size_of`/`align_of`.
- [ ] M1.2 `hot-sdk`: makro `hot_mod!`, `HostCtx<'call>`, `catch_unwind` w każdym eksporcie, stan między generacjami.
- [ ] M1.3 `hot-host`: tylko ładowanie `hot-core` i przekazywanie haków (D2). Paczka składana skryptem: kopia `crates/hot-abi` jako `[[crate]]` (wynik S1, `docs/spike/s1-hot-abi-w-silniku.md`).
- [ ] M1.4 `hot-core`: `hot load <ścieżka>`, ładowanie poza wątkiem serwera, przełączenie na granicy ticku, unikalne nazwy plików, ostatnie dobre generacje (D13).
- [ ] M1.5 `examples/licznik`: liczy ticki, odpowiada na czacie, stan przechodzi przez przeładowanie.
- Sprawdzenie M1:
  - przeładowanie zmienia odpowiedź, PID silnika ten sam, licznik zachowany;
  - 100 przeładowań przy 10 botach: p95 MSPT rośnie < 2 ms; pamięć rośnie ≤ rozmiar `.so` na generację;
  - `panic!()` w `server_tick` → mod wyłączony, poprzednia generacja, serwer działa;
  - przeładowanie samego `hot-core` → PID ten sam, mody działają dalej.

### M2 — CI modów i `hotd` (git → artefakt → załadowanie)
- [ ] M2.1 Workflow `hot.yml` dla repo serwera: budowa `.so`, podpis, artefakt (D14).
- [ ] M2.2 `hotd`: polling repo co 10 s, pobranie artefaktu, weryfikacja podpisu.
- [ ] M2.3 Klasyfikacja zmian D24 (testy jednostkowe na przykładowych diffach).
- [ ] M2.4 `hot apply` przez `horizon-server cmd`, odczyt `hot-state/status.json`, oznaczanie złych commitów (D13).
- [ ] M2.5 `hot status` (terminal) i `/hot status` (gra) (D21).
- Sprawdzenie: push → zmiana na serwerze ≤ 3 min; repo bez `mods/` → czysty serwer; artefakt z błędnym podpisem odrzucony; commit niekompilujący się → serwer bez zmian, `hot status` mówi dlaczego.

### M3 — Dane (D7)
- [ ] M3.1 Kopia `data/` + `reload`; usunięcie moda usuwa paczkę.
- Sprawdzenie: nowy przepis działa, PID ten sam; zmierzony skok ticka przy `reload` zapisany w `docs/sprzet.md`.

### M4 — Haki i funkcje v1 (D5, D6)
- [ ] M4.1 Pozostałe haki D5 i funkcje D6.
- [ ] M4.2 `examples/teleport-blok`, `examples/sklep` (komenda `/sklep`).
- Sprawdzenie: test integracyjny z botem dla każdego haka (zdarzenie → widoczny efekt).

### M5 — Sloty i wygląd (D8, D9)
- [ ] M5.1 `hot-slots.hmod` w rozmiarze z S7; przydział tylko dopisywany.
- [ ] M5.2 `hot-pack`: budowa paczki z `assets/`, sha1, serwowanie na 8131.
- Sprawdzenie: nowy blok z repo stawiany bez restartu; po następnym wejściu ma swój model (zrzut); podmiana silnika nie zmienia przydziału.

### M6 — Serwer na ThinkCentre: instalator i priorytety (D16, D25)
- [ ] M6.1 `server-install.sh` kroki 1–8 i 10 (bez proxy i podmiany — dochodzą w M8).
- [ ] M6.2 Slice'y i hamulec MSPT w `hotd`.
- [ ] M6.3 Idempotencja i `--uninstall`.
- Sprawdzenie: czysta maszyna wirtualna Ubuntu (multipass, wersja jak na ThinkCentre) → jedno polecenie → „Działa”; drugie uruchomienie bez zmian; `stress-ng` w `horizon-bg.slice` + 10 botów → p99 MSPT < 50 ms; potem to samo na ThinkCentre.

### M7 — Samonaprawa (D13, D20)
- [ ] M7.1 Segfault w module testowym → ostatnie dobre generacje, brak pętli.
- [ ] M7.2 Pełny dysk (sztucznie zapełniony) → sprzątanie i komunikat.
- [ ] M7.3 Uszkodzony świat → komunikat i polecenie przywrócenia; przywrócenie po potwierdzeniu.
- Sprawdzenie: każdy wiersz tabeli D20 ma test, który wywołuje błąd i obserwuje reakcję.

### M8 — `hot-proxy` i podmiana silnika (D10–D12)
- [ ] M8.1 Proxy przezroczyste: logowanie online, szyfrowanie, kompresja, forwarding. Sprawdzenie: gra przez proxy; ping i MSPT bez istotnej różnicy.
- [ ] M8.2 Mapowanie id encji i trzymanie keep-alive. Sprawdzenie: 50 sztucznych przełączeń na ten sam silnik bez rozłączenia (10 botów + prawdziwy klient).
- [ ] M8.3 Podmiana na nowy silnik według D11, z powrotem przy błędzie. Sprawdzenie: dodanie `.hnmod` w trakcie gry → mod działa, nikt nie rozłączony, brak ekranu ładowania (zrzuty co 100 ms), pauza w budżecie z sekcji 8; celowo zepsuty silnik → gracze zostają na starym.
- [ ] M8.4 Wznowienie po awarii silnika z przytrzymaniem graczy (D13). Sprawdzenie: `kill -9` silnika → gracze połączeni, wracają do gry po starcie.
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
| S8: pauza podmiany > 1 s | decyzja użytkownika: zaakceptować zmierzoną pauzę albo rozgrzewanie silnika przez mixin na `[transfer]` (większy koszt) |
| S8: klient rozłącza się przy długim braku pakietów | proxy wysyła klientowi keep-alive i własne pakiety podtrzymujące w czasie podmiany |
| S2: klient bez `hot-slots.hmod` jest rozłączany | to oczekiwane; `hot-play` instaluje plik automatycznie |
| Nightly często psuje build `hot-host` | serwer zostaje na starej wersji (D18); `hot-host` tylko na nazwanych hakach |
| `libloading` znika z `Cargo.lock` Horizona | serwer zostaje na starej wersji; zgłoszenie do Horizona |
| Wspólny `native/stage` (R27) | blokada pliku; na serwerze nie ma innych buildów |
| Mod trzyma wątek po `deactivate` | `hot-sdk` daje `HotThread`, który kończy się przy `deactivate` |
| Wyciek pamięci generacji | podmiana silnika po przekroczeniu progu (D4, M8.5) |
| Nowy mod zmienia format danych bloku/encji | podmiana używa konwersji, którą silnik ma dla starych światów; bez niej — odmowa i komunikat |
| ThinkCentre ma mało RAM-u na dwa silniki | podmiana czeka (D16); `hot status` pokazuje, ile brakuje |
| `/reload` daje skok ticka na słabym CPU | pomiar w M3; przeładowanie tylko zmienionej paczki, jeśli Horizon pozwala |
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

**2026-11-12 — M0.14 zrobione:** wyniki S1–S12 w `docs/adr/0002-spike.md` (tabela prób, otwarte punkty z krokiem, który je rozstrzygnie). Poprawki: D6 (`event.replies` w haku komendy, poza nim `tell` albo `tellraw`), D8 (nowa wersja `hot-slots` przy każdej zmianie puli), D9 (`servers.dat` instancji z `acceptTextures`), D16 (silniki nie działają naraz, pamięć na drugi tylko przy teście D18), D18 (każde Nightly traktowane jako zmiana dla klienta do testu w M9), D24 (zmierzone czasy zamiast „mrugnięcia ≤ 1 s”). Etap M0 kończą M0.10 i M0.11 na ThinkCentre. Następny krok: M0b (M0.15).

**2026-11-12 — M0.12 (S11) zrobione:** 6 Nightly (wszystkie istniejące) i 4 ostatnie wydania stabilne. Haki i funkcje `Host`, których używa `hot-host`, oraz pliki `content_identity`, `mod_list`, `mod_net` i numery pakietów są takie same od 11-03 do najnowszego Nightly; `libloading` 0.8.9 wszędzie; kanały `horizon:*` tylko się dopisuje. Poprawka D18: `hot-host` przypina wersję Horizona, więc build aktualizacji musi mieć `--ignore-pins`. Od 11-07 cache treści modów ma osobny katalog na zestaw modów (wniosek S7 bez zmian). Szczegóły: `docs/aktualizacje.md`.

**RYZYKO / SZANSA (HIPOTEZA, do obserwacji):** Horizon od 11-05 sam wprowadza mody bez restartu (`docs/design/mod-hotload.md` w repo Horizona): przełączanie modów treści w grze jednoosobowej i LAN, wyłączanie trait modów (`set_mod_on`), w planach mody wasm z wyładowaniem. Serwer dedykowany jeszcze tego nie ma. Jeśli dostanie, część D11 może być zbędna, a mody wasm mogłyby zastąpić nasze `cdylib` (D2). Sprawdzać przy każdym przeglądzie aktualizacji.

**2026-11-12 — M0.9 (S8) zrobione:** prototyp proxy (`spikes/s8-podmiana/proxy/`) przeniósł prawdziwego klienta 10 razy między dwoma silnikami na jednym świecie: ekran `playing` przez cały czas, połączenie zostało, chunki zostały, moby mrugnęły (ok. 0,8 s). Zamiana id gracza konieczna (próba kontrolna bez niej oblała). Pauza 0,96–1,07 s: prawie cała to stop + start silnika. Chunki od B zastępują kopię klienta (brak bloków-duchów); ruch i akcje gracza z przerwy przepadają. Niesprawdzone, do M8: mody i paczka zasobów w odtwarzanej konfiguracji, tryb online, wielu graczy. Szczegóły: `docs/spike/s8-podmiana-prototyp.md`, `docs/podmiana.md`.

**2026-11-12 — M0.8 (S7) zrobione:** pełna pula 4096/8192 działa (ostatni slot i przedmiot w grze), klient startuje o ok. 1 s wolniej, serwer o ok. 0,35 s, pamięć ok. +230 MB serwer, ok. +250 MB klient, plik 7 MB. Podmiana silnika z pulą trwa dłużej: tabela w `docs/sloty.md`. Cache treści modów zależy od pliku programu, więc podmiana z nowym silnikiem startuje go na zimno. Proxy odtwarza listę modów wobec B bez błędu (30 podmian). Szczegóły: `docs/spike/s7-rozmiar-puli.md`.

**2026-11-12 — M0.7 (S6) zrobione:** proxy wysłało klientowi w grze dwie paczki: wygląd zmienił się bez rozłączenia, przeładowanie trwało 1,39 s i przez ten czas cały ekran zasłaniała nakładka ładowania (zrzuty klatki). Klient pyta o zgodę przy każdej paczce, jeśli serwera nie ma na liście z `accept_textures`. `horizon-ctl status` nie widzi nakładki; podmiana silnika z S8 sprawdzona zrzutami: bez nakładki. Szczegóły: `docs/spike/s6-paczka-w-trakcie-gry.md`.

**DECYZJA UŻYTKOWNIKA (przed M5): kiedy nowy wygląd trafia do graczy.** (a) od razu: `hot-proxy` wysyła paczkę w grze, wszyscy gracze widzą ok. 1,3 s nakładki ładowania; (b) przy następnym wejściu: bez przerwy, ale do tego czasu nowe bloki mają wygląd zastępczy. Mój wybór: (a), bo nowy wygląd pojawia się tylko z nowym modem, czyli rzadko, a twórca moda od razu widzi efekt.

**DECYZJA UŻYTKOWNIKA (przed M5.1): rozmiar puli a budżet pauzy D11 (1 s).** Na Macu: pula 1024/2048 → pauza 1,16–1,34 s, ok. 2,2 s z nowym silnikiem; pula 4096/8192 → 1,45–1,82 s, ok. 3,9 s z nowym silnikiem. Mniejsza pula: po jej zapełnieniu nowy plik u każdego gracza (aplikacja pobierze go sama) i ponowne wejście. Mój wybór: 1024/2048 i budżet „do 1,5 s”, podmiana z nowym silnikiem bez budżetu (rzadka). Do czasu decyzji pracuję dalej; liczby z ThinkCentre (M0.11) mogą je przesunąć.

**2026-11-12 — M0.9 (S8) część 1, analiza kodu klienta:** `StartConfiguration`, drugi `Login` i `Respawn` zawsze pokazują ekran ładowania, więc proxy po pierwszym wejściu nie wysyła żadnego z nich. Jedyna droga: proxy samo loguje się i konfiguruje z B, połyka `Login` B, usuwa encje A i zamienia id gracza pA ↔ pB we wszystkich pakietach. Klient rozłącza się po 30 s ciszy. Bazą proxy będzie `horizon-protocol` (MIT, bez plików gry). Szczegóły: `docs/spike/s8-podmiana-analiza-klienta.md`. Dalej: prototyp proxy (część 2).

**2026-11-12 — M0.13 (S12) zrobione:** świat przez dowiązanie `world -> ../shared/world`; drugi silnik nie otworzy zajętego świata (`session.lock`), ale jego supervisor wpada w pętlę restartów 5→40 s, więc `hotd` musi najpierw zatrzymać stary; przekazanie świata ok. 1 s na Macu. Odkryta sekcja `[transfer]` w `server.toml` (proxy + zamrożenie gracza) — sprawdzić w M0.9. Szczegóły: `docs/spike/s12-dwa-serwery-jeden-swiat.md`.

**2026-11-12 — M0.3–M0.6 (S2–S5) zrobione:** pula slotów w `.hmod` działa, klient bez niej dostaje „Missing: hot_slots 0.1.0”; `/sklep` przez `Flow::Cancel` bez „Unknown command”, odpowiedź tylko przez `event.replies` (`Host::tell` w tym haku = `false`); `run_command` z `server_tick` działa; wygląd, model i nazwa slotu z paczki serwera działają tylko wtedy, gdy `.hmod` nie ma `models/`, `textures/`, `lang/` (D8 poprawione). Szczegóły: `docs/spike/s2-…` – `s5-…`. Dalej: M0.13, M0.9, M0.8, M0.7, M0.12, M0.14, M0b. M0.10 i M0.11 czekają na ThinkCentre.
- Zasoby prób (moje, do usunięcia po M0): katalog `/private/tmp/claude-501/hot-spike/` (serwery `s1-srv` port 25611, `s2-srv` port 25612, silniki `s1-server`, `s2-server`, paczka `pack-src`/`www`); `python3 -m http.server 8137` (log `http.log`); instancja Horizona `hot-spike` (`~/Library/Application Support/horizon/instances/hot-spike`); projekty `~/Developer/horizon-mod-lab/projects/spike-hot-*`.

**2026-11-12 — M0.2 (S1) zrobione:** `hot-abi` jako `[[crate]]` w paczce trait moda buduje się z silnikiem serwera; log `hot_abi::ABI_MAJOR = 1`. Szczegóły: `docs/spike/s1-hot-abi-w-silniku.md`. Dalej: M0.3–M0.6.

**2026-11-12 — plan v2 + diagnostyka i przekazanie zespołowi (D27–D29). Implementacja nie zaczęta: czeka na „go”.**
- Repo: `~/Developer/horizon-hot`, commit `f788091` (M0.1).
- Próba z researchu: `~/Developer/horizon-mod-lab/projects/probe-hot-host` — baza dla M0.2 i M1.3.
- W `native/stage/Cargo.toml` (members) zostało `native-api/probe_hot_host`; launcher przepisuje listę przy buildzie — sprawdzić przy najbliższym buildzie „Drony Test”.
- Następny krok: M0.2, po „go” użytkownika.
