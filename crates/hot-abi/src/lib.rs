//! PL: Stabilna granica C ABI między `hot-core` (gospodarz w silniku Horizon) a modami gorącymi
//! (`.so` na serwerze, `.dylib` w testach na macOS). Obie strony budują się osobno, więc łączy je
//! tylko to, co tu zdefiniowane. Używają: `hot-sdk` (strona moda), `hot-core` (strona gospodarza).
//! Decyzja: ADR o granicy ABI (`docs/adr/`), w planie budowy D3.
//!
//! EN: Stable C ABI boundary between `hot-core` (the host inside the Horizon engine) and hot mods
//! (`.so` on the server, `.dylib` in macOS tests). Both sides are built separately, so only what is
//! defined here connects them. Used by: `hot-sdk` (mod side), `hot-core` (host side).

/// PL: Wersja główna ABI. Gospodarz odrzuca mod, który zgłasza inną wartość.
/// Zmiana niezgodna wstecz w tym crate = podbicie tej liczby.
///
/// EN: Major ABI version. The host rejects a mod that reports a different value.
/// Any backwards-incompatible change in this crate bumps this number.
pub const ABI_MAJOR: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_major_is_one() {
        assert_eq!(ABI_MAJOR, 1);
    }
}
