# S5 — czy wygląd bloku-slotu przychodzi z paczki zasobów serwera

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64.
- Krok planu: M0.6. Wynik: **potwierdzone, pod warunkiem**: `hot-slots.hmod` nie może zawierać modeli, tekstur ani tłumaczeń slotów.

## Problem

Mod gorący nadaje slotowi wygląd i nazwę bez zmiany pliku `.hmod` u graczy (D8, D9). Jedyna droga do klienta bez restartu gry to paczka zasobów serwera (`resource-pack` + `resource-pack-sha1`).

## Co sprawdzono

Serwer `s2-srv` z `hot_slots.hmod`, paczka `spikes/s2-s5-pula-slotow/paczka/` (format 97) serwowana przez `python3 -m http.server`:

```toml
# server.toml
[properties]
resource-pack = "http://127.0.0.1:8137/pack.zip"
resource-pack-sha1 = "<sha1 pliku pack.zip>"
require-resource-pack = false
```

Pięć rund, w każdej restart serwera i ponowne wejście klienta (nowy sha1 tam, gdzie zmieniła się paczka):

| Runda | `.hmod` zawiera | Paczka zawiera | Wynik w grze |
|---|---|---|---|
| 1 | blockstate, model `cube_all`, nazwę | teksturę `slot_0` | tekstura z paczki — **działa** |
| 2 | jak wyżej | + model `cube_column` z nową teksturą `slot_0_top` na górze, nazwę | model i nazwa z `.hmod`, paczka zignorowana |
| 3 | jak wyżej | model z teksturą `minecraft:block/diamond_block` na górze | dalej model z `.hmod` |
| 4 | **tylko blockstate i model przedmiotu** | jak w rundzie 3 (ten sam sha1, paczka z pamięci klienta) | góra diamentowa, boki w szachownicę, nazwa „Blok sklepu (z paczki)” — **model i nazwa z paczki działają** |
| 5 | jak w rundzie 4 | model z nową teksturą `slot_0_top` na górze | góra niebieska; log `block sprites: 1303 layers` (wcześniej 1302) — **paczka dodaje nową teksturę** |

Zrzuty ekranu nie są w repo, bo zawierają tekstury gry; powtórzenie: kroki niżej.

## Przyczyna wyniku rund 2–3

`crates/horizon-client/src/game/packs.rs:497-498` (`stage_pack`):

```rust
let root = startup_root(vanilla_root, selected, server)?;
let root = super::mods::content_root(&root, mods_enabled, dev_mods);
```

Najpierw składane są paczki (wybrane przez gracza i serwerowe), a treść modów kładziona jest **na wierzch**. Plik obecny w `.hmod` zawsze wygrywa z paczką serwera. Tekstura w rundzie 1 zadziałała tylko dlatego, że `.hmod` jej nie miał.

## Wnioski

1. **Reguła dla `hot-slots.hmod`:** zawiera tylko `data/` (definicje bloków i przedmiotów, loot), `blockstates/` i `items/` wskazujące model `hot_slots:block/slot_N`. Bez `models/`, `textures/`, `lang/`. Wszystko widoczne dostarcza `hot-pack`. Bez paczki slot wygląda jak „missing model” (log: `missing block model hot_slots:block/slot_0`).
2. Paczka może dodać nowe tekstury i dowolny model. Kształt jest tylko wizualny: kolizja i twardość zostają z `template` (D8 nadal: slot nie zmienia fizyki).
3. Klient zapamiętuje paczkę po sha1: przy tym samym sha1 nie pobiera jej ponownie (log HTTP). Nowy wygląd = nowy sha1 (D9 potwierdzone).
4. Klient pyta gracza o zgodę na paczkę („This server recommends…”), a do tego czasu stoi na „Joining world…”. Na produkcji: `require-resource-pack = true`, a `hot-play` ustawia zgodę dla naszego serwera z góry, żeby nikt nie odrzucił paczki przez pomyłkę.
5. Zmiana paczki w tej próbie wymagała restartu serwera (adres i sha1 z `server.toml`) i ponownego wejścia. Zmiana bez restartu i bez wyjścia — przez `ResourcePackPush` z proxy (S6, M0.7) albo przy podmianie silnika (D11).

## Powtórzenie

```bash
cd spikes/s2-s5-pula-slotow/hmod && cargo horizon-mod build
cd ../paczka && zip -qrX /tmp/www/pack.zip pack.mcmeta assets && shasum -a1 /tmp/www/pack.zip
python3 -m http.server 8137 --bind 127.0.0.1 --directory /tmp/www
# serwer: hot_slots.hmod w horizon-mods/, [properties] jak wyżej, start
# klient: ten sam hot_slots.hmod w horizon-mods/ instancji; wejście, zgoda na paczkę
# w konsoli serwera: forceload add 0 0; setblock 0 100 0 hot_slots:slot_0
```
