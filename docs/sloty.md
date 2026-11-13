# Pula slotów (`hot-slots.hmod`)

Bloki i przedmioty z modów gorących nie mogą dopisać się do rejestrów gry w trakcie działania (R9). Dlatego gracz instaluje raz plik `hot-slots.hmod` z pulą pustych bloków i przedmiotów, a mody gorące tylko zajmują wolne sloty. Decyzja: D8 w `PLAN.md`. Dowody: `docs/spike/s2-pula-slotow-hmod.md`, `s5-wyglad-slotu-z-paczki-serwera.md`, `s7-rozmiar-puli.md`.

## Co jest w slocie

- Blok `hot_slots:slot_N`: `template: minecraft:stone`, przedmiot bloku, tabela łupów, blockstate wskazujący model `hot_slots:block/slot_N`.
- Przedmiot `hot_slots:item_N`: `template: minecraft:stick`, definicja przedmiotu wskazująca model `hot_slots:item/item_N`.
- Bez `models/`, `textures/` i `lang/`. Wygląd i nazwę daje paczka zasobów z `hot-pack` (D9). Gdyby takie pliki były w `.hmod`, zasłoniłyby paczkę (S5).
- Kolizja, światło i twardość pochodzą z szablonu. Model może mieć dowolny kształt, ale tylko wizualnie.

## Rozmiar

4096 bloków i 8192 przedmioty (stałe `hotd::slots::BLOCKS` i `ITEMS`, wersja puli `4096.8192.0`, plik 7,0 MB). Rozmiar ustalił M0.20: to największa zmierzona pula, przy której podmiana silnika trwa krócej niż 1 s (ADR 0004). Większej nie mierzono. Pomiar szedł na puli z generatora S7 (`spikes/s7-rozmiar-puli/gen.py`), która różni się od pliku z `hotd slots-source` tylko kolejnością kluczy JSON i opisem w `horizon-mod.toml`. Plik z `hotd` (SHA-256 `aca5eef0…`) sprawdzony osobno: klient z nim wchodzi na serwer z nim, `give` dla `hot_slots:item_8191` i `slot_4095` działa, blok `slot_4095` stoi w świecie. Rozgrzany silnik (D11, S13), Mac, gracz w grze, `save-all flush`, po 6 podmian w każdym wierszu:

| Pula | Ten sam silnik | Nowy plik silnika, pusty cache | Rozgrzanie: ciepły / pusty cache | Czekający silnik |
|---|---|---|---|---|
| 1024/2048 | 470–541 ms | 509–531 ms | 0,5 s / 2,8–3,0 s | 363 MB |
| 4096/8192 | 480–549 ms | 505–573 ms | 0,8–0,9 s / 4,1–4,4 s | ok. 515 MB (1 pomiar) |

Pula prawie nie zmienia pauzy, bo jej koszt przechodzi w rozgrzanie, a to dzieje się przed zamrożeniem. Rośnie za to pamięć: czekający silnik i oba procesy w czasie podmiany. ThinkCentre (M0.11) może wymusić mniejszą pulę. Byłaby to wtedy nowa wersja puli, a przydział w `slots.json` zostaje, dopóki zajęte sloty mieszczą się w mniejszej.

Dla porównania pomiary na Macu bez rozgrzania (S7):

| Pula (bloki/przedmioty) | `.hmod` | Start klienta | Podmiana silnika | Podmiana z nowym silnikiem |
|---|---|---|---|---|
| 1/0 | 1 KB | 1,07 s | 1,02–1,11 s | ok. 1,7 s (szacunek) |
| 1024/2048 | 1,7 MB | nie mierzono | 1,16–1,34 s | ok. 2,2 s (szacunek) |
| 4096/8192 | 7,0 MB | 2,04–2,12 s | 1,45–1,82 s | 3,85 s (1 pomiar) |

- Rozgrzany silnik buduje cache przed zamrożeniem, więc czas z tej tabeli przestaje być pauzą, a gracz w tym czasie gra dalej (S13).
- „Podmiana z nowym silnikiem” to podmiana z modem natywnym, nowym `hot-host` albo aktualizacją Horizona. Nowy plik programu unieważnia cache treści modów, więc silnik buduje go przy starcie. Tak jest też od Nightly 2026-11-07, w którym cache dostał osobny katalog na zestaw modów (`docs/aktualizacje.md`). Szacunek: pauza z ciepłym cache plus różnica między zimnym a ciepłym startem serwera.
- Pamięć z pełną pulą: serwer ok. +230 MB, klient ok. +250 MB.
- Powiększenie puli później zmienia `content_identity`. Każdy gracz potrzebuje wtedy nowego pliku (aplikacja gracza pobiera go sama) i musi wejść ponownie.

## Bloki z tickiem

Decyzja M0.20: pula nie ma slotów z `"random_ticks": true`. Powody (FAKT, źródła Horizona 66f3981, szczegóły w M5.2 w `PLAN.md`):
- hak `random_tick` dostaje `LevelWriteHost`, który zmienia tylko właściwość bloku, a bloki ze slotów nie mają właściwości;
- `LevelHost` nie zamienia `BlockStateId` na nazwę, więc `hot-core` nie odróżni slotu z tickiem od innych bloków;
- hak dałby modom tylko cząsteczki i dźwięk, a każdy tykający blok z puli kosztowałby wywołanie przez ABI.

Gdy Horizon pozwoli w haku zmienić cały blok, dojdzie osobny zakres `hot_slots:tick_N` z `"random_ticks": true`, przydzielany tylko blokom, które deklarują tick. To nowa wersja puli, więc każdy gracz pobierze nowy plik. Numery `slot_N` i przydział w `slots.json` się nie zmieniają.

## Budowa i instalacja

```bash
hotd slots-source /tmp/hot-slots        # wypisuje wersję puli
cd /tmp/hot-slots && cargo horizon-mod build
```

Build jest powtarzalny: to samo źródło daje ten sam SHA-256, więc serwer i gracze budują plik sami. Gotowy plik idzie do `horizon-mods/` serwera i instancji gracza, a w `horizon-mods.txt` obok musi być wiersz `mod.hot_slots:true`. Na serwerze robi to instalator (krok 8). Gracz pobiera ten sam plik z `http://<adres serwera>:8131/hot_slots.hmod`.

## Przydział

- Blok deklaruje plik `mods/<id>/assets/blocks/<nazwa>.png` w repo serwera. `hotd` daje mu kolejny wolny slot i zapisuje to w `hot-state/slots.json` (`{"pool": {...}, "blocks": {"<id>:<nazwa>": N}}`). Tylko `hotd` pisze ten plik.
- Usunięty mod nie zwalnia slotu. Gdy wróci, dostaje ten sam numer.
- Pełna pula: commit nie wchodzi, nic się nie zmienia, `HOT-E0209` w `hot status`.
- `hot-core` czyta przydział przy starcie i po komendzie `hot slots`, którą `hotd` wysyła po każdej zmianie. Mod z kodem pisze `<id>:<nazwa>` w `set_block`, `give` i `block`, a w zdarzeniach dostaje tę samą nazwę.
- Pliki `data/` (przepisy, tagi, tabele łupów) muszą na razie podawać `hot_slots:slot_N` z `slots.json`. Tłumaczenia w plikach danych jeszcze nie ma.

## Paczka zasobów

- `hotd` buduje `hot-state/pack/pack.zip` i `pack.sha1` przy każdym commicie, który zmienia wygląd. Paczka ma teksturę, model `cube_all` i nazwę z `mods/<id>/assets/lang.json` (`{"<nazwa>": "…"}`) dla każdego przydzielonego bloku, którego plik jest w repo. Zły obraz albo zły `lang.json` zatrzymuje commit (`HOT-E0210`).
- `hotd serve-pack` (port 8131, usługa `horizon-hot-pack`) wysyła ją pod `/pack.zip`, a pulę pod `/hot_slots.hmod`. Zanim `hotd` zbuduje pierwszą paczkę, idzie paczka bez bloków, żeby gracza nie wyrzuciło. W `server.toml`, w `[properties]`: `resource-pack = "http://<adres>:8131/pack.zip"` i `require-resource-pack = true`, **bez** `resource-pack-sha1`. Silnik czyta te pola tylko przy starcie, a klient bez sha1 pobiera paczkę przy każdym wejściu, więc nowy wygląd dochodzi przy następnym wejściu bez restartu silnika.

## Zasady

1. Przydział slotów tylko dopisywany, w `hot-state/slots.json`. Slot nie wraca do innego moda bez jawnej komendy operatora.
2. Każda zmiana puli to nowa wersja `hot-slots` (S2: serwer porównuje id, wersję i SHA-256).
3. Klucz nazwy przedmiotu z puli zaczyna się od `block.` (`block.hot_slots.item_N`, S7). `hot-pack` używa klucza, który pokazuje gra.
4. Klient bez modelu slotu w paczce loguje „missing block model”, najwyżej 100 linii na minutę. To nie jest błąd.

## Jak sprawdzić

- Serwer: `horizon-server --dir <silnik> cmd give <gracz> hot_slots:slot_<ostatni>`. Odpowiedź „Gave 1 …” oznacza, że cała pula jest wczytana.
- Gracz bez pliku albo z inną wersją dostaje przy wejściu „Your mods don't match this server's” z nazwą `hot_slots` i wersją (S2).
