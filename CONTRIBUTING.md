# CONTRIBUTING

Dzięki, że wchodzisz w projekt. Poniżej zasady, które trzymają kod czytelnym dla kolejnych osób.

## Setup
```bash
git clone <adres>
cd <projekt>
<komenda instalacji zależności>
<komenda build>
<komenda testów>
```

## Gałęzie
| Typ | Nazwa | Od czego |
|---|---|---|
| funkcja | `feat/krotki-opis` | `main` |
| poprawka | `fix/krotki-opis` | `main` |
| dokumentacja | `docs/krotki-opis` | `main` |
| refaktor | `refactor/krotki-opis` | `main` |

`main` jest zawsze zielone (build + testy). Bez pushy bezpośrednich do `main`.

## Commity (Conventional Commits)
```
<typ>(<zakres>): opis co i dlaczego   ← temat, ≤72 znaki

Dlaczego ta zmiana jest potrzebna.
Co się zmienia dla innych modułów.
Jak to sprawdzić.
```
Typy: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
Format wymusza hook: `git config core.hooksPath .githooks` (robisz to raz po klonie; opis w tym pliku niżej nie wystarcza — hook jest obowiązkowy).

## Pull request
1. Rebase na świeży `main`: `git fetch && git rebase origin/main`.
2. Wypełnij szablon PR (co / po co / jak testowane / ryzyka).
3. Minimum 1 review. Review to nie formalność: brakuje testu? brakuje komentarza w nieoczywistym miejscu? → komentarz.
4. Merge dopiero, gdy CI zielone i komentarze rozwiązane.

## Definition of done
- [ ] build + testy + lint przechodzą lokalnie (komendy w `AGENTS.md`)
- [ ] `CHANGELOG.md` uzupełniony (sekcja `Unreleased`)
- [ ] docs/README zaktualizowane, jeśli zmieniło się zachowanie lub struktura
- [ ] brak martwego kodu, debug-printów i plików tymczasowych w diffie
- [ ] commity atomowe i opisowe (nie „fix", „update")

## Style
- Formatowanie: `<formatter>` (konfiguracja w repo) — uruchom przed commitem.
- Nazwy po angielsku, opisowe. Komentarze: tam, gdzie kod nie mówi „dlaczego".
