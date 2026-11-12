//! PL: Panika trafia do logu jako HOT-E0002 z miejscem i backtrace. Osobny plik testów, bo hak
//! paniki jest globalny dla procesu.
//! EN: A panic reaches the log as HOT-E0002 with its location and backtrace. A separate test file,
//! because the panic hook is global to the process.

use std::fs;
use std::sync::Arc;

use hot_diag::{install_panic_hook, DiagError, LogConfig, Logger, Redactor};
use serde_json::Value;

#[test]
fn panic_is_logged_with_location_and_backtrace() -> Result<(), DiagError> {
    let dir = std::env::temp_dir().join(format!("hot-diag-test-panic-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let log = Arc::new(Logger::open(
        LogConfig::new("hot-core", "0.1.0", &dir),
        Redactor::new(),
    )?);
    install_panic_hook(Arc::clone(&log));

    let result = std::thread::Builder::new()
        .name("server".into())
        .spawn(|| {
            let v: Vec<u32> = Vec::new();
            v[3]
        })
        .map(std::thread::JoinHandle::join);
    assert!(matches!(result, Ok(Err(_))), "the thread must panic");

    let text = fs::read_to_string(log.path()).unwrap_or_default();
    let e: Value = serde_json::from_str(text.lines().next().unwrap_or("{}")).unwrap_or_default();
    assert_eq!(e["code"], "HOT-E0002");
    assert!(
        e["what"]
            .as_str()
            .unwrap_or("")
            .contains("thread \"server\""),
        "{e}"
    );
    assert!(
        e["what"]
            .as_str()
            .unwrap_or("")
            .contains("index out of bounds"),
        "{e}"
    );
    assert!(
        e["source"]
            .as_str()
            .unwrap_or("")
            .starts_with("crates/hot-diag/tests/panic.rs:"),
        "{e}"
    );
    let backtrace = e["chain"][1].as_str().unwrap_or("");
    assert!(backtrace.contains("panic"), "no backtrace: {e}");
    let _ = fs::remove_dir_all(dir);
    Ok(())
}
