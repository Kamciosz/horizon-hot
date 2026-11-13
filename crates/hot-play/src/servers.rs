//! PL: Plik `servers.dat` instancji gracza: jeden serwer z `acceptTextures` = `true`, żeby gra nie
//! pytała o paczkę zasobów (D9). Format: nieskompresowane NBT, korzeń bez nazwy z listą `servers`
//! (`horizon-protocol`, `servers.rs`). Napisy w zmodyfikowanym UTF-8 Javy.
//! EN: The player instance's `servers.dat`: one server with `acceptTextures` = `true`, so the game
//! does not ask about the resource pack (D9). Format: uncompressed NBT, an unnamed root with a
//! `servers` list (`horizon-protocol`, `servers.rs`). Strings in Java's modified UTF-8.

const END: u8 = 0;
const BYTE: u8 = 1;
const STRING: u8 = 8;
const LIST: u8 = 9;
const COMPOUND: u8 = 10;

/// PL: Bajty `servers.dat` z jednym serwerem. EN: The `servers.dat` bytes with one server.
#[must_use]
pub fn servers_dat(name: &str, address: &str) -> Vec<u8> {
    let mut out = vec![COMPOUND];
    put_str(&mut out, "");
    out.push(LIST);
    put_str(&mut out, "servers");
    out.push(COMPOUND);
    out.extend_from_slice(&1i32.to_be_bytes());
    out.push(STRING);
    put_str(&mut out, "name");
    put_str(&mut out, name);
    out.push(STRING);
    put_str(&mut out, "ip");
    put_str(&mut out, address);
    out.push(BYTE);
    put_str(&mut out, "acceptTextures");
    out.push(1);
    out.push(END);
    out.push(END);
    out
}

/// PL: Napis NBT: długość `u16` i zmodyfikowany UTF-8 (NUL jako `C0 80`, znaki spoza BMP jako
/// dwa surogaty po 3 bajty). Za długi napis jest obcinany do 65535 bajtów na granicy znaku.
/// EN: An NBT string: a `u16` length and modified UTF-8 (NUL as `C0 80`, characters outside the
/// BMP as two 3-byte surrogates). A string that is too long is cut to 65535 bytes at a character.
fn put_str(out: &mut Vec<u8>, text: &str) {
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        let mut one = Vec::with_capacity(6);
        if c == '\0' {
            one.extend_from_slice(&[0xC0, 0x80]);
        } else if (c as u32) > 0xFFFF {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                let u = u32::from(*unit);
                one.extend_from_slice(&[
                    0xE0 | (u >> 12) as u8,
                    0x80 | ((u >> 6) & 0x3F) as u8,
                    0x80 | (u & 0x3F) as u8,
                ]);
            }
        } else {
            let mut buf = [0u8; 4];
            one.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        if bytes.len() + one.len() > usize::from(u16::MAX) {
            break;
        }
        bytes.extend_from_slice(&one);
    }
    let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_server_with_textures_accepted() {
        let mut want = vec![10, 0, 0, 9, 0, 7];
        want.extend_from_slice(b"servers");
        want.extend_from_slice(&[10, 0, 0, 0, 1]);
        want.extend_from_slice(&[8, 0, 4]);
        want.extend_from_slice(b"name");
        want.extend_from_slice(&[0, 3]);
        want.extend_from_slice(b"Hot");
        want.extend_from_slice(&[8, 0, 2]);
        want.extend_from_slice(b"ip");
        want.extend_from_slice(&[0, 11]);
        want.extend_from_slice(b"a.pl:25565!");
        want.extend_from_slice(&[1, 0, 14]);
        want.extend_from_slice(b"acceptTextures");
        want.extend_from_slice(&[1, 0, 0]);
        assert_eq!(servers_dat("Hot", "a.pl:25565!"), want);
    }

    #[test]
    fn strings_use_java_modified_utf8() {
        let mut out = Vec::new();
        put_str(&mut out, "ż\0😀");
        assert_eq!(
            out,
            [0, 10, 0xC5, 0xBC, 0xC0, 0x80, 0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]
        );
    }
}
