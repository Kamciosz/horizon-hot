//! PL: `hotd report` (D28): log silnika i log instalatora są w raporcie, a sekrety serwera, klucze
//! prywatne i adresy graczy nie.
//! EN: `hotd report` (D28): the engine and installer logs are in the report; the server's secrets,
//! private keys and players' addresses are not.

use std::fs;
use std::path::PathBuf;

const FORWARDING: &str = "fwd-secret-6f1c9a0b2d7e4f3a";
const CI_KEY_LINE: &str = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAAB";
const DEPLOY_KEY_LINE: &str = "QyNTUxOQAAACDeployKeyBodyThatMustNotLeak0123456789";
const PLAYER_IP: &str = "203.0.113.45";

fn root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hotd-report-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn key(line: &str) -> String {
    format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{line}\n-----END OPENSSH PRIVATE KEY-----\n")
}

#[test]
fn the_report_has_engine_logs_and_no_server_secrets() -> std::io::Result<()> {
    let root = root();
    let home = root.join("server");
    fs::create_dir_all(home.join("logs"))?;
    fs::create_dir_all(home.join("hot-keys"))?;
    fs::create_dir_all(root.join(".ssh"))?;
    fs::create_dir_all(root.join("logs"))?;
    fs::write(home.join("forwarding.secret"), format!("{FORWARDING}\n"))?;
    fs::write(home.join("hot-keys").join("hot-ci"), key(CI_KEY_LINE))?;
    fs::write(
        home.join("hot-keys").join("allowed_signers"),
        "hot-ci ssh-ed25519 AAAA\n",
    )?;
    fs::write(root.join(".ssh").join("deploy"), key(DEPLOY_KEY_LINE))?;
    fs::write(home.join("server.toml"), "[server]\nport = 25566\n")?;
    fs::write(
        home.join("logs").join("latest.log"),
        format!(
            "[INFO] Player Steve joined from /{PLAYER_IP}:51234\n\
             [WARN] velocity secret {FORWARDING} rejected\n\
             [INFO] key part {CI_KEY_LINE}\n"
        ),
    )?;
    fs::write(
        home.join("logs").join("hotd.jsonl"),
        format!(
            "{{\"ts\":\"2026-11-14T10:00:00Z\",\"level\":\"error\",\"code\":\"HOT-E0201\",\
             \"what\":\"git failed with {DEPLOY_KEY_LINE}\"}}\n"
        ),
    )?;
    fs::write(
        root.join("logs").join("install.jsonl"),
        "{\"level\":\"info\",\"what\":\"installer step 13 done\"}\n",
    )?;

    let (input, redactor) = hotd::report::input(&home);
    let report = hot_diag::report::build_report(&input, &redactor);

    for leak in [FORWARDING, CI_KEY_LINE, DEPLOY_KEY_LINE, PLAYER_IP] {
        assert!(!report.contains(leak), "{leak} leaked into the report");
    }
    assert!(report.contains("latest.log"), "no engine log section");
    assert!(
        report.contains("Player Steve joined"),
        "engine log lines missing"
    );
    assert!(
        report.contains("installer step 13 done"),
        "installer log missing"
    );
    assert!(report.contains("port = 25566"), "server.toml missing");
    assert!(report.contains("HOT-E0201"), "error entry missing");
    assert!(report.contains("- hotd: "), "hotd version missing");
    assert!(report.len() < hot_diag::report::MAX_REPORT_BYTES);
    let _ = fs::remove_dir_all(root);
    Ok(())
}
