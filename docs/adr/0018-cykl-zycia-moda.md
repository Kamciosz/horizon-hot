# ADR 0018 — cykl życia moda gorącego i generacje (D4)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, przeniesiona z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Nowa wersja moda musi przejąć stan starej, a gra nie może się przy tym zatrzymać. Wyładowanie biblioteki (`dlclose`) w Rust jest niebezpieczne: wątki, timery i destruktory statycznych zmiennych starej wersji mogą jeszcze działać, a wtedy serwer pada.

## Decyzja

- `dlopen` i `init(ctx, prev_state)` nowej generacji idą poza wątkiem serwera. Przełączenie na nową generację następuje na granicy ticku.
- Obowiązkowe `deactivate(ctx) -> state`: mod zatrzymuje swoje wątki i timery i oddaje stan, który dostanie następna generacja.
- `ctx` jest ważny tylko na czas wywołania (`HostCtx<'call>` w `hot-sdk`).
- `catch_unwind` w każdym eksporcie (`hot-sdk`): panika wyłącza mod, a wraca poprzednia generacja (`HOT-E04xx`).
- Każda generacja to kopia pliku pod unikalną nazwą. Starej generacji nie wyładowujemy: zostaje w pamięci, ale nic jej już nie wywołuje.
- Pamięć odzyskuje podmiana silnika (ADR 0022), uruchamiana przez `hotd engine`, gdy pliki nieużywanych generacji w procesie silnika przekroczą próg (`--reclaim-mb`, domyślnie 256 MiB; `crates/hotd/src/reclaim.rs`). Próg liczy bajty plików, nie RSS: RSS rośnie też od świata i graczy, a tego podmiana nie oddaje (M8.5).
- Trwałe dane moda leżą w `hot-data/<mod>/`.

## Konsekwencje

- Przeładowanie jest niewidoczne dla graczy, a zły mod nie zatrzymuje serwera.
- Pamięć rośnie z każdym przeładowaniem aż do progu; potem świat stoi przez czas podmiany silnika (krócej niż 1 s, ADR 0004).
- Autor moda musi w `deactivate` zatrzymać wszystko, co sam uruchomił. Inaczej stara generacja dalej działa w tle.
