//! Spike S3/S4: własna komenda /sklep i komenda konsoli wołana z ticka serwera.
use std::sync::atomic::{AtomicU64, Ordering};

use horizon_client::mod_api::{
    Flow, Hooks, Host, PlayerCommandEvent, ServerTickEvent, ServerTickPhase, Text,
};

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Spike { ticks: AtomicU64::new(0) })
}

struct Spike {
    ticks: AtomicU64,
}

impl Hooks for Spike {
    fn player_command(&self, host: &mut dyn Host, event: &mut PlayerCommandEvent<'_>) -> Flow {
        eprintln!("spike-hot-s2: player_command {:?} from {}", event.command, event.name);
        let cmd = event.command.trim_start_matches('/');
        if cmd == "sklep" || cmd.starts_with("sklep ") {
            event.replies.push(Text::literal("spike-hot-s2 reply: sklep otwarty".to_owned()));
            let told = host.tell(event.player, Text::literal("spike-hot-s2 tell: sklep otwarty".to_owned()));
            eprintln!("spike-hot-s2: /sklep handled, tell={told}");
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if let ServerTickPhase::ServerTickEnd { world_running: true } = event.phase {
            let n = self.ticks.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 100 == 0 && n <= 300 {
                let ok = host.run_command(&format!("say spike-hot-s2 tick {n}"));
                eprintln!("spike-hot-s2: run_command at tick {n} -> {ok}");
            }
        }
    }
}
