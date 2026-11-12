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

Podstawa z kodu Horizona `66f3981`: cache treści modów i paczka powstają przed zajęciem świata (`game.rs:22214` wywołuje `mods::content_root`, ten `mod_content::resource_root` w `game/mods.rs:152`; świat zajmuje `SessionLock::acquire` w `game/world_loading.rs:142`, po komunikacie „Preparing level”). Zysk w czasie to HIPOTEZA, którą sprawdza próba S13 (`PLAN.md`, krok M0.19).

## Konsekwencje

- Przez czas przygotowania podmiany działają dwa procesy silnika, z czego tylko jeden trzyma świat. Zmienia to D16: podmiana z rozgrzaniem rusza tylko wtedy, gdy wolna pamięć mieści czekający silnik. Pamięć ThinkCentre (M0.11) staje się pilna.
- `hot-host` dostaje mixin w kodzie startu silnika. Każde nowe Nightly może przesunąć to miejsce, więc test D18 musi sprawdzać rozgrzanie.
- Jeśli S13 nie zejdzie poniżej 1 s nawet z mniejszą pulą, wracamy do użytkownika ze zmierzonymi liczbami. Budżetu nie zmieniamy bez jego zgody.
