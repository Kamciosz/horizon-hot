# Serwer w Dockerze

Alternatywa dla instalatora na Ubuntu. Decyzja i ograniczenia: `docs/adr/0011-kontener-docker.md`.

## Co jest potrzebne

- Docker z Compose v2 na komputerze x86-64 (Linux). Na Macu z Apple Silicon obraz działa przez emulację Rosetta, wolniej.
- Repo serwera z modami (jak przy instalatorze: `templates/server-repo/`).
- Folder 26.3 przygotowany z gry właściciela w launcherze Horizon (`prepare`). Obraz go nie zawiera i nigdy nie zawierał.

## Pierwsze uruchomienie

```sh
git clone https://github.com/Kamciosz/horizon-hot.git && cd horizon-hot
cp -R <przygotowany folder 26.3> ./game-files     # folder jest w .gitignore
# w compose.yaml uzupełnij HOT_REPO, HOT_ADDRESS, HOT_EULA ("true" po przeczytaniu EULA), HOT_OP
docker compose up -d --build
docker compose logs -f server
```

Pierwsza budowa obrazu kompiluje też Horizona ze źródeł w przypiętej rewizji, więc trwa długo: kilkadziesiąt minut na zwykłym komputerze x86-64, dłużej w emulacji. Kolejne budowy korzystają z pamięci podręcznej. Na końcu logu kontener wypisuje:
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

Bez tego serwer działa, a w logu jest ostrzeżenie `HOT-E0110`. Ta ścieżka w kontenerze nie była jeszcze sprawdzona. Znany problem: launcher pobiera wersję Horizona po tagu Nightly, a datowane tagi znikają z GitHuba po kilku dniach (do rozwiązania w M9).

## Codzienna obsługa

- Stan: `docker compose ps` (kolumna health: status przez proxy, jak lista serwerów w grze).
- Logi: `docker compose logs server`, błędy w formacie D27: `/srv/horizon/logs/container.jsonl`.
- Komenda konsoli: `docker compose exec -u horizon server /opt/horizon/horizon-server --dir /srv/horizon/server cmd "list"`.
- Zatrzymanie: `docker compose stop`. Kolejność: `hotd`, silnik (zapisuje świat), proxy, paczka.
- Aktualizacja horizon-hot: `git pull && docker compose up -d --build`. Świat i klucze zostają w wolumenie.

## Kody wyjścia

- `3`: czeka na właściciela (`HOT-E0115` repo, `HOT-E0108` EULA, `HOT-E0112` adres, `HOT-E0107` pliki gry). Uzupełnij `compose.yaml` i `docker compose up -d`.
- `1`: błąd; opis w `docs/bledy/<kod>.md`.
