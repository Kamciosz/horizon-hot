//! PL: Wkleja katalog błędów `docs/bledy/HOT-E*.md` do programu. `hot report` działa na serwerze,
//! gdzie repo nie ma, a raport ma zawierać treść wpisów (D28).
//! EN: Embeds the error catalog `docs/bledy/HOT-E*.md` into the program. `hot report` runs on the
//! server, where the repository is absent, and the report must carry the entries' text (D28).

use std::error::Error;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let catalog = manifest.join("../../docs/bledy").canonicalize()?;
    println!("cargo:rerun-if-changed={}", catalog.display());

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for item in fs::read_dir(&catalog)? {
        let path = item?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(code) = name.strip_suffix(".md").filter(|c| c.starts_with("HOT-E")) {
            println!("cargo:rerun-if-changed={}", path.display());
            entries.push((code.to_owned(), path.clone()));
        }
    }
    entries.sort();

    let mut out = String::from("pub static CATALOG: &[(&str, &str)] = &[\n");
    for (code, path) in &entries {
        writeln!(
            out,
            "    ({code:?}, include_str!({:?})),",
            path.display().to_string()
        )?;
    }
    out.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("catalog.rs"), out)?;
    Ok(())
}
