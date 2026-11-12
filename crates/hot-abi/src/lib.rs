//! Stabilna granica C ABI między `hot-host` (w silniku) a modami gorącymi (`.dylib`/`.so`).
//! Decyzja: PLAN.md, D2. Typy przybędą w M1.1.

/// Wersja główna ABI. Host odrzuca mod, który zgłasza inną wartość.
pub const ABI_MAJOR: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_major_is_one() {
        assert_eq!(ABI_MAJOR, 1);
    }
}
