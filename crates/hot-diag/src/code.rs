//! PL: Kod błędu `HOT-EXXXX`. Każdy kod ma plik w `docs/bledy/` (sprawdza `tests/catalog.rs`).
//! EN: The `HOT-EXXXX` error code. Every code has a file in `docs/bledy/` (`tests/catalog.rs` checks it).

use std::fmt;

/// PL: Stały kod błędu, np. `HOT-E0002`. Kody definiujemy jako stałe (`const`), wtedy zły format
/// zatrzymuje kompilację, a nie program.
///
/// EN: A stable error code such as `HOT-E0002`. Define codes as constants (`const`), so a bad
/// format stops the build rather than the program.
///
/// ```
/// use hot_diag::Code;
/// const PANIC: Code = Code::new("HOT-E0002");
/// assert_eq!(PANIC.doc(), "docs/bledy/HOT-E0002.md");
/// ```
///
/// ```compile_fail,E0080
/// use hot_diag::Code;
/// const BAD: Code = Code::new("HOT-E02");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Code(&'static str);

impl Code {
    /// PL: Tworzy kod; format `HOT-E` + 4 cyfry. Zły format w stałej = błąd kompilacji.
    /// EN: Creates a code; format `HOT-E` + 4 digits. A bad format in a constant = build error.
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        let b = code.as_bytes();
        let prefix = b"HOT-E";
        if b.len() != prefix.len() + 4 {
            panic!("an error code is HOT-E followed by 4 digits");
        }
        let mut i = 0;
        while i < b.len() {
            let ok = if i < prefix.len() {
                b[i] == prefix[i]
            } else {
                b[i].is_ascii_digit()
            };
            if !ok {
                panic!("an error code is HOT-E followed by 4 digits");
            }
            i += 1;
        }
        Self(code)
    }

    /// PL: Kod jako tekst. EN: The code as text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// PL: Ścieżka wpisu w katalogu błędów, względem katalogu głównego repo.
    /// EN: The catalog entry's path, relative to the repository root.
    #[must_use]
    pub fn doc(self) -> String {
        format!("docs/bledy/{}.md", self.0)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
