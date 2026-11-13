# ADR 0012 — rewizję Horizona w silniku wybiera przypięcie paczki `hot-host`

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Silnik z `hot-host` buduje `horizon-launcher native-build --server-output DIR --version V`. Do dziś instalator, obraz i plan D18 robiły to tak: `horizon-launcher install <tag Nightly>`, potem `native-build --version <ten tag>` (D18: z `--ignore-pins`).

FAKT, 2026-11-13: `horizon-launcher versions` nie zna już `nightly-2026-11-05-66f3981`. Na GitHubie zostały tylko wydania `nightly-2026-11-07`, `-08` i `-13`. Instalator na czystej maszynie padłby więc na kroku 13, zanim zbuduje silnik. Każde nowe Nightly z D18 czekałoby to samo po kilku dniach.

FAKT z kodu launchera w rewizji 66f3981 (czytany, nie uruchomiony z prawdziwą budową):
- `backend::resolve_version`: `--version` musi być zainstalowane, inaczej błąd „is not installed”. Zainstalowane = katalog wersji z plikiem `COMPLETE`.
- `pipeline` woła `backend::need_prepared(base)`: wersja bazowa musi mieć przygotowane pliki gry.
- `pinned_revision` → `newest` → `agreed`: przy jednym przypięciu budowa idzie na przypiętej rewizji, starszej albo nowszej od bazy. Daty z GitHuba potrzebne są tylko przy dwóch różnych przypięciach.
- `with_hooks`: `hot-host` nie ma `[hooks]`, więc rewizja się nie przesuwa.
- `source(revision)` pobiera `codeload.github.com/<repo>/tar.gz/<rewizja>`. Działa po zniknięciu wydania, bo źródło każdego Nightly zostaje pod tagiem `nightly-source/<data>-<skrót>`. `git ls-remote` pokazał te tagi wstecz do najstarszych Nightly.
- `install` po rozpakowaniu woła `reuse_prepared_for`. Nowe wydanie z tą samą wersją gry (26.3) kopiuje przygotowane pliki innej zainstalowanej wersji, bez pobierania od Mojanga.
- Baza daje budowie tylko pliki gry i platformę (`platform_for_game`). Zakładamy, że przypięta rewizja gra w tej samej wersji Minecrafta co baza (dziś wszędzie 26.3). Zmiana wersji gry to osobna, większa aktualizacja.

## Rozważane opcje

1. **Bez zmian**: `install <tag>` + `--version <tag>` (+ `--ignore-pins` w D18). Pada, gdy tag zniknie, czyli po kilku dniach.
2. **Własny katalog wersji w danych launchera** (plik `COMPLETE`, skopiowane zasoby). Zależy od wewnętrznego układu launchera i dotyka plików, które należą do niego.
3. **Przypięcie w kopii paczki**: baza to dowolne zainstalowane wydanie albo Nightly. Gdy nie ma żadnego, `install latest`. Rewizję wybiera `horizon_revision` w kopii paczki `hot-host`. `--ignore-pins` znika.

## Decyzja

Wybieramy **opcję 3**, w `crates/hotd/src/build.rs` (`hotd build-engine`):
- instalator, obraz i aktualizacja Horizona (M9) budują tą samą funkcją;
- rewizja = `HORIZON_REV` (instalator, obraz) albo nowe Nightly z tagu `nightly-source/…` (M9);
- przypięcie zmienia się tylko w kopii paczki, a oryginał zostaje bez zmian;
- wersja `native-…` nigdy nie jest bazą.

## Konsekwencje

- Instalator nie potrzebuje już wydania, które istniało w dniu jego napisania. Potrzebuje dowolnego wydania z 26.3 i przygotowanych plików gry dla użytkownika `horizon`, jak dotąd.
- Folder silnika nazywa się `<commit horizon-hot>-<12 znaków rewizji Horizona>`, a nie od tagu wydania.
- Paczka `hot-host` zostaje na serwerze w `/srv/horizon/hot-host-bundle`. Aktualizacja Horizona buduje z niej.
- Nie sprawdzone: prawdziwa budowa tą drogą. `native-build` uruchamia kod natywny, a zgodę daje tylko właściciel (`--trust-native-code`). Testy (`crates/hotd/tests/build.rs`) używają atrapy launchera.
- Gdy wydanie z inną wersją gry stanie się najnowsze, a nic nie będzie zainstalowane, `install latest` da bazę bez przygotowanych plików. Launcher odmówi wtedy z błędem „is not prepared yet” (HOT-E0301). Sam nic nie zgadnie.
