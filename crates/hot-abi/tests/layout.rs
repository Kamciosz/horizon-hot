//! PL: Pilnuje układu struktur w pamięci (64 bity). Zmiana rozmiaru albo wyrównania łamie mody
//! zbudowane wcześniej, więc wymaga podbicia `ABI_MAJOR`. Test ma oblać przy przypadkowej zmianie.
//! EN: Guards the structs' memory layout (64-bit). A size or alignment change breaks mods built
//! earlier and needs an `ABI_MAJOR` bump. The test must fail on an accidental change.

use std::mem::{align_of, offset_of, size_of};

use hot_abi::{Buf, Event, HostApi, ModVTable, PlayerInfo, Str};

#[test]
fn sizes_and_alignment_are_stable() {
    assert_eq!((size_of::<Str>(), align_of::<Str>()), (16, 8));
    assert_eq!((size_of::<Buf>(), align_of::<Buf>()), (24, 8));
    assert_eq!((size_of::<Event>(), align_of::<Event>()), (120, 8));
    assert_eq!((size_of::<PlayerInfo>(), align_of::<PlayerInfo>()), (80, 8));
    assert_eq!((size_of::<HostApi>(), align_of::<HostApi>()), (72, 8));
    assert_eq!((size_of::<ModVTable>(), align_of::<ModVTable>()), (72, 8));
}

#[test]
fn field_offsets_are_stable() {
    assert_eq!(offset_of!(Event, player), 4);
    assert_eq!(offset_of!(Event, name), 24);
    assert_eq!(offset_of!(Event, number), 112);
    assert_eq!(offset_of!(HostApi, data_dir), 64);
    assert_eq!(offset_of!(ModVTable, free_buf), 64);
}
