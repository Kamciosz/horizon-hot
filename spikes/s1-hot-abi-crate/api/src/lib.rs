//! Spike S1: trait mod czyta stałą z hot-abi i wypisuje ją przy starcie serwera.
use horizon_client::mod_api::{Hooks, Host, ServerAboutToStartEvent};

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Spike)
}

struct Spike;

impl Hooks for Spike {
    fn server_about_to_start(&self, _host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        eprintln!("spike-hot-s1: hot_abi::ABI_MAJOR = {}", hot_abi::ABI_MAJOR);
    }
}
