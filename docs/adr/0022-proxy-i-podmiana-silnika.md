# ADR 0022 — `hot-proxy` i podmiana silnika bez rozłączania graczy (D10, D11, D12)

- **Data:** 2026-11-14 (decyzje z 2026-11-12 i 2026-11-13, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Mod natywny (`.hnmod`), nowa wersja Horizona, nowy `hot-host` i odzyskanie pamięci po generacjach (ADR 0018) wymagają nowego procesu silnika. Zwykły restart rozłącza wszystkich graczy. Przejście klienta przez `StartConfiguration` pokazuje ekran ładowania (R17), a podmiana całkiem niewidoczna, z przeniesieniem stanu gry między procesami, to miesiące pracy (R19).

## Decyzja

- **D10.** Gracze łączą się z `hot-proxy` (`crates/hot-proxy`, na `horizon-protocol`; zależności: ADR 0010) na porcie 25565. Silnik słucha na `127.0.0.1:25566` w trybie forwardingu Velocity, z sekretem `forwarding.secret`. Proxy kończy szyfrowanie, kompresję i keep-alive, mapuje identyfikatory encji między klientem a silnikiem i sprawdza logowanie Microsoft (tryb online), więc serwer zostaje premium. Proxy nigdy nie działa w trybie offline.
- **D11. Wariant A:** gracz zostaje połączony, świat stoi krócej niż 1 s, encje „mrugają”, ekranu ładowania nie ma. Budżet i pomiary: ADR 0004, kolejność kroków: `docs/podmiana.md`. W skrócie: nowy silnik startuje w tle i czeka przed zajęciem świata (mixin `wait_for_world` w `hot-host`, w logu „horizon-hot: waiting for the world”), proxy wstrzymuje ruch, stary silnik dostaje `save-all flush` i `stop`, nowy zajmuje świat, a proxy loguje graczy do nowego silnika i podmienia im identyfikatory. Błąd nowego silnika: gracze wracają na stary tą samą drogą.
- Podmianę prowadzi `hotd engine` (usługa `horizon-hot-server`, silnik jest jego procesem potomnym). Na żądanie: `hotd swap [--to PLIK]`. Pid silnika `hotd` bierze z `engine_pid` w `horizon-server status --json`, nigdy ze ścieżki programu.
- **D12. Dwie strony silnika na jednym świecie.** Strona `a` to katalog serwera (`/srv/horizon/server`, `HOT_HOME`), strona `b` to `/srv/horizon/server/engine-b` z kopią `server.toml`, portem o jeden wyższym (25567) i dowiązaniami do świata i wspólnych plików (`crates/hotd/src/swap.rs`). Aktywną stronę zapisuje `hot-state/engine.json`. Dwa silniki nigdy nie trzymają świata naraz (`session.lock`, próba S12). W `PLAN.md` były osobne katalogi `/srv/horizon/engine-a` i `engine-b`; wystarczyła jedna dodatkowa strona.

## Konsekwencje

- Wszystko, co gracz zrobił w czasie pauzy, przepada (ok. 0,5 s, Mac).
- Każde nowe Nightly może przesunąć miejsce haka `wait_for_world`; test aktualizacji Horizona sprawdza linię w logu (ADR 0027).
- Zmiana rejestrów (nowe bloki w `.hnmod`, nowa pula) wymaga nowych plików u gracza i ponownego wejścia. Dlatego nowa treść idzie przez sloty (ADR 0021).
- Nie sprawdzone na serwerze docelowym (brak maszyny): czas pauzy na ThinkCentre (M0.11) i podmiana na nowy plik silnika z `.hnmod` w trakcie gry (M8.3). Zmiana w `native/` repo serwera nie uruchamia dziś budowy ani podmiany; `hotd` tylko ją odnotowuje (`crates/hotd/src/classify.rs`).
