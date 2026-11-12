# S3 — czy mod obsłuży własną komendę (`/sklep`) bez rejestracji w drzewie komend

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64.
- Krok planu: M0.4. Wynik: **potwierdzone**, z jednym zastrzeżeniem (`Host::tell`).

## Problem

Mody gorące nie mogą dopisywać komend do drzewa komend silnika w locie. Plan zakłada, że `hot-host` przechwytuje komendę gracza hakiem `player_command` i anuluje ją, zanim silnik odpowie „Unknown command”.

## Co sprawdzono

Trait mod `spikes/s3-s4-hak-komendy-i-tick/`, hak:

```rust
fn player_command(&self, host: &mut dyn Host, event: &mut PlayerCommandEvent<'_>) -> Flow {
    let cmd = event.command.trim_start_matches('/');
    if cmd == "sklep" || cmd.starts_with("sklep ") {
        event.replies.push(Text::literal("spike-hot-s2 reply: sklep otwarty".to_owned()));
        let told = host.tell(event.player, Text::literal("spike-hot-s2 tell: sklep otwarty".to_owned()));
        eprintln!("spike-hot-s2: /sklep handled, tell={told}");
        return Flow::Cancel;
    }
    Flow::Continue
}
```

Build: `horizon-launcher native-build <paczka> --server-output s2-server --version nightly-2026-11-05-66f3981 --trust-native-code` (26 s przyrostowo). Klient wysłał `/sklep`.

## Wynik

Czat klienta (`game_messages`): jedna linia `spike-hot-s2 reply: sklep otwarty`, bez „Unknown command”.

stderr silnika (`s2-srv/logs/supervisor.log`):

```
spike-hot-s2: player_command "sklep" from kamciosz
spike-hot-s2: /sklep handled, tell=false
```

## Wnioski

1. `Flow::Cancel` w `player_command` wycisza „Unknown command”. Komendy modów gorących idą tą drogą: `hot-host` pyta `hot-core`, czy któryś mod zna komendę.
2. Odpowiedź na komendę: **tylko przez `event.replies`**. `Host::tell` do gracza, który wywołał komendę, zwraca `false` na serwerze dedykowanym. Przyczyna w kodzie Horizona: `native_tell` → `native_send_all` (`crates/horizon-client/src/game/native_server.rs:246`) sprawdza `lan_acting_uuid` i hub LAN; na serwerze dedykowanym w tym haku żaden warunek nie jest spełniony. Czy `tell` działa poza hakiem gracza (np. z `server_tick`) — NIEWIADOMA, sprawdzić w M4 testem.
3. Logi z `eprintln!` w modzie natywnym trafiają do `logs/supervisor.log`, nie do `latest.log`. `hot-diag` musi pisać do własnego pliku, nie liczyć na stderr.
4. Podpowiedzi komend (Tab) dla `/sklep` nie będzie, bo komenda nie jest w drzewie. Do rozważenia w M4, jeśli gracze będą o to prosić.
