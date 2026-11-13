//! PL: Instalator nie może wyłączyć kontroli zakupu: jednostka proxy nigdy nie ma `--offline`,
//! a silnik dostaje `online_mode = false` dopiero po sekrecie i adresie `127.0.0.1`.
//! EN: The installer must not turn off the purchase check: the proxy unit never has `--offline`,
//! and the engine gets `online_mode = false` only after the secret and the `127.0.0.1` address.

const SCRIPT: &str = include_str!("../../../scripts/server-install.sh");

fn position(needle: &str) -> usize {
    SCRIPT
        .find(needle)
        .unwrap_or_else(|| panic!("scripts/server-install.sh has no {needle:?}"))
}

#[test]
fn the_installer_never_starts_the_proxy_offline() {
    assert!(!SCRIPT.contains("--offline"));
    let unit = &SCRIPT[position("horizon-hot-proxy.service \"[Unit]")..];
    let exec = unit
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .unwrap_or_default();
    assert!(exec.contains("hot-proxy --listen 0.0.0.0:$PORT --engine 127.0.0.1:$ENGINE_PORT"));
}

#[test]
fn offline_mode_comes_after_the_secret_and_the_local_address() {
    let setup = position("proxy_setup() {");
    let secret = position("head -c 32 /dev/urandom");
    let bind = position("hz config set server.bind=127.0.0.1");
    let checked = position("grep -qx 'server.bind = \"127.0.0.1\"'");
    let offline = position("hz config set server.online_mode=false");
    assert!(setup < secret && secret < bind && bind < checked && checked < offline);
    assert_eq!(
        SCRIPT
            .matches("hz config set server.online_mode=false")
            .count(),
        1
    );
}
