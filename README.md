# horizon-hot

Serwer Horizon (Minecraft Java 26.3 napisany w Rust), który bierze mody z repozytorium git i wczytuje je w trakcie gry, bez restartu serwera. Dla małej grupy znajomych na własnym sprzęcie.

Stan: wczesny etap. Działa szkielet repozytorium; składniki powstają krok po kroku (`docs/ARCHITECTURE.md`).

Pracujesz nad projektem (człowiek albo agent AI)? Zacznij od `AGENTS.md`.

## Wymagania
- Rust stable (edycja 2021), `cargo`
- Do prób z silnikiem: launcher Horizon (`horizon-launcher`) z przygotowaną wersją Nightly

## Instalacja na serwerze (Ubuntu x86_64)

Potrzebne: Ubuntu 22.04 albo 24.04 na x86_64, dostęp `sudo`, repo serwera na GitHubie (wzór: `templates/server-repo/`) i własny, kupiony Minecraft Java u właściciela serwera.

Dopóki `spike/m0` nie jest scalone do `main`, instalator bierze się z tej gałęzi:

```bash
curl -fsSL https://raw.githubusercontent.com/Kamciosz/horizon-hot/spike/m0/scripts/server-install.sh \
  | sudo bash -s -- --hot-ref spike/m0 --repo https://github.com/<właściciel>/<repo-serwera> --address <IP-serwera> --eula --op <nick>
```

Skrypt zatrzymuje się kodem 3 w dwóch miejscach i wypisuje polecenie do powtórki:

1. Pliki gry. Na komputerze z Minecraftem i launcherem Horizon przygotuj wersję 26.3, skopiuj jej folder na serwer (`scp -r`) i uruchom polecenie jeszcze raz z `--resources <folder>`. Pliki gry nie są w tym repo i nikt ich za Ciebie nie pobiera.
2. Kod natywny. Przeczytaj `crates/hot-host/` i, jeśli mu ufasz, dodaj `--trust-native-code`. Tę zgodę daje tylko właściciel serwera.

Nowe wersje Horizona serwer sprawdza sam co godzinę. Domyślnie tylko pokazuje je w `hot status`. Z `--trust-native-code --auto-update-horizon` sam je buduje, testuje na kopii świata z botami, podmienia, gdy nikt nie gra (najpóźniej po 24 h), i wraca na starą wersję, gdy nowa padnie albo zwolni (`docs/aktualizacje.md`).

Sam horizon-hot serwer aktualizuje raz na dobę z gałęzi podanej w `--hot-ref`: poprawka `hot-core` wchodzi od razu, bez restartu, a reszta, gdy nikt nie gra. Zmiana silnika albo instalatora czeka na właściciela i `hot status` mówi, co zrobić. Tag albo commit w `--hot-ref` wyłącza aktualizacje (ADR 0014).

`--address` to adres, który gracze wpisują w grze. Z niego gra pobiera paczkę zasobów z wyglądem bloków modów (port 8131).

Gracze łączą się z portem 25565 (`--port`). Słucha na nim `hot-proxy`: sprawdza w usłudze sesji Microsoftu, czy gracz ma kupioną grę, i dopiero wtedy wpuszcza go do silnika. Silnik słucha tylko na `127.0.0.1:25566` (`--engine-port`; druga strona silnika przy podmianie na 25567), więc z sieci nie da się go ominąć. Zapora otwiera tylko SSH, 25565 i 8131.

Na końcu skrypt pokazuje, co raz dodać w repo serwera: `hot.yml` w `.github/workflows/` i sekret `HOT_SIGNING_KEY`. Błędy idą do `/srv/horizon/logs/install.jsonl`, opisy kodów są w `docs/bledy/`. Ponowne uruchomienie niczego nie psuje.

Odinstalowanie: `sudo bash server-install.sh --uninstall` pokazuje, co zniknie, a z `--yes` to usuwa. Świat, kopie, logi i użytkownik `horizon` zostają, a stan modów trafia najpierw do `/srv/horizon/backups/`.

## Gracze

Każdy gracz potrzebuje konta Microsoft z kupionym Minecraft: Java Edition. Aplikację `hot-play` instaluje jedną linijką w terminalu (`ADRES` to adres serwera, np. `serwer.pl`):

```bash
# macOS i Linux
curl -fsSL https://github.com/Kamciosz/horizon-hot/releases/download/hot-play/install.sh | sh -s -- ADRES
```

```powershell
# Windows (PowerShell)
irm https://github.com/Kamciosz/horizon-hot/releases/download/hot-play/install.ps1 -OutFile $env:TEMP\hot-play-install.ps1
powershell -ExecutionPolicy Bypass -File $env:TEMP\hot-play-install.ps1 ADRES
```

Na pulpicie pojawia się skrót „Graj”. Menu ma trzy pozycje: „Graj” (instaluje i naprawia launcher Horizona, wersję gry, plik bloków serwera i listę serwerów, za pierwszym razem prosi o logowanie, potem łączy z serwerem), „Napraw” (sprawdza wszystko od nowa i zapisuje na pulpicie `hot-play-raport.md` do wysłania właścicielowi) i „Wyjdź”. Aplikacja sama się aktualizuje przy starcie (ADR 0016). Bez pliku bloków serwera (`hot_slots.hmod`) serwer nie wpuści gracza („Your mods don't match”), dlatego „Graj” zawsze go sprawdza.

Wydanie `hot-play` na GitHubie powstaje dopiero po pierwszym merge'u na `main`: wcześniej linki wyżej nie działają, a pliki są tylko w artefaktach przebiegów workflow `hot-play`.

## Testy
```bash
git config core.hooksPath .githooks   # raz po klonie
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # oczekiwane: wszystkie testy „ok”
```

## Struktura
```
crates/hot-abi/   — granica C ABI między gospodarzem w silniku a modami gorącymi
crates/hot-diag/  — logi JSONL z kodami błędów i hot report
crates/hot-sdk/   — biblioteka dla autorów modów gorących (hot_mod!)
crates/hot-core/  — gospodarz modów gorących w silniku (hot load, generacje)
crates/hot-host/  — mod natywny w silniku, który ładuje hot-core
crates/hotd/      — demon serwera: mody z CI, podmiana silnika, aktualizacje, hamulec, hotd status/report
crates/hot-proxy/ — wejście graczy: kontrola zakupu, trzymanie połączeń przy podmianie silnika
crates/hot-play/  — aplikacja gracza: instalacja, naprawa i start gry
templates/server-repo/ — wzór repo serwera z modami (README, hot.yml)
scripts/          — instalator serwera (server-install.sh), budowa i publikacja modów w CI repo serwera
examples/licznik/, examples/sklep/, examples/teleport-blok/ — przykładowe mody gorące
docs/             — architektura, decyzje (ADR), standard kodu, katalog błędów
PLAN.md           — plan budowy i dziennik prac (historia); decyzje są w docs/adr/
```

## Uruchomienie w Dockerze

Zamiast instalatora można użyć kontenera: `compose.yaml` w katalogu głównym. Uzupełnij w nim `HOT_REPO`, `HOT_ADDRESS`, `HOT_EULA` i `HOT_OP`, skopiuj przygotowany folder 26.3 do `./game-files` i uruchom `docker compose up -d --build`. Na zewnątrz wychodzą tylko porty 25565 (proxy z kontrolą zakupu) i 8131 (paczka zasobów). Obraz nie zawiera plików gry. Szczegóły: `docs/docker.md`.

## Dokumentacja
- `AGENTS.md` — wejście dla nowej osoby i agenta AI: słownik, zasady, typowe zadania
- `docs/ARCHITECTURE.md` — jak to jest zbudowane i dlaczego
- `docs/STANDARD_KODU.md` — komentarze, błędy i logi, testy
- `docs/docker.md` — serwer w kontenerze Docker
- `docs/operator.md` — instrukcja dla osoby, u której stoi serwer
- `docs/przewodniki/` — pierwszy mod, nowy hak, nowy kod błędu, wydanie
- `docs/bledy/` — katalog kodów błędów `HOT-EXXXX`
- `docs/adr/` — decyzje projektowe
- `CONTRIBUTING.md` — jak pracujemy (gałęzie, commity, pull requesty)
- `SECURITY.md` — zgłaszanie luk

## Licencja
MIT, zobacz `LICENSE`.
