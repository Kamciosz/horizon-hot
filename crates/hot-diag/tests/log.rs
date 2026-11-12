//! PL: Zapis wpisów D27, maskowanie sekretów, rotacja i błąd zapisu.
//! EN: D27 entries, secret masking, rotation and a write failure.

use std::fs;
use std::path::{Path, PathBuf};

use hot_diag::{codes, DiagError, LogConfig, Logger, Redactor};
use serde_json::Value;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hot-diag-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn entries(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[test]
fn error_entry_has_every_d27_field() -> Result<(), DiagError> {
    let dir = temp_dir("fields");
    let log = Logger::open(
        LogConfig::new("hotd", "0.1.0+abc1234", &dir),
        Redactor::new(),
    )?;
    let io = std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file mods/sklep/7.so not found",
    );
    log.error(
        &DiagError::new(
            codes::PANIC,
            "Hot mod \"sklep\" failed",
            "Rebuild the mod and push",
        )
        .expected("The mod loads")
        .context("mod", "sklep")
        .context("generation", 7)
        .cause("The CI artifact is missing")
        .caused_by(&io)
        .state("Generation 6 stays active"),
    );
    let e = &entries(&log.path())[0];
    for key in [
        "ts",
        "level",
        "code",
        "component",
        "version",
        "what",
        "expected",
        "context",
        "cause",
        "fix",
        "doc",
        "source",
        "chain",
        "state",
    ] {
        assert!(e.get(key).is_some(), "missing {key} in {e}");
    }
    assert_eq!(e["code"], "HOT-E0002");
    assert_eq!(e["component"], "hotd");
    assert_eq!(e["version"], "0.1.0+abc1234");
    assert_eq!(e["doc"], "docs/bledy/HOT-E0002.md");
    assert_eq!(e["context"]["generation"], 7);
    assert_eq!(e["chain"][0], "file mods/sklep/7.so not found");
    let source = e["source"].as_str().unwrap_or("");
    assert!(
        source.starts_with("crates/hot-diag/tests/log.rs:"),
        "{source}"
    );
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn known_secret_in_context_is_masked_in_the_file() -> Result<(), DiagError> {
    let dir = temp_dir("secret");
    let mut r = Redactor::new();
    r.add("fw-secret-0123456789");
    let log = Logger::open(LogConfig::new("hot-proxy", "0.1.0", &dir), r)?;
    log.error(
        &DiagError::new(codes::PANIC, "Forwarding failed", "Check forwarding.secret")
            .context("secret", "fw-secret-0123456789"),
    );
    let text = fs::read_to_string(log.path()).unwrap_or_default();
    assert!(!text.contains("fw-secret"), "{text}");
    assert!(text.contains("[secret]"), "{text}");
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn rotation_keeps_the_configured_number_of_files() -> Result<(), DiagError> {
    let dir = temp_dir("rotate");
    let mut config = LogConfig::new("hotd", "0.1.0", &dir);
    config.max_bytes = 2_000;
    config.files = 3;
    let log = Logger::open(config, Redactor::new())?;
    for n in 0..60 {
        log.error(&DiagError::new(codes::PANIC, format!("entry {n}"), "none").context("n", n));
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    assert_eq!(names, ["hotd.1.jsonl", "hotd.2.jsonl", "hotd.jsonl"]);
    for name in &names {
        let size = fs::metadata(dir.join(name))
            .map(|m| m.len())
            .unwrap_or(u64::MAX);
        assert!(size <= 2_000, "{name}: {size} B");
    }
    let last = entries(&dir.join("hotd.jsonl"));
    assert_eq!(
        last.last().map(|e| e["context"]["n"].clone()),
        Some(59.into())
    );
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn unwritable_log_directory_reports_hot_e0001() {
    let dir = temp_dir("blocked");
    let _ = fs::create_dir_all(&dir);
    let blocker = dir.join("not-a-dir");
    let _ = fs::write(&blocker, "a file where the log directory should be");
    match Logger::open(LogConfig::new("hotd", "0.1.0", &blocker), Redactor::new()) {
        Ok(_) => panic!("opening a log inside a file must fail"),
        Err(e) => {
            assert_eq!(e.code(), codes::LOG_WRITE_FAILED);
            let f = e.fields();
            assert!(
                f["what"].as_str().unwrap_or("").contains("not-a-dir"),
                "{f:?}"
            );
            assert!(f.contains_key("chain"), "{f:?}");
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn info_and_warn_entries_carry_level_context_and_masking() -> Result<(), DiagError> {
    let dir = temp_dir("info");
    let mut redactor = Redactor::new();
    redactor.add("tajny-token-123");
    let log = Logger::open(LogConfig::new("hot-core", "0.1.0", &dir), redactor)?;
    log.info(
        "Hot mod \"licznik\" generation 2 is active",
        serde_json::json!({ "mod": "licznik", "generation": 2 }),
    );
    log.warn(
        "Token tajny-token-123 was in a message",
        serde_json::json!({}),
    );
    let all = entries(&log.path());
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["level"], "info");
    assert_eq!(all[0]["component"], "hot-core");
    assert_eq!(all[0]["context"]["generation"], 2);
    assert_eq!(all[1]["level"], "warn");
    assert!(
        !all[1].to_string().contains("tajny-token-123"),
        "{}",
        all[1]
    );
    let _ = fs::remove_dir_all(dir);
    Ok(())
}
