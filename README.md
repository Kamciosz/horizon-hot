# horizon-hot

Serwer Horizon (Minecraft Java 26.3 napisany w Rust), który bierze mody z repozytorium git i wczytuje je w trakcie gry, bez restartu serwera. Dla małej grupy znajomych na własnym sprzęcie.

Stan: wczesny etap (M0 z `PLAN.md`). Działa na razie tylko szkielet repozytorium.

## Wymagania
- Rust stable (edycja 2021), `cargo`
- Do prób z silnikiem: launcher Horizon (`horizon-launcher`) z przygotowaną wersją Nightly

## Testy
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # oczekiwane: wszystkie testy „ok”
```

## Struktura
```
crates/hot-abi/   — granica C ABI między gospodarzem w silniku a modami gorącymi
docs/             — architektura, decyzje (ADR), sprzęt, aktualizacje
PLAN.md           — plan wdrożenia i decyzje architektoniczne
```

## Dokumentacja
- `PLAN.md` — cel, decyzje D1–D25, kroki M0–M12
- `docs/ARCHITECTURE.md` — jak to jest zbudowane i dlaczego
- `docs/adr/` — decyzje projektowe
- `CONTRIBUTING.md` — jak pracujemy (gałęzie, commity, review)

## Licencja
Do ustalenia.
