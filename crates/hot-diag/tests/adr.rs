//! PL: Decyzje w `docs/adr/` (D29, M12.1). Każdy ADR ma części Kontekst, Decyzja, Konsekwencje
//! i pole Status; indeks `docs/adr/README.md` wskazuje ADR dla każdej decyzji D1–D29 i każdego
//! pliku; każde odwołanie `D<n>` w kodzie i dokumentacji ma swój wiersz w indeksie.
//! EN: Decisions in `docs/adr/` (D29, M12.1). Every ADR has the Kontekst, Decyzja, Konsekwencje
//! parts and a Status field; the `docs/adr/README.md` index points to an ADR for every decision
//! D1–D29 and lists every file; every `D<n>` reference in code and docs has an index row.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PARTS: [&str; 4] = [
    "**Status:**",
    "## Kontekst",
    "## Decyzja",
    "## Konsekwencje",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// PL: Pliki `NNNN-*.md` z `docs/adr/`. EN: The `NNNN-*.md` files in `docs/adr/`.
fn adr_files() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(repo().join("docs/adr"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.ends_with(".md") && n.len() > 5 && n.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        })
        .collect();
    names.sort();
    names
}

/// PL: Wszystkie `D<liczba>` i `D<liczba>a` jako osobne słowa (`3D` i `D3D` nie pasują).
/// EN: Every `D<number>` and `D<number>a` as a separate word (`3D` and `D3D` do not match).
fn decisions_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut found = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'D' || (i > 0 && word(bytes[i - 1])) {
            continue;
        }
        let digits = bytes[i + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            continue;
        }
        let mut end = i + 1 + digits;
        if bytes.get(end) == Some(&b'a') && !bytes.get(end + 1).is_some_and(|&b| word(b)) {
            end += 1;
        }
        if bytes.get(end).is_some_and(|&b| word(b)) {
            continue;
        }
        found.push(text[i..end].to_owned());
    }
    found
}

/// PL: Pliki z kodem i dokumentacją poza `PLAN.md`, który decyzje definiuje.
/// EN: Code and doc files except `PLAN.md`, which defines the decisions.
fn text_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                text_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| {
            ["rs", "md", "sh", "ps1", "toml", "yml", "yaml"]
                .iter()
                .any(|x| e == *x)
        }) {
            out.push(path);
        }
    }
}

#[test]
fn every_adr_has_the_required_parts() {
    let files = adr_files();
    assert!(files.len() >= 32, "too few ADR files: {files:?}");
    let mut missing = Vec::new();
    for name in &files {
        let text = read(&repo().join("docs/adr").join(name));
        let title = format!("# ADR {} ", &name[..4]);
        if !text.starts_with(&title) {
            missing.push(format!("{name}: first line must start with `{title}`"));
        }
        for part in PARTS {
            if !text.contains(part) {
                missing.push(format!("{name}: no `{part}`"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "ADR parts missing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_index_covers_every_decision_and_file() {
    let index = read(&repo().join("docs/adr/README.md"));
    assert!(!index.is_empty(), "docs/adr/README.md is missing");
    let mut problems = Vec::new();
    let mut decisions: Vec<String> = (1..=29).map(|n| format!("D{n}")).collect();
    decisions.push("D6a".to_owned());
    for d in &decisions {
        let row = index.lines().find(|l| l.starts_with(&format!("| {d} |")));
        match row {
            Some(row) if row.contains(".md)") => {}
            Some(_) => problems.push(format!("{d}: the index row links no ADR")),
            None => problems.push(format!("{d}: no `| {d} |` row in the index")),
        }
    }
    for name in adr_files() {
        if !index.contains(&format!("]({name})")) {
            problems.push(format!("{name} is not linked from the index"));
        }
    }
    let mut rest = index.as_str();
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        let target = after.split(')').next().unwrap_or("");
        if target.ends_with(".md") && !repo().join("docs/adr").join(target).is_file() {
            problems.push(format!("the index links a missing file: {target}"));
        }
        rest = after;
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_decision_reference_has_an_index_row() {
    let index = read(&repo().join("docs/adr/README.md"));
    let known: BTreeSet<String> = index
        .lines()
        .filter_map(|l| l.strip_prefix("| "))
        .filter_map(|l| l.split(" |").next())
        .filter(|d| decisions_in(d).len() == 1 && decisions_in(d)[0] == *d)
        .map(str::to_owned)
        .collect();
    let mut files = Vec::new();
    for dir in ["crates", "docs", "scripts", "templates", "examples"] {
        text_files(&repo().join(dir), &mut files);
    }
    for name in ["AGENTS.md", "README.md", "CONTRIBUTING.md", "CHANGELOG.md"] {
        files.push(repo().join(name));
    }
    let mut unknown = Vec::new();
    for file in files {
        for d in decisions_in(&read(&file)) {
            if !known.contains(&d) {
                unknown.push(format!("{d} in {}", file.display()));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "decision references without a row in docs/adr/README.md:\n{}",
        unknown.join("\n")
    );
}

#[test]
fn decisions_are_found_as_whole_words() {
    assert_eq!(
        decisions_in("D1, D6a i D29 (D11). 3D, D3D, D6ab, HD2 i D"),
        ["D1", "D6a", "D29", "D11"]
    );
}
