//! PL: Zamiana id encji gracza na surowych bajtach pakietów gry. Klient zna gracza pod id z
//! pierwszego `Login`, a każdy kolejny silnik nadaje mu inne. Proxy nie wysyła klientowi drugiego
//! `Login` (ekran ładowania), więc zamienia „id u klienta ↔ id w silniku” w obie strony, tylko w
//! polach, które mogą nieść id gracza. Reszta pakietu zostaje bajt w bajt. Lista pól: struktury
//! pakietów w `horizon-protocol` (rewizja z `Cargo.toml`), wcześniej próba S8.
//!
//! EN: Swaps the player's entity id in raw game packets. The client knows the player by the id from
//! the first `Login`, and every later engine gives it another one. The proxy never sends the client
//! a second `Login` (a loading screen), so it swaps "client id ↔ engine id" both ways, only in the
//! fields that may carry the player's id. The rest of the packet stays byte for byte. Field list:
//! the packet structs in `horizon-protocol` (revision in `Cargo.toml`), earlier spike S8.

use horizon_protocol::buf::write_varint;
use horizon_protocol::ids::play::{clientbound as cb, serverbound as sb};

/// PL: Para id gracza: u klienta i w aktywnym silniku. Zamiana jest inwolucją, więc ta sama
/// funkcja działa w obie strony. EN: The player's id on the client and in the active engine. The
/// swap is an involution, so one function works both ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swap {
    pub client: i32,
    pub engine: i32,
}

impl Swap {
    /// PL: Id po zamianie. EN: The id after the swap.
    #[must_use]
    pub fn apply(self, id: i32) -> i32 {
        if id == self.engine {
            self.client
        } else if id == self.client {
            self.engine
        } else {
            id
        }
    }
}

/// PL: Czyta VarInt; zwraca wartość i liczbę bajtów. EN: Reads a VarInt; returns it and its length.
#[must_use]
pub fn read_varint(b: &[u8]) -> Option<(i32, usize)> {
    let mut v: u32 = 0;
    for i in 0..5 {
        let byte = *b.get(i)?;
        v |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((i32::from_ne_bytes(v.to_ne_bytes()), i + 1));
        }
    }
    None
}

/// PL: Zastępuje VarInt od `at` wartością `f(v)`; zwraca pozycję za nim.
/// EN: Replaces the VarInt at `at` with `f(v)`; returns the position after it.
fn varint(p: &mut Vec<u8>, at: usize, f: impl Fn(i32) -> i32) -> Option<usize> {
    let (v, n) = read_varint(p.get(at..)?)?;
    let nv = f(v);
    if nv == v {
        return Some(at + n);
    }
    let mut enc = Vec::with_capacity(5);
    write_varint(&mut enc, nv);
    let len = enc.len();
    p.splice(at..at + n, enc);
    Some(at + len)
}

/// PL: Int (4 bajty, big-endian) od `at`. EN: An Int (4 bytes, big-endian) at `at`.
fn int(p: &mut [u8], at: usize, f: impl Fn(i32) -> i32) -> Option<usize> {
    let raw: [u8; 4] = p.get(at..at + 4)?.try_into().ok()?;
    let nv = f(i32::from_be_bytes(raw));
    p[at..at + 4].copy_from_slice(&nv.to_be_bytes());
    Some(at + 4)
}

/// PL: Lista VarIntów z długością na początku. EN: A length-prefixed list of VarInts.
fn varint_list(p: &mut Vec<u8>, at: usize, f: impl Fn(i32) -> i32 + Copy) -> Option<usize> {
    let (count, n) = read_varint(p.get(at..)?)?;
    let mut pos = at + n;
    for _ in 0..count {
        pos = varint(p, pos, f)?;
    }
    Some(pos)
}

/// PL: Opcjonalne id jako VarInt `id + 1` (0 = brak), jak w `DamageEvent`.
/// EN: An optional id as a VarInt `id + 1` (0 = none), as in `DamageEvent`.
fn opt_varint(p: &mut Vec<u8>, at: usize, s: Swap) -> Option<usize> {
    varint(p, at, |v| if v == 0 { 0 } else { s.apply(v - 1) + 1 })
}

/// PL: Zamienia id w pakiecie od silnika do klienta. `None`: pakiet krótszy, niż mówi jego
/// struktura (zostaje bez zmian). EN: Swaps ids in an engine-to-client packet. `None`: the packet
/// is shorter than its struct says (left as it is).
pub fn clientbound(id: i32, p: &mut Vec<u8>, s: Swap) -> Option<()> {
    let f = move |v| s.apply(v);
    match id {
        cb::ADD_ENTITY
        | cb::ANIMATE
        | cb::ENTITY_POSITION_SYNC
        | cb::HURT_ANIMATION
        | cb::MOVE_ENTITY_POS
        | cb::MOVE_ENTITY_POS_ROT
        | cb::MOVE_ENTITY_ROT
        | cb::MOVE_MINECART_ALONG_TRACK
        | cb::PLAYER_COMBAT_KILL
        | cb::PROJECTILE_POWER
        | cb::REMOVE_MOB_EFFECT
        | cb::ROTATE_HEAD
        | cb::SET_CAMERA
        | cb::SET_ENTITY_DATA
        | cb::SET_ENTITY_MOTION
        | cb::SET_EQUIPMENT
        | cb::SWING_ANIMATION
        | cb::TELEPORT_ENTITY
        | cb::UPDATE_ATTRIBUTES
        | cb::UPDATE_MOB_EFFECT => {
            varint(p, 0, f)?;
        }
        cb::ENTITY_EVENT => {
            int(p, 0, f)?;
        }
        cb::SET_ENTITY_LINK => {
            int(p, 0, f)?;
            int(p, 4, f)?;
        }
        cb::TAKE_ITEM_ENTITY => {
            let at = varint(p, 0, f)?;
            varint(p, at, f)?;
        }
        cb::SET_PASSENGERS => {
            let at = varint(p, 0, f)?;
            varint_list(p, at, f)?;
        }
        cb::REMOVE_ENTITIES => {
            varint_list(p, 0, f)?;
        }
        cb::DAMAGE_EVENT => {
            // PL: ofiara, typ obrażeń, przyczyna i sprawca jako `id + 1`.
            // EN: victim, damage type, cause and direct source as `id + 1`.
            let at = varint(p, 0, f)?;
            let at = varint(p, at, |v| v)?;
            let at = opt_varint(p, at, s)?;
            opt_varint(p, at, s)?;
        }
        cb::MOUNT_SCREEN_OPEN => {
            let at = varint(p, 0, |v| v)?;
            let at = varint(p, at, |v| v)?;
            int(p, at, f)?;
        }
        cb::PLAYER_LOOK_AT => {
            // PL: kotwica, punkt (3 × double), potem opcjonalna encja z kotwicą.
            // EN: anchor, a point (3 × double), then an optional entity with its anchor.
            let at = varint(p, 0, |v| v)? + 24;
            if *p.get(at)? == 1 {
                varint(p, at + 1, f)?;
            }
        }
        cb::SOUND_ENTITY => {
            // PL: dźwięk (id + 1 albo 0 i definicja), źródło, encja.
            // EN: the sound (id + 1, or 0 and a definition), the source, the entity.
            let (sound, n) = read_varint(p)?;
            let mut at = n;
            if sound == 0 {
                let (len, n) = read_varint(p.get(at..)?)?;
                at += n + usize::try_from(len).ok()?;
                at += if *p.get(at)? == 1 { 5 } else { 1 };
            }
            let at = varint(p, at, |v| v)?;
            varint(p, at, f)?;
        }
        _ => {}
    }
    Some(())
}

/// PL: Zamienia id w pakiecie od klienta do silnika. EN: Swaps ids in a client-to-engine packet.
pub fn serverbound(id: i32, p: &mut Vec<u8>, s: Swap) -> Option<()> {
    let f = move |v| s.apply(v);
    match id {
        sb::ATTACK | sb::INTERACT | sb::PICK_ITEM_FROM_ENTITY | sb::PLAYER_COMMAND => {
            varint(p, 0, f)?;
        }
        sb::ENTITY_TAG_QUERY => {
            let at = varint(p, 0, |v| v)?;
            varint(p, at, f)?;
        }
        _ => {}
    }
    Some(())
}

/// PL: Przesuwa numer wiadomości czatu (`global_index`, pierwsze pole `PlayerChat`). Klient
/// rozłącza się, gdy numer nie jest kolejny, a nowy silnik liczy od zera. Zwraca numer po zmianie.
/// EN: Shifts the chat message number (`global_index`, the first field of `PlayerChat`). The
/// client disconnects when the number is not the next one, and a new engine counts from zero.
/// Returns the number after the shift.
pub fn shift_chat_index(p: &mut Vec<u8>, base: i32) -> Option<i32> {
    let (v, _) = read_varint(p)?;
    let shifted = v.wrapping_add(base);
    varint(p, 0, |_| shifted)?;
    Some(shifted)
}

/// PL: Id z `RemoveEntities`. EN: The ids in `RemoveEntities`.
#[must_use]
pub fn removed_entities(p: &[u8]) -> Vec<i32> {
    let mut out = Vec::new();
    let Some((count, mut pos)) = read_varint(p) else {
        return out;
    };
    for _ in 0..count {
        let Some((e, n)) = p.get(pos..).and_then(read_varint) else {
            break;
        };
        out.push(e);
        pos += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol::packets::play::{self, Clientbound, Serverbound};

    const S: Swap = Swap {
        client: 7,
        engine: 300,
    };

    /// PL: Koduje pakiet, zamienia id i dekoduje z powrotem. EN: Encodes, swaps and decodes back.
    fn through_cb(packet: &Clientbound) -> Clientbound {
        let body = packet.encode();
        let (id, n) = read_varint(&body).unwrap_or_default();
        let mut p = body[n..].to_vec();
        assert!(clientbound(id, &mut p, S).is_some(), "{packet:?}");
        Clientbound::decode(id, &p).unwrap_or_else(|e| panic!("{packet:?}: {e}"))
    }

    fn through_sb(packet: &Serverbound) -> Serverbound {
        let body = packet.encode();
        let (id, n) = read_varint(&body).unwrap_or_default();
        let mut p = body[n..].to_vec();
        assert!(serverbound(id, &mut p, S).is_some(), "{packet:?}");
        Serverbound::decode(id, &p).unwrap_or_else(|e| panic!("{packet:?}: {e}"))
    }

    #[test]
    fn the_swap_is_its_own_inverse() {
        for id in [7, 300, 5, -1] {
            assert_eq!(S.apply(S.apply(id)), id);
        }
        assert_eq!(S.apply(300), 7);
        assert_eq!(S.apply(7), 300);
        assert_eq!(S.apply(5), 5);
    }

    #[test]
    fn damage_names_the_player_as_victim_cause_and_source() {
        let out = through_cb(&Clientbound::DamageEvent(play::DamageEvent {
            id: 300,
            source_type: 300,
            cause: Some(300),
            direct: None,
            position: Some([1.0, 2.0, 3.0]),
        }));
        assert_eq!(
            out,
            Clientbound::DamageEvent(play::DamageEvent {
                id: 7,
                source_type: 300,
                cause: Some(7),
                direct: None,
                position: Some([1.0, 2.0, 3.0]),
            })
        );
    }

    #[test]
    fn look_at_mount_screen_and_entity_sound_keep_the_other_fields() {
        let look = through_cb(&Clientbound::PlayerLookAt(play::PlayerLookAt {
            from: 1,
            target: [4.0, 5.0, 6.0],
            entity: Some(play::LookAtEntity { id: 300, anchor: 1 }),
        }));
        assert_eq!(
            look,
            Clientbound::PlayerLookAt(play::PlayerLookAt {
                from: 1,
                target: [4.0, 5.0, 6.0],
                entity: Some(play::LookAtEntity { id: 7, anchor: 1 }),
            })
        );
        let mount = through_cb(&Clientbound::MountScreenOpen(play::MountScreenOpen {
            container: 300,
            columns: 3,
            entity: 300,
        }));
        assert_eq!(
            mount,
            Clientbound::MountScreenOpen(play::MountScreenOpen {
                container: 300,
                columns: 3,
                entity: 7,
            })
        );
        for sound in [
            (12, None),
            (0, Some(("minecraft:x".to_owned(), None))),
            (0, Some(("minecraft:y".to_owned(), Some(16.0)))),
        ] {
            let packet = |id| {
                Clientbound::SoundEntity(play::SoundEntity {
                    sound: sound.clone(),
                    source: 300,
                    id,
                    volume: 1.0,
                    pitch: 0.5,
                    seed: 9,
                })
            };
            // PL: Porównanie po dekodowaniu: kodek zmienia id wbudowanego dźwięku na -1.
            // EN: Compared after decoding: the codec turns an inline sound's id into -1.
            let body = packet(7).encode();
            let expected = Clientbound::decode(body[0].into(), &body[1..]).ok();
            assert_eq!(Some(through_cb(&packet(300))), expected);
        }
    }

    #[test]
    fn simple_packets_swap_the_first_id_and_lists_swap_every_one() {
        let pass = through_cb(&Clientbound::SetPassengers(play::SetPassengers {
            vehicle: 300,
            passengers: vec![1, 7, 300],
        }));
        assert_eq!(
            pass,
            Clientbound::SetPassengers(play::SetPassengers {
                vehicle: 7,
                passengers: vec![1, 300, 7],
            })
        );
        let kill = through_cb(&Clientbound::HurtAnimation(play::HurtAnimation {
            id: 300,
            yaw: 1.5,
        }));
        assert_eq!(
            kill,
            Clientbound::HurtAnimation(play::HurtAnimation { id: 7, yaw: 1.5 })
        );
        let pick = through_sb(&Serverbound::PickItemFromEntity(play::PickItemFromEntity {
            id: 7,
            include_data: true,
        }));
        assert_eq!(
            pick,
            Serverbound::PickItemFromEntity(play::PickItemFromEntity {
                id: 300,
                include_data: true,
            })
        );
        let query = through_sb(&Serverbound::EntityTagQuery(play::EntityTagQuery {
            transaction: 7,
            entity: 7,
        }));
        assert_eq!(
            query,
            Serverbound::EntityTagQuery(play::EntityTagQuery {
                transaction: 7,
                entity: 300,
            })
        );
    }

    #[test]
    fn a_larger_varint_moves_the_rest_of_the_packet() {
        let s = Swap {
            client: 1,
            engine: 100_000,
        };
        let packet = Clientbound::SetEntityMotion(play::SetEntityMotion {
            id: 1,
            velocity: [0.0; 3],
        });
        let body = packet.encode();
        let (id, n) = read_varint(&body).unwrap_or_default();
        let mut p = body[n..].to_vec();
        assert!(clientbound(id, &mut p, s).is_some());
        assert_eq!(
            Clientbound::decode(id, &p).ok(),
            Some(Clientbound::SetEntityMotion(play::SetEntityMotion {
                id: 100_000,
                velocity: [0.0; 3],
            }))
        );
    }

    #[test]
    fn the_chat_index_continues_where_the_old_engine_stopped() {
        let mut p = Vec::new();
        write_varint(&mut p, 0);
        p.extend_from_slice(b"rest");
        assert_eq!(shift_chat_index(&mut p, 200), Some(200));
        assert_eq!(read_varint(&p), Some((200, 2)));
        assert_eq!(&p[2..], b"rest");
    }

    #[test]
    fn removed_entities_reads_the_list() {
        let body = Clientbound::RemoveEntities(play::RemoveEntities {
            ids: vec![3, 300, 70_000],
        })
        .encode();
        assert_eq!(removed_entities(&body[1..]), vec![3, 300, 70_000]);
    }
}
