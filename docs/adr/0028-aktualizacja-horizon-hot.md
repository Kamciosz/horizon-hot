# ADR 0028 — aktualizacja samego horizon-hot (D19)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, zmieniona przez ADR 0014, przeniesiona z `PLAN.md`)
- **Status:** accepted, wykonanie: ADR 0014
- **Autor:** Szymon Sosnowski

## Kontekst

Składniki serwera (`hotd`, `hot-proxy`, `hot-core`, `hotd serve-pack`) mają się aktualizować same, bez wizyty właściciela. Restart `hot-proxy` rozłącza graczy, a od M8.3 restart usługi silnika (`hotd engine`) zatrzymuje grę.

## Decyzja

- Wydaniem jest głowa gałęzi podanej w `--hot-ref` repo horizon-hot (tag albo commit wyłącza aktualizacje). `hotd upgrade` sprawdza ją raz na dobę (`--every 86400`), z timera co 10 minut.
- Poprawka samego `hot-core` (z `hot-sdk`) wchodzi od razu przez przeładowanie, bez przerwy w grze. Reszta (`hotd`, `hot-proxy`, `hot-diag`, `Cargo.lock`) czeka na chwilę bez graczy i zmienia się naraz. Zmiana `hot-host`, `hot-abi`, instalatora albo puli slotów czeka na właściciela. Szczegóły: ADR 0014, `docs/aktualizacje.md`.
- `PLAN.md` zakładał przekazanie gniazd graczy i stanu szyfrowania do nowego procesu `hot-proxy` (`SCM_RIGHTS`), żeby proxy zmieniało się przy graczach. To nie jest zrobione (M13); do tego czasu proxy zmienia się tylko na pustym serwerze.

## Konsekwencje

- Na serwerze nigdy nie działają obok siebie różne wersje `hotd` i `hot-proxy`.
- Na serwerze, na którym ktoś gra stale, aktualizacja poza `hot-core` może czekać długo; `hot status` pokazuje, na co czeka.
- Kontener nie aktualizuje się sam: programy są w obrazie (`docs/docker.md`).
