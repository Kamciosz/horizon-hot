# ADR 0010 — zależności `hot-proxy`: `horizon-protocol`, `sha2`, `ureq`

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

`hot-proxy` (M8) stoi między graczami a silnikiem. Musi:
- mówić protokołem gry: ramki, kompresja, pakiety logowania;
- zalogować gracza online: klucz RSA, szyfrowanie AES/CFB8, hash serwera i zapytanie `hasJoined` do usługi sesji po HTTPS;
- podpisać silnikowi dane gracza HMAC-SHA256 (przekazywanie Velocity, `docs/velocity-forwarding.md` Horizona).

Biblioteka standardowa Rusta nie ma ani kryptografii, ani klienta HTTPS. Własna implementacja szyfrowania byłaby ryzykiem bezpieczeństwa: od tego zależy kontrola zakupu.

## Rozważane opcje

1. **Te same biblioteki co Horizon.** `horizon-protocol` z gita Horizona, w rewizji serwera (MIT, już użyty w próbie S8), `sha2` 0.10 i `ureq` 2 z TLS (rustls). Silnik używa ich do tego samego: `lan/conn.rs`, `lan/forwarding.rs`, `auth.rs`.
2. **Ogólne biblioteki** (`tokio`, `reqwest`, `aes`, `rsa`). Więcej kodu i zależności, a zgodność z protokołem gry trzeba by pisać od nowa.
3. **Własny kod** protokołu i kryptografii. Odrzucone z powodu ryzyka.

## Decyzja

Wybieramy **opcję 1**, wszystko w `[workspace.dependencies]`:
- `horizon-protocol` z gita, w rewizji Horizona serwera (dziś `66f3981`);
- `sha2 = "0.10.9"`;
- `ureq = "2.12.1"` bez domyślnych funkcji, z `tls`.

Wersje są takie jak w `Cargo.lock` Horizona, więc proxy i silnik liczą to samo tym samym kodem.

## Konsekwencje

- CI pobiera repozytorium Horizona (sam kod na MIT, bez plików Mojanga). Pierwsza budowa trwa dłużej, potem działa cache.
- Nowe Nightly może zmienić protokół. Rewizję `horizon-protocol` podnosimy razem z silnikiem (D18), a test aktualizacji obejmuje logowanie przez proxy.
- Testy i CI nie łączą się z Mojangiem. Adres dokumentu discovery można podmienić, a testy podstawiają lokalny serwer HTTP.
