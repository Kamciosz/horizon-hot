//! PL: Pytanie usługi sesji, czy gracz wszedł na ten serwer (`hasJoined`). Tylko tak serwer wie, że
//! gracz ma kupioną grę: klient loguje się kontem Microsoft i zgłasza wejście, a usługa sesji to
//! potwierdza. Adres usługi bierzemy z dokumentu discovery, jak Horizon
//! (`horizon-client-render/src/auth.rs`). Testy podstawiają własny dokument, więc nie łączą się
//! z Mojangiem.
//!
//! EN: Asks the session service whether a player joined this server (`hasJoined`). Only this tells
//! the server the player owns the game: the client signs in with a Microsoft account and reports
//! the join, and the session service confirms it. The service URL comes from the discovery
//! document, as in Horizon (`horizon-client-render/src/auth.rs`). Tests supply their own document,
//! so they never reach Mojang.

use horizon_protocol::packets::login::Property;
use serde_json::Value;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// PL: Dokument discovery, z którego korzysta gra. EN: The discovery document the game uses.
pub const DEFAULT_DISCOVERY: &str = "https://discovery.minecraftservices.com/minecraft/client";

/// PL: Profil potwierdzony przez usługę sesji. EN: A profile the session service confirmed.
#[derive(Debug, PartialEq)]
pub struct Joined {
    pub uuid: u128,
    /// PL: Nazwa z usługi sesji; `None`, gdy jej nie podała. EN: The name from the service, if given.
    pub name: Option<String>,
    pub properties: Vec<Property>,
}

/// PL: Klient usługi sesji z dokumentem discovery pobranym raz (błąd: ponów przy następnym graczu).
/// EN: A session service client; the discovery document is fetched once (a failure retries next time).
pub struct SessionService {
    discovery_url: String,
    doc: Mutex<Option<Arc<Value>>>,
    agent: ureq::Agent,
}

impl SessionService {
    #[must_use]
    pub fn new(discovery_url: &str) -> Self {
        Self {
            discovery_url: discovery_url.to_owned(),
            doc: Mutex::new(None),
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .user_agent(&format!("hot-proxy/{}", crate::VERSION))
                .build(),
        }
    }

    /// PL: Adres dokumentu discovery. EN: The discovery document URL.
    #[must_use]
    pub fn discovery_url(&self) -> &str {
        &self.discovery_url
    }

    /// PL: `Ok(Some)` — gracz potwierdzony; `Ok(None)` — usługa mówi „nie”; `Err` — brak odpowiedzi.
    /// EN: `Ok(Some)`: confirmed; `Ok(None)`: the service says no; `Err`: no answer.
    ///
    /// # Errors
    /// PL: Usługa sesji albo dokument discovery nie odpowiada (błąd sieci, status 5xx).
    /// EN: The session service or the discovery document does not answer (network, status 5xx).
    pub fn has_joined(&self, name: &str, server_hash: &str) -> Result<Option<Joined>, String> {
        let doc = self.discovery()?;
        let url = doc
            .pointer("/discovery/session/endpoints/verify/uri")
            .and_then(Value::as_str)
            .ok_or("the discovery document has no session verify endpoint")?;
        let separator = if url.contains('?') { '&' } else { '?' };
        let query = format!(
            "{url}{separator}username={}&serverId={}",
            url_encode(name),
            url_encode(server_hash)
        );
        match self.agent.get(&query).call() {
            Ok(response) => Ok(parse_joined(&response.into_string().unwrap_or_default())),
            Err(ureq::Error::Status(status, _)) if status >= 500 => {
                Err(format!("the session service answered with status {status}"))
            }
            Err(ureq::Error::Status(_, _)) => Ok(None),
            Err(ureq::Error::Transport(e)) => Err(format!("the session service: {e}")),
        }
    }

    fn discovery(&self) -> Result<Arc<Value>, String> {
        let mut cached = self.doc.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(doc) = cached.clone() {
            return Ok(doc);
        }
        let text = self
            .agent
            .get(&self.discovery_url)
            .call()
            .map_err(|e| format!("the discovery document {}: {e}", self.discovery_url))?
            .into_string()
            .map_err(|e| format!("the discovery document {}: {e}", self.discovery_url))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| format!("the discovery document is not JSON: {e}"))?;
        let doc = Arc::new(doc);
        *cached = Some(Arc::clone(&doc));
        Ok(doc)
    }
}

/// PL: `URLEncoder.encode(…, UTF-8)` jak w grze. EN: `URLEncoder.encode(…, UTF-8)`, as the game does.
fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// PL: Odpowiedź `hasJoined`: `id`, `name`, `properties`. Bez `id` nie ma profilu (status 204).
/// EN: The `hasJoined` answer: `id`, `name`, `properties`. No `id` means no profile (status 204).
#[must_use]
pub fn parse_joined(body: &str) -> Option<Joined> {
    let v: Value = serde_json::from_str(body).ok()?;
    let hex: String = v
        .get("id")?
        .as_str()?
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if hex.len() != 32 {
        return None;
    }
    let uuid = u128::from_str_radix(&hex, 16).ok()?;
    let properties = v
        .get("properties")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Property {
                name: p.get("name")?.as_str()?.to_owned(),
                value: p.get("value")?.as_str()?.to_owned(),
                signature: p
                    .get("signature")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect();
    Some(Joined {
        uuid,
        name: v.get("name").and_then(Value::as_str).map(str::to_owned),
        properties,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_is_read_and_an_empty_answer_is_none() {
        let body = r#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch",
            "properties":[{"name":"textures","value":"v","signature":"s"}]}"#;
        let got = parse_joined(body);
        assert_eq!(
            got,
            Some(Joined {
                uuid: 0x069a_79f4_44e9_4726_a5be_fca9_0e38_aaf5,
                name: Some("Notch".into()),
                properties: vec![Property {
                    name: "textures".into(),
                    value: "v".into(),
                    signature: Some("s".into()),
                }],
            })
        );
        assert_eq!(parse_joined(""), None);
        assert_eq!(parse_joined(r#"{"id":"short"}"#), None);
    }

    #[test]
    fn query_values_are_encoded_like_java() {
        assert_eq!(url_encode("-1a2b"), "-1a2b");
        assert_eq!(url_encode("a b&c"), "a+b%26c");
    }
}
