//! PL: Paczka zasobów serwera (D9, M5.2). Wygląd i nazwy bloków ze slotów: z
//! `mods/<id>/assets/blocks/<nazwa>.png` i `mods/<id>/assets/lang.json` powstają model, tekstura
//! i napis slotu w przestrzeni `hot_slots`. Paczka jest powtarzalna (posortowane wpisy, stała
//! data, bez kompresji), więc sha1 zmienia się tylko wtedy, gdy zmienia się treść.
//!
//! `serve` podaje plik pod `GET /pack.zip` (`hotd serve-pack`, osobny proces). Silnik czyta adres
//! paczki tylko przy starcie, a `server.toml` nie ma sha1, więc klient pobiera paczkę przy każdym
//! wejściu i widzi nowy wygląd bez restartu silnika (PLAN, M5.2).
//!
//! EN: The server resource pack (D9, M5.2): the look and names of slot blocks. Each
//! `mods/<id>/assets/blocks/<name>.png` and `mods/<id>/assets/lang.json` become a slot's model,
//! texture and name in the `hot_slots` namespace. The pack is reproducible (sorted entries, a
//! fixed date, no compression), so its sha1 changes only with its content. `serve` hands the file
//! out at `GET /pack.zip` (`hotd serve-pack`, a separate process). The engine reads the pack URL
//! only at start and `server.toml` has no sha1, so the client downloads the pack at every join and
//! sees a new look without an engine restart.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hot_diag::{DiagError, Logger};
use serde_json::{json, Value};

use crate::codes;
use crate::slots::{Slots, NAMESPACE};

/// PL: Format paczki zasobów sprawdzony w S5 (gra 26.3). EN: The resource pack format proven in S5.
const PACK_FORMAT: u32 = 97;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// PL: Wygląd jednego bloku z repo. EN: One block's look from the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub texture: Vec<u8>,
    pub name: Option<String>,
}

/// PL: Nazwy bloków z `lang.json` moda (`{"<nazwa>": "Nazwa w grze"}`).
/// EN: Block names from a mod's `lang.json`.
///
/// # Errors
/// PL: Plik nie jest obiektem z napisami. EN: The file is not an object of strings.
pub fn parse_lang(id: &str, bytes: &[u8]) -> Result<BTreeMap<String, String>, DiagError> {
    let bad = |reason: String| {
        pack_error(
            &format!("mods/{id}/assets/lang.json"),
            reason,
            "Make lang.json an object of strings, e.g. {\"lada\": \"Lada sklepu\"}",
        )
    };
    let value: Value = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
    let Some(map) = value.as_object() else {
        return Err(bad("the top level is not an object".into()));
    };
    map.iter()
        .map(|(k, v)| match v.as_str() {
            Some(s) => Ok((k.clone(), s.to_owned())),
            None => Err(bad(format!("the value of {k} is not a string"))),
        })
        .collect()
}

/// PL: Sprawdza teksturę bloku. EN: Checks a block texture.
///
/// # Errors
/// PL: To nie jest PNG. EN: Not a PNG.
pub fn check_texture(file: &str, bytes: &[u8]) -> Result<(), DiagError> {
    if bytes.starts_with(PNG_MAGIC) {
        Ok(())
    } else {
        Err(pack_error(
            file,
            "the file does not start with the PNG signature".into(),
            "Save the texture as PNG (16x16 or another square size) and push a new commit",
        ))
    }
}

fn pack_error(file: &str, cause: String, fix: &str) -> DiagError {
    DiagError::new(
        codes::PACK_FAILED,
        format!("hotd cannot put {file} into the resource pack"),
        fix,
    )
    .expected("a PNG texture per block and an object of strings in lang.json")
    .cause(cause)
    .context("file", file.to_owned())
    .state("Nothing changed: players get the previous resource pack")
}

/// PL: Pliki paczki: ścieżka → treść. Tylko bloki, które mają slot i wygląd w tym commicie.
/// EN: The pack files: path → content. Only blocks with both a slot and a look in this commit.
#[must_use]
pub fn files(slots: &Slots, looks: &BTreeMap<String, Look>) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut lang = serde_json::Map::new();
    for (block, look) in looks {
        let Some(slot) = slots.blocks.get(block) else {
            continue;
        };
        let name = format!("slot_{slot}");
        let texture = format!("{NAMESPACE}:block/{name}");
        out.insert(
            format!("assets/{NAMESPACE}/textures/block/{name}.png"),
            look.texture.clone(),
        );
        out.insert(
            format!("assets/{NAMESPACE}/models/block/{name}.json"),
            json!({ "parent": "minecraft:block/cube_all", "textures": { "all": texture } })
                .to_string()
                .into_bytes(),
        );
        let shown = look.name.clone().unwrap_or_else(|| block.clone());
        lang.insert(format!("block.{NAMESPACE}.{name}"), Value::String(shown));
    }
    out.insert(
        format!("assets/{NAMESPACE}/lang/en_us.json"),
        Value::Object(lang).to_string().into_bytes(),
    );
    out.insert(
        "pack.mcmeta".into(),
        json!({ "pack": {
            "description": "horizon-hot: bloki modów",
            "min_format": PACK_FORMAT,
            "max_format": PACK_FORMAT,
        } })
        .to_string()
        .into_bytes(),
    );
    out
}

/// PL: Archiwum zip bez kompresji, wpisy w kolejności nazw, data 1980-01-01.
/// EN: A zip archive without compression, entries in name order, dated 1980-01-01.
#[must_use]
pub fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    const DOS_DATE: u16 = 0x0021; // 1980-01-01
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let (crc, size) = (crc32(data), u32::try_from(data.len()).unwrap_or(u32::MAX));
        let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        let common = |b: &mut Vec<u8>| {
            for v in [0u16, 0, 0, DOS_DATE] {
                b.extend_from_slice(&v.to_le_bytes()); // flags, method, time, date
            }
            for v in [crc, size, size] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&name_len.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes()); // extra
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        common(&mut central);
        for v in [0u16, 0, 0] {
            central.extend_from_slice(&v.to_le_bytes()); // comment, disk, internal attrs
        }
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let start = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let count = u16::try_from(files.len()).unwrap_or(u16::MAX);
    let size = u32::try_from(central.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, count, count] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// PL: SHA-1 jako 40 znaków hex (klient porównuje paczkę po sha1, D9).
/// EN: SHA-1 as 40 hex characters (the client keys packs by sha1, D9).
#[must_use]
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

/// PL: Katalog paczki w `hot-state`. EN: The pack folder in `hot-state`.
#[must_use]
pub fn dir(home: &Path) -> PathBuf {
    home.join("hot-state").join("pack")
}

/// PL: Zapisuje `pack.zip` i `pack.sha1` przez pliki tymczasowe; zwraca sha1.
/// EN: Writes `pack.zip` and `pack.sha1` through temporary files; returns the sha1.
///
/// # Errors
/// PL: Zapis się nie udał. EN: The write failed.
pub fn install(home: &Path, zip: &[u8]) -> Result<String, DiagError> {
    let dir = dir(home);
    let sha1 = sha1_hex(zip);
    let put = |name: &str, bytes: &[u8]| {
        let tmp = dir.join(format!("{name}.tmp"));
        fs::write(&tmp, bytes)
            .and_then(|()| fs::rename(&tmp, dir.join(name)))
            .inspect_err(|_| {
                // PL: Ucięty plik zabrałby miejsce potrzebne do ponowienia.
                // EN: A torn file would hold the space the retry needs.
                let _ = fs::remove_file(&tmp);
            })
    };
    fs::create_dir_all(&dir)
        .and_then(|()| put("pack.zip", zip))
        .and_then(|()| put("pack.sha1", format!("{sha1}\n").as_bytes()))
        .map_err(|e| {
            pack_error(
                &dir.join("pack.zip").display().to_string(),
                e.to_string(),
                "Free disk space or give the hotd user the hot-state folder (chown -R horizon: <HOT_HOME>/hot-state), then wait for the next hotd step",
            )
        })?;
    Ok(sha1)
}

/// PL: Podaje `GET /pack.zip` i `GET /hot_slots.hmod` z katalogu paczki oraz `GET /hot-play.json`
/// dla aplikacji gracza. Pliki czytane przy każdym żądaniu, więc nowa paczka idzie od razu. Zanim `hotd` zbuduje pierwszą paczkę, idzie
/// paczka bez bloków: przy `require-resource-pack = true` brak paczki wyrzuciłby gracza. Każde
/// połączenie w osobnym wątku, z limitem czasu.
/// EN: Serves `GET /pack.zip` and `GET /hot_slots.hmod` from the pack folder and `GET /hot-play.json`
/// for the player app. Files are read per request, so a new pack goes out at once. Until `hotd` builds the first pack, a pack without
/// blocks goes out: with `require-resource-pack = true` a missing pack would kick the player. One
/// thread per connection, with a timeout.
pub fn serve(listener: &TcpListener, home: &Path, log: &Arc<Logger>) {
    let folder = dir(home);
    let base: Arc<[u8]> = zip(&files(&Slots::default(), &BTreeMap::new())).into();
    for stream in listener.incoming().flatten() {
        let (home, folder, base, log) = (
            home.to_owned(),
            folder.clone(),
            Arc::clone(&base),
            Arc::clone(log),
        );
        std::thread::spawn(move || {
            if let Err(e) = answer(stream, &home, &folder, &base) {
                log.warn(
                    "hotd serve-pack could not answer a request",
                    json!({ "error": e.to_string() }),
                );
            }
        });
    }
}

/// PL: Co aplikacja gracza (`hot-play`, D26) musi zgrać z serwerem: rewizja Horizona działającego
/// silnika (zmienia się po podmianie z M9.1, więc liczona przy każdym żądaniu) i sha1 puli slotów.
/// EN: What the player app (`hot-play`, D26) has to match with the server: the running engine's
/// Horizon revision (it changes after an M9.1 swap, so it is read per request) and the slot pool's
/// sha1.
#[must_use]
pub fn play_info(home: &Path) -> Value {
    let horizon = fs::read_to_string(home.join("server.toml"))
        .ok()
        .and_then(|text| crate::swap::toml_value(&text, "engine", "path"))
        .and_then(|engine| crate::build::engine_revision(Path::new(&engine)))
        // PL: Silnik bez `native-build` (zbudowany przez instalator ze źródeł) nie ma
        // `native-mods.json`; jego rewizję zapisuje instalator. EN: An engine without
        // `native-build` (built from source by the installer) has no `native-mods.json`; the
        // installer writes its revision.
        .or_else(|| {
            fs::read_to_string(home.join("horizon-rev"))
                .ok()
                .map(|t| t.trim().to_owned())
                .filter(|r| crate::build::is_revision(r))
        });
    let slots = fs::read(dir(home).join("hot_slots.hmod"))
        .ok()
        .map(|b| sha1_hex(&b));
    json!({ "horizon": horizon, "slots_sha1": slots })
}

fn answer(mut stream: TcpStream, home: &Path, folder: &Path, base: &[u8]) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 8192 {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(&buf[..n]);
    }
    let line = String::from_utf8_lossy(&head);
    let mut words = line.split_whitespace();
    let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let path = path.split('?').next().unwrap_or("");
    let body = match (method, path) {
        ("GET" | "HEAD", "/pack.zip") => Some(
            fs::read(folder.join("pack.zip"))
                .map(|b| (b, "application/zip"))
                .unwrap_or_else(|_| (base.to_vec(), "application/zip")),
        ),
        ("GET" | "HEAD", "/hot_slots.hmod") => fs::read(folder.join("hot_slots.hmod"))
            .ok()
            .map(|b| (b, "application/octet-stream")),
        ("GET" | "HEAD", "/hot-play.json") => {
            Some((play_info(home).to_string().into_bytes(), "application/json"))
        }
        _ => None,
    };
    let Some(body) = body else {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let (body, kind) = body;
    stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    )?;
    if method == "GET" {
        stream.write_all(&body)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_and_crc32_match_known_values() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(sha1_hex(long), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    fn look(byte: u8, name: Option<&str>) -> Look {
        let mut texture = PNG_MAGIC.to_vec();
        texture.push(byte);
        Look {
            texture,
            name: name.map(str::to_owned),
        }
    }

    fn slots(items: &[(&str, u32)]) -> Slots {
        Slots {
            blocks: items.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
        }
    }

    #[test]
    fn a_block_gets_its_model_texture_and_name() {
        let looks = BTreeMap::from([("sklep:lada".to_owned(), look(1, Some("Lada")))]);
        let f = files(&slots(&[("sklep:lada", 7)]), &looks);
        assert!(f.contains_key("assets/hot_slots/textures/block/slot_7.png"));
        let model = String::from_utf8_lossy(&f["assets/hot_slots/models/block/slot_7.json"]);
        assert!(model.contains("hot_slots:block/slot_7"), "{model}");
        let lang = String::from_utf8_lossy(&f["assets/hot_slots/lang/en_us.json"]);
        assert!(
            lang.contains("\"block.hot_slots.slot_7\":\"Lada\""),
            "{lang}"
        );
        assert!(f.contains_key("pack.mcmeta"));
    }

    #[test]
    fn the_pack_changes_only_with_its_content() {
        let s = slots(&[("a:x", 0), ("a:y", 1)]);
        let one = BTreeMap::from([
            ("a:x".to_owned(), look(1, None)),
            ("a:y".to_owned(), look(2, None)),
        ]);
        let same = sha1_hex(&zip(&files(&s, &one)));
        assert_eq!(same, sha1_hex(&zip(&files(&s, &one.clone()))));
        let mut other = one.clone();
        other.insert("a:y".into(), look(3, None));
        assert_ne!(same, sha1_hex(&zip(&files(&s, &other))));
    }

    #[test]
    fn the_zip_opens_with_unzip() {
        let f = files(
            &slots(&[("a:x", 0)]),
            &BTreeMap::from([("a:x".to_owned(), look(1, None))]),
        );
        let dir = std::env::temp_dir().join(format!("hotd-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).ok();
        let file = dir.join("p.zip");
        fs::write(&file, zip(&f)).ok();
        let out = std::process::Command::new("unzip")
            .arg("-tq")
            .arg(&file)
            .output();
        let _ = fs::remove_dir_all(&dir);
        let o = out.map_err(|e| format!("unzip is needed for this test: {e}"));
        assert!(
            o.as_ref().is_ok_and(|o| o.status.success()),
            "{:?}",
            o.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        );
    }

    #[test]
    fn bad_textures_and_lang_files_are_errors() {
        assert!(check_texture("a.png", b"GIF89a").is_err());
        assert!(check_texture("a.png", PNG_MAGIC).is_ok());
        assert!(parse_lang("a", b"[1]").is_err());
        assert!(parse_lang("a", br#"{"x": 1}"#).is_err());
        assert_eq!(
            parse_lang("a", r#"{"x": "Ładny"}"#.as_bytes()).ok(),
            Some(BTreeMap::from([("x".to_owned(), "Ładny".to_owned())]))
        );
    }

    #[test]
    fn serve_answers_only_the_pack() {
        let home = std::env::temp_dir().join(format!("hotd-serve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
            return;
        };
        let addr = listener.local_addr().ok();
        let Ok(log) = Logger::open(
            hot_diag::LogConfig::new("hotd-test", "0", home.join("logs")),
            hot_diag::Redactor::new(),
        ) else {
            panic!("no logger");
        };
        let log = Arc::new(log);
        let home2 = home.clone();
        std::thread::spawn(move || serve(&listener, &home2, &log));
        let get = |path: &str| {
            let mut s = TcpStream::connect(addr?).ok()?;
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .ok()?;
            let mut out = Vec::new();
            s.read_to_end(&mut out).ok()?;
            Some(String::from_utf8_lossy(&out).into_owned())
        };
        let first = get("/pack.zip").unwrap_or_default();
        assert!(
            first.starts_with("HTTP/1.1 200") && first.contains("pack.mcmeta"),
            "before the first build the pack without blocks goes out: {first}"
        );
        assert!(get("/hot_slots.hmod")
            .unwrap_or_default()
            .starts_with("HTTP/1.1 404"));
        let Ok(sha1) = install(&home, b"zawartosc") else {
            panic!("install failed");
        };
        assert_eq!(sha1, sha1_hex(b"zawartosc"));
        let info = get("/hot-play.json").unwrap_or_default();
        assert!(
            info.ends_with(r#"{"horizon":null,"slots_sha1":null}"#),
            "{info}"
        );
        let built = "1111111111111111111111111111111111111111";
        let _ = fs::write(home.join("horizon-rev"), format!("{built}\n"));
        let info = get("/hot-play.json").unwrap_or_default();
        assert!(
            info.ends_with(&format!(r#"{{"horizon":"{built}","slots_sha1":null}}"#)),
            "an engine built from source: {info}"
        );
        let _ = fs::write(dir(&home).join("hot_slots.hmod"), b"pula");
        let engine = home.join("engines").join("e").join("horizon-server");
        let _ = fs::create_dir_all(engine.parent().unwrap_or(&home));
        let rev = "66f3981f522b24c250b21523a70080965b940424";
        let _ = fs::write(
            engine.with_file_name("native-mods.json"),
            format!(r#"{{"horizon":"{rev}"}}"#),
        );
        let _ = fs::write(
            home.join("server.toml"),
            format!("[engine]\npath = \"{}\"\n", engine.display()),
        );
        let info = get("/hot-play.json").unwrap_or_default();
        assert!(
            info.ends_with(&format!(
                r#"{{"horizon":"{rev}","slots_sha1":"{}"}}"#,
                sha1_hex(b"pula")
            )),
            "{info}"
        );
        let hmod = get("/hot_slots.hmod").unwrap_or_default();
        assert!(
            hmod.starts_with("HTTP/1.1 200") && hmod.ends_with("pula"),
            "{hmod}"
        );
        let ok = get("/pack.zip").unwrap_or_default();
        assert!(
            ok.starts_with("HTTP/1.1 200") && ok.ends_with("zawartosc"),
            "{ok}"
        );
        assert!(get("/../etc/passwd")
            .unwrap_or_default()
            .starts_with("HTTP/1.1 404"));
        let _ = fs::remove_dir_all(&home);
    }
}
