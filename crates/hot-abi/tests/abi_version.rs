//! PL: Pilnuje wersji ABI. Zmiana tej liczby oznacza przebudowę wszystkich modów, więc test ma oblać
//! przy przypadkowej zmianie.
//! EN: Guards the ABI version. Changing it forces every mod to be rebuilt, so the test must fail on an
//! accidental change.

#[test]
fn abi_major_is_one() {
    assert_eq!(hot_abi::ABI_MAJOR, 1);
}
