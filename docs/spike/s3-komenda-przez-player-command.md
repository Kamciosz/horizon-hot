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
2. Odpowiedź na komendę: **tylko przez `event.replies`**. `Host::tell` do gracza, który wywołał komendę, zwraca `false` na serwerze dedykowanym. Przyczyna (z kodu Horizona, zgodna z obserwacją), ścieżki w `crates/horizon-client/src/game/`:
   - `native_tell` → `native_send_all` (`native_server.rs:246-256`) wysyła przez `self.lan`, a dla „gracza, który działa” — przez `lan_acting_uuid`;
   - na czas obsługi pakietów graczy hub jest wyjęty z `self.lan` (`lan.rs:2075`, `lan.rs:2218`: „the hub is back after the commands”) i przekazany do `guest_command` (`lan_actions.rs:3357`) jako parametr, więc `self.lan` jest wtedy `None`;
   - `lan_acting_uuid` ustawiają tylko komendy wykonywane w imieniu gracza (`commands/world_cmds.rs:1973`), nie hak `player_command`.

   Poza obsługą pakietów (np. z `server_tick`) hub jest na miejscu, więc `tell` powinien działać — HIPOTEZA, sprawdzić testem w M4.
3. Logi z `eprintln!` w modzie natywnym trafiają do `logs/supervisor.log`, nie do `latest.log`. `hot-diag` musi pisać do własnego pliku, nie liczyć na stderr.
4. Podpowiedzi komend (Tab) dla `/sklep` nie będzie, bo komenda nie jest w drzewie. Do rozważenia w M4, jeśli gracze będą o to prosić.
