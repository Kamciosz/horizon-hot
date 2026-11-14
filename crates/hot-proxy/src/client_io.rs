//! PL: Połączenie z graczem: ramki, kompresja i szyfrowanie CFB8 jak w `horizon_protocol::connection`,
//! ale ze stanem na wierzchu. `ConnReader` czyta naprzód do prywatnego, już odszyfrowanego bufora,
//! a `hot-proxy upgrade` (M13.1, ADR 0033) musi przekazać nowemu kodowi wszystko, co przeczytał,
//! i rejestr szyfru w obu kierunkach. Rejestr CFB8 to ostatnie 16 bajtów szyfrogramu (na początku
//! sekret), więc `Cipher` liczy go sam z bajtów, które szyfruje i odszyfrowuje.
//!
//! EN: The connection to the player: frames, compression and CFB8 encryption as in
//! `horizon_protocol::connection`, but with the state in the open. `ConnReader` reads ahead into a
//! private, already decrypted buffer, and `hot-proxy upgrade` (M13.1, ADR 0033) must hand the new
//! code everything it read and the cipher register in both directions. The CFB8 register is the
//! last 16 ciphertext bytes (the secret before any), so `Cipher` tracks it from the bytes it
//! encrypts and decrypts.

use horizon_protocol::{
    buf::{write_varint, Reader},
    crypto::Cfb8,
    frame::{self, Body, Inflater},
    Error, Result,
};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

/// PL: Tyle bajtów czyta jeden odczyt z gniazda. EN: How many bytes one socket read takes.
const READ: usize = 1 << 16;

/// PL: Szyfr jednego kierunku z rejestrem, który da się przekazać.
/// EN: One direction's cipher, with a register that can be handed over.
#[derive(Clone)]
pub struct Cipher {
    secret: [u8; 16],
    cfb: Cfb8,
    register: [u8; 16],
}

impl Cipher {
    /// PL: Szyfr od początku połączenia: sekret jest kluczem i wektorem.
    /// EN: The cipher from the start: the secret is the key and the IV.
    pub fn new(secret: [u8; 16]) -> Self {
        Self::resume(secret, secret)
    }

    /// PL: Szyfr w połowie strumienia, od zapisanego rejestru. EN: Mid-stream, from a saved register.
    pub fn resume(secret: [u8; 16], register: [u8; 16]) -> Self {
        Self {
            secret,
            cfb: Cfb8::new(&secret, &register),
            register,
        }
    }

    pub fn secret(&self) -> [u8; 16] {
        self.secret
    }

    pub fn register(&self) -> [u8; 16] {
        self.register
    }

    fn encrypt(&mut self, data: &mut [u8]) {
        self.cfb.encrypt(data);
        slide(&mut self.register, data);
    }

    fn decrypt(&mut self, data: &mut [u8]) {
        slide(&mut self.register, data);
        self.cfb.decrypt(data);
    }
}

/// PL: Rejestr po kolejnych bajtach szyfrogramu. EN: The register after more ciphertext bytes.
fn slide(register: &mut [u8; 16], ciphertext: &[u8]) {
    let n = ciphertext.len();
    if n >= 16 {
        register.copy_from_slice(&ciphertext[n - 16..]);
    } else {
        register.copy_within(n.., 0);
        register[16 - n..].copy_from_slice(ciphertext);
    }
}

/// PL: Odczyt od gracza: bufor tekstu jawnego jeszcze nie rozłożonego na pakiety.
/// EN: Reading from the player: a buffer of plaintext not split into packets yet.
pub struct ClientReader {
    stream: TcpStream,
    cipher: Option<Cipher>,
    buffer: Vec<u8>,
    start: usize,
    threshold: Option<usize>,
    inflater: Inflater,
}

/// PL: Zapis do gracza. EN: Writing to the player.
pub struct ClientWriter {
    stream: TcpStream,
    cipher: Option<Cipher>,
    threshold: Option<usize>,
}

/// PL: Zamyka połączenie z innego wątku. EN: Closes the connection from another thread.
pub struct ClientCloser(TcpStream);

impl ClientCloser {
    pub fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

/// PL: Obie strony przed podziałem na wątki. EN: Both halves before the split into threads.
pub struct ClientConn {
    pub reader: ClientReader,
    pub writer: ClientWriter,
}

/// PL: Stan strony gracza do przekazania nowemu kodowi. EN: The player side's state for the new code.
pub struct Handover {
    pub secret: Option<[u8; 16]>,
    pub read_register: [u8; 16],
    pub write_register: [u8; 16],
    /// PL: Odczytany, odszyfrowany, jeszcze nie przetworzony. EN: Read, decrypted, not processed yet.
    pub pending: Vec<u8>,
    pub threshold: Option<usize>,
}

impl ClientConn {
    pub fn new(stream: TcpStream) -> Result<Self> {
        let writer = ClientWriter {
            stream: stream.try_clone()?,
            cipher: None,
            threshold: None,
        };
        Ok(Self {
            reader: ClientReader::new(stream, None, Vec::new(), None),
            writer,
        })
    }

    /// PL: Odtwarza połączenie z gniazda i stanu ze starego procesu.
    /// EN: Rebuilds the connection from the socket and the old process's state.
    pub fn resume(stream: TcpStream, h: Handover) -> Result<Self> {
        let writer = ClientWriter {
            stream: stream.try_clone()?,
            cipher: h.secret.map(|s| Cipher::resume(s, h.write_register)),
            threshold: h.threshold,
        };
        let cipher = h.secret.map(|s| Cipher::resume(s, h.read_register));
        Ok(Self {
            reader: ClientReader::new(stream, cipher, h.pending, h.threshold),
            writer,
        })
    }

    pub fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        self.reader.recv()
    }

    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        self.writer.send(body)
    }

    /// PL: Ujemny próg wyłącza kompresję. EN: A negative threshold turns compression off.
    pub fn set_compression(&mut self, threshold: i32) {
        let t = usize::try_from(threshold).ok();
        self.reader.threshold = t;
        self.writer.threshold = t;
    }

    /// PL: Oba kierunki z kluczem `secret`; bajty przeczytane naprzód były już zaszyfrowane.
    /// EN: Both directions keyed with `secret`; bytes read ahead were sent encrypted.
    pub fn enable_encryption(&mut self, secret: &[u8; 16]) {
        let mut cipher = Cipher::new(*secret);
        let r = &mut self.reader;
        cipher.decrypt(&mut r.buffer[r.start..]);
        r.cipher = Some(cipher);
        self.writer.cipher = Some(Cipher::new(*secret));
    }

    pub fn split(self) -> (ClientReader, ClientWriter) {
        (self.reader, self.writer)
    }
}

impl ClientReader {
    fn new(
        stream: TcpStream,
        cipher: Option<Cipher>,
        pending: Vec<u8>,
        threshold: Option<usize>,
    ) -> Self {
        Self {
            stream,
            cipher,
            buffer: pending,
            start: 0,
            threshold,
            inflater: Inflater::default(),
        }
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        Ok(self.stream.set_read_timeout(timeout)?)
    }

    /// PL: Termin zapisu działa na całe gniazdo, więc obejmuje też [`ClientWriter`] z tego samego
    /// połączenia. EN: The write timeout applies to the whole socket, so it also covers the
    /// [`ClientWriter`] of the same connection.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        Ok(self.stream.set_write_timeout(timeout)?)
    }

    pub fn closer(&self) -> Result<ClientCloser> {
        Ok(ClientCloser(self.stream.try_clone()?))
    }

    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }

    /// PL: Czeka na cały pakiet: (id, treść). EN: Blocks until a whole packet arrives: (id, payload).
    pub fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        loop {
            if let Some(packet) = self.next()? {
                return Ok(packet);
            }
            self.fill()?;
        }
    }

    /// PL: Następny pakiet albo `None`, gdy minął limit czasu odczytu. Nic nie przepada: część
    /// ramki zostaje w buforze. EN: The next packet, or `None` when the read timeout passed.
    /// Nothing is lost: part of a frame stays in the buffer.
    pub fn poll(&mut self) -> Result<Option<(i32, Vec<u8>)>> {
        loop {
            if let Some(packet) = self.next()? {
                return Ok(Some(packet));
            }
            match self.fill() {
                Ok(()) => {}
                Err(e) if e.is_timed_out() => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    /// PL: Stan odczytu do przekazania: rejestr i nieprzetworzony tekst jawny.
    /// EN: The read state to hand over: the register and the unprocessed plaintext.
    pub fn handover(&self) -> (Option<[u8; 16]>, [u8; 16], Vec<u8>, Option<usize>) {
        let secret = self.cipher.as_ref().map(Cipher::secret);
        let register = self
            .cipher
            .as_ref()
            .map(Cipher::register)
            .unwrap_or_default();
        (
            secret,
            register,
            self.buffer[self.start..].to_vec(),
            self.threshold,
        )
    }

    /// PL: Cały pakiet z bufora albo `None`. EN: A whole packet from the buffer, or `None`.
    fn next(&mut self) -> Result<Option<(i32, Vec<u8>)>> {
        loop {
            let Some((prefix, len)) = frame::header(&self.buffer[self.start..])? else {
                return Ok(None);
            };
            if self.buffer.len() - self.start < prefix + len {
                return Ok(None);
            }
            let range = self.start + prefix..self.start + prefix + len;
            self.start = range.end;
            let frame = &self.buffer[range];
            let packet = match frame::body(frame, self.threshold, &mut self.inflater)? {
                Body::Plain(at) => split_id(&frame[at..])?,
                Body::Inflated(body) => split_id(&body)?,
            };
            if packet.is_some() {
                return Ok(packet);
            }
        }
    }

    /// PL: Jeden odczyt z gniazda, odszyfrowany na koniec bufora.
    /// EN: One socket read, decrypted onto the end of the buffer.
    fn fill(&mut self) -> Result<()> {
        if self.start > 0 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
        let at = self.buffer.len();
        self.buffer.resize(at + READ, 0);
        let read = self.stream.read(&mut self.buffer[at..]);
        let n = *read.as_ref().unwrap_or(&0);
        self.buffer.truncate(at + n);
        match read {
            Ok(0) => Err(Error::end_of_stream()),
            Ok(_) => {
                if let Some(c) = &mut self.cipher {
                    c.decrypt(&mut self.buffer[at..]);
                }
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => Ok(()),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Err(Error::timed_out())
            }
            Err(e) => Err(e.into()),
        }
    }
}

/// PL: Id i treść pakietu; pusta ramka to `None`, jak w grze. EN: The id and payload; an empty
/// frame is `None`, as in the game.
fn split_id(body: &[u8]) -> Result<Option<(i32, Vec<u8>)>> {
    if body.is_empty() {
        return Ok(None);
    }
    let mut r = Reader::new(body);
    let id = r.varint()?;
    Ok(Some((id, body[r.position()..].to_vec())))
}

impl ClientWriter {
    /// PL: Jeden pakiet (id + treść). EN: One packet body (id + payload).
    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        let contents = frame::contents(body, self.threshold)?;
        let mut framed = Vec::with_capacity(contents.len() + 3);
        write_varint(&mut framed, contents.len() as i32);
        framed.extend_from_slice(&contents);
        if let Some(c) = &mut self.cipher {
            c.encrypt(&mut framed);
        }
        self.stream.write_all(&framed)?;
        Ok(())
    }

    /// PL: Rejestr zapisu do przekazania. EN: The write register to hand over.
    pub fn register(&self) -> [u8; 16] {
        self.cipher
            .as_ref()
            .map(Cipher::register)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)] // PL: testy. EN: tests.

    use super::*;
    use horizon_protocol::connection::Connection;
    use std::net::TcpListener;

    const SECRET: [u8; 16] = *b"0123456789abcdef";

    #[test]
    fn a_cipher_resumed_from_its_register_continues_the_stream() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        for cut in [0, 1, 15, 16, 17, 333, 999] {
            let mut whole = data.clone();
            Cfb8::new(&SECRET, &SECRET).encrypt(&mut whole);

            let mut first = Cipher::new(SECRET);
            let (a, b) = data.split_at(cut);
            let mut a = a.to_vec();
            first.encrypt(&mut a);
            let mut b = b.to_vec();
            Cipher::resume(SECRET, first.register()).encrypt(&mut b);
            a.extend_from_slice(&b);
            assert_eq!(a, whole, "encrypt, cut at {cut}");

            let mut reader = Cipher::new(SECRET);
            let (a, b) = whole.split_at(cut);
            let mut a = a.to_vec();
            reader.decrypt(&mut a);
            let mut b = b.to_vec();
            Cipher::resume(SECRET, reader.register()).decrypt(&mut b);
            a.extend_from_slice(&b);
            assert_eq!(a, data, "decrypt, cut at {cut}");
        }
    }

    /// PL: Połączenie z gry po drugiej stronie, jego surowe gniazdo i nasze połączenie.
    /// EN: The game's connection on the far side, its raw socket and our connection.
    fn pair() -> (Connection, TcpStream, ClientConn) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let raw = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let far = Connection::from_stream(raw.try_clone().unwrap()).unwrap();
        let (near, _) = listener.accept().unwrap();
        (far, raw, ClientConn::new(near).unwrap())
    }

    fn keyed(far: &mut Connection, near: &mut ClientConn) {
        far.set_compression(64);
        far.enable_encryption(&SECRET);
        near.set_compression(64);
        near.enable_encryption(&SECRET);
    }

    #[test]
    fn packets_match_the_games_connection_with_compression_and_encryption() {
        let (mut far, _raw, mut near) = pair();
        keyed(&mut far, &mut near);
        let big = vec![9u8; 500];
        far.send(&[1, 2, 3]).unwrap();
        far.send(&[[4u8].as_slice(), &big].concat()).unwrap();
        assert_eq!(near.recv().unwrap(), (1, vec![2, 3]));
        assert_eq!(near.recv().unwrap(), (4, big.clone()));
        near.send(&[5, 6]).unwrap();
        near.send(&[[7u8].as_slice(), &big].concat()).unwrap();
        assert_eq!(far.recv().unwrap(), (5, vec![6]));
        assert_eq!(far.recv().unwrap(), (7, big));
    }

    #[test]
    fn a_connection_resumed_mid_frame_reads_and_writes_on() {
        let (mut far, mut raw, mut near) = pair();
        keyed(&mut far, &mut near);
        far.send(&[1, 10]).unwrap();
        near.send(&[2, 20]).unwrap();
        assert_eq!(near.recv().unwrap(), (1, vec![10]));
        assert_eq!(far.recv().unwrap(), (2, vec![20]));

        // PL: Gra wysyła pół ramki; proxy przekazuje połączenie, zanim dojdzie reszta.
        // EN: The game sends half a frame; the proxy hands the connection over before the rest.
        let mut framed = Vec::new();
        let contents = frame::contents(&[3, 30, 31, 32], Some(64)).unwrap();
        far.writer.frame_into(&contents, &mut framed);
        let half = framed.len() / 2;
        raw.write_all(&framed[..half]).unwrap();
        let (mut reader, writer) = near.split();
        reader
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        assert_eq!(reader.poll().unwrap(), None);
        let (secret, read_register, pending, threshold) = reader.handover();
        assert_eq!(pending.len(), half);
        let h = Handover {
            secret,
            read_register,
            write_register: writer.register(),
            pending,
            threshold,
        };
        let stream = reader.stream().try_clone().unwrap();
        drop((reader, writer));
        let mut again = ClientConn::resume(stream, h).unwrap();

        raw.write_all(&framed[half..]).unwrap();
        assert_eq!(again.recv().unwrap(), (3, vec![30, 31, 32]));
        far.send(&[4, 40]).unwrap();
        assert_eq!(again.recv().unwrap(), (4, vec![40]));
        again.send(&[5, 50]).unwrap();
        assert_eq!(far.recv().unwrap(), (5, vec![50]));
    }
}
