# ADR 0021 — sloty na nowe bloki i wygląd z paczki zasobów serwera (D8, D9)

- **Data:** 2026-11-14 (decyzje z 2026-11-12 i 2026-11-13, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Rejestry Horizona (bloki, przedmioty) zamykają się przy starcie, a klient i serwer muszą mieć ten sam ich zestaw (`content_identity`), inaczej klient dostaje rozłączenie z niejasną przyczyną. Mod gorący nie może więc dodać bloku w trakcie gry. Wygląd (modele, tekstury, nazwy) może za to przyjść z paczki zasobów serwera.

## Decyzja

- **D8. Pula slotów.** Plik `hot-slots.hmod` (przestrzeń nazw `hot_slots`, `side = "both"`) ma stałą pulę pustych bloków i przedmiotów: 4096 bloków i 8192 przedmiotów (`hotd::slots::BLOCKS`, `ITEMS`; liczby i pomiary: `docs/sloty.md`). Źródło generuje `hotd slots-source`, plik buduje `cargo horizon-mod build`; ten sam kod daje te same bajty na serwerze i u gracza. Gracz instaluje pulę raz (robi to `hot-play`). `hot-host` i `hot-core` nie rejestrują żadnej treści.
- Przydział slotów prowadzi tylko `hotd`, w `hot-state/slots.json`, i tylko go dopisuje. Slot nigdy nie wraca do innego moda. Blok moda to plik `mods/<id>/assets/blocks/<nazwa>.png`, a w kodzie nazwa `<id>:<nazwa>`, którą `hot-core` tłumaczy na slot. Pełna pula: `HOT-E0209`.
- Pula ma tylko `data/`, `blockstates/` i `items/`, bez `models/`, `textures/` i `lang/`. Treść modów leży nad paczką serwera, więc taki plik w `.hmod` zasłoniłby wygląd z paczki (próba S5).
- **D9. Wygląd.** `hotd` buduje paczkę zasobów (`hot-state/pack/pack.zip`, powtarzalną), a serwuje ją osobny proces `hotd serve-pack` na porcie 8131 (usługa `horizon-hot-pack`), razem z `hot_slots.hmod`. Silnik czyta adres paczki tylko przy starcie, więc `server.toml` ma adres bez sha1: klient pobiera paczkę przy każdym wejściu, a nowy wygląd dochodzi przy następnym wejściu, bez restartu silnika. Serwer wymaga paczki (`require-resource-pack = true`). `hot-play` zgadza się na nią z góry: dopisuje serwer do `servers.dat` swojej instancji z `acceptTextures = true`, nigdy do globalnego `servers.dat`.

## Konsekwencje

- Blok z puli ma kolizję, światło i twardość z szablonu; model może mieć inny kształt tylko wizualnie.
- Zmiana rozmiaru puli to nowa wersja `hot-slots` i nowy plik u każdego gracza.
- Nie zrobione: wysłanie nowej paczki w trakcie gry (`ResourcePackPush` z proxy). Próba S6 pokazała, że działa bez rozłączenia, ale przez ok. 1,3 s ekran zasłania nakładka ładowania. Dziś nowy wygląd widać dopiero po ponownym wejściu.
- Znana luka: przepisy w `data/` muszą na razie podawać `hot_slots:slot_<numer>`.
