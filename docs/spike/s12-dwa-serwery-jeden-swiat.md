# S12 — czy dwa serwery mogą na zmianę używać jednego świata

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64 (Apple M4 Pro, 24 GB), silnik bez modów.
- Krok planu: M0.13. Wynik: **potwierdzone**. Jednoczesne otwarcie świata jest zablokowane przez sam silnik.

## Problem

Podmiana silnika (wariant A, D10–D12) zakłada dwa supervisory `horizon-server`: stary oddaje świat, nowy go przejmuje. Trzeba sprawdzić, że świat może leżeć poza katalogiem serwera, że drugi serwer nie otworzy go równocześnie z pierwszym i ile trwa przekazanie.

## Co sprawdzono

```bash
horizon-server --dir a init --eula --world world --port 25613 --offline --yes --no-whitelist
horizon-server --dir b init --eula --world world --port 25566 --offline --yes --no-whitelist
mkdir -p shared/world
ln -s ../shared/world a/world
ln -s ../shared/world b/world
```

1. Start A, `forceload add 0 0`, `setblock 0 120 0 minecraft:gold_block`, `save-all flush`.
2. Start B, gdy A nadal działa.
3. Pięć przekazań na zmianę: `horizon-server --dir <stary> stop`, `horizon-server --dir <nowy> start`, czekanie na `Done (` w `logs/latest.log`. Po drodze B stawia `diamond_block` na 1 120 0, a A to sprawdza.

## Wynik

Krok 2 — B odmawia:

```
[Server thread/ERROR]: Horizon: .../s12/b/./world is already open in another instance
[Server thread/ERROR]: Horizon crashed: .../s12/b/./world is already open in another instance
```

Supervisor B traktuje to jak awarię i ponawia start po 5, 10, 20, 40 s (`restart_on_crash = true`). `horizon-server status`: `Restarting, up 1 min, 4 crashes`. Każda próba zostawia raport w `crash-reports/`.

Krok 3 — przekazania (zegar ścienny, z narzutem pomiaru ok. 0,1 s):

| Przekazanie | `stop` starego | `start` nowego do `Done` | Razem |
|---|---|---|---|
| A → B | 0,44 s | 0,56 s | 1,00 s |
| B → A | 0,66 s | 0,54 s | 1,20 s |
| A → B | 0,43 s | 0,54 s | 0,98 s |
| B → A | 0,44 s | 0,54 s | 0,99 s |
| A → B | 0,44 s | 0,54 s | 0,99 s |

`execute if block 0 120 0 minecraft:gold_block` na B i `execute if block 1 120 0 minecraft:diamond_block` na A: `Test passed`.

## Wnioski

1. Świat poza katalogiem serwera działa przez dowiązanie symboliczne `world -> ../shared/world`. Nie trzeba zmieniać `level-name`.
2. Silnik trzyma `session.lock` (`crates/horizon-client/src/save.rs:136`, jak `DirectoryLock` w grze). Drugi silnik nie otworzy świata, dopóki pierwszy działa — uszkodzenie świata przez dwa silniki naraz jest wykluczone na poziomie silnika.
3. **`hotd` nie może startować nowego supervisora, zanim stary się zatrzyma.** Inaczej nowy wpada w pętlę restartów z rosnącą przerwą (5 → 40 s) i wydłuża podmianę. Kolejność: `stop` starego → `start` nowego. Supervisor nowej strony ma `restart_on_crash = true` dopiero po pierwszym udanym starcie albo `hotd` sprawdza `session.lock` przed startem.
   **Zmiana po S13 (2026-11-13):** silnik z `hot-host` czeka na zwolnienie świata w miejscu (mixin `wait_for_world`) i startuje przed `stop` starego. Ten wniosek dotyczy już tylko przerwanej podmiany (`hotd` zatrzymuje czekający silnik) i silnika bez `hot-host` (`docs/spike/s13-rozgrzany-silnik.md`).
4. Przekazanie na pustym, małym świecie: ok. 1 s na tym Macu. To dolna granica pauzy dla wariantu A bez rozgrzewania; na ThinkCentre i z graczami (więcej chunków do zapisu) będzie dłużej — zmierzyć w M0.9 i M0.11.
5. Rozgrzanie nowego silnika przed zwolnieniem świata nie jest możliwe zwykłym startem: blokada sprawdzana jest zaraz po „Preparing level” i wczytaniu danych generatora (w logu B ok. 0,1–0,2 s), a nie później. Zysk byłby niewielki, bo cały start trwa ok. 0,5 s.
6. Dwa supervisory na jednej maszynie walczą o port panelu (`[panel] bind = "127.0.0.1:8130"` w obu): drugi loguje `The web panel can't listen on 127.0.0.1:8130: Address already in use`. Instalator ustawia panel tylko po jednej stronie albo różne porty.
7. Przy okazji: `server.toml` ma sekcję `[transfer]` — „A proxy handing players over between servers”, z zamrożeniem gracza (`max_freeze_seconds`) i Velocity forwarding. To może być gotowa droga dla wariantu A. Sprawdzić w M0.9 przed pisaniem własnego proxy.
