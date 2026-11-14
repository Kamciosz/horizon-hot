//! PL: `hotd upgrade` przy zmianie tylko w `hot-proxy` (ADR 0033) na prawdziwych plikach: udawane
//! proxy na gnieździe odpowiada „przejęte”, odmawia albo milknie, a test patrzy, który plik
//! `bin/hot-proxy` zostaje na miejscu.
//! EN: `hotd upgrade` for a change only in `hot-proxy` (ADR 0033) on real files: a fake proxy on
//! the socket answers "taken over", refuses or goes silent, and the test checks which
//! `bin/hot-proxy` file stays in place.
#![allow(clippy::unwrap_used)] // PL: testy. EN: tests.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use hot_diag::{LogConfig, Logger, Redactor};
use hotd::upgrade::Env;
use hotd::upgrade_env::Real;

fn real(name: &str) -> Real {
    let home = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("proxy-{name}"));
    let _ = fs::remove_dir_all(&home);
    let bin = home.join("bin");
    fs::create_dir_all(bin.join(".next")).unwrap();
    fs::write(bin.join("hot-proxy"), "old").unwrap();
    fs::write(bin.join(".next").join("hot-proxy"), "new").unwrap();
    let log = Logger::open(
        LogConfig::new("hotd-upgrade", "test", home.join("logs")),
        Redactor::new(),
    )
    .unwrap();
    Real {
        home: home.clone(),
        root: home.clone(),
        src: home.clone(),
        bin,
        source: String::new(),
        git_ref: String::new(),
        server: PathBuf::from("horizon-server"),
        user: None,
        units: Vec::new(),
        log: Arc::new(log),
    }
}

/// PL: Udawane proxy: czyta jedno polecenie i odpowiada `reply` (pusta = zamyka bez odpowiedzi).
/// Zwraca polecenie. EN: A fake proxy: reads one command and answers `reply` (empty = closes
/// without a reply). Returns the command.
fn fake_proxy(home: &Path, reply: &'static str) -> JoinHandle<String> {
    let listener = UnixListener::bind(home.join("hot-proxy.sock")).unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut command = String::new();
        BufReader::new(&stream).read_line(&mut command).unwrap();
        if !reply.is_empty() {
            writeln!(&stream, "{reply}").unwrap();
        }
        command
    })
}

fn live(env: &Real) -> String {
    fs::read_to_string(env.bin.join("hot-proxy")).unwrap()
}

#[test]
fn a_takeover_keeps_the_new_file() {
    let mut env = real("taken");
    let proxy = fake_proxy(
        &env.home,
        r#"{"upgraded":true,"players":2,"handover_ms":80}"#,
    );
    assert!(env.load_proxy().unwrap());
    assert_eq!(
        proxy.join().unwrap().trim(),
        format!("upgrade {}", env.bin.join("hot-proxy").display())
    );
    assert_eq!(live(&env), "new");
    assert!(!env.bin.join(".next").exists());
}

#[test]
fn a_refusal_puts_the_old_file_back() {
    let mut env = real("refused");
    let proxy = fake_proxy(
        &env.home,
        r#"{"error":"HOT-E0609 a player is in configuration"}"#,
    );
    assert!(!env.load_proxy().unwrap());
    proxy.join().unwrap();
    assert_eq!(live(&env), "old");
}

#[test]
fn no_proxy_on_the_socket_puts_the_old_file_back() {
    let mut env = real("absent");
    assert!(!env.load_proxy().unwrap());
    assert_eq!(live(&env), "old");
}

#[test]
fn silence_after_the_command_is_a_rollback() {
    let mut env = real("silent");
    let proxy = fake_proxy(&env.home, "");
    let e = env.load_proxy().unwrap_err();
    proxy.join().unwrap();
    assert_eq!(e.code().as_str(), "HOT-E0708");
    assert_eq!(live(&env), "old");
}
