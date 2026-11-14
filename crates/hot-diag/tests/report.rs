//! PL: `hot report` ze sztucznego błędu: pięć części D28, < 200 KB, sekrety i adresy zamaskowane.
//! EN: `hot report` from an artificial error: the five D28 parts, < 200 KB, secrets and IPs masked.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use hot_diag::report::{build_report, ReportInput, MAX_REPORT_BYTES, SECTIONS};
use hot_diag::{codes, DiagError, LogConfig, Logger, Redactor};

const SECRET: &str = "fw-secret-0123456789";

fn logs_with_artificial_error(name: &str) -> Result<PathBuf, DiagError> {
    let dir = std::env::temp_dir().join(format!("hot-diag-report-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let log = Logger::open(
        LogConfig::new("hotd", "0.1.0+abc1234", &dir),
        Redactor::new(),
    )?;
    log.error(
        &DiagError::new(
            codes::PANIC,
            "Artificial panic for the report test",
            "Nothing to fix",
        )
        .context("player_ip", "83.12.4.200")
        .context("secret", SECRET),
    );
    // PL: Duży log innego składnika (5 MB), żeby sprawdzić limit rozmiaru.
    // EN: A large log of another component (5 MB) to exercise the size limit.
    let line = format!("{{\"level\":\"info\",\"what\":\"{}\"}}\n", "x".repeat(1000));
    fs::write(dir.join("hot-proxy.jsonl"), line.repeat(5_000)).map_err(|e| {
        DiagError::new(
            codes::LOG_WRITE_FAILED,
            "Test log cannot be written",
            "Check /tmp",
        )
        .caused_by(&e)
    })?;
    fs::write(
        dir.join("hotd.1.jsonl"),
        "rotated file, must not be in the report\n",
    )
    .ok();
    Ok(dir)
}

#[test]
fn report_has_five_parts_in_order_and_fits_200_kb() -> Result<(), DiagError> {
    let dir = logs_with_artificial_error("lib")?;
    let mut redactor = Redactor::new();
    redactor.add(SECRET);
    let input = ReportInput {
        versions: vec![("Horizon".into(), "nightly-2026-11-05-66f3981".into())],
        status: format!("engine: running, forwarding secret {SECRET}"),
        logs_dir: dir.clone(),
        text_logs: Vec::new(),
        config: vec![("hot.toml".into(), "port = 25565\n".into())],
    };
    let report = build_report(&input, &redactor);

    assert!(report.len() < MAX_REPORT_BYTES, "{} B", report.len());
    let mut last = 0;
    for title in SECTIONS {
        let at = report.find(&format!("\n{title}\n")).unwrap_or(0);
        assert!(at > last, "{title} missing or out of order");
        last = at;
    }
    assert!(report.contains("Artificial panic for the report test"));
    assert!(report.contains("### Catalog entry HOT-E0002"));
    assert!(
        report.contains("\n#### HOT-E0002 — program spanikował\n"),
        "catalog text missing"
    );
    assert!(
        report.contains("\n##### Naprawa\n"),
        "catalog headings not demoted"
    );
    assert_eq!(
        report.matches("\n## ").count(),
        5,
        "only the five D28 parts are level 2"
    );
    assert!(
        report.contains("Kupiony Minecraft"),
        "AGENTS.md rules missing"
    );
    assert!(
        report.contains("earlier lines cut"),
        "large log was not trimmed"
    );
    assert!(
        !report.contains("rotated file"),
        "rotated logs must be skipped"
    );
    assert!(!report.contains(SECRET), "secret leaked");
    assert!(!report.contains("83.12.4.200"), "player IP leaked");
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn cli_writes_the_report_and_masks_the_secret_file() -> Result<(), DiagError> {
    let dir = logs_with_artificial_error("cli")?;
    let secret_file = dir.join("forwarding.secret");
    let out = dir.join("report.md");
    fs::write(&secret_file, format!("{SECRET}\n")).ok();
    let status = Command::new(env!("CARGO_BIN_EXE_hot-diag"))
        .args(["report", "--logs"])
        .arg(&dir)
        .arg("--secret-file")
        .arg(&secret_file)
        .arg("--out")
        .arg(&out)
        .status()
        .map_err(|e| {
            DiagError::new(codes::PANIC, "hot-diag did not start", "Build it").caused_by(&e)
        })?;
    assert!(status.success());
    let report = fs::read_to_string(&out).unwrap_or_default();
    assert!(report.len() < MAX_REPORT_BYTES && report.contains(SECTIONS[4]));
    assert!(!report.contains(SECRET));
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn the_log_folder_in_headings_is_masked_too() -> Result<(), DiagError> {
    let dir = logs_with_artificial_error("path")?;
    let mut redactor = Redactor::new();
    redactor.add(dir.display().to_string());
    let input = ReportInput {
        logs_dir: dir.clone(),
        text_logs: Vec::new(),
        ..ReportInput::default()
    };
    let report = build_report(&input, &redactor);
    assert!(report.contains("### Log: "), "{report}");
    assert!(!report.contains(&dir.display().to_string()), "path leaked");
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}
