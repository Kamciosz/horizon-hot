# ADR 0014 — serwer sam aktualizuje horizon-hot (M10.1, D19)

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

D19: `hotd`, `hot-proxy`, `hot-core` (i `hot-pack`, czyli `hotd serve-pack`) mają się aktualizować same, raz na dobę. `hot-core` przez przeładowanie, `hotd` przez restart usługi, `hot-proxy` dopiero bez graczy (do M13).

Od M8.3 to się zmieniło: silnik jest procesem potomnym `hotd engine` (usługa `horizon-hot-server`), więc restart tej usługi zatrzymuje silnik. Zdanie z D19 „`hotd` → restart, gra nie zauważa” jest już prawdziwe tylko dla `hotd run`, `hotd serve-pack` i `hotd horizon`.

Nie każda nowa wersja da się wdrożyć samymi plikami binarnymi:
- zmiana w `crates/hot-host` albo `crates/hot-abi` wymaga nowego silnika (kod natywny, zgoda właściciela, ADR 0013). Zmiana `ABI_MAJOR` wymaga też nowego `ref` w `hot.yml` repo serwera, którego `hotd` nie zmienia;
- zmiana instalatora albo szablonów (`scripts/server-install.sh`, `templates/`) może dodawać usługi, pliki i programy;
- zmiana `crates/hotd/src/slots.rs` zmienia pulę slotów, a pulę buduje instalator.

Wersje różnych programów działające obok siebie przez wiele godzin (nowy `hotd run`, stary `hotd engine`) rozmawiają przez gniazdo `hotd engine` i pliki w `hot-state`. Nikt nie pilnuje, że te formaty są zgodne między wersjami.

## Rozważane opcje

1. **Każdy składnik osobno, od razu**, a proxy i `hotd engine` bez graczy. Daje godziny pracy różnych wersji obok siebie.
2. **Wszystko naraz, w chwili bez graczy.** Nie ma różnych wersji obok siebie, ale nawet sam `hot-core` czeka na pusty serwer.
3. **Trzy rodzaje wersji według zmienionych plików:**
   - tylko `crates/hot-core`: przeładowanie `hot core load` od razu, przy graczach;
   - pozostały kod (`hotd`, `hot-proxy`, `hot-diag`, `hot-core`, `Cargo.lock`): wszystko naraz w chwili bez graczy;
   - zmiany, których same pliki binarne nie wystarczą: nic się nie wdraża, a `hot status` mówi, co zrobić.

## Decyzja

Wybieramy **opcję 3** (`crates/hotd/src/upgrade.rs`, `hotd upgrade`):
- **Wydanie** to głowa gałęzi podanej przy instalacji w `--hot-ref`. Gdy `--hot-ref` to tag albo commit, serwer się nie aktualizuje (przypięcie). Każdy push na tę gałąź trafia na serwer najpóźniej po dobie, dlatego właściciel instaluje z gałęzi, na którą trafia tylko kod z zielonym CI (`main`), albo przypina tag. Tagów ani wydań na GitHubie `hotd` nie potrzebuje.
- **Klasyfikacja** po `git diff --name-only <obecna> <nowa>` jest czystą funkcją (`plan`). Pliki, które nie trafiają na serwer (`docs/`, `*.md` oprócz `AGENTS.md`, testy, `spikes/`, `.github/`, `examples/`, `crates/hot-play/`, `scripts/install.sh` i `scripts/install.ps1`, pliki kontenera), nie liczą się. `crates/hot-sdk` liczy się jak `crates/hot-core`, bo trafia na serwer tylko w nim. `AGENTS.md` liczy się jak kod, bo `hot-diag report` go wbudowuje.
- **Tryb `core`:** nowy plik obok starego, `hot core load` w aktywnym silniku. Dopiero po odpowiedzi „Rdzeń podmieniony” nowy plik zastępuje stary. Odmowa silnika („Nie wczytam rdzenia”, „nie wystartował”) odrzuca wersję. Brak odpowiedzi albo inna odpowiedź to tylko ponowienie po dobie. Podmiana silnika (D11) wyklucza się z wczytaniem rdzenia przez `hot-state/swap.lock`. `hot-host` wczytuje go też przy każdym starcie silnika, a przy podmianie startuje nowy silnik, więc zły plik nie może zostać na miejscu.
- **Tryb `full`:** budowa w tle, pliki czekają w `bin/.next/`. Każdy nowy program musi się najpierw uruchomić (`--help`). W pierwszej chwili bez graczy (sprawdzanej co 10 minut) stare pliki dostają końcówkę `.previous`, nowe wchodzą na ich miejsce i wszystkie usługi restartują się naraz. Usługa z `Restart=always` jest aktywna zaraz po starcie, nawet gdy program pada po sekundzie, dlatego po wstaniu wszystkich `hotd` czeka jeszcze 30 s i porównuje `MainPID` i `NRestarts` każdej usługi. Gdy któraś nie wstanie w 3 minuty, zrestartuje się sama w tych 30 s albo silnik nie dojdzie do `running`, wracają stare pliki i stare usługi, a wersja trafia na listę odrzuconych.
- **Uruchamianie:** timer `horizon-hot-upgrade.timer` co 10 minut odpala jednorazową usługę jako root. Git, cargo, `horizon-server` i próba nowych programów działają z uid i gid użytkownika `horizon` (bez `runuser`: bez sesji PAM co 10 minut, a hamulec MSPT zatrzymuje samo cargo). Restart usług idzie przez `systemctl`. Stan (`hot-state/upgrade.json`) i log (`logs/hotd-upgrade.jsonl`) dostają właściciela `horizon`, bo czytają je `hot status` i `hot-diag`. Sieć i budowa idą najwyżej raz na dobę. Usługa nie jest procesem stałym, więc następne uruchomienie po wdrożeniu korzysta już z nowego `hotd`.
- **Kontener:** pliki binarne są w obrazie, więc kontener się nie aktualizuje sam. Właściciel robi `docker compose build && docker compose up -d` (`docs/docker.md`).

## Konsekwencje

- Poprawka samego `hot-core` wchodzi bez przerwy w grze. Reszta czeka na pusty serwer, jak proxy w D19.
- Na serwerze nie ma chwili, w której różne wersje `hotd` i `hot-proxy` działają razem.
- Zmiana w instalatorze, w `hot-host`, `hot-abi` albo w puli slotów zatrzymuje samoaktualizację (`HOT-E0706`), dopóki właściciel nie uruchomi instalatora.
- Kontener nie spełnia M10.1. Aktualizację robi tam przebudowa obrazu.
