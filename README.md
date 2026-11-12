# horizon-hot

Serwer Horizon (Minecraft Java 26.3 napisany w Rust), który bierze mody z repozytorium git i wczytuje je w trakcie gry, bez restartu serwera. Dla małej grupy znajomych na własnym sprzęcie.

Stan: wczesny etap. Działa szkielet repozytorium; składniki powstają krok po kroku (`docs/ARCHITECTURE.md`).

Pracujesz nad projektem (człowiek albo agent AI)? Zacznij od `AGENTS.md`.

## Wymagania
- Rust stable (edycja 2021), `cargo`
- Do prób z silnikiem: launcher Horizon (`horizon-launcher`) z przygotowaną wersją Nightly

## Testy
```bash
git config core.hooksPath .githooks   # raz po klonie
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # oczekiwane: wszystkie testy „ok”
```

## Struktura
```
crates/hot-abi/   — granica C ABI między gospodarzem w silniku a modami gorącymi
crates/hot-diag/  — logi JSONL z kodami błędów i hot report
crates/hot-sdk/   — biblioteka dla autorów modów gorących (hot_mod!)
crates/hot-core/  — gospodarz modów gorących w silniku (hot load, generacje)
crates/hot-host/  — mod natywny w silniku, który ładuje hot-core
crates/hotd/      — demon serwera: repo serwera → podpisane mody z CI → hot load
scripts/          — instalator serwera (server-install.sh), budowa i publikacja modów w CI repo serwera
examples/licznik/, examples/sklep/, examples/teleport-blok/ — przykładowe mody gorące
docs/             — architektura, decyzje (ADR), standard kodu, katalog błędów
PLAN.md           — plan budowy (roboczy; po zakończeniu treść przechodzi do docs/)
```

## Dokumentacja
- `AGENTS.md` — wejście dla nowej osoby i agenta AI: słownik, zasady, typowe zadania
- `docs/ARCHITECTURE.md` — jak to jest zbudowane i dlaczego
- `docs/STANDARD_KODU.md` — komentarze, błędy i logi, testy
- `docs/bledy/` — katalog kodów błędów `HOT-EXXXX`
- `docs/adr/` — decyzje projektowe
- `CONTRIBUTING.md` — jak pracujemy (gałęzie, commity, pull requesty)
- `SECURITY.md` — zgłaszanie luk

## Licencja
MIT, zobacz `LICENSE`.
