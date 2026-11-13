//! PL: Odpowiedź na pytanie silnika o gracza (przekazywanie Velocity „modern”). Silnik pyta na
//! kanale [`CHANNEL`] jednym bajtem: najwyższą wersją, którą rozumie. Proxy odpowiada 32 bajtami
//! podpisu HMAC-SHA256 i podpisanymi danymi: wersja (VarInt), adres gracza (tekst), UUID
//! (16 bajtów), nazwa (tekst), właściwości profilu (VarInt liczba; każda: nazwa, wartość, bool
//! i podpis, gdy jest). Format i nagranie z prawdziwego Velocity są w Horizonie
//! (`horizon-client/src/lan/forwarding.rs`, `docs/velocity-forwarding.md`).
//!
//! EN: The answer to the engine's question about a player (Velocity "modern" forwarding). The
//! engine asks on [`CHANNEL`] with one byte, the highest version it understands. The proxy answers
//! with a 32-byte HMAC-SHA256 signature and the signed data: version (VarInt), the player's address
//! (string), UUID (16 bytes), name (string), profile properties (VarInt count; each a name, a
//! value, a bool and the signature when set). The format and a real Velocity recording are in
//! Horizon (`horizon-client/src/lan/forwarding.rs`, `docs/velocity-forwarding.md`).

use horizon_protocol::{buf::Writer, packets::login::Property};
use sha2::{Digest, Sha256};
use std::net::IpAddr;

/// PL: Kanał pytania silnika. EN: The engine's query channel.
pub const CHANNEL: &str = "velocity:player_info";

/// PL: Jedyna wersja, którą czyta silnik Horizona. EN: The only version Horizon's engine reads.
const VERSION: u8 = 1;

/// PL: Gracz, za którego proxy ręczy. EN: The player the proxy vouches for.
pub struct Player<'a> {
    pub address: IpAddr,
    pub uuid: u128,
    pub name: &'a str,
    pub properties: &'a [Property],
}

/// PL: Podpisana odpowiedź na pytanie z danymi `request`.
/// EN: The signed answer to a question with data `request`.
///
/// # Errors
/// PL: Silnik pyta o wersję niższą niż 1. EN: The engine asks for a version below 1.
pub fn answer(secret: &[u8], request: &[u8], player: &Player) -> Result<Vec<u8>, String> {
    match request.first() {
        Some(&v) if v >= VERSION => {}
        other => {
            return Err(format!(
                "the engine asked for forwarding version {other:?}, not 1"
            ))
        }
    }
    let mut w = Writer::new();
    w.varint(i32::from(VERSION));
    w.string(&player.address.to_string());
    w.uuid(player.uuid);
    w.string(player.name);
    w.varint(i32::try_from(player.properties.len()).unwrap_or(i32::MAX));
    for p in player.properties {
        w.string(&p.name);
        w.string(&p.value);
        w.bool(p.signature.is_some());
        if let Some(signature) = &p.signature {
            w.string(signature);
        }
    }
    let mut out = hmac_sha256(secret, &w.data).to_vec();
    out.extend_from_slice(&w.data);
    Ok(out)
}

/// PL: HMAC-SHA256 (RFC 2104), tak jak liczy go silnik. EN: HMAC-SHA256 (RFC 2104), as the engine does.
#[must_use]
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }
    let inner = Sha256::new()
        .chain_update(padded.map(|b| b ^ 0x36))
        .chain_update(data)
        .finalize();
    Sha256::new()
        .chain_update(padded.map(|b| b ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect()
    }

    #[test]
    fn hmac_matches_the_rfc_4231_vectors() {
        assert_eq!(
            hmac_sha256(&[0x0b; 20], b"Hi There").to_vec(),
            hex("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
        );
        assert_eq!(
            hmac_sha256(b"Jefe", b"what do ya want for nothing?").to_vec(),
            hex("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
        );
        assert_eq!(
            hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )
            .to_vec(),
            hex("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
        );
    }

    /// PL: Odpowiedź prawdziwego Velocity 4.3.1 nagrana w Horizonie (`lan/forwarding.rs`):
    /// nasz koder musi dać te same bajty.
    /// EN: A real Velocity 4.3.1 answer recorded in Horizon (`lan/forwarding.rs`): our encoder
    /// must produce the same bytes.
    #[test]
    fn the_answer_matches_a_recorded_velocity_answer() {
        const SECRET: &[u8] = b"ISWncFxltwFE";
        const RECORDED: &str = "189e73f57e7fe3d387de5f252fc0226e8288e5b15f23ee12924b34cf216b7f6901093132372e302e302e31f3d28cb072253cb1baeb2dadd2be89ae0654657374657200";
        let player = Player {
            address: IpAddr::from([127, 0, 0, 1]),
            uuid: 0xf3d2_8cb0_7225_3cb1_baeb_2dad_d2be_89ae,
            name: "Tester",
            properties: &[],
        };
        assert_eq!(answer(SECRET, &[1], &player), Ok(hex(RECORDED)));
    }

    #[test]
    fn properties_with_and_without_a_signature_are_written() {
        let properties = [
            Property {
                name: "textures".into(),
                value: "abc".into(),
                signature: Some("sig".into()),
            },
            Property {
                name: "x".into(),
                value: "y".into(),
                signature: None,
            },
        ];
        let player = Player {
            address: IpAddr::from([127, 0, 0, 1]),
            uuid: 7,
            name: "Alex",
            properties: &properties,
        };
        let out = answer(b"k", &[1], &player).unwrap_or_default();
        let data = &out[32..];
        assert_eq!(out[..32], hmac_sha256(b"k", data));
        // PL: liczba właściwości, potem „textures” z flagą podpisu 1 i „x” z flagą 0.
        // EN: the property count, then "textures" with signature flag 1 and "x" with flag 0.
        let tail = b"\x02\x08textures\x03abc\x01\x03sig\x01x\x01y\x00";
        assert!(data.ends_with(tail));
    }

    #[test]
    fn a_version_below_one_is_refused() {
        let player = Player {
            address: IpAddr::from([127, 0, 0, 1]),
            uuid: 1,
            name: "A",
            properties: &[],
        };
        assert!(answer(b"k", &[], &player).is_err());
        assert!(answer(b"k", &[0], &player).is_err());
        assert!(answer(b"k", &[4], &player).is_ok());
    }
}
