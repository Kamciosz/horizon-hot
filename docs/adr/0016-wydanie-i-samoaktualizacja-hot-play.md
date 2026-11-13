# ADR 0016 — wydanie, instalacja i samoaktualizacja `hot-play` (M11.3, D26)

- **Data:** 2026-11-14
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

D26: gracz instaluje aplikację jedną linijką, dostaje skrót „Graj”, a aplikacja sama się aktualizuje. Gracze mają macOS (arm64 i x86_64), Windows i Linuksa. Nie mają Rusta, więc potrzebują gotowych plików. Plików nie podpisujemy certyfikatem Apple ani Microsoftu (nie ma konta dewelopera).

Fakty (2026-11-14, Mac):
- plik pobrany przez `curl` nie dostaje atrybutu `com.apple.quarantine`, więc Gatekeeper go nie blokuje; dostaje tylko `com.apple.provenance`. Ten sam plik pobrany przez przeglądarkę dostałby kwarantannę;
- `rustc` na arm64 sam podpisuje plik podpisem ad hoc, więc działa bez certyfikatu;
- PowerShell 5.1 czyta skrypt bez BOM jako ANSI, a bajty polskich znaków w UTF-8 zawierają 0x84 („) i 0x93/0x94, które PowerShell traktuje jak cudzysłów.

## Rozważane opcje

1. **Skrypt instalacyjny z serwera gry** (`hotd serve-pack`). Jeden adres dla gracza, ale to zwykły HTTP przekazany do `sh` (podmiana po drodze = cudzy kod), a każda zmiana skryptu to pełna aktualizacja serwera (ADR 0014).
2. **Wydanie na GitHubie, publikowane ręcznie.** HTTPS, ale ktoś musi pamiętać o publikacji, a agenci nie publikują sami.
3. **Wydanie na GitHubie, publikowane przez CI po merge'u na `main`.**

## Decyzja

Wybieramy **opcję 3** (`.github/workflows/hot-play.yml`):
- Każdy push buduje `hot-play` na cztery systemy (`hot-play-macos-arm64`, `hot-play-macos-x64` budowany krzyżowo na arm64, `hot-play-linux-x64` na Ubuntu 22.04 dla starszej glibc, `hot-play-windows-x64.exe`) i zostawia je jako artefakty przebiegu. Nic nie jest publiczne.
- Push na `main` publikuje te pliki, `install.sh`, `install.ps1` i `SHA256SUMS` w wydaniu `hot-play` (prerelease o stałej nazwie, pliki nadpisywane). **Merge na `main` to zgoda właściciela na publiczne wydanie.**
- `install.sh` (macOS, Linux) i `install.ps1` (Windows) pobierają plik do folderu danych `hot-play`, sprawdzają SHA-256 i dopiero wtedy wstawiają go na miejsce, więc przerwana instalacja niczego nie psuje. Opcjonalny argument to adres serwera (`hot-play ustaw`). Skrót „Graj”: `~/Desktop/Graj.command` (macOS), `Graj.desktop` w menu aplikacji i na pulpicie (Linux), `Graj.lnk` (Windows). `install.ps1` ma BOM UTF-8 i uruchamia się z pliku, nie z `iex`.
- Samoaktualizacja (`crates/hot-play/src/update.rs`) działa tylko w plikach z CI: workflow ustawia przy kompilacji `HOT_PLAY_ASSET` i `HOT_PLAY_UPDATE_URL`. Przy „Graj”, „Napraw”, „sprawdz” i w menu program porównuje SHA-256 swojego pliku z `SHA256SUMS`. Inna suma: pobiera, sprawdza, wstawia (Unix: `rename`; Windows: stary plik dostaje `.old`) i uruchamia się od nowa z tymi samymi argumentami i `HOT_PLAY_NO_UPDATE=1`. Każdy błąd idzie do logu (`HOT-E0909`), a program działa dalej na obecnej wersji. `HOT_PLAY_NO_UPDATE` wyłącza samoaktualizację, `HOT_PLAY_UPDATE_URL` w otoczeniu zmienia adres wydania (test).
- Podpis `.sig` launchera Horizona i podpis plików `hot-play` nie są sprawdzane: zaufanie opiera się na HTTPS GitHuba i sumach z tego samego wydania.

## Konsekwencje

- Gracz potrzebuje tylko jednej linijki z README. Aktualizacja `hot-play` dociera do niego przy następnym starcie po merge'u.
- Konto GitHub z prawem zapisu do repo może wydać graczom dowolny kod. To ten sam poziom zaufania co serwer, który instaluje z tego repo (ADR 0014).
- Między wgraniem nowego pliku a nowego `SHA256SUMS` gracz może trafić na niezgodność. Wtedy aktualizacja się nie uda, gra pójdzie na starej wersji, a następny start spróbuje znowu.
- Plik `hot-play` pobrany przeglądarką na macOS dostanie kwarantannę. Dlatego README podaje tylko instalację z terminala.
- Nie sprawdzone (brak maszyn): Windows 11, Ubuntu Desktop, Mac x86_64, skrót `.desktop` w GNOME i KDE.
