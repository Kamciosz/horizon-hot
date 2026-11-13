//! PL: Instalator i kontener nie mogą wyłączyć kontroli zakupu: proxy nigdy nie ma `--offline`,
//! a silnik dostaje `online_mode = false` dopiero po sekrecie i adresie `127.0.0.1`. Kontener
//! wystawia na zewnątrz tylko port proxy i paczki, nigdy port silnika.
//! EN: The installer and the container must not turn off the purchase check: the proxy never has
//! `--offline`, and the engine gets `online_mode = false` only after the secret and the
//! `127.0.0.1` address. The container publishes only the proxy and pack ports, never the engine's.

const INSTALLER: &str = include_str!("../../../scripts/server-install.sh");
const CONTAINER: &str = include_str!("../../../scripts/container-entrypoint.sh");
const COMPOSE: &str = include_str!("../../../compose.yaml");
const DOCKERFILE: &str = include_str!("../../../Dockerfile");

fn position(script: &str, needle: &str) -> usize {
    script
        .find(needle)
        .unwrap_or_else(|| panic!("the script has no {needle:?}"))
}

fn assert_proxy_setup_order(script: &str) {
    let setup = position(script, "proxy_setup() {");
    let secret = position(script, "head -c 32 /dev/urandom");
    let bind = position(script, "hz config set server.bind=127.0.0.1");
    let checked = position(script, "grep -qx 'server.bind = \"127.0.0.1\"'");
    let offline = position(script, "hz config set server.online_mode=false");
    assert!(setup < secret && secret < bind && bind < checked && checked < offline);
    assert_eq!(
        script
            .matches("hz config set server.online_mode=false")
            .count(),
        1
    );
}

#[test]
fn the_installer_never_starts_the_proxy_offline() {
    assert!(!INSTALLER.contains("--offline"));
    let unit = &INSTALLER[position(INSTALLER, "horizon-hot-proxy.service \"[Unit]")..];
    let exec = unit
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .unwrap_or_default();
    assert!(exec.contains("hot-proxy --listen 0.0.0.0:$PORT --engine 127.0.0.1:$ENGINE_PORT"));
}

#[test]
fn offline_mode_comes_after_the_secret_and_the_local_address() {
    assert_proxy_setup_order(INSTALLER);
}

#[test]
fn the_container_never_starts_the_proxy_offline() {
    assert!(!CONTAINER.contains("--offline"));
    assert!(CONTAINER.contains(
        "CMD[proxy]=\"$HOTBIN/hot-proxy --listen 0.0.0.0:$PORT --engine 127.0.0.1:$ENGINE_PORT"
    ));
    assert_proxy_setup_order(CONTAINER);
    // PL: Ustawienie silnika za proxy przed startem procesów. EN: The engine is set up before anything starts.
    assert!(position(CONTAINER, "  proxy_setup\n") < position(CONTAINER, "  supervise\n"));
}

#[test]
fn the_container_publishes_only_the_proxy_and_the_pack() {
    let ports: Vec<&str> = COMPOSE
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- \"") && l.ends_with('"') && l.contains(':'))
        .filter(|l| !l.contains('/'))
        .collect();
    assert_eq!(ports, ["- \"25565:25565\"", "- \"8131:8131\""]);
    assert!(!COMPOSE.contains("network_mode"));
    let expose = DOCKERFILE
        .lines()
        .find(|l| l.starts_with("EXPOSE "))
        .unwrap_or_default();
    assert_eq!(expose, "EXPOSE 25565 8131");
}

#[test]
fn only_the_owner_turns_on_native_code_in_the_container() {
    let enabled = COMPOSE
        .lines()
        .filter(|l| l.contains("HOT_TRUST_NATIVE_CODE"))
        .all(|l| l.trim_start().starts_with('#'));
    assert!(
        enabled,
        "compose.yaml must keep HOT_TRUST_NATIVE_CODE commented out"
    );
    assert!(!DOCKERFILE.contains("HOT_TRUST_NATIVE_CODE"));
    assert!(CONTAINER.contains("TRUST_NATIVE=${HOT_TRUST_NATIVE_CODE:-0}"));
}

/// PL: Wartość `NAZWA=wartość` z początku linii. EN: The value of a `NAME=value` line.
fn assignment<'a>(script: &'a str, name: &str) -> &'a str {
    script
        .lines()
        .find_map(|l| l.strip_prefix(name)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {name}= line"))
        .split_whitespace()
        .next()
        .unwrap_or_default()
}

/// PL: Datowane wydania Nightly znikają z GitHuba po kilku dniach, więc Horizon budujemy ze
/// źródeł, w tej samej pełnej rewizji w instalatorze i w obrazie.
/// EN: Dated Nightly releases vanish from GitHub after a few days, so Horizon is built from
/// source, at the same full revision in the installer and in the image.
#[test]
fn installer_and_image_build_the_same_horizon_revision_from_source() {
    let installer = assignment(INSTALLER, "HORIZON_REV");
    let image = assignment(DOCKERFILE, "ARG HORIZON_REV");
    assert_eq!(
        installer.len(),
        40,
        "HORIZON_REV must be a full commit hash"
    );
    assert_eq!(installer, image);
    assert_eq!(
        assignment(INSTALLER, "HORIZON_VERSION"),
        assignment(DOCKERFILE, "ARG HORIZON_VERSION")
    );
    for script in [INSTALLER, DOCKERFILE] {
        assert!(!script.contains("packaging/server/install.sh"));
        assert!(script.contains("--no-default-features --bin horizon-client"));
    }
}
