# Wdrożenie w Dockerze przez agenta AI

Dla agenta AI albo człowieka, który na prośbę właściciela stawia horizon-hot na jego serwerze przez SSH, w Dockerze. Cel: działający serwer bez kontaktu z autorami projektu. Właściciel podaje dane i zgody na początku; poza warunkami stopu niżej agent go o nic nie pyta.

Podstawy kontenera są w `docs/docker.md`, kody błędów w `docs/bledy/`, zasady projektu w `AGENTS.md`.

## Gotowe, gdy

1. `docker compose ps` pokazuje `healthy`.
2. Po linii `Gracze: <adres>:25565 …` log kontenera nie ma błędów.
3. Spoza sieci serwera port 25565 odpowiada, a `http://<adres>:8131/hot-play.json` zwraca JSON. Gdy to zależy od routera albo panelu dostawcy, do których nie masz dostępu, wystarczy raport z dokładnymi ustawieniami dla właściciela (krok 9).
4. `docker compose ps` pokazuje tylko porty 25565 i 8131.
5. Gracze z listy właściciela są na białej liście.
6. Właściciel dostał raport (krok 11).

Pierwsze wejście do gry wymaga konta Microsoft właściciela, więc robi je on sam, po raporcie.

## Zasady

- Kontrola zakupu zostaje. Nie zmieniasz `online_mode`, nie dodajesz `--offline` i nie publikujesz portu 25566.
- Pliki gry kopiujesz z komputera właściciela, i tylko folder, który właściciel sam przygotował w launcherze Horizona. Nie uruchamiasz `prepare`, nie logujesz się na jego konto i nie pobierasz plików Mojanga. Pliki nie trafiają do repo ani do obrazu.
- Kod natywny (`HOT_TRUST_NATIVE_CODE`, `HOT_AUTO_UPDATE_HORIZON`) włączasz tylko wtedy, gdy właściciel napisał „tak”. Sam tej zgody nie dajesz.
- Świat jest w wolumenie `horizon-hot`. Nie uruchamiasz `docker compose down -v`, nie kasujesz wolumenu i nie przywracasz kopii bez polecenia.
- Klucze (`/srv/horizon/keys/hot-ci`, `forwarding.secret`, klucz deploy) dostaje tylko właściciel. Nie wklejasz ich do repo, zgłoszeń ani czatów.
- Usuwasz tylko to, co sam utworzyłeś.

## Dane od właściciela

| Dane | Gdzie trafiają | Gdy brak |
|---|---|---|
| SSH do serwera i czy jest `sudo` | — | stop |
| adres dla graczy: domena albo publiczne IP, bez portu | `HOT_ADDRESS` | publiczne IP serwera, jeśli właściciel na to pozwolił, inaczej stop |
| repo serwera z modami (wzór: `templates/server-repo/`) | `HOT_REPO` | stop |
| akceptacja EULA Minecrafta (https://aka.ms/MinecraftEULA) | `HOT_EULA: "true"` | stop |
| nick operatora | `HOT_OP` | serwer bez białej listy |
| nicki pozostałych graczy | biała lista (krok 8) | tylko operator |
| folder z przygotowanymi plikami gry | `./game-files` | szukasz go sam (krok 4) |
| zgody: `sudo` do instalacji, zapora, kod natywny | — | bez zgody nie robisz |

## Warunki stopu

Zatrzymaj się i napisz właścicielowi, co ma zrobić, tylko gdy:

- brakuje danej, przy której tabela mówi „stop”;
- następny krok wymaga zgody, której właściciel nie dał;
- serwer nie jest x86_64;
- budowa nie może pobrać przypiętej rewizji Horizona (`66f3981…`), bo zniknęła z GitHuba;
- problem nie ustąpił po trzech próbach i blokuje dalsze kroki.

Pierwsze trzy sprawdzasz w kroku 1, póki właściciel jest jeszcze przy komputerze. Resztę naprawiasz sam według tabeli objawów i `docs/bledy/`. Przekierowanie portów na routerze albo w panelu dostawcy, do których nie masz dostępu, i łącze za CGNAT nie są powodem do stopu: kończysz pozostałe kroki, a co trzeba ustawić, wpisujesz do raportu (krok 9).

## Kroki

### 1. Rozpoznanie

Zanim cokolwiek zainstalujesz, sprawdź wszystko, co może wymagać właściciela: dane z tabeli, dostęp przez SSH i `sudo`, pliki gry na jego komputerze (wyszukiwanie z kroku 4) i serwer:

```sh
uname -m                                   # musi być x86_64
. /etc/os-release && echo "$PRETTY_NAME"
nproc && free -h && df -h /
docker --version; docker compose version; docker buildx version
ss -ltnp | grep -E ':(25565|8131) ' || echo 'porty wolne'
```

Braki zgłoś od razu, w jednej wiadomości. Potem właściciel nie jest już potrzebny aż do raportu.

Sprawdzone 15.11.2026: obraz zbudował się na 4 rdzeniach i 8 GiB pamięci, w emulacji x86-64 na Macu, a kontener wystartował na atrapie plików gry (z prawdziwymi plikami w Dockerze nikt go jeszcze nie uruchamiał). Mniejszej pamięci nikt nie sprawdzał; gdy kompilacja pada, pomaga `CARGO_BUILD_JOBS` (tabela objawów). Na dysku potrzeba co najmniej 10 GB wolnego miejsca: przy tej budowie pamięć podręczna Dockera zajęła ok. 7 GB, obraz ok. 0,4 GB, pliki gry zajmują ok. 0,6 GB, a do tego dochodzi świat.

### 2. Docker

Brak Dockera: zainstaluj Docker Engine według https://docs.docker.com/engine/install/ dla tej dystrybucji (potrzebna zgoda na `sudo`). Docker już jest, a brakuje Compose v2 albo buildx: doinstaluj tylko brakującą wtyczkę (w Ubuntu pakiety `docker-compose-v2` i `docker-buildx`, inaczej według tej samej strony). Działającego Dockera nie odinstalowujesz, bo właściciel może mieć w nim inne kontenery. Bez buildx `docker compose` przechodzi na stary builder i budowa pada, bo Dockerfile używa `RUN --mount`. Jeśli użytkownik SSH nie jest w grupie `docker`, poprzedzaj polecenia `sudo`.

### 3. Kod

```sh
git clone https://github.com/Kamciosz/horizon-hot.git ~/horizon-hot
cd ~/horizon-hot
```

Wszystkie dalsze polecenia `docker compose` uruchamiasz w tym folderze.

### 4. Pliki gry

Na komputerze właściciela znajdź folder `26.3` z plikiem `horizon-resources.json`:

```sh
# macOS
find ~/Library/Application\ Support/horizon-launcher -name horizon-resources.json 2>/dev/null
# Linux
find ~ -name horizon-resources.json -path '*26.3*' 2>/dev/null
```

```powershell
# Windows
Get-ChildItem $env:APPDATA, $env:LOCALAPPDATA -Recurse -Force -Filter horizon-resources.json -ErrorAction SilentlyContinue | Select-Object FullName, LastWriteTime
```

Na Windows bez wyniku przeszukaj cały `$HOME`. Każda wersja w launcherze ma własny folder, więc trafień bywa kilka: weź najnowszy. Brak trafień znaczy, że właściciel nie przygotował plików (stop). Napisz mu, żeby zalogował się w launcherze Horizona kontem z kupionym Minecraftem i raz uruchomił grę w wersji 26.3 albo wpisał w terminalu `horizon-launcher prepare --download`. Sam tego nie robisz.

Folder ma ok. 0,6 GB i 28 tys. plików, więc kopiuj go jednym archiwum (`tar` i `scp` są też w Windows 10 i 11):

```sh
tar -C "<folder, w którym leży 26.3>" -czf ~/gra-26.3.tgz 26.3   # na macOS: COPYFILE_DISABLE=1 tar …
scp ~/gra-26.3.tgz <serwer>:horizon-hot/
ssh <serwer> 'cd horizon-hot && mkdir -p game-files && tar -xzf gra-26.3.tgz -C game-files --strip-components=1 && rm gra-26.3.tgz && chmod -R a+rX game-files && ls game-files/horizon-resources.json'
```

W PowerShellu zamiast `~` wpisz `$HOME`: programy `tar` i `scp` nie rozwijają tyldy. Potem usuń archiwum z komputera właściciela. `horizon-resources.json` ma leżeć bezpośrednio w `game-files`; kontener sprawdza to przy starcie, a przy błędzie podaje `HOT-E0107` z poleceniem naprawy.

### 5. Ustawienia

Ustawienia wpisz do `compose.override.yaml`, a `compose.yaml` zostaw bez zmian. Compose sam łączy oba pliki, a git pomija ten drugi, więc `git pull` przy aktualizacji niczego w nim nie zmieni.

```sh
cat > compose.override.yaml <<'EOF'
services:
  server:
    environment:
      HOT_REPO: "https://github.com/<właściciel>/<repo-serwera>"
      HOT_ADDRESS: "<adres bez portu>"
      HOT_EULA: "true"
      HOT_OP: "<nick operatora>"
EOF
docker compose config | grep -E 'HOT_|published'
```

Sprawdź w wyniku, czy cztery wartości `HOT_` są wypełnione, a `published` pokazuje tylko 25565 i 8131. `HOT_EULA: "true"` wpisujesz tylko wtedy, gdy właściciel zaakceptował EULA.

### 6. Budowa

Budowa kompiluje Horizona ze źródeł, więc trwa długo. Na Macu z 4 rdzeniami, w emulacji x86-64, trwała od 32 minut (15.11.2026) do ponad 2 godzin (13.11.2026). Na serwerze czas zależy od rdzeni i pamięci; tam nikt go jeszcze nie mierzył. Uruchom ją tak, żeby przeżyła rozłączenie SSH:

```sh
tmux new -d -s budowa 'docker compose build > budowa.log 2>&1; echo "koniec: $?" >> budowa.log'
tail -f budowa.log
```

Bez `tmux`: `nohup sh -c 'docker compose build > budowa.log 2>&1; echo "koniec: $?" >> budowa.log' &`. Budowa się udała, gdy ostatnia linia to `koniec: 0`. Inny kod: tabela objawów.

### 7. Start

```sh
docker compose up -d
docker compose logs -f server       # Ctrl+C kończy podgląd, serwer działa dalej
```

Log poprawnego startu: „Świat utworzony” (tylko za pierwszym razem), ostrzeżenie `HOT-E0110`, adresy dla graczy i paczki, ścieżki do repo serwera, potem `engine: pid`, `proxy: pid` i `pack: pid`. `HOT-E0110` bez zgody na kod natywny jest spodziewane: serwer działa, tylko bez modów gorących.

Kod wyjścia 3 (`docker compose ps -a` pokazuje `Exited (3)`, a Docker uruchamia kontener w kółko) znaczy, że czegoś brakuje. Kod i naprawa są w logu. Gdy kontener działa, ale co kilka sekund wypisuje `HOT-E0608`, silnik pada przy starcie, a przyczynę podaje dopiero `supervisor.log` (tabela objawów).

Pierwszy start czyta pliki gry i tworzy świat. Na wolnym dysku `docker compose ps` może przez kilka minut pokazywać `starting` albo `unhealthy`; dopóki w logu nie ma błędu, czekaj. Bez `healthy` po 15 minutach przejdź do pętli naprawy.

### 8. Biała lista

Z ustawionym `HOT_OP` serwer wpuszcza tylko graczy z listy (operator jest na niej od startu). Dodaj każdego gracza osobno, gdy silnik działa:

```sh
docker compose exec -u horizon server /opt/horizon/horizon-server --dir /srv/horizon/server cmd "whitelist add <nick>"
```

Tylko przez `cmd`. Według źródeł Horizona `horizon-server … whitelist add` bez `cmd` zapisuje identyfikator offline, a proxy przekazuje silnikowi prawdziwy identyfikator konta, więc gracz by nie wszedł.

### 9. Sieć

Docker sam otwiera opublikowane porty, także przy włączonym `ufw`. Gdy z zewnątrz nie działa, przyczyna zwykle leży przed serwerem:

- serwer w domu: na routerze przekieruj TCP 25565 i 8131 na lokalny adres serwera (`hostname -I`). Bez dostępu do routera wpisz do raportu ten adres i oba porty: właściciel ustawi to sam;
- VPS: otwórz te dwa porty w zaporze w panelu dostawcy, a bez dostępu do panelu wpisz to do raportu;
- CGNAT: adres WAN routera różni się od publicznego IP albo leży w 100.64.0.0/10, więc przekierowanie nie zadziała. Wybór (publiczne IP od dostawcy internetu, VPS) należy do właściciela: opisz go w raporcie.

Sprawdzenie z zewnątrz:

- komputer właściciela w innej sieci niż serwer: `nc -vz <adres> 25565` i `curl -s http://<adres>:8131/hot-play.json`;
- ta sama sieć: router bez „hairpin NAT” nie łączy z własnym publicznym adresem, więc taki test niczego nie dowodzi. Za zgodą właściciela sprawdź port gry przez publiczny serwis, np. `curl -s https://api.mcsrvstat.us/3/<adres>` (szukaj `"online":true`). Bez zgody poproś w raporcie o test z telefonu bez Wi-Fi: strona `http://<adres>:8131/hot-play.json` ma pokazać JSON.

### 10. Mody gorące (tylko za zgodą)

Bez zgody na kod natywny pomijasz ten krok. Serwer działa wtedy jak zwykły serwer Horizona z kontrolą zakupu, a repo serwera czeka na później. Ze zgodą: `docs/docker.md`, sekcja „Mody gorące”, a w repo serwera jednorazowo `hot.yml` i sekret `HOT_SIGNING_KEY` (ścieżki wypisuje log). Tej drogi w kontenerze nikt jeszcze nie sprawdził na prawdziwym serwerze.

### 11. Raport dla właściciela

Krótko i bez sekretów:

- adres dla graczy i polecenie instalacji `hot-play` z tym adresem (README, sekcja „Gracze”);
- wynik `docker compose ps` i sprawdzenia z zewnątrz;
- kto jest na białej liście;
- co zostało dla właściciela: pierwsze wejście do gry, przekierowanie portów, test z telefonu, mody gorące;
- sprzęt i czas budowy: `lscpu | head -20`, `free -h`, nazwa systemu, `df -h /var/lib/docker`;
- lokalne poprawki z `LOKALNE-POPRAWKI.md` i problemy, których nie udało się naprawić, z listą prób.

## Pętla naprawy

1. Odczytaj błąd: `docker compose logs --tail 200 server`. Błędy kontenera mają format D27 (`code`, `what`, `cause`, `fix`, `source`) i trafiają też do `/srv/horizon/logs/container.jsonl` w wolumenie.
2. Kod `HOT-EXXXX` opisuje `docs/bledy/HOT-EXXXX.md`, sekcja „Naprawa”. Pole `source` wskazuje funkcję w kodzie.
3. Szerszy obraz: `docker compose exec -T -u horizon server /opt/horizon-hot/bin/hotd report --home /srv/horizon/server > raport.md` (sekrety są zamaskowane).
4. Jedna zmiana naraz, potem `docker compose up -d` i znowu log.
5. Najwyżej trzy próby na jeden problem. Potem zapisz, co próbowałeś, i idź dalej, jeśli problem nie blokuje reszty; jeśli blokuje, to warunek stopu.

## Objawy i naprawy

| Objaw | Przyczyna | Naprawa |
|---|---|---|
| `buildx … not found: falling back to the classic builder`, potem błąd budowy | brak wtyczki buildx | zainstaluj buildx z pakietów Dockera, zbuduj jeszcze raz |
| `signal: 9`, `SIGKILL` albo `Killed` przy kompilacji | brak pamięci | `docker compose build --build-arg CARGO_BUILD_JOBS=2`, potem `1`; swap za zgodą na `sudo`; dalej pada: sekcja „Budowa poza serwerem” |
| `no space left on device` | pełny dysk | `docker system df`; usuń tylko to, co sam utworzyłeś, inaczej stop |
| `git fetch` z `66f3981…` kończy się `not our ref` albo `couldn't find remote ref` | rewizji Horizona nie ma na GitHubie | stop: to wymaga zmiany w horizon-hot |
| `Could not resolve host`, przekroczony czas pobierania | sieć albo DNS serwera | powtórz; sprawdź `curl -sI https://github.com` |
| `exec format error`, `no matching manifest` | serwer nie jest x86_64 | stop |
| `Exited (3)` z `HOT-E0115`, `HOT-E0108` albo `HOT-E0112` | brak repo, EULA albo adresu | uzupełnij `compose.override.yaml`, `docker compose up -d` |
| `HOT-E0107`, „not mounted” | pusty `./game-files` | krok 4 |
| `HOT-E0107`, „one level too deep” | folder trafił do środka `game-files` | polecenie `mv` z logu |
| `HOT-E0107`, „not readable” | prawa tylko dla właściciela pliku albo SELinux | `chmod -R a+rX game-files`; przy SELinux (np. Fedora) w `compose.override.yaml` dodaj `volumes: ["./game-files:/game-files:ro,z"]` |
| co kilka sekund `HOT-E0608 … see /srv/horizon/server/logs/supervisor.log`, potem `engine zakończył się; wraca za 5 s.` | silnik pada przy starcie | `docker compose exec server tail -40 /srv/horizon/server/logs/supervisor.log`, dalej według komunikatu silnika |
| w `supervisor.log`: `the prepared game files in /game-files are incomplete or damaged` | niepełna kopia | skopiuj cały folder jeszcze raz (krok 4); polecenia `horizon-prepare` z komunikatu nie uruchamiasz |
| ten sam błąd po pełnej kopii | pliki przygotował launcher nowszy niż silnik w obrazie | sekcja „Wersja Horizona” |
| `port is already allocated`, `address already in use` | port zajmuje inny program | `ss -ltnp`; port gry przenieś (niżej); 8131 zostaje, więc gdy zajmuje go coś potrzebnego właścicielowi, stop |
| spoza sieci `Connection refused` albo przekroczony czas | router, zapora dostawcy albo CGNAT | krok 9 |
| gracza wyrzuca przy paczce zasobów | 8131 nieosiągalny z zewnątrz albo zły `HOT_ADDRESS` | krok 9; `HOT_ADDRESS` bez portu |
| „Your mods don't match” | gracz nie ma `hot_slots.hmod` | gracz klika „Graj” w `hot-play` |
| `HOT-E0602` | serwer nie łączy się z usługą sesji | wyjście HTTPS (443) z serwera, DNS; `docs/bledy/HOT-E0602.md` |
| `HOT-E0601` | konto gracza nie przeszło kontroli zakupu | po stronie gracza, serwer jest w porządku |
| gracz z listy nie wchodzi | wpis dodany bez `cmd` | `cmd "whitelist remove <nick>"`, potem znowu krok 8 |
| długo `unhealthy`, w logu brak błędów | wolny dysk przy pierwszym starcie | czekaj; `docker compose exec server /opt/horizon-hot/bin/hot-proxy ping 127.0.0.1:25565` |

Port gry na innym porcie hosta, np. 25570, w `compose.override.yaml`:

```yaml
services:
  server:
    ports: !override
      - "25570:25565"
      - "8131:8131"
```

Gracze wpisują wtedy `<adres>:25570`, także przy instalacji `hot-play`. `HOT_ADDRESS` zostaje bez portu, a paczka zawsze jest na 8131.

## Budowa poza serwerem

Za słaby serwer (kompilacja pada mimo `CARGO_BUILD_JOBS=1`): obraz zbuduj na komputerze właściciela, jeśli ma Dockera i tę samą architekturę co serwer (x86_64), a potem przenieś go na serwer:

```sh
# na komputerze właściciela, w klonie tego repo
docker compose build
docker save -o horizon-hot.tar horizon-hot:local
scp horizon-hot.tar <serwer>:
ssh <serwer> 'docker load -i horizon-hot.tar && rm horizon-hot.tar'
```

Na serwerze uruchamiasz potem `docker compose up -d` bez `--build`. Aktualizacja takiego serwera to te same cztery polecenia, a nie `up -d --build` z sekcji „Aktualizacja”. Zapisz to w `LOKALNE-POPRAWKI.md`. To zwykła droga Dockera, ale w tym projekcie jeszcze jej nie sprawdzano.

## Wznowienie

Sesja agenta się urwała: stan odczytasz z serwera, w folderze `~/horizon-hot`.

- `ls compose.override.yaml game-files/horizon-resources.json`: kroki 3–5 zrobione, gdy oba pliki są;
- `tmux ls` i `tail -3 budowa.log`: budowa trwa albo skończyła się (`koniec: 0` znaczy sukces);
- `docker compose ps`: serwer działa;
- `LOKALNE-POPRAWKI.md`: zmiany zrobione wcześniej.

## Wersja Horizona

Obraz buduje silnik z rewizji `66f3981` (Nightly z 5.11.2026). `hot-play` instaluje graczom tę wersję, a gdy jej wydania nie ma już na GitHubie, najnowszą. Według źródeł obie wersje mówią protokołem 777 (Minecraft 26.3), a silnik `66f3981` sprawdza w plikach gry te same sumy kontrolne, które zapisuje launcher z 13.11.2026. W grze tego nie sprawdzono.

Jeśli silnik odrzuca pełną, świeżą kopię plików, zbuduj obraz z rewizji, z której pochodzi launcher właściciela (ta droga nie była sprawdzana):

```sh
git ls-remote https://github.com/horizon-rust/horizon 'refs/tags/nightly-source/*'   # pełny hash wersji właściciela
docker compose build --build-arg HORIZON_REV=<pełny hash> --build-arg HORIZON_VERSION=nightly-<data>-<skrót>
```

Wpisz to do `LOKALNE-POPRAWKI.md`.

## Lokalne poprawki

Ustawienia i obejścia trzymaj w `compose.override.yaml`. Plików z repo (`compose.yaml`, `Dockerfile`, `scripts/`) nie zmieniaj, chyba że inaczej się nie da. Każdą zmianę wpisz do `LOKALNE-POPRAWKI.md` w folderze repo (git go pomija): co, dlaczego, jakim poleceniem to cofnąć.

Po każdej zmianie sprawdź, czy kontrola zakupu przetrwała:

```sh
docker compose config | grep -E 'published|HOT_TRUST|HOT_AUTO'
# tylko 25565 i 8131; HOT_TRUST i HOT_AUTO tylko za zgodą właściciela
```

Po zmianie w plikach repo uruchom też testy, które pilnują proxy i portów:

```sh
docker build --target rust -t horizon-hot:rust .
docker run --rm -v "$PWD":/src:ro -w /src -e CARGO_TARGET_DIR=/tmp/t horizon-hot:rust \
  cargo test -q --locked -p hot-proxy --test installer
```

Test pada: cofnij zmianę. Testów nie zmieniasz.

## Aktualizacja

```sh
cd ~/horizon-hot && git pull && docker compose up -d --build
```

Świat, klucze i ustawienia zostają. Gdy `git pull` zgłasza konflikt w pliku z repo, cofnij swoją zmianę (`LOKALNE-POPRAWKI.md` mówi jak) i spróbuj jeszcze raz.
