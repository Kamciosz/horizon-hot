# ADR 0015 — aplikacja gracza `hot-play` (M11.1, M11.2, D26)

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

D26: gracz ma jedną aplikację w terminalu. „Graj” sprawdza i naprawia wszystko, czego gra na tym serwerze potrzebuje, potem łączy z serwerem. „Napraw” robi to od nowa i zapisuje plik do wysłania właścicielowi.

Fakty z launchera 0.2.49 (2026-11-13):
- launcher ma polecenia z `--json`: `versions`, `versions --installed`, `instance list`, `account list`, `instance create`, `instance version`, `install`, `prepare [--download]`, `account sign-in` (kod urządzenia), `play --instance --server`;
- instancja ma własny folder gry (`home`), a w nim `servers.dat`, `horizon-mods/` i `horizon-mods.txt` (linia `mod.<id>:true`, sprawdzone w `hot-spike`);
- `multiplayer add` nie ustawia `acceptTextures`, a bez tego gra pyta o paczkę zasobów przy każdym wejściu (D9);
- datowane wydania Nightly znikają z GitHuba po kilku dniach: wydania serwera `nightly-2026-11-05-66f3981` już nie ma (`release not found`), dostępne są 11-07, 11-08 i 11-13;
- launcher sam się aktualizuje; wydania `launcher@…` mają pliki dla macOS arm64 i x86_64, Linux x86_64, Windows x86_64 oraz `SHA256SUMS`.

## Rozważane opcje

1. **Skrypt powłoki** wołający launcher. Dwa skrypty (sh i PowerShell), brak testów sekwencji, trudne parsowanie JSON.
2. **Program w Rust nad launcherem.** Jeden kod na trzy systemy; sekwencja „Graj” jako czysta funkcja nad cechą, testowana na fałszywym świecie.
3. **Program w Rust bez launchera** (własne pobieranie gry i logowanie). Powiela launcher, w tym kontrolę zakupu, której nie wolno obchodzić.

## Decyzja

Wybieramy **opcję 2** (`crates/hot-play`):
- `game::graj` to kolejne kroki nad cechą `World`: launcher → `/hot-play.json` z serwera → wersja → logowanie tylko, gdy w launcherze nie ma konta → `prepare`, gdy ta wersja nie jest zapisana w `state.json` (albo przy „Napraw”) → instancja `horizon-hot` przypięta do wersji → `hot_slots.hmod` po SHA-1 → `horizon-mods.txt` → `servers.dat` → `play --server`. Każdy krok zapisuje tylko to, co się różni, więc drugie „Graj” tylko uruchamia grę.
- **Wersja:** Nightly z tym samym skrótem rewizji co silnik (najpierw zainstalowany), inaczej najnowszy Nightly z komunikatem dla gracza. HIPOTEZA: klient z innego Nightly tej samej wersji gry (26.3) łączy się z serwerem; sprawdzenie w M11 na prawdziwym serwerze.
- **Serwer** podaje `GET /hot-play.json` (`{"horizon": rewizja silnika, "slots_sha1": suma puli}`) w `hotd serve-pack`, obok paczki i `/hot_slots.hmod`. Rewizja pochodzi z `native-mods.json` silnika z `native-build`, a dla silnika zbudowanego przez instalator ze źródeł z pliku `<HOT_HOME>/horizon-rev` (instalator, krok 4, i kontener). Gdy serwer jej nie poda, gracz dostaje najnowszy Nightly z osobnym komunikatem.
- **`servers.dat`** zapisujemy sami (NBT, jeden serwer z `acceptTextures` = 1), we własnej instancji `hot-play`. Innych instancji ani globalnej listy serwerów nie ruszamy.
- **Launcher:** pierwszy działający z `HOT_PLAY_LAUNCHER`, `<dane>/launcher`, `PATH`, `/Applications/Horizon.app`. Brak = pobranie najnowszego `launcher@…` z GitHuba, sprawdzenie SHA-256 z `SHA256SUMS`, rozpakowanie `tar`. Podpisu `SHA256SUMS.sig` nie sprawdzamy (brak opisanego klucza); suma chroni przed uszkodzonym pobraniem, nie przed podmienionym wydaniem.
- **Zależności:** `sha1` 0.10.7 (już w `Cargo.lock` przez `horizon-protocol`) do sumy puli, taka sama jak w `hotd`; `sha2` i `ureq` jak w `hot-proxy` (ADR 0010); `serde_json` jak w `hot-diag` (ADR 0003). Nowych bibliotek w drzewie nie ma.
- **„Napraw”** różni się od D26 („ponowne pobranie wszystkiego”): od nowa robi `prepare` i pobiera pulę slotów, a launcher i wersję Horizona tylko sprawdza (działający launcher i zainstalowanej wersji nie pobiera drugi raz; zepsuty launcher i tak jest pobierany przy każdym „Graj”). Nie uruchamia gry; zapisuje raport `hot-play-raport.md` (`hot_diag::report`) na pulpicie. Błędy mają kody `HOT-E0901`–`HOT-E0908` i zdanie po polsku dla gracza (`codes::for_player`).

## Konsekwencje

- Kontrola zakupu zostaje w launcherze i na serwerze (proxy); `hot-play` nie ma plików Mojanga, a `prepare --download` pobiera je tylko dla konta, które ma grę.
- Gracz może grać na innym Nightly niż serwer, gdy wersji serwera już nie ma do pobrania (HIPOTEZA wyżej).
- `hot-play` nie zna stanu konta poza „jest/nie ma”; wygasłe logowanie kończy się `HOT-E0908` i komunikatem launchera.
- Samoaktualizacja `hot-play`, skrypty instalacyjne i budowa w CI: ADR 0016 (M11.3).
