# Serwer w Dockerze

Alternatywa dla instalatora na Ubuntu. Decyzja i ograniczenia: `docs/adr/0011-kontener-docker.md`.

## Co jest potrzebne

- Docker z Compose v2 na Linuksie, x86-64 albo ARM64. Na ARM64 wpisz `platform: linux/arm64` w `compose.override.yaml` (pod `services.server`); serwer działa wtedy bez modów gorących (ADR 0034). Na Macu z Apple Silicon domyślny obraz x86-64 działa przez emulację Rosetta, wolniej.
- Repo serwera z modami (jak przy instalatorze: `templates/server-repo/`).
- Folder 26.3 przygotowany z gry właściciela w launcherze Horizon (`prepare`). Obraz go nie zawiera i nigdy nie zawierał.

## Pierwsze uruchomienie

```sh
git clone https://github.com/Kamciosz/horizon-hot.git && cd horizon-hot
cp -R <przygotowany folder 26.3> ./game-files     # folder jest w .gitignore
# uzupełnij HOT_REPO, HOT_ADDRESS, HOT_EULA ("true" po przeczytaniu EULA) i HOT_OP: w compose.yaml albo w compose.override.yaml
docker compose up -d --build
docker compose logs -f server
```

Ustawienia w `compose.override.yaml` (git go pomija, Compose łączy go z `compose.yaml`) nie blokują `git pull` przy aktualizacji. Wdrożenie przez agenta AI krok po kroku, z naprawami typowych problemów: `docs/przewodniki/wdrozenie-docker.md`.

Pierwsza budowa obrazu kompiluje też Horizona ze źródeł w przypiętej rewizji, więc trwa długo: kilkadziesiąt minut na zwykłym komputerze x86-64, dłużej w emulacji. Kolejne budowy korzystają z pamięci podręcznej. Gdy przy małej pamięci kompilacja pada z `signal: 9`, zbuduj z mniejszą liczbą zadań naraz: `docker compose build --build-arg CARGO_BUILD_JOBS=2`. Na końcu logu kontener wypisuje:
- adres dla graczy i adres paczki zasobów;
- ścieżkę `hot.yml` i klucza `hot-ci` do repo serwera (raz, jak przy instalatorze);
- przy repo prywatnym klucz tylko do odczytu do Settings → Deploy keys.

Pliki są w wolumenie: `docker compose exec server ls /srv/horizon`. Klucz CI wyciągniesz tak: `docker compose exec server cat /srv/horizon/keys/hot-ci`.

## Porty

| Port | Co | Na zewnątrz |
|---|---|---|
| 25565 | `hot-proxy`, kontrola zakupu gry | tak |
| 8131 | paczka zasobów z pulą slotów | tak |
| 25566 | silnik, tylko `127.0.0.1` w kontenerze | nie |

## Mody gorące

Silnik z `hot-host` to kod natywny bez piaskownicy. Buduje go launcher Horizona (`native-build`) i włącza tylko właściciel:
1. przeczytaj `crates/hot-host`;
2. połóż launcher Horizona dla Linux x86-64 w wolumenie, np. `/srv/horizon/launcher/horizon-launcher`;
3. odkomentuj `HOT_TRUST_NATIVE_CODE` i `HOT_LAUNCHER` w `compose.yaml`, potem `docker compose up -d`.

Bez tego serwer działa, a w logu jest ostrzeżenie `HOT-E0110`. Ta ścieżka w kontenerze nie była jeszcze sprawdzona. Na ARM64 ostrzeżenie `HOT-E0110` pojawia się zawsze, bo launchera Horizona dla Linux ARM64 nie ma (ADR 0034). Silnik budowany jest z pełnej rewizji Horizona, więc znikające datowane wydania mu nie przeszkadzają (ADR 0012).

Nowe Nightly Horizona: kontener sprawdza je co godzinę i pokazuje w `hot status`. Żeby sam je budował, testował na kopii świata i podmieniał, odkomentuj dodatkowo `HOT_AUTO_UPDATE_HORIZON` (ADR 0013, opis kroków w `docs/aktualizacje.md`).

## Codzienna obsługa

- Stan: `docker compose ps` (kolumna health: status przez proxy, jak lista serwerów w grze).
- Logi: `docker compose logs server`, błędy w formacie D27: `/srv/horizon/logs/container.jsonl`.
- Stan po polsku (`hot status`): `docker compose exec -u horizon server /opt/horizon-hot/bin/hotd status --home /srv/horizon/server`.
- Raport dla modelu AI (`hot report`): `docker compose exec -T -u horizon server /opt/horizon-hot/bin/hotd report --home /srv/horizon/server > raport.md` (`-T`: bez terminala, więc w pliku nie ma znaków `\r`).
- Komenda konsoli: `docker compose exec -u horizon server /opt/horizon/horizon-server --dir /srv/horizon/server cmd "list"`.
- Zatrzymanie: `docker compose stop`. Kolejność: `hotd`, silnik (zapisuje świat), proxy, paczka.
- Aktualizacja horizon-hot: `git pull && docker compose up -d --build`. Świat i klucze zostają w wolumenie. Kontener sam się nie aktualizuje, bo programy są w obrazie; na Ubuntu robi to `hotd upgrade` (ADR 0014).

## Kody wyjścia

- `3`: czeka na właściciela (`HOT-E0115` repo, `HOT-E0108` EULA, `HOT-E0112` adres, `HOT-E0107` pliki gry). Uzupełnij `compose.yaml` i `docker compose up -d`.
- `1`: błąd; opis w `docs/bledy/<kod>.md`.
