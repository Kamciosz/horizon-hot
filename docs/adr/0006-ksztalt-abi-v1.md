# ADR 0006 — Kształt ABI v1 (`hot-abi`)

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

D3–D6 w `PLAN.md` opisują granicę między gospodarzem a modem: stabilne C ABI, cykl życia `init` / `deactivate`, haki v1 i funkcje gospodarza. Przy pisaniu `hot-abi`, `hot-sdk` i `hot-core` (M1) trzeba było rozstrzygnąć szczegóły, których plan nie przesądzał, a w trzech miejscach zmienić jego zapis.

## Rozważane opcje

1. **Osobny wskaźnik funkcji na każdy hak** (`server_tick`, `player_chat`, … jak w M1.1). Nowy hak zmienia układ tabeli, więc wymaga nowego `ABI_MAJOR`, a każdy mod trzeba przebudować.
2. **Jedno `on_event` ze strukturą `Event` i polem `kind`.** Nowy rodzaj zdarzenia to nowa stała `kind::*`. Stary mod dostaje go jako `Event::Unknown` i może go zignorować.

## Decyzja

- **Zdarzenia:** opcja 2. `Event` ma stały układ (120 bajtów) i wspólne pola: gracz, nazwa, tekst, przedmiot, blok, pozycja, ściana, liczba. Znaczenie pól zależy od `kind`.
- **`init` w wątku serwera.** D4 mówiło „`init` i `dlopen` poza wątkiem serwera”. Poza wątkiem zostaje kopia pliku i `dlopen`. `init` dostaje tabelę gospodarza, która jest ważna tylko w wątku serwera i tylko w trakcie wywołania, dlatego rusza dopiero przy przełączeniu na granicy ticku.
- **Nazwa:** w `hot-sdk` kontekst wywołania to `Host<'call>`, a nie `HostCtx<'call>` jak w M1.2. Znaczenie jest to samo: obiekt żyje tylko w trakcie wywołania.
- **Tablice przez wskaźnik:** `tell` dostaje `&Uuid`, a `block` dostaje `&[i32; 3]`. W C tablicy nie da się przekazać przez wartość (lint `improper_ctypes_definitions`).
- **Wersja:** pole `abi` jest pierwsze w `ModVTable`. Gospodarz czyta je przed resztą tabeli i odrzuca inną wersję (`HOT-E0402`). Osobnej funkcji `hot_mod_abi()` z D3 nie ma.
- **Panika:** `hot-sdk` łapie panikę w każdym eksporcie i oddaje gospodarzowi miejsce i backtrace przez `log` z poziomem `ERROR` i tekstem zaczynającym się od `panicked at`. `hot-core` zamienia to na wpis `HOT-E0403`. Hak paniki działa w kopii biblioteki standardowej moda, więc nie zmienia zachowania silnika.
- **Rdzeń jak mod:** `hot-core` eksportuje tę samą tabelę co mody (`hot_mod!`), a `hot-host` ładuje go jak mod (D2). `Host::raw` i `HotMod::on_raw_event` są ukryte w dokumentacji i służą tylko rdzeniowi do przekazania zdarzenia modom bez zmian.

## Konsekwencje

- Układ struktur pilnuje `crates/hot-abi/tests/layout.rs`. Zmiana rozmiaru albo przesunięcia pola to nowe `ABI_MAJOR` i przebudowa wszystkich modów.
- Nowy hak silnika wymaga stałej w `hot_abi::kind`, wariantu w `hot_sdk::Event` i tłumaczenia w `hot-host`, ale nie nowej wersji ABI.
- `init` nowej generacji wydłuża tick, w którym następuje przełączenie. Pomiar MSPT przy przeładowaniach to sprawdzenie M1.
