# ADR 0024 — mody gorące budowane w CI, silnik na serwerze (D14, D15)

- **Data:** 2026-11-14 (decyzje z 2026-11-12, przeniesione z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Budowa w Rust zajmuje dużo procesora i pamięci, a serwer ma najpierw obsługiwać grę (ADR 0025). Biblioteka moda gorącego nie zawiera plików Mojanga, więc może powstać w CI. Silnik z `hot-host` budowany przez `native-build` wymaga przygotowanych plików gry z kupionego Minecrafta, których nie wolno pobierać w CI (R21).

## Decyzja

- **D14.** Mody gorące buduje GitHub Actions w repo serwera (`templates/server-repo/hot.yml`, instalator kopiuje go do `/srv/horizon/server-repo-files/`). Workflow buduje `.so` dla serwera, podpisuje go kluczem CI (`scripts/hot-build.sh`, `scripts/hot-publish.sh`) i wypycha na gałąź `hot-builds`. `hotd` pobiera ją gitem i sprawdza podpis `ssh-keygen -Y verify` z `hot-keys/allowed_signers`. Szczegóły i powód (artefakt Actions wymaga tokenu i wygasa): ADR 0008.
- **D15.** Silnik buduje sam serwer, w tle (`horizon-bg.slice`): `hotd build-engine` uruchamia `native-build` launchera Horizona, a rewizję wybiera przypięcie w paczce `hot-host` (ADR 0012). Kod natywny buduje się tylko z `--trust-native-code`, czyli za zgodą właściciela. Silnik budujemy rzadko: przy nowej wersji Horizona albo nowym `hot-host`.

## Konsekwencje

- Serwer nie kompiluje modów gorących; dostaje gotowe, podpisane pliki.
- Repo serwera potrzebuje sekretu `HOT_SIGNING_KEY` (klucz prywatny z instalatora). Bez niego CI nie podpisze modów, a `hotd` ich nie załaduje.
- Nie sprawdzone: przebieg workflow w GitHub Actions dla prawdziwego repo serwera (M2.1) i `native-build` na Ubuntu x86_64 (M0.10).
