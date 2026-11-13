//! PL: Pytanie serwera o status, tak jak robi to lista serwerów w grze. Instalator sprawdza tak,
//! czy przez proxy da się dojść do silnika, bez konta i bez usługi sesji (status nie loguje).
//! EN: Asks a server for its status, as the game's server list does. The installer uses it to
//! check that the engine is reachable through the proxy, with no account and no session service
//! (status involves no login).

use horizon_protocol::{
    connection::Connection,
    packets::{
        handshake::{self, Intention},
        status, Empty,
    },
    PROTOCOL_VERSION,
};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// PL: Tekst JSON statusu serwera pod `address`. EN: The status JSON of the server at `address`.
///
/// # Errors
/// PL: Nikt nie słucha, połączenie się urwało albo odpowiedź nie jest statusem.
/// EN: Nothing listens, the connection broke, or the answer is not a status.
pub fn status(address: SocketAddr, timeout: Duration) -> Result<String, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{address}: {e}");
    let stream = TcpStream::connect_timeout(&address, timeout).map_err(|e| fail(&e))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| fail(&e))?;
    let mut c = Connection::from_stream(stream).map_err(|e| fail(&e))?;
    let intention = handshake::Serverbound::Intention(Intention {
        protocol: PROTOCOL_VERSION,
        host: address.ip().to_string(),
        port: address.port(),
        intent: handshake::STATUS,
    });
    c.send(&intention.encode()).map_err(|e| fail(&e))?;
    c.send(&status::Serverbound::StatusRequest(Empty).encode())
        .map_err(|e| fail(&e))?;
    let (id, payload) = c.recv().map_err(|e| fail(&e))?;
    match status::Clientbound::decode(id, &payload).map_err(|e| fail(&e))? {
        status::Clientbound::StatusResponse(r) => Ok(r.json),
        other => Err(fail(&format!("expected a status, got {}", other.name()))),
    }
}
