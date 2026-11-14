# ADR 0025 — gra ma pierwszeństwo, tło pracuje z resztek (D16)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, zmiana z M6.2, przeniesiona z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Serwer to jeden komputer (ThinkCentre). Na nim działa gra, a obok praca w tle: git, pobieranie modów, budowa silnika, kopie. Gracz nie może odczuć tła jako przycięć.

## Decyzja

- Jednostki systemd w trzech slice'ach (`scripts/server-install.sh`, krok 10): `horizon.slice` (`CPUWeight=1000`) zawiera `horizon-game.slice` (silnik, proxy, paczka; `CPUWeight=10000`, `IOWeight=10000`) i `horizon-bg.slice` (`hotd`, kopie, budowa; `CPUWeight=100`, `IOWeight=1`). Usługi tła mają `Nice=19`, a kopia `hot-state` także `CPUSchedulingPolicy=idle` i `IOSchedulingClass=idle`.
- Zmiana z M6.2: waga tła to 100, nie 1. Przy wadze 1 i grze na wszystkich rdzeniach `hotd` przez 25 s nie zrobił ani kroku; przy 100 robi kroki co 10 s, a gra dostaje ok. 99% CPU (pomiar w maszynie wirtualnej). Z tego samego powodu `hotd` nie ma `SCHED_IDLE`.
- Hamulec MSPT (`crates/hotd/src/brake.rs`): `hotd` co sekundę czyta `status --json` silnika. Gdy p95 przekroczy 35 ms, wstrzymuje (`SIGSTOP`) swoje ciężkie procesy potomne; gdy p95 jest niższe niż 25 ms przez 10 s, wznawia je. Nigdy nie wstrzymuje siebie, `hotd serve-pack`, gita ani komend dla silnika: dostarczają mody, a serwer, który stale nie nadąża, nie dostałby ich nigdy.
- Test nowej wersji Horizona (drugi silnik z botami, ADR 0027) działa w `horizon-bg.slice`. W `PLAN.md` miał ruszać tylko przy wolnej pamięci z zapasem; tego sprawdzenia nie ma, jest tylko próg 8 GiB wolnego dysku.

## Konsekwencje

- Opóźnienie reakcji hamulca to 1–5 s (okno 100 ticków).
- Kopie świata robi sam `horizon-server` w swoim procesie, więc są w grupie gry; ani slice, ani hamulec ich nie spowalniają.
- Nie zrobione: `MemoryLow`, `MemoryMax` i `AllowedCPUs`. Wartości da dopiero pomiar na ThinkCentre (M0.11, `docs/sprzet.md`).
