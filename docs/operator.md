# Instrukcja operatora

Dla osoby, u której stoi serwer. Instalacja: `README.md` i ADR 0031, kontener: `docs/docker.md`. Wszystkie polecenia uruchamiasz na serwerze.

## Gdzie co leży

| Ścieżka | Co tam jest |
|---|---|
| `/srv/horizon/server` | serwer: świat, `server.toml`, stan modów `hot-state/`, logi `logs/` |
| `/srv/horizon/server/engine-b` | drugi katalog silnika używany przy podmianie (ADR 0022) |
| `/srv/horizon/bin` | programy: `hotd`, `hot-proxy`, `horizon-server` |
| `/srv/horizon/logs/install.jsonl` | dziennik instalatora |
| `/srv/horizon/backups` | kopie stanu modów (`hot-state-<data>.tar.gz`) |
| `/srv/horizon/server-repo-files` | `hot.yml` do skopiowania do repo serwera |

Porty: gracze 25565 (`hot-proxy`), paczka zasobów 8131, silnik 25566 tylko lokalnie.

## Stan serwera

```sh
sudo -u horizon /srv/horizon/bin/hotd status --home /srv/horizon/server
```

Po polsku: commit modów, mody i ich wersje, ostatnie błędy z kodem, oczekujące aktualizacje, a gdy coś wymaga Twojej decyzji, polecenie do wykonania. Czyta pliki stanu, więc działa także wtedy, gdy silnik stoi.

W grze (operator) albo w konsoli silnika jest też `/hot status`. Pokazuje to samo z perspektywy działającego silnika i zapisuje `hot-state/status.json`.

Usługi:

```sh
systemctl status horizon-hot-server horizon-hot-proxy horizon-hot-hotd horizon-hot-pack
journalctl -u horizon-hot-server -n 100
```

## Coś nie działa

```sh
sudo -u horizon /srv/horizon/bin/hotd report --home /srv/horizon/server --out /tmp/raport.md
```

Raport to jeden plik Markdown: opis systemu, wersje, ostatnie błędy z opisami, stan, końcówki logów i konfiguracja. Sekrety i adresy IPv4 graczy są zamaskowane. Wyślij go właścicielowi albo wklej do czatu z modelem AI. Każdy kod `HOT-EXXXX` ma opis i naprawę w `docs/bledy/HOT-EXXXX.md`.

## Kopie i przywracanie

Dwa rodzaje kopii, oba co noc, trzymane 7 dni:

- świat: robi je sam `horizon-server`;
- stan modów (`hot-state`, `hot-keys`): `horizon-hot-backup.timer` o 04:00 do `/srv/horizon/backups`.

Serwer nigdy nie przywraca kopii sam. Gdy świat trzeba cofnąć, `hot status` to powie (np. `HOT-E0212`), a decyzja należy do Ciebie.

```sh
# lista kopii świata
sudo -u horizon /srv/horizon/bin/horizon-server --dir /srv/horizon/server backups
# przywrócenie (serwer stoi na czas przywracania)
sudo systemctl stop horizon-hot-server
sudo -u horizon /srv/horizon/bin/horizon-server --dir /srv/horizon/server restore <kopia>.zip --yes
sudo systemctl start horizon-hot-server
```

Stan modów:

```sh
sudo systemctl stop horizon-hot-hotd horizon-hot-server
sudo -u horizon tar -xzf /srv/horizon/backups/hot-state-<data>.tar.gz -C /srv/horizon/server
sudo systemctl start horizon-hot-server horizon-hot-hotd
```

## Aktualizacje

- Mody: same, z każdego commita w repo serwera (ADR 0026).
- horizon-hot: sam, raz na dobę, gdy nikt nie gra (ADR 0028). Zmiany w `hot-host` i w instalatorze czekają na Ciebie: `hot status` mówi, które polecenie uruchomić.
- Horizon: bez zgody na kod natywny `hot status` tylko pokazuje nową wersję. Samodzielne aktualizacje włącza właściciel (ADR 0027, `docs/aktualizacje.md`).

## Kontener

To samo w kontenerze (`docs/docker.md`):

```sh
docker compose exec -u horizon server /opt/horizon-hot/bin/hotd status --home /srv/horizon/server
docker compose exec -T -u horizon server /opt/horizon-hot/bin/hotd report --home /srv/horizon/server > raport.md
docker compose logs server
```
