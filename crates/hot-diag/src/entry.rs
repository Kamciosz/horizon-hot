//! PL: Błąd z polami D27. Wymagane pola są argumentami konstruktora, więc wpisu bez nich nie da się
//! skompilować. Reszta to metody, które można dopisać.
//! EN: An error with the D27 fields. Required fields are constructor arguments, so an entry without
//! them does not compile. The rest are optional methods.

use std::error::Error;
use std::fmt;
use std::panic::Location;

use serde_json::{Map, Value};

use crate::Code;

/// PL: Błąd gotowy do zapisu w logu. Składnik zwraca go wyżej przez `?`, a na końcu wywołuje
/// [`crate::Logger::error`].
///
/// EN: An error ready for the log. A component returns it upwards with `?` and finally calls
/// [`crate::Logger::error`].
///
/// ```
/// use hot_diag::{codes, DiagError};
/// let e = DiagError::new(codes::PANIC, "Hot mod \"sklep\" panicked", "Fix the panic and push")
///     .context("mod", "sklep")
///     .context("generation", 7);
/// assert_eq!(e.code(), codes::PANIC);
/// ```
///
/// Bez kodu, opisu albo naprawy wpis się nie kompiluje. Without a code, a description or a fix
/// the entry does not compile:
///
/// ```compile_fail,E0061
/// use hot_diag::DiagError;
/// let e = DiagError::new("Hot mod \"sklep\" panicked", "Fix the panic and push");
/// ```
///
/// ```compile_fail,E0061
/// use hot_diag::{codes, DiagError};
/// let e = DiagError::new(codes::PANIC, "Fix the panic and push");
/// ```
///
/// ```compile_fail,E0061
/// use hot_diag::{codes, DiagError};
/// let e = DiagError::new(codes::PANIC, "Hot mod \"sklep\" panicked");
/// ```
#[derive(Debug, Clone)]
pub struct DiagError {
    // PL: W `Box`, żeby `Result<_, DiagError>` był mały (clippy `result_large_err`).
    // EN: Boxed so `Result<_, DiagError>` stays small (clippy `result_large_err`).
    f: Box<Fields>,
}

#[derive(Debug, Clone)]
struct Fields {
    code: Code,
    what: String,
    fix: String,
    expected: Option<String>,
    context: Map<String, Value>,
    cause: Option<String>,
    chain: Vec<String>,
    state: Option<String>,
    source: String,
}

/// PL: Miejsce w kodzie jako ścieżka z repo z `/`, także w programach zbudowanych na Windows
/// (tam `file!()` daje `\`), żeby `source` dało się wkleić tak samo na każdym systemie.
/// EN: A code location as a repo path with `/`, also in programs built on Windows (where
/// `file!()` gives `\`), so `source` reads the same on every system.
fn repo_path(file: &str, line: u32) -> String {
    format!("{}:{line}", file.replace('\\', "/"))
}

impl DiagError {
    /// PL: `what` i `fix` pełnymi zdaniami, bez skrótów projektowych (`docs/STANDARD_KODU.md`).
    /// `source` to miejsce wywołania.
    ///
    /// EN: `what` and `fix` as full sentences without project shorthand. `source` is the caller.
    #[must_use]
    #[track_caller]
    pub fn new(code: Code, what: impl Into<String>, fix: impl Into<String>) -> Self {
        let at = Location::caller();
        Self {
            f: Box::new(Fields {
                code,
                what: what.into(),
                fix: fix.into(),
                expected: None,
                context: Map::new(),
                cause: None,
                chain: Vec::new(),
                state: None,
                source: repo_path(at.file(), at.line()),
            }),
        }
    }

    /// PL: Co powinno się stać. EN: What should have happened.
    #[must_use]
    pub fn expected(mut self, text: impl Into<String>) -> Self {
        self.f.expected = Some(text.into());
        self
    }

    /// PL: Identyfikator potrzebny do odtworzenia błędu (mod, generacja, ścieżka, wersja).
    /// EN: An identifier needed to reproduce the error (mod, generation, path, version).
    #[must_use]
    pub fn context(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.f.context.insert(key.to_owned(), value.into());
        self
    }

    /// PL: Najbardziej prawdopodobna przyczyna. EN: The most likely cause.
    #[must_use]
    pub fn cause(mut self, text: impl Into<String>) -> Self {
        self.f.cause = Some(text.into());
        self
    }

    /// PL: Łańcuch przyczyn z `std::error::Error::source`, od błędu do pierwotnej przyczyny.
    /// EN: The cause chain from `std::error::Error::source`, from the error to the root cause.
    #[must_use]
    pub fn caused_by(mut self, error: &(dyn Error + 'static)) -> Self {
        let mut next = Some(error);
        while let Some(e) = next {
            self.f.chain.push(e.to_string());
            next = e.source();
        }
        self
    }

    /// PL: Dodatkowa linia łańcucha, np. backtrace przy panice.
    /// EN: An extra chain line, e.g. the backtrace of a panic.
    #[must_use]
    pub fn chain_line(mut self, line: impl Into<String>) -> Self {
        self.f.chain.push(line.into());
        self
    }

    /// PL: Co system zrobił sam i w jakim stanie zostawił serwer.
    /// EN: What the system did on its own and the state it left the server in.
    #[must_use]
    pub fn state(mut self, text: impl Into<String>) -> Self {
        self.f.state = Some(text.into());
        self
    }

    /// PL: Inne miejsce w kodzie niż wywołanie `new`, np. miejsce paniki.
    /// EN: A code location other than the `new` call, e.g. where a panic happened.
    #[must_use]
    pub fn source_at(mut self, file: &str, line: u32) -> Self {
        self.f.source = repo_path(file, line);
        self
    }

    /// PL: Kod błędu. EN: The error code.
    #[must_use]
    pub fn code(&self) -> Code {
        self.f.code
    }

    /// PL: Pola D27 bez `component`, `version` i czasu: dopisuje je [`crate::Logger`].
    /// EN: The D27 fields without `component`, `version` and time: [`crate::Logger`] adds them.
    #[must_use]
    pub fn fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("level".into(), "error".into());
        m.insert("code".into(), self.f.code.as_str().into());
        m.insert("what".into(), self.f.what.clone().into());
        m.insert("fix".into(), self.f.fix.clone().into());
        m.insert("doc".into(), self.f.code.doc().into());
        m.insert("source".into(), self.f.source.clone().into());
        m.insert("context".into(), Value::Object(self.f.context.clone()));
        let optional = [
            ("expected", &self.f.expected),
            ("cause", &self.f.cause),
            ("state", &self.f.state),
        ];
        for (key, value) in optional {
            if let Some(v) = value {
                m.insert(key.into(), v.clone().into());
            }
        }
        if !self.f.chain.is_empty() {
            m.insert("chain".into(), self.f.chain.clone().into());
        }
        m
    }
}

impl fmt::Display for DiagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.f.code, self.f.what)
    }
}

impl Error for DiagError {}
