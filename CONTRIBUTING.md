# Jak pracujemy

## Pierwsze uruchomienie

```bash
git clone https://github.com/Kamciosz/horizon-hot.git
cd horizon-hot
git config core.hooksPath .githooks
cargo test --workspace
```

Sprawdź, czy hook działa: `git config core.hooksPath` ma zwrócić `.githooks`. Wersję Rusta ustawia `rust-toolchain.toml`.

## Gałęzie

| Typ | Nazwa | Od czego |
|---|---|---|
| funkcja | `feat/krotki-opis` | `main` |
| poprawka | `fix/krotki-opis` | `main` |
| dokumentacja | `docs/krotki-opis` | `main` |
| refaktor | `refactor/krotki-opis` | `main` |

`main` zawsze się buduje i przechodzi testy. Nikt nie wrzuca zmian prosto do `main`.

## Commity

```
<typ>(<zakres>): co i po co        temat od 10 do 72 znaków

Dlaczego ta zmiana jest potrzebna.
Jak to sprawdzić.
```

Typy: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`. Zakres to nazwa składnika: `hot-core`, `hotd`, `hot-proxy`, `hot-play`…

Temat piszemy po polsku w formie bezosobowej: „dodanie”, „poprawka”, „usunięcie”. Przykład: `feat(hot-core): ładowanie generacji poza wątkiem serwera`.

Hook odrzuca zły format, temat dłuższy niż 72 znaki i ślady AI w commicie. CI sprawdza to samo w każdym pull requeście.

## Pull request

1. `git fetch && git rebase origin/main`.
2. Dodaj `docs/zmiany/<gałąź>.md` z wpisem do changelogu (`docs/zmiany/README.md`). `CHANGELOG.md` zmienia tylko właściciel przy wydaniu — dzięki temu równoległe PR-y nie mają konfliktów.
3. Wypełnij szablon: co, po co, jak sprawdzone, ryzyka.
4. Szymon przegląda i scala. Bez zielonego CI nie scalamy.

## Kiedy gotowe

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` przechodzą
- [ ] jest test, który obleje bez tej zmiany
- [ ] nowe błędy mają kod i wpis w `docs/bledy/` (`docs/STANDARD_KODU.md`)
- [ ] plik `docs/zmiany/<gałąź>.md`
- [ ] dokumentacja zmieniona razem z kodem
- [ ] brak martwego kodu, `println!` do debugowania i plików tymczasowych
