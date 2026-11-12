# S4 — czy mod może wykonać komendę serwera z haka `server_tick`

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64.
- Krok planu: M0.5. Wynik: **potwierdzone**.

## Problem

`hot-core` musi działać w tle bez gracza: aktualizacje slotów, ogłoszenia na czacie operatorów, zadania okresowe. Plan zakłada `Host::run_command` wywołane z haka `server_tick`.

## Co sprawdzono

Ten sam trait mod co S3 (`spikes/s3-s4-hak-komendy-i-tick/`):

```rust
fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
    if let ServerTickPhase::ServerTickEnd { world_running: true } = event.phase {
        let n = self.ticks.fetch_add(1, Ordering::Relaxed) + 1;
        if n % 100 == 0 && n <= 300 {
            let ok = host.run_command(&format!("say spike-hot-s2 tick {n}"));
            eprintln!("spike-hot-s2: run_command at tick {n} -> {ok}");
        }
    }
}
```

## Wynik

`s2-srv/logs/latest.log`:

```
[15:41:39] [Server thread/INFO]: [CHAT] [Server] spike-hot-s2 tick 100
[15:41:44] [Server thread/INFO]: [CHAT] [Server] spike-hot-s2 tick 200
[15:41:49] [Server thread/INFO]: [CHAT] [Server] spike-hot-s2 tick 300
```

`logs/supervisor.log`: `run_command at tick 100 -> true` (tak samo 200 i 300).

## Wnioski

1. `run_command` z `server_tick` działa i zwraca `true`. Odstęp 100 ticków = 5 s, czyli serwer tyka równo 20 razy na sekundę bez graczy.
2. Komenda wykonuje się w wątku serwera, wewnątrz ticku. Długa praca w tym miejscu zatrzymuje grę — `hot-core` robi w ticku tylko krótkie wywołania, resztę w osobnym wątku (D16).
3. Hak dostaje `world_running`; przy `false` (serwer wstrzymany, „Server empty for 60 seconds, pausing”) nie wolno zakładać, że świat się zmienia.
