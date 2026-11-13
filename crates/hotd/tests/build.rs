//! PL: `hotd::build` z atrapą `horizon-launcher` (skrypt sh): przypięcie w kopii paczki, wersja
//! bazowa, zgoda właściciela, sprzątanie po błędzie. Prawdziwego `native-build` testy nie wołają:
//! to kod natywny i wymaga zgody właściciela.
//! EN: `hotd::build` with a fake `horizon-launcher` (an sh script): the pin in the bundle copy, the
//! base version, the owner's consent, cleanup after an error. Tests never call the real
//! `native-build`: it is native code and needs the owner's consent.

#![allow(clippy::unwrap_used, clippy::expect_used)] // PL: testy. EN: tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hotd::build::{build_engine, pinned};

const OLD: &str = "66f3981f522b24c250b21523a70080965b940424";
const NEW: &str = "9f42064aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const FAKE: &str = r#"#!/bin/sh
here=$(dirname "$0")
echo "$*" >> "$here/calls.log"
case "$1" in
  versions) cat "$here/installed.json" ;;
  install) echo '{"installed":["nightly-2026-11-13-9f42064"]}' > "$here/installed.json" ;;
  native-build)
    [ -f "$here/fail" ] && { echo "error: wait_for_world does not apply" >&2; mkdir -p "$4"; exit 1; }
    mkdir -p "$4" && cp "$2/horizon-mixin.toml" "$4/pin.toml" && echo engine > "$4/horizon-engine" ;;
esac
"#;

fn setup(name: &str, installed: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("build-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bundle")).unwrap();
    let launcher = dir.join("horizon-launcher");
    fs::write(&launcher, FAKE).unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("installed.json"), installed).unwrap();
    fs::write(
        dir.join("bundle/horizon-mixin.toml"),
        format!("[mod]\nid = \"hot_host\"\n\n[compatibility]\nhorizon_revision = \"{OLD}\"\n"),
    )
    .unwrap();
    (dir.clone(), launcher, dir.join("bundle"))
}

fn calls(dir: &Path) -> String {
    fs::read_to_string(dir.join("calls.log")).unwrap_or_default()
}

#[test]
fn the_pin_chooses_the_revision_and_any_installed_nightly_is_the_base() {
    let (dir, launcher, bundle) = setup(
        "pin",
        r#"{"installed":["native-66f3981-782a","nightly-2026-11-05-66f3981"]}"#,
    );
    let out = dir.join("engines/new");
    let engine = build_engine(&launcher, &bundle, NEW, &out, true).unwrap();
    assert_eq!(engine, out.join("horizon-engine"));
    assert!(engine.is_file());
    let built_pin = fs::read_to_string(out.join("pin.toml")).unwrap();
    assert!(
        built_pin.contains(&format!("horizon_revision = \"{NEW}\"")),
        "{built_pin}"
    );
    assert!(
        built_pin.contains("id = \"hot_host\""),
        "the rest of the manifest stays"
    );
    assert_eq!(
        pinned(&bundle).as_deref(),
        Some(OLD),
        "the original bundle is unchanged"
    );
    assert!(
        !out.with_extension("bundle").exists(),
        "the copy is removed"
    );
    let log = calls(&dir);
    assert!(
        log.contains("--version nightly-2026-11-05-66f3981 --trust-native-code"),
        "{log}"
    );
    assert!(
        !log.contains("install latest"),
        "nothing is installed: {log}"
    );
    assert!(!log.contains("--ignore-pins"), "{log}");
}

#[test]
fn with_nothing_installed_the_newest_release_is_installed_first() {
    let (dir, launcher, bundle) = setup("latest", r#"{"installed":[]}"#);
    build_engine(&launcher, &bundle, OLD, &dir.join("engines/old"), true).unwrap();
    let log = calls(&dir);
    assert!(log.contains("install latest"), "{log}");
    assert!(
        log.contains("--version nightly-2026-11-13-9f42064"),
        "{log}"
    );
}

#[test]
fn without_the_owners_consent_nothing_runs() {
    let (dir, launcher, bundle) = setup("trust", r#"{"installed":["nightly-2026-11-05-66f3981"]}"#);
    let e = build_engine(&launcher, &bundle, NEW, &dir.join("engines/new"), false).unwrap_err();
    assert_eq!(e.code(), hotd::codes::ENGINE_BUILD_FAILED);
    assert_eq!(calls(&dir), "", "the launcher is never started");
    let e = build_engine(
        &launcher,
        &bundle,
        "9f42064",
        &dir.join("engines/new"),
        true,
    )
    .unwrap_err();
    assert_eq!(
        e.code(),
        hotd::codes::ENGINE_BUILD_FAILED,
        "a short hash is refused"
    );
}

#[test]
fn a_failed_build_leaves_no_engine_folder_and_says_why() {
    let (dir, launcher, bundle) = setup("fail", r#"{"installed":["nightly-2026-11-05-66f3981"]}"#);
    fs::write(dir.join("fail"), "").unwrap();
    let out = dir.join("engines/new");
    let e = build_engine(&launcher, &bundle, NEW, &out, true).unwrap_err();
    assert_eq!(e.code(), hotd::codes::ENGINE_BUILD_FAILED);
    assert!(!out.exists(), "the half-built folder is removed");
    assert!(!out.with_extension("bundle").exists());
    let fields = serde_json::Value::Object(e.fields()).to_string();
    assert!(fields.contains("wait_for_world does not apply"), "{fields}");
    assert!(fields.contains(NEW), "{fields}");
}
