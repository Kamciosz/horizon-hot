# S2 — czy serwer dedykowany ładuje `.hmod` z pulą slotów i pilnuje go u klienta

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64, serwer i klient na tej samej maszynie.
- Krok planu: M0.3. Wynik: **potwierdzone**.

## Problem

Nowe bloki modów gorących dostają wygląd i zachowanie, ale rejestr bloków musi istnieć od startu gry (R9). Plan zakłada pulę pustych bloków w `hot-slots.hmod` (D8). Trzeba sprawdzić, że serwer dedykowany ją ładuje, a klient bez tego pliku nie wejdzie, zamiast wejść i zobaczyć błędy.

## Co sprawdzono

Paczka `spikes/s2-s5-pula-slotow/hmod/` (`id = "hot_slots"`, `side = "both"`, `capabilities = ["registries"]`): jeden blok `hot_slots:slot_0` z `data/hot_slots/horizon/block/slot_0.json` (`"template": "minecraft:stone"`).

```bash
cargo horizon-mod build                    # -> target/horizon-mod/hot_slots.hmod
# serwer: <katalog serwera>/horizon-mods/hot_slots.hmod + horizon-mods.txt: mod.hot_slots:true
# klient: <instancja>/horizon-mods/hot_slots.hmod  + <instancja>/horizon-mods.txt: mod.hot_slots:true
```

## Wynik

Serwer:

```
Horizon: content from 1 mod(s) in .../s2-srv/.horizon-cache/mod-content
> forceload add 0 0
> setblock 0 100 0 hot_slots:slot_0          -> Changed the block at 0, 100, 0
> execute if block 0 100 0 hot_slots:slot_0  -> Test passed
```

Klient bez moda — rozłączony przed wejściem do świata, ekran z przyciskiem „Back to Server List”:

```
kamciosz (f5aba27b-…) lost connection: Your mods don't match this server's. Missing: hot_slots 0.1.0
```

Klient z modem — wchodzi (`logged in with entity id 5`), blok widoczny, `game_state` celownika: `hot_slots:slot_0`.

## Wnioski

1. Pula slotów w `.hmod` działa na serwerze dedykowanym. Wpis `horizon-mods.txt` jest per instancja klienta (`instances/<nazwa>/horizon-mods/`), więc aplikacja gracza instaluje plik do swojej instancji i nie rusza innych.
2. Brak pliku u gracza daje czytelny komunikat z nazwą i wersją moda. `hot-play` może go rozpoznać i zaproponować „Napraw” (doinstalowanie `hot-slots.hmod`).
3. Komunikat podaje id i wersję. Serwer ogłasza też SHA-256 pliku (`announced_entry` w `crates/horizon-client/src/game/mods.rs`), ale czy inny SHA przy tej samej wersji też rozłącza — NIEWIADOMA. Bezpiecznie: wersję `hot-slots` podbijamy przy każdej zmianie puli (D8).
4. `setblock` na niewczytanym chunku zwraca „That position is not loaded”. Testy i `hot-core` muszą najpierw wczytać chunk (`forceload`).
5. Uwaga z konfiguracji próby: `horizon-server init --offline` ustawia `online-mode=false`. Na ThinkCentre serwer musi działać z `online-mode=true` (zasada: kupiony Minecraft u każdego gracza).
