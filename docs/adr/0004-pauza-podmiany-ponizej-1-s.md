# ADR 0004 — Pauza podmiany silnika krótsza niż 1 s (D11)

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Przy podmianie silnika (D11) gracz zostaje połączony, ale świat na chwilę stoi. Próby S7, S8 i S12 zmierzyły tę pauzę na Macu przy kolejności „zatrzymaj stary silnik, potem uruchom nowy”:

| Przypadek | Pauza |
|---|---|
| bez puli slotów | 0,96–1,07 s |
| pula 1024/2048, ten sam silnik | 1,16–1,34 s |
| pula 1024/2048, nowy plik silnika (zimny cache) | ok. 2,2 s |
| pula 4096/8192, ten sam silnik | 1,45–1,82 s |
| pula 4096/8192, nowy plik silnika (zimny cache) | ok. 3,9 s |

Pauza to prawie w całości zapis i zatrzymanie starego silnika (ok. 0,4 s) oraz start nowego. Nowy silnik nie może zająć świata, dopóki trzyma go stary (S12). Użytkownik zdecydował: **pauza krótsza niż 1 s w każdym przypadku**, także przy nowym pliku silnika. Na ThinkCentre czasy będą dłuższe niż na Macu.

## Rozważane opcje

1. **Przyjąć zmierzoną pauzę** — bez dodatkowej pracy, ale sprzeczne z decyzją użytkownika.
2. **Mała pula albo brak puli** — skraca start, ale nawet bez puli pauza sięga 1,07 s. Po zapełnieniu małej puli każdy gracz potrzebuje nowego pliku i ponownego wejścia (D8).
3. **Rozgrzany silnik** — wszystko, co nie wymaga świata, dzieje się przed zamrożeniem:
   - `hotd` raz uruchamia nowy silnik przed podmianą, żeby zbudował cache treści modów;
   - mixin w `hot-host` wstrzymuje start tuż przed zajęciem świata do sygnału od `hotd`;
   - `save-all flush` tuż przed zamrożeniem skraca zatrzymanie starego silnika.
4. **Przekazanie graczy przez `[transfer]` Horizona** — wymaga dwóch serwerów działających naraz, a dwa silniki nie otworzą naraz wspólnego świata (S12, `docs/spike/s8-podmiana-analiza-klienta.md`).

## Decyzja

Wybieramy **opcję 3**, a opcję 2 jako uzupełnienie. Pula jest największa, przy której pauza mieści się poniżej 1 s; do pomiaru zostaje 1024/2048.

Podstawa z kodu Horizona `66f3981`: cache treści modów i paczka powstają przed zajęciem świata (`game.rs:22214` wywołuje `mods::content_root`, ten `mod_content::resource_root` w `game/mods.rs:152`; świat zajmuje `SessionLock::acquire` w `game/world_loading.rs:142`, po komunikacie „Preparing level”). Zysk w czasie sprawdziła próba S13 (niżej).

## Konsekwencje

- Przez czas przygotowania podmiany działają dwa procesy silnika, z czego tylko jeden trzyma świat. Zmienia to D16: podmiana z rozgrzaniem rusza tylko wtedy, gdy wolna pamięć mieści czekający silnik. Pamięć ThinkCentre (M0.11) staje się pilna.
- `hot-host` dostaje mixin w kodzie startu silnika. Każde nowe Nightly może przesunąć to miejsce, więc test D18 musi sprawdzać rozgrzanie.
- Jeśli S13 nie zejdzie poniżej 1 s nawet z mniejszą pulą, wracamy do użytkownika ze zmierzonymi liczbami. Budżetu nie zmieniamy bez jego zgody.

## Wynik S13 (2026-11-13)

Próba S13 (`docs/spike/s13-rozgrzany-silnik.md`) potwierdziła opcję 3 na Macu, pula 1024/2048, z graczem w grze:

| Przypadek | Pauza przed | Pauza z rozgrzanym silnikiem |
|---|---|---|
| ten sam silnik | 1,16–1,34 s | 470–541 ms z `save-all flush`, 501–703 ms bez |
| nowy plik silnika, pusty cache | ok. 2,2 s | 509–531 ms |

Dwie zmiany względem opisu opcji 3:
- Osobne uruchomienie rozgrzewające odpadło. Nowy silnik startuje normalnie przed `stop` starego, sam buduje cache w swoim katalogu i czeka przed zajęciem świata. Mixin `wait_for_world` nie czeka na sygnał od `hotd`, tylko na zwolnienie `session.lock`.
- Czekający silnik zajmuje 363 MB RSS przez 0,5 s z ciepłym cache albo ok. 3 s z zimnym.

Zapas na Macu to ok. 0,3 s. Większość pauzy to `stop` starego silnika, który zależy od dysku, więc ThinkCentre trzeba zmierzyć (M0.11). Rozmiar puli rozstrzyga M0.20.
