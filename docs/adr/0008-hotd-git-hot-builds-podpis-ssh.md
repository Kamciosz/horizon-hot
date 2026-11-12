# ADR 0008 — `hotd`: wyniki CI na gałęzi `hot-builds`, podpis `ssh-keygen`

- **Data:** 2026-11-12
- **Status:** accepted (zmienia część D14 w `PLAN.md`)
- **Autor:** Szymon Sosnowski

## Kontekst

D14 zakładało, że CI publikuje bibliotekę moda jako artefakt GitHub Actions, a `hotd` ją pobiera. Przy budowie M2 wyszły trzy problemy:
1. Artefakt Actions pobiera się przez REST API z tokenem. Klucz tylko do odczytu (deploy key, D22) daje dostęp wyłącznie przez git po SSH, więc serwer potrzebowałby drugiego sekretu.
2. Artefakty Actions wygasają (domyślnie po 90 dniach). Po dłuższej przerwie serwer mógłby nie znaleźć plików dla ostatniego dobrego commita.
3. Podpis potrzebuje biblioteki kryptograficznej, a każda nowa zależność wymaga ADR.

## Rozważane opcje

- Transport: artefakty Actions + token API; wydania GitHuba; osobna gałąź w repo serwera, na którą CI wypycha wynik.
- Podpis: `ed25519-dalek` w `hotd` (nowa zależność); `minisign`/`signify` (nowy program na serwerze); `ssh-keygen -Y sign/verify` (OpenSSH jest w Ubuntu i w GitHub Actions).

## Decyzja

- **Transport:** workflow `hot.yml` w repo serwera (`templates/server-repo/.github/workflows/hot.yml`) buduje mody na `ubuntu-22.04` (`scripts/hot-build.sh`) i wypycha wynik na gałąź `hot-builds` (`scripts/hot-publish.sh`): `<sha>/build.json` oraz `<sha>/<platforma>/<id>.<so|dylib>` z podpisem `.sig`. Platforma to `std::env::consts::{ARCH}-{OS}`, np. `x86_64-linux`. Gałąź trzyma katalogi ostatnich 50 commitów. Kompilacja, która padła, też trafia na gałąź jako `"ok": false` z końcem logu, więc `hot status` pokazuje powód.
- **`hotd`:** lustro `hot-state/repo.git`, jedno `git fetch` co 10 s po wszystkie gałęzie, pliki czytane z obiektów gita (`git cat-file`). Zmiany liczone od ostatniego **zastosowanego** commita (D24, `crates/hotd/src/classify.rs`). Repo jest źródłem prawdy: mod aktywny w silniku, którego nie ma w `mods/`, zostaje wyłączony. Po `hot load` / `hot unload` `hotd` wysyła `hot status`, aż `hot-core` pokaże zmiany w `status.json` (pauza pustego serwera, ADR 0007).
- **Podpis:** `ssh-keygen -Y sign -n horizon-hot-mod` w CI kluczem z sekretu `HOT_SIGNING_KEY`; na serwerze `ssh-keygen -Y verify` z plikiem `<HOT_HOME>/hot-keys/allowed_signers` (tożsamość `hot-ci`). Bez tego pliku `hotd` nie ładuje niczego. Plik z błędnym podpisem jest usuwany.
- **Zależności:** `hotd` używa tylko `hot-diag` i `serde_json`; z gitem i OpenSSH rozmawia przez programy.
- **Zły commit (D13):** jeśli w ciągu 60 s od zastosowania silnik nie działa albo ma inny PID (`horizon-server status --json`, pole `engine_pid`), commit trafia na listę złych i nie jest ponawiany.

## Konsekwencje

- Serwer potrzebuje tylko klucza do odczytu repo, `git` i `ssh-keygen`; żadnego tokenu API.
- Gałąź `hot-builds` rośnie o biblioteki (ok. 0,4 MB na mod i commit); retencja 50 commitów ogranicza rozmiar katalogów, ale nie historię gita. Jeśli repo urośnie za bardzo, `hot-publish.sh` może zacząć wypychać gałąź bez historii (do decyzji, gdy będzie potrzebne).
- Workflow ma `contents: write`, żeby wypchnąć `hot-builds`, i `concurrency`, żeby dwa pushe nie ścigały się o tę gałąź.
- Biblioteka budowana na Ubuntu 22.04 działa na nowszym Ubuntu (starsze glibc). Serwer ARM wymaga drugiej platformy w workflow (`HOT-E0204` mówi, czego brakuje).
- Nie sprawdzone bez repo serwera na GitHubie: sam przebieg `hot.yml` w Actions i czas push → serwer. Lokalnie sprawdzone: `hot-build.sh` i `hot-publish.sh` na repo z `licznik`, `hotd once` na prawdziwym silniku (szczegóły w `PLAN.md`, sekcja 8).
