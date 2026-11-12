# S7 — ile kosztuje pula slotów 4096/8192

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64 (Apple M4 Pro, 24 GB), klient i serwery na jednej maszynie, pusty świat, jeden gracz.
- Krok planu: M0.8. Wynik: **kryterium spełnione**: klient z pełną pulą startuje wolniej o ok. 1 s (próg: 2 s).
- **Skutek uboczny ważniejszy od kryterium:** pełna pula wydłuża podmianę silnika (D11) do 1,45–1,82 s, a podmiana z nowym silnikiem (mod natywny) trwa ok. 3,9 s. Budżet D11 to 1 s. Decyzja użytkownika: `PLAN.md`, sekcja 8.

## Co sprawdzono

Generator `spikes/s7-rozmiar-puli/gen.py` tworzy `hot_slots` z N blokami (`template: minecraft:stone`, model z blockstate, tabela łupów, przedmiot bloku) i M przedmiotami (`template: minecraft:stick`). Pliki są takie jak w S2, bez `models/`, `textures/` i `lang/` (D8).

| Wariant | Bloki | Przedmioty | `.hmod` |
|---|---|---|---|
| 1/0 | 1 | 0 | 1 KB |
| 1024/2048 | 1024 | 2048 | 1,7 MB |
| 4096/0 | 4096 | 0 | 3,7 MB |
| 4096/8192 | 4096 | 8192 | 7,0 MB |

Pomiary:

1. Start serwera (`srvbench.py`): od `horizon-server start` do otwarcia portu, RSS silnika 10 s później.
   - **Ciepły** start: cache treści modów (`<serwer>/.horizon-cache/mod-content`) jest aktualny. Od Nightly 2026-11-07 cache ma osobny podkatalog na każdy zestaw modów (`mod-content/<16 znaków hex>/`), ale klucz nadal zawiera plik programu, więc wnioski poniżej się nie zmieniają (`docs/aktualizacje.md`, punkt 6).
   - **Zimny** start: cache usunięty, więc silnik buduje go od nowa.
2. Start klienta (`clibench.py`): od pojawienia się procesu klienta do ekranu `playing` z chunkami, RSS 15 s później, liczba ostrzeżeń w logu. Klient: instancja `hot-spike` uruchamiana przez MCP `play --server … --offscreen`.
3. Podmiana silnika (`variant.sh`, `swaps.sh`): proxy z S8, dwa serwery z S12 na wspólnym świecie, ta sama pula na A, B i u klienta, 6 podmian na wariant. Pauza jak w S8: od początku podmiany do `accept_teleportation` od klienta.
4. Ostatni slot działa: `give kamciosz hot_slots:slot_4095` i `hot_slots:item_8191` z konsoli serwera, oba widoczne w ekwipunku klienta.

## Wynik

### Serwer

| Wariant | Ciepły start | Zimny start | RSS (ciepły) |
|---|---|---|---|
| 1/0 | 0,64–0,68 s | 1,23–1,26 s | 290–305 MB |
| 1024/2048 | 0,70 s | 1,60–1,73 s | 350 MB |
| 4096/0 | 0,82 s | 2,16–2,38 s | 364 MB |
| 4096/8192 | 0,98–1,01 s | 2,85–2,88 s | 516–542 MB |

Po zimnym starcie silnik zajmuje ok. 700 MB niezależnie od puli (budowa cache) i tyle zostaje 10 s później.

### Klient

| Wariant | Start z cache | Pierwszy start po zmianie `.hmod` | RSS |
|---|---|---|---|
| 1 slot (S2) | 1,07–1,16 s (5 prób, mediana 1,07) | 2,70 s | 777–997 MB, mediana 823 |
| 4096/8192 | 2,04–2,12 s (3 próby) | 4,40 s | 804–1130 MB, mediana 1098 |

Pierwsza próba w sesji z małą pulą trwała 2,21 s (zimny dysk), więc w tabeli jej nie ma. RSS klienta waha się o ±150 MB między próbami, więc różnica ok. 250 MB to szacunek.

### Podmiana silnika

| Wariant | Pauza (6 podmian, cache B rozgrzany) |
|---|---|
| 1/0 | 1,02–1,11 s |
| 1024/2048 | 1,16–1,34 s |
| 4096/0 | 1,26–1,71 s |
| 4096/8192 | 1,45–1,82 s (dwie serie po 6, bez pierwszej podmiany z zimnym cache) |

- W pierwszej serii z pełną pulą cache B nie był rozgrzany. Pierwsza podmiana trwała wtedy **3,85 s**.
- Zatrzymanie starego silnika trwa niezależnie od puli ok. 0,4 s (396–482 ms). Cały wzrost to start nowego.
- Z pełną pulą podmiana B → A trwała ok. 0,3 s dłużej niż A → B (1,72–1,82 s wobec 1,45–1,52 s). Przyczyny nie szukałem.
- We wszystkich 30 podmianach ekran klienta był przez cały czas `playing`, a `horizon-ctl events` niczego nie zapisało.

### Inne obserwacje

- **Cache treści a nowy silnik (FAKT z kodu).** `crates/horizon-client/src/mod_content.rs`, `fn stamp` liczy klucz cache z czterech rzeczy:
  - pliku programu (ścieżka, rozmiar i czas zmiany `current_exe`);
  - plików gry;
  - każdego pliku moda z treścią (ścieżka, rozmiar, czas zmiany);
  - bloków i przedmiotów zadeklarowanych w kodzie.

  Każdy nowy silnik (mod natywny, nowy `hot-host`, aktualizacja Horizona) startuje więc z zimnym cache. Nawet `cp` tego samego `.hmod` unieważnia cache, bo zmienia czas zmiany pliku: sprawdzone, ciepły start 1/0 wzrósł po `cp` do 2,21 s.
- **`hotd` nie może rozgrzać cache B przed podmianą tak jak `variant.sh`.** Na produkcji A trzyma świat, a B nie wystartuje na tym samym świecie (S12). Cache budowany poza B przed podmianą to pomysł na M8 (HIPOTEZA): klucz zawiera ścieżki bezwzględne, więc musiałby powstać w katalogu B.
- **Ostrzeżenia.** Klient zapisuje „missing block model hot_slots:block/slot_N” dla każdego slotu bez modelu w paczce. Log ogranicza je do 100 linii na minutę („100 lines like this one in a minute: the next ones are counted, not logged”), więc 4096 slotów nie zalewa logu. Serwer nie zgłasza ostrzeżeń o puli.
- **Nazwa przedmiotu.** `give … hot_slots:item_8191` odpowiada „Gave 1 [block.hot_slots.item_8191]”, czyli klucz tłumaczenia przedmiotu z `data/…/horizon/item/` też zaczyna się od `block.`. S5 sprawdziło nazwę tylko dla bloku. `hot-pack` musi użyć klucza, który gra naprawdę pokazuje: do sprawdzenia w M5 (D9).
- **Konfiguracja z modami w proxy.** W 30 podmianach proxy odtwarzało wobec B odpowiedź klienta na listę modów (`horizon:mod_list`), a B ją przyjął. Działa to przy tej samej puli na A i B. Inna lista modów na B i paczka zasobów nadal są niesprawdzone (S8).

## Wnioski

1. Kryterium M0.8 spełnione: klient z pełną pulą startuje wolniej o ok. 1 s z cache i o ok. 1,7 s przy pierwszym starcie po zmianie puli. Gracz pobiera raz plik 7 MB.
2. Pamięć: serwer zajmuje ok. +230 MB, klient ok. +250 MB (szacunek). Ile RAM-u ma ThinkCentre, nie wiadomo (M0.10).
3. **Pula wydłuża podmianę silnika:** 1,02–1,11 s → 1,45–1,82 s. Podmiana z nowym silnikiem (zimny cache) trwa z pełną pulą ok. 3,9 s. Mniejsza pula skraca obie, ale jej powiększenie później wymaga nowego pliku u każdego gracza i ponownego wejścia (D8). To decyzja użytkownika.
4. Koszt rośnie i z blokami, i z przedmiotami. 4096 bloków bez przedmiotów kosztuje więcej niż 1024/2048, a 8192 przedmioty dokładają do tego ok. 0,2 s ciepłego startu serwera. Innych kombinacji nie mierzyłem.

## Czego nie sprawdzono

- Przeładowania paczki zasobów z modelami dla wszystkich 4096 slotów u klienta. Sprawdzić przy M0.7 (S6), które i tak wysyła paczkę.
- ThinkCentre (M0.11): tam wszystkie czasy mogą być dłuższe.
- Świata z prawdziwą zawartością i wielu graczy.

## Powtórzenie

```bash
export H=/tmp/hot-spike/s7   # katalog roboczy poza repo
S=spikes/s7-rozmiar-puli
python3 $S/gen.py $H/full 4096 8192 && (cd $H/full && cargo horizon-mod build)
# serwer: horizon-server --dir $H/srv init --eula --world world --seed 7 --port 25622 --no-whitelist --offline --yes
#         cp $H/full/target/horizon-mod/hot_slots.hmod $H/srv/horizon-mods/; echo mod.hot_slots:true > $H/srv/horizon-mods.txt
python3 $S/srvbench.py $H/srv 25622 full-ciepły
# klient: hmod do <instancja>/horizon-mods/, serwer uruchomiony, potem
python3 $S/clibench.py full-1 &   # i MCP play --server 127.0.0.1:25622 --offscreen; $S/quit.sh
# podmiana: katalogi $H/s12/a i $H/s12/b jak w S12, proxy z S8 zbudowane (cargo build --release)
$S/variant.sh full 4096 8192      # i MCP play --server 127.0.0.1:25700 --offscreen
$S/swaps.sh 6
```
