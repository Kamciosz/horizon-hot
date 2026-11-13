# AGENTS.md — przeczytaj najpierw (dla ludzi i agentów AI bez kontekstu)

Ten plik wystarcza, żeby zacząć rozwijać albo naprawiać projekt bez wcześniejszej wiedzy. Uzasadnienia decyzji: `docs/adr/`. Jak to działa: `docs/ARCHITECTURE.md`.

## 1. Czym jest projekt (30 sekund)

Serwer gry dla małej grupy znajomych. Gra to **Horizon** — reimplementacja Minecraft Java 26.3 w Rust (wydania tylko Nightly: `nightly-<data>-<commit>`). Projekt dodaje:

- **mody gorące** — biblioteki `cdylib` wczytywane do działającego serwera bez restartu;
- **podmianę silnika** — nowa wersja Horizona albo mod natywny wchodzi bez rozłączania graczy: świat stoi krótko (cel: krócej niż 1 s, decyzja użytkownika; dziś bez rozgrzania silnika 1–4 s, zależnie od puli slotów i tego, czy silnik jest nowy), ale gracz nie widzi ekranu ładowania;
- **samoaktualizację i samonaprawę** wszystkiego;
- **instalator serwera** jednym poleceniem (Ubuntu) i **aplikację gracza** w terminalu (macOS, Windows, Linux).

Właściciel: Szymon Sosnowski. Serwer stoi na współdzielonym ThinkCentre z Ubuntu — sprzęt słaby, gra ma zawsze pierwszeństwo.

## 2. Słownik

| Pojęcie | Znaczenie |
|---|---|
| silnik | proces serwera Horizon (`horizon-server` + skompilowana gra) |
| `.hmod` | mod Horizona z danymi i zasobami; czytany tylko przy starcie gry |
| `.hnmod` | mod natywny Horizona wkompilowany w silnik (`native-build`) |
| mod gorący | nasz `cdylib` ładowany w locie przez `hot-core` |
| generacja | kolejna załadowana wersja moda gorącego; stare zostają w pamięci, nie są wywoływane |
| `hot-host` | stały, cienki mod natywny w silniku; ładuje `hot-core` |
| `hot-core` | logika gospodarza: mody, sloty, komenda `/hot`; sam przeładowywalny |
| `hotd` | demon w tle: git, CI, aktualizacje, kopie, hamulec MSPT |
| `hot-proxy` | proxy przed silnikiem; trzyma graczy przy podmianie i awarii silnika |
| `hotd serve-pack` | serwer HTTP paczki zasobów (dawniej planowany `hot-pack`) |
| `hot-play` | aplikacja gracza (terminal) |
| `hot-diag` | wspólne logi i raporty błędów |
| slot | z góry zarejestrowany pusty blok/przedmiot z `hot-slots.hmod`, któremu mod gorący nadaje nazwę, wygląd i zachowanie |
| podmiana (wariant A) | przejście graczy na nowy silnik bez rozłączenia i bez ekranu ładowania |
| MSPT | milisekundy na tick serwera; > 50 = gra laguje |
| repo serwera | repo na GitHubie z modami (`mods/`, `native/`, `hot.toml`); to nie jest to repo |

## 3. Mapa repo

```
crates/hot-abi/    granica C ABI (stabilna, wersjonowana ABI_MAJOR)
crates/hot-diag/   logi JSONL, kody błędów, panika w logu, szkielet hot report
crates/hot-sdk/    makro hot_mod!, Host<'call>, panika z miejscem w logu
crates/hot-core/   gospodarz modów gorących: hot load, generacje, powrót po panice
crates/hot-host/   mod natywny w silniku: ładuje hot-core, przekazuje haki; poza workspace
                   (build: crates/hot-host/build-bundle.sh + horizon-launcher native-build)
examples/licznik/  przykładowy mod gorący: licznik ticków, stan przez przeładowanie
examples/sklep/    przykład: komenda /sklep, monety, give, tell poza komendą
examples/teleport-blok/  przykład: klik w magnetyt, set_block, particles, run_later
crates/hotd/       demon w tle: repo serwera → hot-builds → hot load; data/ → paczki danych świata; assets/ → sloty i paczka zasobów; hotd status
scripts/           server-install.sh (instalator serwera na Ubuntu, D25); hot-build.sh i
                   hot-publish.sh (budowa modów w CI repo serwera, ADR 0008)
templates/server-repo/  pliki do repo serwera: hot.yml (kopiuje instalator) i README z układem repo dla autorów modów
crates/hot-proxy/  proxy: logowanie online, podpis Velocity dla silnika, przekazywanie ruchu (M8.1; podmiana silnika od M8.2)
crates/hot-play/   aplikacja gracza: menu, „Graj”, „Napraw” nad horizon-launcher (M11, ADR 0015)
docs/bledy/        katalog kodów błędów HOT-EXXXX (test: crates/hot-diag/tests/catalog.rs)
docs/adr/          decyzje (jedna decyzja = jeden plik)
docs/zmiany/       wpisy do changelogu z pull requestów
docs/STANDARD_KODU.md  komentarze, błędy i logi, testy — obowiązuje w każdym pliku
docs/operator.md   obsługa serwera: hot status, hot report, przywracanie z kopii   (planowany)
docs/spike/       wyniki prób z etapu M0 (zbiorczo: docs/adr/0002-spike.md)
spikes/           kod prób z M0; poza workspace, nie jest kodem produkcyjnym
PLAN.md            plan budowy (tymczasowy; po zakończeniu treść jest w docs/); stan prac w sekcji 8
```

## 4. Typowe zadania

- **Coś nie działa:** weź `hot report` (serwer) albo raport z „Napraw” (gracz). Każdy błąd ma kod `HOT-EXXXX` → opis i naprawa w `docs/bledy/HOT-EXXXX.md`, miejsce w kodzie w polu `source`.
- **Nowy mod gorący:** przewodnik `docs/przewodniki/nowy-mod.md` (planowany).
- **Nowy hak w ABI:** `docs/przewodniki/nowy-hak.md` (planowany). Zmiana niezgodna wstecz = podbicie `ABI_MAJOR`.
- **Nowy błąd:** nowy kod + wpis w `docs/bledy/` (sekcja 7).
- **Zmiana decyzji architektonicznej:** najpierw nowy ADR w `docs/adr/`, potem kod.
- **Nie wiesz, a dokumentacja nie odpowiada:** zapytaj właściciela; w sprawach z sekcji 6 nie zgaduj. Po wyjaśnieniu dopisz odpowiedź do dokumentacji.

## 5. Komendy

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # oczekiwane: wszystkie „ok”
# hot-host nie jest w workspace (buduje się tylko w silniku); format: rustfmt --edition 2021 --check crates/hot-host/api/src/lib.rs
crates/hot-host/build-bundle.sh <katalog> && horizon-launcher native-build <katalog> \
  --server-output <wyjście> --version nightly-2026-11-05-66f3981 --trust-native-code   # ok. 2 min
```

Próba `hotd` bez GitHuba: lokalne repo z `mods/<id>/`, `HOT_SIGNING_KEY_FILE=<klucz> scripts/hot-build.sh <repo> <out>`, `scripts/hot-publish.sh <repo> <out>`, potem `hotd once --home <HOT_HOME> --repo file://<origin.git> --engine <horizon-server> --allowed-signers <plik>`. Testy `cargo test -p hotd` potrzebują `git` i `ssh-keygen`.

Instalator serwera: `scripts/server-install.sh --repo URL --eula` jako root na Ubuntu x86_64 (kroki i kody `HOT-E0101`–`HOT-E0115` w nagłówku skryptu i w `docs/bledy/`; silnik na `127.0.0.1:25566` za `hot-proxy` na porcie graczy). Kod wyjścia 3 = czeka na właściciela: pliki gry (`--resources DIR`) albo zgoda na kod natywny (`--trust-native-code`; tej zgody agent nie daje). Próba bez Horizona w maszynie wirtualnej: `--test-no-horizon` (pomija pobranie Horizona, świat i silnik). Nie uruchamiaj go na Macu ani na maszynie z grą: zakłada użytkownika, usługi systemd i zaporę.

Kontener (ADR 0011, `docs/docker.md`): `Dockerfile`, `compose.yaml`, `scripts/container-entrypoint.sh`. Skrypt startowy powtarza kroki instalatora dotyczące danych i pilnuje procesów zamiast systemd. Zmieniasz krok w instalatorze, zmień go też tam; `crates/hot-proxy/tests/installer.rs` pilnuje w obu kolejności sekret → `127.0.0.1` → `online_mode = false`, portów i zgody na kod natywny. `HOT_TRUST_NATIVE_CODE` i `HOT_AUTO_UPDATE_HORIZON` w `compose.yaml` zostają zakomentowane, a instalatora nie uruchamiasz z `--auto-update-horizon` (ADR 0013: samodzielne budowanie nowych Nightly to osobna zgoda właściciela). Obrazu nie publikujemy w rejestrze bez zgody właściciela. Plików gry nie montujesz ani nie pobierasz za właściciela.

Gotowe = te trzy przechodzą + test, który by oblał bez Twojej zmiany + zaktualizowana dokumentacja w tym samym commicie.

Środowisko testowe silnika (Mac właściciela): launcher Horizon `/Applications/Horizon.app/Contents/MacOS/horizon-launcher`, źródła wersji w `~/Library/Application Support/horizon-launcher/native/stage`. Na innej maszynie zasoby gry przygotowuje człowiek, który ma kupionego Minecrafta (`horizon-launcher install` + `prepare`). Agent nie uruchamia `prepare` i nie pobiera plików Mojanga; jeśli ich brakuje, zatrzymuje się i prosi właściciela maszyny.

Do czego nie jest potrzebny silnik: `cargo test` w tym repo działa bez Horizona i bez plików gry. Próby z silnikiem opisuje sekcja „Powtórzenie” w każdym pliku `docs/spike/`.

## 6. Zasady nienaruszalne

- **Bez restartu serwera** przy zmianie modów i aktualizacjach. Rozwiązanie, które wymaga restartu, jest błędne — zobacz ADR o podmianie silnika w `docs/adr/`.
- **Kupiony Minecraft u każdego gracza.** Nie budujemy niczego, co omija sprawdzenie zakupu (konta offline jak TLauncher). Logowanie Microsoft raz, potem tryb offline Horizona — dozwolone.
- **Żadnych plików Mojanga** w repo, w CI, w aplikacji gracza (polityka Horizona `docs/microsoft-account.md`). Silnik budujemy tylko na serwerze, z zasobami przygotowanymi z konta właściciela.
- **Bez logo i marek** Minecrafta, Mojanga, Horizona jako naszych.
- **Świat z kopii przywraca tylko człowiek** (operator potwierdza). Żadnych automatycznych przywróceń ani kasowania świata.
- **Gra ma pierwszeństwo przed tłem** — praca w tle w `horizon-bg.slice`, pod hamulcem MSPT (D16).
- Płatności, publikacja na zewnątrz, działania nieodwracalne → pytanie do właściciela.
- Sekretów nie commitujesz. Sekret w historii = rotacja klucza.
- **Zrzutów ekranu z gry nie commitujesz** (tekstury i logo należą do Mojanga). Zostają poza repo.
- **Nie ruszasz cudzych instancji, serwerów ani światów** na maszynie, na której pracujesz, ani globalnego `servers.dat` launchera. Próby robisz we własnej instancji i we własnym katalogu, a po próbie usuwasz tylko to, co sam utworzyłeś.
- `--trust-native-code` tylko dla kodu z tego repo albo kodu, który przeczytałeś. Nigdy dla moda z zewnątrz bez zgody właściciela.

## 7. Logi i błędy (D27, D28)

- Loguj **tylko przez `hot-diag`**. Każdy błąd ma kod `HOT-EXXXX`, pola `what`, `expected`, `context`, `cause`, `fix`, `doc`, `source`, `chain`, `state` — tak, żeby model bez wiedzy o projekcie naprawił błąd z samego wpisu.
- Nowy błąd = stała `const X: Code = Code::new("HOT-EXXXX")` w module `codes` składnika + plik `docs/bledy/HOT-EXXXX.md` (sekcje jak w `docs/bledy/README.md`). Test `crates/hot-diag/tests/catalog.rs` sprawdza obie strony: kod w `crates/` bez pliku i plik bez kodu dają czerwone CI. Uwaga: skaner widzi też kody w komentarzach, więc w przykładach używaj tylko istniejących kodów.
- Wpis: `DiagError::new(code, what, fix)` (bez tych trzech pól kod się nie kompiluje), potem `.context()`, `.caused_by()`, `.state()`; zapis: `Logger::error`. Panika: `install_panic_hook` zaraz po otwarciu loggera.
- Raport do ręcznej diagnozy (do czasu `hotd`): `cargo run -p hot-diag -- report --logs <katalog logów> [--secret-file <plik>]... [--out raport.md]`.
- Zakazane: `unwrap()`/`expect()` poza testami, komunikaty bez kontekstu („failed”), połykanie błędów.
- Debugujesz zgłoszenie? Zacznij od `hot report` (serwer) albo raportu z „Napraw” (gracz): mają opis systemu, wersje, błędy z wpisami z katalogu i logi.

## 8. Kod i git

- Rust stable (`rust-toolchain.toml`), edycja 2021. Nazwy po angielsku. Komentarze `PL:` + `EN:`, tylko „dlaczego”; na górze każdego `lib.rs`/`main.rs`: do czego crate służy, kto go używa. Szczegóły: `docs/STANDARD_KODU.md`.
- Nowa funkcja = nowy moduł. Plik > 600 linii → podziel. Bez martwego kodu i `println!` do debugowania.
- Nowa zależność → najpierw sprawdź, czy jest już w workspace; nowa = wpis w ADR.
- Conventional Commits, temat w formie bezosobowej po polsku, 10–72 znaki, np. `feat(hot-core): ładowanie generacji poza wątkiem serwera`. Hook `.githooks/commit-msg` pilnuje formatu (`git config core.hooksPath .githooks`); CI sprawdza to samo.
- Autor commitów: Szymon Sosnowski (ustawione w git). Bez stopek i dopisków o AI, bez `Co-Authored-By`.
- Gałęzie `feat/…`, `fix/…`, `docs/…`; bez `--force` na wspólnych gałęziach.
- Jedno zadanie = jedna gałąź = jeden pull request; ruszasz tylko swój składnik.
- Wpis do changelogu w `docs/zmiany/<gałąź>.md` (nie w `CHANGELOG.md` — `docs/zmiany/README.md`).
- Zmiana zachowania → `README.md`, `docs/ARCHITECTURE.md` przy nowym module, przewodniki — w tym samym PR.
- Repo: https://github.com/Kamciosz/horizon-hot. `main` chroniony, scala właściciel po zielonym CI.
