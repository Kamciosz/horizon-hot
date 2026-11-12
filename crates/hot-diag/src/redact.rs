//! PL: Maskowanie sekretów w logach i raporcie (D27, D28). Sekrety podaje składnik, który je zna
//! (np. `hotd` po wczytaniu `forwarding.secret`).
//! EN: Masking secrets in logs and the report (D27, D28). The component that knows a secret registers
//! it (e.g. `hotd` after reading `forwarding.secret`).

use serde_json::Value;

/// PL: Zastępuje sekret w tekście. EN: Replaces a secret in text.
pub const MASK: &str = "[secret]";

/// PL: Lista znanych sekretów. EN: The list of known secrets.
#[derive(Debug, Default, Clone)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// PL: Pusta lista. EN: An empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// PL: Dodaje sekret. Pusty tekst pomijamy, bo „zamaskowanie” go nic nie znaczy.
    /// EN: Adds a secret. Empty text is skipped, as "masking" it means nothing.
    pub fn add(&mut self, secret: impl Into<String>) {
        let s = secret.into();
        if !s.is_empty() && !self.secrets.contains(&s) {
            self.secrets.push(s);
            // PL: Najpierw dłuższe, żeby sekret zawierający inny sekret zniknął w całości.
            // EN: Longer first, so a secret containing another secret disappears whole.
            self.secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        }
    }

    /// PL: Tekst bez znanych sekretów. EN: The text without known secrets.
    #[must_use]
    pub fn text(&self, input: &str) -> String {
        let mut out = input.to_owned();
        for s in &self.secrets {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), MASK);
            }
        }
        out
    }

    /// PL: Maskuje każdy napis w wartości JSON, także w zagnieżdżonym `context` i `chain`.
    /// EN: Masks every string in a JSON value, including nested `context` and `chain`.
    pub fn value(&self, v: &mut Value) {
        match v {
            Value::String(s) => *s = self.text(s),
            Value::Array(items) => items.iter_mut().for_each(|i| self.value(i)),
            Value::Object(map) => map.values_mut().for_each(|i| self.value(i)),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

/// PL: Zastępuje adresy IPv4 przez `x.x.x.x` (adresy graczy w raporcie, D28). Zostawia adresy
/// lokalne `127.*` i `0.0.0.0`, bo pokazują konfigurację, a nie gracza.
///
/// EN: Replaces IPv4 addresses with `x.x.x.x` (player addresses in the report, D28). Keeps local
/// `127.*` and `0.0.0.0`, as they show configuration, not a player.
#[must_use]
pub fn mask_ipv4(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let (before, from_digit) = rest.split_at(start);
        out.push_str(before);
        let len = from_digit
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(from_digit.len());
        let token = &from_digit[..len];
        let prev_ok = !out.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '.');
        if prev_ok && is_public_ipv4(token.trim_end_matches('.')) {
            out.push_str("x.x.x.x");
            out.push_str(&token[token.trim_end_matches('.').len()..]);
        } else {
            out.push_str(token);
        }
        rest = &from_digit[len..];
    }
    out.push_str(rest);
    out
}

fn is_public_ipv4(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    let valid = parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255));
    valid && parts[0] != "127" && token != "0.0.0.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_secret_is_masked_everywhere_in_json() {
        let mut r = Redactor::new();
        r.add("s3cr3t-forwarding-key");
        let mut v = serde_json::json!({
            "what": "Login failed with s3cr3t-forwarding-key",
            "context": {"secret": "s3cr3t-forwarding-key", "n": 7},
            "chain": ["x s3cr3t-forwarding-key y"]
        });
        r.value(&mut v);
        let text = v.to_string();
        assert!(!text.contains("s3cr3t"), "{text}");
        assert_eq!(text.matches(MASK).count(), 3, "{text}");
    }

    #[test]
    fn longer_secret_masked_whole_when_it_contains_shorter() {
        let mut r = Redactor::new();
        r.add("abc");
        r.add("abcdef");
        assert_eq!(r.text("key=abcdef"), format!("key={MASK}"));
    }

    #[test]
    fn ipv4_masked_but_local_and_versions_kept() {
        let s = "player 83.12.4.200:51234 joined, proxy 127.0.0.1:25566, bind 0.0.0.0, hot 0.1.0";
        assert_eq!(
            mask_ipv4(s),
            "player x.x.x.x:51234 joined, proxy 127.0.0.1:25566, bind 0.0.0.0, hot 0.1.0"
        );
        assert_eq!(mask_ipv4("v1.2.3.4"), "v1.2.3.4");
        assert_eq!(mask_ipv4("ends 10.0.0.5."), "ends x.x.x.x.");
    }
}
