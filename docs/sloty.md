# Pula slotów (`hot-slots.hmod`)

Bloki i przedmioty z modów gorących nie mogą dopisać się do rejestrów gry w trakcie działania (R9). Dlatego gracz instaluje raz plik `hot-slots.hmod` z pulą pustych bloków i przedmiotów, a mody gorące tylko zajmują wolne sloty. Decyzja: D8 w `PLAN.md`. Dowody: `docs/spike/s2-pula-slotow-hmod.md`, `s5-wyglad-slotu-z-paczki-serwera.md`, `s7-rozmiar-puli.md`.

## Co jest w slocie

- Blok `hot_slots:slot_N`: `template: minecraft:stone`, przedmiot bloku, tabela łupów, blockstate wskazujący model `hot_slots:block/slot_N`.
- Przedmiot `hot_slots:item_N`: `template: minecraft:stick`, definicja przedmiotu wskazująca model `hot_slots:item/item_N`.
- Bez `models/`, `textures/` i `lang/`. Wygląd i nazwę daje paczka zasobów z `hot-pack` (D9). Gdyby takie pliki były w `.hmod`, zasłoniłyby paczkę (S5).
- Kolizja, światło i twardość pochodzą z szablonu. Model może mieć dowolny kształt, ale tylko wizualnie.

## Rozmiar

**Do decyzji użytkownika** (`PLAN.md`, sekcja 8). Zmierzone na Macu (S7):

| Pula (bloki/przedmioty) | `.hmod` | Start klienta | Podmiana silnika | Podmiana z nowym silnikiem |
|---|---|---|---|---|
| 1/0 | 1 KB | 1,07 s | 1,02–1,11 s | ok. 1,7 s (szacunek) |
| 1024/2048 | 1,7 MB | nie mierzono | 1,16–1,34 s | ok. 2,2 s (szacunek) |
| 4096/8192 | 7,0 MB | 2,04–2,12 s | 1,45–1,82 s | 3,85 s (1 pomiar) |

- „Podmiana z nowym silnikiem” to podmiana z modem natywnym, nowym `hot-host` albo aktualizacją Horizona. Nowy plik programu unieważnia cache treści modów, więc silnik buduje go przy starcie. Tak jest też od Nightly 2026-11-07, w którym cache dostał osobny katalog na zestaw modów (`docs/aktualizacje.md`). Szacunek: pauza z ciepłym cache plus różnica między zimnym a ciepłym startem serwera.
- Pamięć z pełną pulą: serwer ok. +230 MB, klient ok. +250 MB.
- Powiększenie puli później zmienia `content_identity`. Każdy gracz potrzebuje wtedy nowego pliku (aplikacja gracza pobiera go sama) i musi wejść ponownie.

## Zasady

1. Przydział slotów tylko dopisywany, w `hot-state/slots.json`. Slot nie wraca do innego moda bez jawnej komendy operatora.
2. Każda zmiana puli to nowa wersja `hot-slots` (S2: serwer porównuje id, wersję i SHA-256).
3. Klucz nazwy przedmiotu z puli zaczyna się od `block.` (`block.hot_slots.item_N`, S7). `hot-pack` używa klucza, który pokazuje gra.
4. Klient bez modelu slotu w paczce loguje „missing block model”, najwyżej 100 linii na minutę. To nie jest błąd.

## Jak sprawdzić

- Serwer: `horizon-server --dir <silnik> cmd give <gracz> hot_slots:slot_<ostatni>`. Odpowiedź „Gave 1 …” oznacza, że cała pula jest wczytana.
- Gracz bez pliku albo z inną wersją dostaje przy wejściu „Your mods don't match this server's” z nazwą `hot_slots` i wersją (S2).
