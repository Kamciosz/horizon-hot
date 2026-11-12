//! PL: Zgodność kodów błędów z katalogiem `docs/bledy/` (D27, M0.17). Kod użyty w `crates/` bez
//! pliku w katalogu albo plik bez użycia w kodzie = czerwone CI.
//! EN: Error codes match the `docs/bledy/` catalog (D27, M0.17). A code used in `crates/` without a
//! catalog file, or a file with no use in code = red CI.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SECTIONS: [&str; 6] = [
    "## Znaczenie",
    "## Typowe przyczyny",
    "## Diagnostyka",
    "## Naprawa",
    "## Co system robi sam",
    "## Kod",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// PL: Wszystkie `HOT-E` + dokładnie 4 cyfry. `HOT-EXXXX` (wzór) nie pasuje.
/// EN: Every `HOT-E` + exactly 4 digits. `HOT-EXXXX` (the template) does not match.
fn codes_in(text: &str) -> Vec<String> {
    let prefix = "HOT-E";
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(prefix) {
        let after = &rest[i + prefix.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let next_is_word = after
            .as_bytes()
            .get(digits)
            .is_some_and(u8::is_ascii_alphanumeric);
        if digits == 4 && !next_is_word {
            found.push(format!("{prefix}{}", &after[..4]));
        }
        rest = after;
    }
    found
}

fn used_codes() -> BTreeMap<String, Vec<String>> {
    let mut files = Vec::new();
    rust_files(&repo().join("crates"), &mut files);
    let mut used: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap_or_default();
        for code in codes_in(&text) {
            used.entry(code)
                .or_default()
                .push(file.display().to_string());
        }
    }
    used
}

fn catalog() -> BTreeMap<String, String> {
    fs::read_dir(repo().join("docs/bledy"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let code = name.strip_suffix(".md")?.to_owned();
            code.starts_with("HOT-E")
                .then(|| (code, fs::read_to_string(e.path()).unwrap_or_default()))
        })
        .collect()
}

#[test]
fn every_code_in_crates_has_a_catalog_entry() {
    let catalog = catalog();
    let missing: Vec<String> = used_codes()
        .into_iter()
        .filter(|(code, _)| !catalog.contains_key(code))
        .map(|(code, files)| format!("{code} used in {files:?} has no docs/bledy/{code}.md"))
        .collect();
    assert!(
        missing.is_empty(),
        "add a catalog entry (template: docs/bledy/README.md):\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_catalog_entry_is_used_in_crates() {
    let used: BTreeSet<String> = used_codes().into_keys().collect();
    let unused: Vec<String> = catalog()
        .into_iter()
        .filter(|(code, text)| !used.contains(code) && !text.contains("nieużywany od wersji"))
        .map(|(code, _)| code)
        .collect();
    assert!(
        unused.is_empty(),
        "catalog entries with no code in crates/ (use them or mark „nieużywany od wersji X”): {unused:?}"
    );
}

#[test]
fn every_catalog_entry_has_title_and_all_sections() {
    let mut problems = Vec::new();
    for (code, text) in catalog() {
        if !text.starts_with(&format!("# {code} — ")) {
            problems.push(format!(
                "{code}: first line must be \"# {code} — <short name>\""
            ));
        }
        let mut last = 0;
        for section in SECTIONS {
            match text.find(&format!("\n{section}\n")) {
                Some(at) if at >= last => last = at,
                Some(_) => problems.push(format!("{code}: \"{section}\" out of order")),
                None => problems.push(format!("{code}: missing \"{section}\"")),
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn embedded_catalog_matches_the_files() {
    let files: Vec<String> = catalog().into_keys().collect();
    let embedded: Vec<&str> = hot_diag::catalog_codes().collect();
    assert_eq!(embedded, files);
}

#[test]
fn scanner_ignores_templates_and_longer_tokens() {
    let text = "HOT-EXXXX HOT-E12345 HOT-E0412x HOT-E0001, \"HOT-E0002\"";
    assert_eq!(codes_in(text), ["HOT-E0001", "HOT-E0002"]);
}
