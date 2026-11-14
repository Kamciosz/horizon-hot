# ADR 0023 — ochrona przed pętlą awarii i samonaprawa (D13, D20)

- **Data:** 2026-11-14 (decyzje z 2026-11-12 i 2026-11-13, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Serwer działa bez stałego nadzoru. Zły mod nie może wpędzić go w pętlę awarii, a awaria silnika nie może rozłączyć graczy. Supervisor Horizona (`horizon-server/src/supervisor.rs`) wznawia grę po awarii z przerwą 5, 10, 20… s i poddaje się po 5 awariach w 10 minut.

## Decyzja

- **D13.** Przy starcie `hot-core` ładuje tylko generacje oznaczone jako ostatnie dobre: takie, które przeżyły 6000 ticków (5 min, `GOOD_AFTER_TICKS`). `hotd` oznacza commit jako zły, gdy silnik padł w ciągu 60 s od jego załadowania, i nie stosuje go ponownie (`HOT-E0206`). Druga awaria z rzędu w ciągu 5 min od startu `hot-core` (`SAFE_MODE_AFTER`, `CRASH_LOOP_WINDOW_MS`) włącza tryb bezpieczny: żaden mod gorący nie startuje, a wpisy wracają po czystym restarcie albo `hot load` (`HOT-E0408`).
- Po awarii silnika `hot-proxy` trzyma połączenia graczy (świat stoi, bez ekranu ładowania) i wprowadza ich do wznowionego silnika jak przy podmianie (ADR 0022, M8.4). Klient rozłącza się po 30 s ciszy (`READ_TIMEOUT`), ale proxy wysyła w tym czasie keep-alive.
- **D20. Reakcje automatyczne:**

| Błąd | Reakcja |
|---|---|
| panika w modzie | mod wyłączony, wraca poprzednia generacja (ADR 0018) |
| commit się nie buduje w CI | na serwerze nic się nie zmienia; powód w `hot status` |
| awaria silnika | supervisor wznawia silnik z ostatnimi dobrymi generacjami, proxy trzyma graczy |
| zły commit (awaria w ciągu 60 s) | oznaczony i pominięty |
| zła aktualizacja Horizona | powrót do poprzedniego silnika (ADR 0027) |
| pada `hotd`, `hotd serve-pack` albo `hot-proxy` | systemd `Restart=always`; usługa silnika ma `Restart=on-failure`, bo jej proces sam wznawia grę |
| uszkodzony świat | **bez automatu:** `hot status` pokazuje powód i polecenie przywrócenia z kopii, operator decyduje (`HOT-E0212`) |
| mało miejsca na dysku | `hotd` usuwa tylko własne zbędne pliki: stare artefakty (`hot-state/artifacts/`) i stare kopie `hot-core` (`hot-state/core/`), potem mówi w `hot status`, co zostało (`crates/hotd/src/disk.rs`) |

## Konsekwencje

- Zły mod kosztuje najwyżej jedną awarię; zła seria awarii zatrzymuje mody gorące, ale nie serwer.
- `hotd` nigdy sam nie przywraca kopii świata: przywrócenie zastępuje świat, a zmiany po kopii przepadają.
- Kopii świata, cache launchera ani dziennika systemu `hotd` nie usuwa, nawet przy pełnym dysku: nie są jego.
