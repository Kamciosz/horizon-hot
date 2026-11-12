# PLAN — horizon-hot

Serwer Horizon (Minecraft 26.3 w Rust), który bierze mody z repozytorium git i wczytuje je w trakcie gry, bez restartu serwera.

Stan: plan zatwierdzony do dyskusji, kod jeszcze nie istnieje. Data: 2026-11-12.
Oznaczenia: **FAKT** = sprawdzone w kodzie Horizona (rewizja `66f3981`, `~/Library/Application Support/horizon-launcher/native/stage`) albo próbą; **NIEWIADOMA** = rozstrzyga ją wskazany spike z M0.

---

## 1. Cel i kryterium „gotowe”

Cel: operator robi `git push` do repo serwera, a w ciągu kilkudziesięciu sekund zmiana działa na serwerze. **Serwer nigdy się nie restartuje i nikt nie zostaje rozłączony** — także przy modach natywnych `.hnmod` i aktualizacji Horizona. Te dwa przypadki idą przez przeszczep silnika za proxy (D12): gracze zostają połączeni, a świat stoi kilka sekund. Wszystko inne działa bez żadnej pauzy (D1, D8, D16, D17).

„Gotowe” dla całego projektu (każdy punkt ma obserwację, która by oblała):

1. Repo bez katalogu `mods/` → serwer działa jak zwykły Horizon 26.3 (log: `hot-host: 0 modów`, gracz wchodzi).
2. Push zmiany w kodzie moda → nowa wersja logiki działa bez restartu: PID procesu `horizon-engine` taki sam przed i po, gracz połączony bez przerwy, zachowanie zmienione.
3. Push zmiany w `data/` moda → `/reload`, nowy przepis działa, PID bez zmian.
4. Push nowego bloku z puli slotów → blok istnieje w grze bez restartu serwera (wygląd: patrz D9).
5. Push moda, który panikuje → mod wyłączony, serwer działa dalej, poprzednia generacja przywrócona, commit oznaczony jako zły.
6. Push moda, który wywołuje segfault → serwer wstaje z ostatnią dobrą generacją, a nie wpada w pętlę awarii.
7. 100 przeładowań pod obciążeniem → MSPT w granicach ustalonego marginesu, wzrost pamięci zmierzony i ograniczony.
7a. Na docelowym ThinkCentre, z 10 botami, w trakcie wdrożenia (pobranie, przeładowanie, migracja) p99 MSPT < 50 ms — gracze nie mają lagu. Test obciążeniowy obok (np. `stress-ng` w slice tła) nie podnosi MSPT ponad ten próg.
8. `~/engineering-standard/bin/quality-audit` ≥ 120/125.

---

## 2. Ustalenia z researchu (podstawa decyzji)

| # | Ustalenie | Status | Źródło |
|---|---|---|---|
| R1 | Mod natywny (trait mod) może zależeć od `libloading 0.8.9`; przechodzi kontrolę `E_CRATE_DEPENDENCY` | FAKT (próba) | `projects/probe-hot-host` w horizon-mod-lab, build 39 s |
| R2 | Taki mod buduje się do samodzielnego silnika serwera (`native-build --server-output`) i na serwerze dedykowanym ładuje `.dylib`, wywołując z niej funkcję | FAKT (próba) | log: `hot-host probe: hot_probe() = 4242` |
| R3 | Haki serwera działają na jednym wątku „Server thread”, 20 TPS | FAKT | `entry.rs:35`, `docs/server.md:42` |
| R4 | Lista modów natywnych to jeden `OnceLock`, bez wyładowania ani przeładowania | FAKT | `mod_api.rs:594` |
| R5 | Mod może zarejestrować własną komendę (gracza i konsoli), np. `hot reload` | FAKT | `docs/native-mods.md:2095-2105`, `native_server.rs:999-1054`, `dedicated.rs:1904` |
| R6 | `horizon-server cmd` przekazuje komendę na stdin silnika; komenda moda ma pierwszeństwo | FAKT | `cli.rs:150-161`, `supervisor.rs:685` |
| R7 | Haki przekazują typy Rusta (`&mut dyn Host`, `ItemStack`, `Text`); silnik ma własny alokator `horizon_threads::Allocator` | FAKT | `launcher/src/native_mods/api.rs:104` |
| R8 | Bloki i przedmioty rejestrują się tylko przy starcie; `/reload` ich nie doda | FAKT | `docs/mods.md:23-25` |
| R9 | Zachowanie bloku można ustalać hakami w locie (`use_item_on`, `right_click_block`, `destroy_block`, `place_block`, `tick_block`, `random_tick`) | FAKT | `native_hooks.rs:6584-6892` |
| R10 | `/reload` na serwerze dedykowanym przeładowuje przepisy, łupy, tagi, zaawansowania i funkcje `mcfunction`, po czym synchronizuje graczy | FAKT | `game.rs:20248-20300`, `functions.rs:716` |
| R11 | Klient Horizon musi mieć identyczny rejestr treści (hash `content_identity`); inaczej dostaje rozłączenie „Content registry mismatch” | FAKT | `content_identity.rs:1-17`, `lan/conn.rs:1041-1076` |
| R12 | Mody natywne nie są porównywane z klientem; `.hmod` z `side="both"` tak | FAKT | `docs/mods.md:86-93` |
| R13 | Serwerowy resource pack jest wysyłany tylko w fazie konfiguracji; serwer nie ma kodu wysyłania paczki w trakcie gry ani hostingu HTTP | FAKT | `lan/conn.rs:1151-1170` |
| R14 | Klient obsługuje wysłanie paczki w trakcie gry, `item_model` i `custom_model_data` | FAKT | `net.rs:885-890`, `item_model.rs:1116,1355` |
| R15 | `/transfer` działa po obu stronach (wymaga `accepts-transfers`) | FAKT | `commands.rs:1122`, `net.rs:925` |
| R16 | `server.toml` i `[properties]` (adres paczki, `accepts-transfers`) działają dopiero po restarcie | FAKT | `docs/hosting.md:200` |
| R17 | Katalog budowy `native/stage` i `build.log` są wspólne dla wszystkich buildów natywnych launchera | FAKT (próba) | nagłówek logu wymieniał `drony_native` |
| R18 | Horizon planuje mody `dylib` ładowane przy starcie i **nigdy** nie wyładowywane; WASM usunięty; Plugin API (W9) niezaimplementowane | FAKT | issue #7386, `docs/native-mods.md:664`, `docs/server-next.md:153` |

---

## 3. Decyzje architektoniczne

Każda decyzja ma postać gotową do przeniesienia do `docs/adr/NNNN-*.md` (kontekst → decyzja → konsekwencje).

### D1. Jeden mod natywny „hot-host” wkompilowany w silnik raz; reszta to biblioteki ładowane w locie
- **Kontekst:** R4 i R18 — Horizon nie przeładuje modu natywnego. R1 i R2 — `libloading` działa w silniku serwera.
- **Decyzja:** `hot-host` to trait mod budowany `native-build --server-output`. Implementuje haki z listy D5 i przekazuje je do modów „gorących” (`.dylib` na macOS, `.so` na Linuksie), wczytywanych przez `libloading`.
- **Konsekwencje:** silnik budujemy wyłącznie, gdy zmienia się sam `hot-host` albo wersja Horizona. Mod gorący potrafi tylko to, co daje lista haków D5 i funkcje gospodarza D6.

### D2. Granica host ↔ mod to stabilne C ABI (`hot-abi`), nie ABI Rusta
- **Kontekst:** R7 — silnik ma własny alokator, a haki przekazują typy Rusta. ABI Rusta jest stabilne tylko przy identycznym kompilatorze, źródłach i flagach.
- **Decyzja:** crate `hot-abi`: struktury `#[repr(C)]`, napisy jako `(ptr, len)` pożyczane na czas wywołania, wynik `Flow` jako `u8`. Każdy mod eksportuje `hot_mod_abi() -> u32` i `hot_mod_entry() -> *const HotModVTable`. Host odrzuca mod z inną wersją główną ABI. Pamięć zwalnia zawsze ta strona, która ją przydzieliła.
- **Konsekwencje:** mod gorący można zbudować dowolnym stabilnym Rustem (≥ wersja minimalna w `hot-sdk`). Każdy hak trzeba przetłumaczyć ręcznie, dlatego lista z D5 jest celowo krótka i rośnie tylko wtedy, gdy ktoś potrzebuje nowego haka.

### D3. Starej generacji nigdy nie wyładowujemy
- **Kontekst:** `dlclose` biblioteki Rusta grozi awarią (wątki, statyki, destruktory TLS), a na macOS zwykle i tak nic nie robi.
- **Decyzja:** każda generacja to kopia pliku pod unikalną nazwą `gen/<mod>-<sha>-<n>.dylib`. Stara generacja zostaje w pamięci, ale nie dostaje już wywołań. Wyładowanie następuje dopiero przy restarcie procesu.
- **Konsekwencje:** każde przeładowanie zostawia rozmiar biblioteki w pamięci. Kryterium M1 mierzy ten wzrost. Opcjonalny restart nocny (M7) czyści pamięć.

### D4. Kontrakt cyklu życia moda jest w ABI od wersji 1
- `init(ctx, prev_state: bytes) -> status` — wywoływane **poza** wątkiem serwera; tam też jest `dlopen`. Przełączenie generacji następuje na granicy ticku (w `server_tick`), więc przeładowanie nie zatrzymuje ticków.
- `deactivate(ctx) -> bytes` — **obowiązkowe**: zatrzymuje wątki i timery moda i zwraca stan do przekazania nowej generacji.
- Kontekst `ctx` jest ważny tylko w czasie jednego wywołania. `hot-sdk` wymusza to czasem życia (`HostCtx<'call>`).
- Każde wejście do moda jest opakowane w `catch_unwind` (w `hot-sdk`). Panika → kod błędu → host wyłącza mod i wraca do poprzedniej generacji.
- Trwałe dane mod trzyma w `hot-data/<mod>/` w katalogu serwera.

### D5. Haki w v1 (kuratorowana lista)
`server_tick`, `player_logged_in`, `player_logged_out`, `player_chat`, `player_command`, `use_item`, `use_item_on`, `right_click_block`, `left_click_block`, `place_block`, `destroy_block`, `tick_block`, `random_tick`, `server_data_loaded`. Komendy modów idą przez jedną komendę główną `/hot <mod> <argumenty…>`, zarejestrowaną przez `hot-host` przy starcie (R5). Własne komendy najwyższego poziomu (`/sklep`) są możliwe tylko wtedy, gdy spike S3 potwierdzi, że da się przechwycić i anulować `player_command`.

### D6. Funkcje gospodarza dla modu (v1)
Wiadomość do gracza i do wszystkich, `run_command` (R: `native_server.rs:479`; spike S4), odczyt i zapis bloku, danie przedmiotu, informacje o graczu (nazwa, UUID, pozycja), cząsteczki, log. Wszystkie jako tabela wskaźników funkcji w `HostVTable`, ważna tylko w trakcie wywołania.

### D7. Pula slotów na bloki i przedmioty w osobnym `.hmod` (`hot-slots`, `side="both"`)
- **Kontekst:** R8 — rejestry są zamrożone od startu. R11 i R12 — klient musi mieć ten sam rejestr, a mody natywne nie są sprawdzane.
- **Decyzja:** sloty (`hot:block_000…255`, `hot:item_000…511`) leżą w modzie zawartości `hot-slots.hmod`. **`hot-host` nie rejestruje żadnej treści** (żadnego `native_blocks` ani `native_items`) — inaczej klienci dostaliby rozłączenie z niejasną przyczyną. Gracz instaluje `hot-slots.hmod` raz.
- **Przydział slotów** jest tylko dopisywany i zapisywany w stanie serwera (`hot-state/slots.json`). Raz przydzielony slot nigdy nie trafia do innego moda, bo świat zapisuje bloki po identyfikatorze. Zwolnienie slotu to jawna komenda operatora.
- **Konsekwencje:** zabraknie slotów → trzeba wydać większą pulę = restart serwera i aktualizacja u graczy. Rozmiar puli jest wersjonowany.

### D8. Dane modów jako paczka danych świata + `/reload`
Katalog `mods/<id>/data/` jest kopiowany do `<świat>/datapacks/hot-<id>/`, po czym daemon wysyła `reload` (R10). Zmiana samych danych nie wymaga budowania niczego.

### D9. Wygląd: resource pack serwowany przez daemon pod stałym adresem
- **Kontekst:** R13 i R16 — adres paczki ustawia się raz, a serwer wysyła paczkę tylko przy wejściu gracza.
- **Decyzja:** daemon sam serwuje `http://<host>:8131/pack.zip` (stały adres, ustawiony raz w `init`). Paczka powstaje z `mods/*/assets/` i przydziału slotów. Pole `sha1` jest puste, więc klient pobiera paczkę przy każdym wejściu (spike S5). Komenda `/hot refresh` robi graczom `/transfer` na ten sam serwer (R15): kilka sekund ekranu ładowania, bez restartu serwera.
- **Konsekwencje:** jeśli S5 obleje, nowy wygląd widać dopiero po ponownym wejściu gracza. Logika i dane i tak działają od razu. Wysłanie paczki w trakcie gry (R14 — klient to umie) wymaga dostępu `hot-host` do pakietów — sprawdza to spike S6. Jeśli się uda, `/transfer` przestaje być potrzebny.

### D10. Format repozytorium serwera
```
repo-serwera/
  hot.toml                  # włączone mody, ustawienia
  mods/                     # może nie istnieć → czysty serwer
    <id>/
      mod.toml              # id, wersja, wymagane sloty
      Cargo.toml + src/     # opcjonalnie: logika (cdylib, zależy od hot-sdk)
      data/                 # opcjonalnie: paczka danych
      assets/               # opcjonalnie: modele i tekstury slotów
  cold/                     # opcjonalnie: zwykłe .hmod/.hnmod (ścieżka zimna, D12)
```
Mody budujemy **ze źródeł** na maszynie serwera (`cargo build --release`), nie z gotowych binarek: powtarzalność i przegląd kodu w gicie.

### D11. Daemon `hotd` jako osobny proces obok `horizon-server`
- Nie forkujemy Horizona ani `horizon-server`. Daemon działa na tej samej maszynie (R: silnik działa tylko na systemie i architekturze, na których go zbudowano).
- Git przez polecenie `git` (nie `git2`): `fetch` co 10 s albo webhook. Bez tokio: wątki std + `tiny_http` (webhook i serwowanie paczki) + `zip`.
- Do serwera mówi przez `horizon-server --dir <X> cmd "hot apply <manifest>"` (R6). Host czyta manifest z dysku i zapisuje wynik do `hot-state/status.json`.
- **Zaufanie:** daemon przyjmuje tylko commity podpisane kluczem z `allowed_signers` (`git verify-commit`). Kod modów działa bez piaskownicy.

### D12. Przeszczep silnika przez własne proxy (`hot-proxy`) — zamiast restartu
- **Kontekst:** mod natywny `.hnmod` i aktualizacja Horizona to nowy plik programu; działającego procesu nie da się przekompilować. Horizon ma już wszystko, czego potrzebuje proxy:
  - FAKT: obsługa forwardingu Velocity (`horizon-server/src/config.rs:69`, kanał `velocity:player_info` w `horizon-protocol/src/packets/login.rs:92`);
  - FAKT: pakiet `StartConfiguration` w protokole (`horizon-protocol/src/packets/play.rs:2075`);
  - FAKT: klient Horizon obsługuje powrót do konfiguracji (`horizon-client/src/net.rs:891`, `game/remote.rs:10902`).
- **Decyzja:** gracze łączą się z `hot-proxy` (Rust, na crate `horizon-protocol`), a nie bezpośrednio z silnikiem. Proxy trzyma połączenie TCP gracza i szyfrowanie przez cały czas. Za nim działa silnik, w trybie forwardingu.
- Podmiana silnika to **migracja na żywo** (jak live migration maszyn wirtualnych), wymaga forka silnika (D19):
  1. **Rozgrzanie:** nowy silnik buduje się i startuje obok w trybie gotowości — rejestry, zasoby, wątki gotowe, bez świata. Start (próba: 1,6 s) dzieje się, gdy stary silnik normalnie gra; nie ma go na krytycznej ścieżce.
  2. **Kopia wstępna:** stary silnik, nadal tykając, przesyła przez gniazdo lokalne wszystkie wczytane chunki, encje i stan graczy. Śledzi chunki i encje zmienione od kopii („brudne”) i wysyła je w kolejnych rundach, aż delta zmieści się w budżecie.
  3. **Zamrożenie na jeden tick:** stary silnik kończy tick, wysyła ostatnią deltę i stan sesji graczy (identyfikatory encji, wysłane chunki, otwarte okna, identyfikatory teleportów, keep-alive).
  4. **Przełączenie:** nowy silnik wykonuje następny tick, a proxy przekierowuje strumienie pakietów graczy na nowy silnik. Klient nie dostaje żadnego ekranu ładowania — stan po obu stronach jest identyczny.
  5. Stary proces czeka 60 s jako zapas. Błąd nowego przed przełączeniem = przerwanie migracji, gracze nic nie widzą; błąd po przełączeniu = migracja z powrotem tą samą drogą.
- **Budżet pauzy: ≤ 50 ms (jeden tick)**, czyli tyle, ile trwa zwykły pojedynczy wolny tick. Gracz tego nie odróżni od lagu sieci.
- **Wyjątek:** jeśli nowy silnik zmienia rejestry (nowe bloki z `.hnmod`), klient musi je dostać. Bez forka klienta wymaga to `StartConfiguration` (ekran ładowania u gracza, świat na serwerze dalej tyka). Z forkiem klienta (D18) rejestry dochodzą pakietem w trakcie gry, bez ekranu ładowania.
- **Konsekwencje:** nikt nie jest rozłączony, świat stoi najwyżej jeden tick. Zmiany z warstwy gorącej (D1, D8) idą bez żadnej pauzy. `hot-proxy` zmienia się rzadko; jego własna aktualizacja to przekazanie gniazd TCP i stanu szyfrowania do nowego procesu (`SCM_RIGHTS`, jak w nginx) — etap M9, pauza rzędu milisekund.

### D19. Fork silnika: eksport i import stanu na żywo
- Zapis chunków i encji już istnieje (format zapisu świata); fork używa go do serializacji w pamięci zamiast na dysk. Nowe są: śledzenie brudnych chunków i encji, serializacja sesji gracza oraz tryb gotowości bez świata.
- Fork trzymamy jako mixin bundle (`horizon-mixins`) albo łatki na przypięty commit, żeby aktualizacje Horizona dało się przenosić.
- NIEWIADOMA: jak bardzo stan sesji gracza w `native-game` jest rozproszony. Rozstrzyga spike S8 (M0): mapa stanu sesji + prototyp eksportu jednego chunka i jednego gracza.
- Buildy natywne biorą blokadę pliku, bo `native/stage` jest wspólny (R17), i nie działają równolegle z buildami w laboratorium.

### D20. Gra ma pierwszeństwo; wszystko inne pracuje w tle z resztek
- **Kontekst:** serwer stoi na współdzielonym ThinkCentre kolegi (część maszyny). Ograniczony sprzęt nie może przerwać gry.
- **Decyzja — trzy klasy pracy:**
  1. **Gra** (silnik + proxy): zarezerwowane rdzenie i pamięć, najwyższy priorytet. Linux: osobny `systemd` slice z `CPUWeight=10000`, `MemoryLow=` (pamięć chroniona) i przypięciem do rdzeni (`AllowedCPUs=`).
  2. **Tło** (daemon, git, paczki, kopia stanu przy migracji): `SCHED_IDLE` + `nice 19` + `ionice -c3`, osobny slice z `CPUWeight=1`, `MemoryMax=`, `IOWeight=1`. Dostaje CPU tylko wtedy, gdy gra go nie potrzebuje.
  3. **Kompilacja**: nigdy na serwerze (D21).
- **Hamulec MSPT:** daemon co sekundę czyta `horizon-server status --json`. MSPT > 35 ms → wszystkie prace w tle stają (`SIGSTOP` grupy procesów, kopia migracji wstrzymana), MSPT < 25 ms przez 10 s → ruszają dalej. Gra nigdy nie czeka na tło; tło zawsze czeka na grę.
- **Migracja (D12) tylko, gdy się mieści:** przed rozgrzaniem drugiego silnika daemon sprawdza, czy po jego starcie zostanie zapas pamięci ponad `MemoryLow` gry. Jeśli nie — czeka. Silnik w gotowości trzyma tylko rejestry, bez świata.
- **Konsekwencje:** na słabym sprzęcie zmiany wchodzą wolniej (minuty zamiast sekund), ale gracze nie odczuwają ich wcale. Kryterium „gotowe” mierzy MSPT graczy, nie czas wdrożenia.

### D21. Kompilacja poza serwerem — w CI na tym samym systemie i architekturze
- **Kontekst:** budowa modów i silnika zjadłaby słaby sprzęt na minuty. Silnik z `--server-output` działa tylko na systemie i architekturze, na których powstał (bez kompilacji skrośnej), a ThinkCentre to najpewniej x86_64, a Mac — arm64.
- **Decyzja:** GitHub Actions (runner Linux x86_64) buduje `.so` modów gorących i silnik, podpisuje je i publikuje jako artefakt commita. Daemon na serwerze tylko pobiera, sprawdza podpis i ładuje. Zastępuje decyzję D10 o budowaniu ze źródeł na serwerze (źródła nadal w gicie, budowa powtarzalna w CI).
- NIEWIADOMA: czy `horizon-launcher native-build` działa w CI bez okna i bez konta Microsoft. Rozstrzyga spike S9.

### D22. Automatyczna aktualizacja Horizona — bez restartu, z bramkami
- **Kontekst:** wbudowane `horizon-server update` działa tylko dla instalacji z `install.sh` (FAKT: odmawia dla innego układu katalogów) i robi restart. Nasz silnik jest własnym buildem (hot-host + łatki forka D19), więc aktualizację prowadzimy sami.
- **Decyzja — potok:**
  1. **Wykrycie:** CI co godzinę pyta API GitHuba o nowe wydania `horizon-rust/horizon`. Domyślnie tylko wydania stabilne (tagi `vRRRR.MM.DD`), nie Nightly (ustawienie w `hot.toml`).
  2. **Przeniesienie forka:** łatki D19 i `hot-host` nakładane na nowy commit, build silnika (D21). Nie nakładają się albo nie kompilują → aktualizacja wstrzymana, zgłoszenie (issue w repo + wiadomość do operatora), serwer zostaje na starej wersji.
  3. **Test w CI:** silnik startuje na kopii świata z ostatniej kopii zapasowej, 10 botów gra 5 minut, mody gorące ładują się, MSPT i log bez błędów, test migracji stanu stary → nowy. Oblany → wstrzymanie jak wyżej.
  4. **Okno zgodności klientów:** jeśli nowa wersja zmienia `content_identity` (R11) albo protokół, starsi klienci Horizona byliby rozłączani. Daemon czeka, aż launcher graczy zdąży się zaktualizować (domyślnie 24 h od wydania, ustawienie), i ogłasza na czacie „serwer przejdzie na wersję X o …”. Zmiana niewidoczna dla klienta → bez czekania.
  5. **Podmiana:** migracja na żywo (D12) w klasie „tło” (D20), z kopią zapasową świata przed nią.
  6. **Powrót:** nowy silnik w ciągu 10 min zgłasza błędy albo MSPT wyraźnie gorszy niż przed aktualizacją → migracja z powrotem na starą wersję tą samą drogą; wersja oznaczona jako zła.
- **Konsekwencje:** aktualizacje wchodzą same, bez restartu, a zepsuta nie trafia na serwer. Koszt: łatki forka trzeba utrzymywać przy każdej wersji Horizona — dlatego dążymy do wysłania ich do Horizona jako PR, żeby forka było jak najmniej. Zmiana formatu zapisu świata między wersjami: migracja stanu używa konwersji, którą nowa wersja i tak ma dla starych światów; brak konwersji → aktualizacja przez zapis na dysk z krótką pauzą świata, zapowiedzianą na czacie.
- NIEWIADOMA: jak często wydania Horizona zmieniają `content_identity` i protokół. Rozstrzyga S11 (historia ostatnich 10 wydań).

### D23. Własny launcher serwera — nakładka na `horizon-launcher`, nie nowy launcher
- **Kontekst:** gracze mają wejść na serwer jednym kliknięciem. `horizon-launcher` ma pełne CLI: `account sign-in`, `install`, `prepare`, `instance create`, `multiplayer add`, `play --server`.
- **Decyzja:** mała aplikacja w Rust (styl Rune), która woła CLI `horizon-launcher`. Kroki przy pierwszym uruchomieniu: instalacja Horizona → logowanie Microsoft **raz** (kod urządzenia, logowanie robi Horizon) → `prepare` z kupionej gry gracza → własna instancja serwera → wgranie `hot-slots.hmod` do `horizon-mods/` i wpis w `horizon-mods.txt` → adres serwera na liście. Potem przycisk „Graj” = `play --server`, także offline pod nickiem (`account offline`).
- **Granice:** bez pomijania sprawdzenia zakupu gry; bez plików Mojanga w launcherze; bez logo Minecrafta, Mojanga i Horizona jako własnej marki.
- **Konsekwencje:** launcher jest mały, bo całą ciężką pracę robi `horizon-launcher`; aktualizacje Horizona przychodzą same. Zmiana CLI `horizon-launcher` może zepsuć nakładkę — test na każdym Nightly w D22.

### D24. Odporność na pomyłki: gracz instaluje jednym ruchem na macOS, Windows i Linuksie
- **Decyzja (wybór użytkownika, 2026-11-12): tylko skrypt**, bez `.dmg`, `.exe` i `.AppImage`:
  - macOS i Linux: `curl -fsSL <adres>/install.sh | sh`;
  - Windows (PowerShell): `irm <adres>/install.ps1 | iex`.
- Skrypt pobiera launcher, sprawdza jego SHA-256, kładzie go w folderze użytkownika i tworzy skrót („Graj na <serwer>”) na pulpicie / w menu. Bez uprawnień administratora.
- Plik pobrany przez `curl` lub PowerShell zwykle nie dostaje znacznika „z internetu” (kwarantanna macOS, Mark of the Web w Windows), więc ostrzeżenia „nieznany deweloper” prawdopodobnie nie będzie i podpis kodu nie jest potrzebny. HIPOTEZA — sprawdzenie w M11.3 na czystych maszynach.
- Instalator sam sprawdza wszystko, co może pójść nie tak (brak miejsca, brak internetu, stara wersja, przerwana instalacja), mówi po polsku, co zrobić, i da się go uruchomić drugi raz bez szkody (idempotentny).
- Launcher sam się aktualizuje. Przycisk „Coś nie działa” zbiera log i diagnozę do jednego pliku, który gracz wysyła Tobie.
- Builda na trzy systemy robi CI (macOS arm64 + x86_64, Windows x86_64, Linux x86_64). Launcher nie zawiera plików Mojanga, więc tu CI jest dozwolone (inaczej niż przy silniku, R: `docs/microsoft-account.md`).
- Podpis kodu (płatny) wraca do decyzji tylko wtedy, gdy M11.3 pokaże ostrzeżenia systemu mimo instalacji skryptem.

### D25. Serwer: jeden skrypt na Ubuntu Server instaluje i konfiguruje wszystko
- **Cel:** ThinkCentre kolegi z już zainstalowanym Ubuntu (FAKT od użytkownika, 2026-11-12); wersja i architektura do odczytu w S10.
- **Polecenie:** `curl -fsSL <adres>/server-install.sh | sudo bash -s -- --repo <git-url>`.
- **Skrypt robi:** sprawdza system (Ubuntu 24.04 LTS, x86_64/arm64, RAM, dysk, porty) → tworzy użytkownika `horizon` → instaluje `horizon-server` oficjalnym `install.sh` i zależności (git, Rust do budowy w tle) → przygotowanie zasobów gry: logowanie Microsoft kodem urządzenia (jedyny krok ręczny: „otwórz link, wpisz kod”) albo `import-resources` z folderu skopiowanego z Twojego Maca → `init` świata, generuje sekrety (forwarding, klucze podpisu), whitelistę, tryb serwera → jednostki systemd: silnik, `hot-proxy`, `hotd` + slice'y z D20 → zapora `ufw` (25565, paczka zasobów) → kopie zapasowe co noc z retencją 7 dni → na końcu **autotest**: łączy bota, sprawdza `status`, ładuje testowy mod gorący, pokazuje adres serwera i „✅ Działa”.
- Idempotentny: drugie uruchomienie niczego nie psuje i naprawia brakujące elementy. `--uninstall` usuwa wszystko oprócz świata i kopii.
- **Wymaga `sudo`** — jeśli kolega da tylko konto bez roota albo kontener, potrzebny wariant bez systemd (do ustalenia, gdy będzie wiadomo, jak udostępni maszynę).

### D16. `hot-host` jako cienki, stały loader
`hot-host` w silniku robi tylko jedno: ładuje bibliotekę `hot-core` i przekazuje jej wszystkie haki. Logika gospodarza (lista modów, przydział slotów, komendy `/hot`) siedzi w `hot-core`, przeładowywanym jak zwykły mod gorący. Zmiana gospodarza nie wymaga więc nowego silnika.

### D17. Duża pula slotów i konwerter `.hmod` → sloty
- Pula: 4096 bloków i 8192 przedmioty (limit Horizona to 2^20 stanów bloków, `mod_content.rs:1860`). Rozmiar sprawdzany w M5 pod kątem czasu startu i pamięci klienta.
- Gotowe mody zawartości `.hmod` daemon przepisuje na sloty, paczkę danych i paczkę zasobów. Wtedy klient nie musi niczego instalować ani restartować. Ograniczenie: w grze identyfikator to slot, a nazwę i wygląd daje paczka zasobów.

### D18. Fork klienta Horizon — nowa treść bez restartu u gracza (etap późny)
Klient wczytuje bloki i przedmioty tylko przy starcie (R8), więc treść spoza puli slotów wymagałaby restartu **klienta**. Rozwiązanie w forku `horizon-client`: przy `StartConfiguration` klient pobiera z serwera listę treści, dobudowuje rejestr bloków i przedmiotów i przeładowuje zasoby; przeładowanie zasobów Horizon już ma. Zgodność `content_identity` jest liczona po dobudowaniu. Do tego czasu nowa treść idzie przez sloty (D17).

### D13. Ochrona przed pętlą awarii
Segfaultu w modzie nie złapie żaden `catch_unwind`. `restart_on_crash` podniesie serwer, więc:
- przy starcie host ładuje tylko generacje oznaczone jako **ostatnie dobre** — takie, które przeżyły 5 minut;
- daemon oznacza commit jako zły, jeśli silnik padł w ciągu 60 s od `hot apply`, i nie aplikuje go ponownie.

### D14. Tabela klasyfikacji zmian (serce daemona)

| Co zmienił commit | Działanie | Restart? |
|---|---|---|
| tylko `mods/*/data/` | kopia do `datapacks/`, `reload` | nie |
| `mods/*/src/` | `cargo build` → `hot apply` (nowa generacja) | nie |
| `mods/*/assets/` lub sloty w ramach puli | przebudowa paczki, `/hot refresh` | nie (gracze robią szybki transfer) |
| nowy mod / usunięty mod | jak wyżej: `deactivate` albo `init` | nie |
| `hot-core` (logika gospodarza) | jak mod gorący (D16) | nie |
| `.hmod` w `cold/` | konwersja na sloty (D17) | nie |
| `.hnmod` w `cold/`, wersja Horizona, nowa pula slotów | przeszczep silnika przez proxy (D12) | nie — gracze połączeni, świat stoi kilka sekund |

### D15. Repozytorium i narzędzia projektu
Jeden workspace Rust `~/Developer/horizon-hot`: `crates/hot-abi`, `crates/hot-sdk`, `crates/hotd`, `bundle/hot-host` (bundle `horizon-mixin.toml`), `content/hot-slots` (`.hmod`), `examples/` (przykładowe mody). Start przez `~/engineering-standard/bin/repo-init --stack rust` i `~/.claude/tools/verify-init`. Conventional Commits, forma bezosobowa, autor Szymon Sosnowski.

---

## 4. Kroki

Każdy krok ma sprawdzenie, które obleje, jeśli zmiana nie zadziała. Odhaczaj `[x]`.

### M0 — Repo i spike'i (rozstrzygają niewiadome)
- [ ] M0.1 `repo-init ~/Developer/horizon-hot --stack rust`, `verify-init`. Sprawdzenie: `quality-audit` uruchamia się, hook `commit-msg` odrzuca commit „test”.
- [ ] M0.2 **S1** Czy bundle `hot-host` może mieć crate ścieżkowy (`hot-abi`) jako `[[crate]]`? Sprawdzenie: `native-build --server-output` przechodzi. Gdy nie przejdzie: `hot-abi` jako moduł kopiowany do bundla skryptem `xtask`.
- [ ] M0.3 **S2** Czy serwer dedykowany ładuje `.hmod` (`horizon-mods/` + `horizon-mods.txt` w katalogu serwera)? Sprawdzenie: blok z testowego `.hmod` stawiany `setblock` na serwerze. **Blokuje D7 i D12** — jeśli nie ładuje, sloty idą do natywnego builda serwera, a klient dostaje ten sam `.hmod`; trzeba wtedy porównać `content_identity`.
- [ ] M0.4 **S3** Czy `player_command` pozwala anulować nieznaną komendę? Sprawdzenie: `/sklep` obsłużone bez błędu „Unknown command”. Gdy nie: zostaje tylko `/hot …`.
- [ ] M0.5 **S4** Czy `Host::run_command` działa z `server_tick`? Sprawdzenie: `say` z ticku widać w logu.
- [ ] M0.6 **S5** Czy klient bez `sha1` pobiera paczkę ponownie przy każdym wejściu i po `/transfer` na ten sam serwer? Sprawdzenie: zmiana tekstury widoczna na zrzucie po transferze.
- [ ] M0.7 **S6** Czy trait mod może wysłać pakiet `ResourcePackPush` w trakcie gry? Sprawdzenie: zmiana tekstury bez transferu. Wynik opcjonalny.
- [ ] M0.8 **S7** Czy model z paczki serwera nadpisuje model bloku z `.hmod`? Sprawdzenie: zrzut ekranu.
- [ ] M0.85 **S8** Mapa stanu sesji gracza i świata w `native-game` + prototyp: eksport i import jednego chunka i jednego gracza między dwoma procesami. Sprawdzenie: gracz przeniesiony między procesami zostaje w tym samym miejscu z tym samym ekwipunkiem, bez ekranu ładowania. **Blokuje D12 w wersji ≤ 50 ms**; jeśli obleje, wracamy do wariantu z zapisem na dysk i mierzymy.
- [ ] M0.86 **S9** `native-build --server-output` i `cargo build` modu gorącego na runnerze GitHub Actions (Linux x86_64), bez okna i bez konta. Sprawdzenie: artefakt uruchamia się na Linuksie x86_64.
- [ ] M0.87 **S10** Profil ThinkCentre: przydział (VM, kontener czy konto), rdzenie, RAM, system; pomiar RAM i MSPT pustego serwera i serwera z 10 botami. Sprawdzenie: liczby zapisane w `docs/sprzet.md`, z nich wartości `AllowedCPUs`, `MemoryLow`, `MemoryMax` dla D20.
- [ ] M0.88 **S11** Historia ostatnich 10 wydań Horizona: które zmieniły `content_identity`, protokół, format świata, API haków. Sprawdzenie: tabela w `docs/aktualizacje.md`; z niej domyślne okno zgodności w D22.
- [ ] M0.9 Zapis wyników S1–S11 jako `docs/adr/0001-spikes.md`; korekta D5–D9, jeśli trzeba.

### M1 — Rdzeń przeładowania (`hot-abi`, `hot-sdk`, `hot-host`)
- [ ] M1.1 `hot-abi`: VTable z wersją, `init`, `deactivate`, `server_tick`, `player_chat`; testy układu pamięci (`size_of`/`align_of`).
- [ ] M1.2 `hot-sdk`: makro `hot_mod!`, `HostCtx<'call>`, `catch_unwind` na każdym wejściu, serializacja stanu.
- [ ] M1.3 `hot-host`: komenda konsoli `hot load <ścieżka>`, ładowanie poza wątkiem serwera, przełączenie na granicy ticku, kopia pliku pod unikalną nazwą.
- [ ] M1.4 Przykład `examples/licznik`: liczy ticki i odpowiada na czacie; stan przechodzi między generacjami.
- Sprawdzenie M1 (wszystkie muszą przejść):
  - przeładowanie zmienia odpowiedź na czacie, PID silnika bez zmian;
  - stan licznika przechodzi przez przeładowanie;
  - 100 przeładowań z 10 botami (`horizon-bots`): MSPT p95 wzrasta < 2 ms, a pamięć rośnie liniowo o ≤ rozmiar `.dylib` na generację;
  - mod z `panic!()` w `server_tick` zostaje wyłączony, serwer działa, wraca poprzednia generacja.

### M2 — Daemon `hotd`: git → build → apply
- [ ] M2.1 Polling `git fetch` i webhook, weryfikacja podpisu commita.
- [ ] M2.2 Klasyfikacja zmian według D14 (testy jednostkowe na diffach).
- [ ] M2.3 `cargo build` modów z limitem CPU i RAM, logi buildu do `hot-state/builds/`.
- [ ] M2.4 `hot apply` przez `horizon-server cmd`, odczyt `status.json`, oznaczanie złych commitów (D13).
- Sprawdzenie M2: push do lokalnego repo → zmiana działa w ≤ 60 s; repo bez `mods/` → serwer czysty; niepodpisany commit odrzucony z wpisem w logu; commit z błędem kompilacji → poprzednia wersja dalej działa.

### M3 — Dane
- [ ] M3.1 Kopia `data/` do `datapacks/hot-<id>/` + `reload`; usunięcie moda usuwa paczkę.
- Sprawdzenie: nowy przepis działa po pushu, PID bez zmian.

### M4 — Pełna lista haków v1 (D5) i funkcje gospodarza (D6)
- [ ] M4.1 Haki bloków i graczy, `/hot <mod> …`.
- [ ] M4.2 Przykład `examples/teleport-blok`: kliknięcie bloku teleportuje gracza.
- Sprawdzenie: test integracyjny z botem dla każdego haka (zdarzenie → widoczny efekt).

### M5 — Sloty i wygląd
- [ ] M5.1 `hot-slots.hmod` (256 bloków, 512 przedmiotów), przydział tylko dopisywany.
- [ ] M5.2 Budowa paczki z `assets/`, serwowanie na `:8131`, `/hot refresh`.
- Sprawdzenie: nowy blok z repo stawiany bez restartu serwera; po `/hot refresh` ma swój model (zrzut); restart serwera nie zmienia przydziału slotów.

### M6 — `hot-proxy` i przeszczep silnika (D12)
- [ ] M6.1 Proxy przezroczyste: logowanie, szyfrowanie, kompresja, forwarding Velocity do silnika. Sprawdzenie: gracz gra przez proxy; `ping` i MSPT bez istotnej różnicy.
- [ ] M6.2 Przytrzymanie gracza w konfiguracji (`StartConfiguration`) i powrót do tego samego silnika. Sprawdzenie: 10 botów i jeden prawdziwy klient przeżywają pętlę 50 razy bez rozłączenia.
- [ ] M6.3 Migracja na żywo (D12, D19): rozgrzanie, kopia wstępna, zamrożenie na jeden tick, przełączenie strumieni, powrót przy błędzie. Sprawdzenie: dodanie `.hnmod` w trakcie gry → mod działa, nikt nie został rozłączony (log proxy), **najdłuższy tick w oknie migracji ≤ 50 ms** (`horizon-server report`), klient nie pokazał ekranu ładowania (zrzut co 100 ms); celowo zepsuty silnik → migracja przerwana, gracze nic nie widzą.
- [ ] M6.4 Automatyczna aktualizacja (D22): wykrycie, przeniesienie forka, test w CI, okno zgodności, migracja, powrót. Sprawdzenie: symulowane wydanie (lokalny tag na starszym i nowszym commicie) → serwer sam przechodzi na nową wersję, gracze połączeni; wydanie z celowo zepsutą łatką → serwer zostaje na starej wersji i jest zgłoszenie.

### M8 — Konwerter `.hmod` → sloty (D17)
- [ ] Sprawdzenie: testowy `.hmod` z laboratorium (`projects/horizon-test-mod`) działa na serwerze bez restartu silnika i bez instalacji u klienta.

### M9 — Aktualizacja samego proxy przez przekazanie gniazd (`SCM_RIGHTS`)
- [ ] Sprawdzenie: nowa wersja proxy, gracze połączeni, zero rozłączeń.

### M11 — Launcher serwera (D23)
- [ ] M11.1 Pierwsze uruchomienie: instalacja, logowanie raz, `prepare`, instancja, `hot-slots.hmod`, adres serwera.
- [ ] M11.2 Przycisk „Graj” i tryb offline pod nickiem.
- [ ] M11.3 Skrypty `install.sh` i `install.ps1` (D24), build launchera w CI, samoaktualizacja, przycisk „Coś nie działa”.
- Sprawdzenie: na czystej maszynie wirtualnej macOS, Windows 11 i Ubuntu Desktop jedna linijka → skrót na pulpicie → wejście na serwer, bez ostrzeżenia systemu; drugi start bez logowania; przerwana w połowie instalacja + ponowne uruchomienie → działa.

### M12 — Skrypt instalacji serwera (D25)
- [ ] M12.1 `server-install.sh`: sprawdzenie systemu, użytkownik, instalacja, zasoby, `init`, sekrety, systemd, slice'y, zapora, kopie, autotest.
- [ ] M12.2 Idempotencja i `--uninstall`.
- Sprawdzenie: czysta maszyna wirtualna Ubuntu Server 24.04 (multipass) → jedno polecenie → autotest „✅ Działa” i gracz wchodzi; drugie uruchomienie → bez zmian i bez błędów; `--uninstall` → świat i kopie zostają.

### M10 — Fork klienta: nowa treść przy rekonfiguracji (D18)
- [ ] Sprawdzenie: blok spoza puli slotów pojawia się u połączonego gracza bez restartu klienta i serwera.

### M7 — Utwardzenie i dokumentacja
- [ ] M7.1 Ochrona przed pętlą awarii (D13): test z modem, który robi segfault.
- [ ] M7.2 `horizon-server service install` + usługa `hotd` (launchd/systemd), opcjonalny restart nocny.
- [ ] M7.3 README, ARCHITECTURE, ADR 0002–0015 (D1–D15), przewodnik „pierwszy mod gorący”.
- Sprawdzenie: `quality-audit` ≥ 120/125; segfault → serwer wstaje z ostatnią dobrą generacją, bez pętli.

---

## 5. Przewidywane problemy

| Problem | Co wtedy |
|---|---|
| S2: serwer nie ładuje `.hmod` | sloty w natywnym buildzie serwera + identyczny `.hmod` u klienta; test zgodności `content_identity` |
| S5 i S6 obleją | wygląd po ponownym wejściu gracza; logika i dane dalej bez restartu |
| Aktualizacja Horizona zmienia sygnatury haków | `hot-host` przypięty do commita; aktualizacja = ścieżka zimna + rebuild; `hot-abi` się nie zmienia, więc mody gorące działają dalej |
| Wspólny `native/stage` (R17) | blokada pliku; nie budować w czasie prac w laboratorium |
| Mod trzyma wątek po `deactivate` | wątek zostaje w pamięci; `hot-sdk` daje `HotThread`, który kończy się sam przy `deactivate` |
| Wyciek pamięci przez generacje | pomiar w M1; restart nocny w M7 |
| Przeszczep gubi stan gracza (otwarta skrzynia, przedmiot na kursorze, jazda na koniu) | przed zapisem świata proxy zamyka okna i zsiada graczy; ekwipunek wraca z zapisu świata; test w M6.3 |
| Delta brudnych chunków nie mieści się w jednym ticku (np. wielka maszyna redstone) | więcej rund kopii wstępnej; zamrożenie dopiero, gdy delta < budżet; limit rund, po nim migracja odkładana na spokojniejszy moment |
| Nowy mod zmienia format danych encji lub bloku | migracja przez mapowanie stanu (jak przy wczytaniu starego świata); bez mapowania migracja odmawia i mówi dlaczego |
| Port 25565 lub 8131 zajęty | porty w `hot.toml`, sprawdzane przy starcie daemona |

## 6. Poza zakresem
- Prawdziwe wyładowanie kodu (`dlclose`) i podmiana kodu w trakcie wywołania.
- Mody Fabric/Forge (`.jar`) — w Horizonie nie działają.
- Piaskownica dla kodu modów (jest tylko kontrola podpisu commita).
- Kilka serwerów za jednym proxy (sieć serwerów); proxy obsługuje jeden świat.
- Publikacja w rejestrze `horizon-mods` i na GitHubie.

## 7. Stan prac (do wznowienia w innym wątku)
- Próba z researchu: `~/Developer/horizon-mod-lab/projects/probe-hot-host` (trait mod z `libloading`), eksport w `/private/tmp/claude-501/probe-hot-host-export`. Do wykorzystania w M1.3.
- Uwaga: w `native/stage/Cargo.toml` (members) zostało `native-api/probe_hot_host` po próbie. Launcher przepisuje tę listę przy każdym buildzie, więc prawdopodobnie nic to nie psuje — sprawdzić przy najbliższym buildzie „Drony”.
- Następny krok: M0.1.
