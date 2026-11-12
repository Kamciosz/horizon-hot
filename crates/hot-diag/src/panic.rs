//! PL: Panika dowolnego wątku trafia do logu jako `HOT-E0002` z miejscem i backtrace.
//! EN: A panic on any thread reaches the log as `HOT-E0002` with its location and backtrace.

use std::backtrace::Backtrace;
use std::panic::{self, PanicHookInfo};
use std::sync::Arc;

use crate::{codes, DiagError, Logger};

/// PL: Instaluje hak paniki. Poprzedni hak (domyślnie: wypis na stderr) nadal działa po zapisie.
/// EN: Installs the panic hook. The previous hook (by default: print to stderr) still runs after.
pub fn install_panic_hook(logger: Arc<Logger>) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        logger.error(&panic_entry(info, &Backtrace::force_capture().to_string()));
        previous(info);
    }));
}

fn panic_entry(info: &PanicHookInfo<'_>, backtrace: &str) -> DiagError {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-text panic payload".to_owned());
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let mut e = DiagError::new(
        codes::PANIC,
        format!("The program panicked on thread \"{thread}\": {message}"),
        "Open the file and line in `source`, read the backtrace in `chain`, fix the cause and release a new version",
    )
    .expected("The program runs without panicking")
    .context("thread", thread)
    .chain_line(message)
    .chain_line(backtrace.to_owned())
    .state("Inside a hot mod call the host disabled that mod and kept the previous generation (D4); elsewhere the process exits and systemd restarts it");
    if let Some(at) = info.location() {
        e = e.source_at(at.file(), at.line());
    }
    e
}
