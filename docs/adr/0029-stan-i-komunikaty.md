# ADR 0029 — stan i komunikaty bez zewnętrznych powiadomień (D21)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, przeniesiona z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Operator to kolega właściciela, nie administrator. Serwer nie powinien zależeć od zewnętrznych usług (poczta, Discord, push), a stan ma być czytelny bez znajomości projektu.

## Decyzja

- `hot status` ma dwie postacie:
  - w terminalu serwera: `hotd status --home /srv/horizon/server` (`crates/hotd/src/status.rs`), po polsku, z plików `hot-state/*.json`, więc działa też wtedy, gdy silnik albo `hotd` nie działa;
  - w grze i w konsoli silnika: komenda `/hot status` (`hot-core`), która dodatkowo zapisuje `hot-state/status.json`.
- Treść: commit modów, mody i ich generacje, ostatnie błędy z kodem, oczekujące aktualizacje Horizona i horizon-hot, polecenie naprawy, gdy potrzebna jest decyzja operatora (np. przywrócenie świata, `HOT-E0212`).
- Do diagnozy przez kogoś spoza projektu: `hotd report` (ADR 0032).

## Konsekwencje

- Operator musi sam zajrzeć do `hot status`; nic go nie powiadamia.
- Nie zrobione: wiadomość na czacie dla operatorów w grze, gdy coś wymaga ich decyzji. Na czat trafia dziś tylko ogłoszenie terminu aktualizacji Horizona (`say`, ADR 0027).
