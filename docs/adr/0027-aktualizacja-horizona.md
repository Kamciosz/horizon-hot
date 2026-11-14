# ADR 0027 — aktualizacja Horizona z kanału Nightly (D18)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, zmieniona przez ADR 0012 i ADR 0013, przeniesiona z `PLAN.md`)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Horizon wydaje Nightly prawie codziennie, a datowane wydania znikają z GitHuba po kilku dniach. Tagi źródeł `nightly-source/<data>-<skrót>` zostają. Nowa wersja może zepsuć `hot-host` albo spowolnić serwer, a budowa silnika uruchamia kod natywny, na który zgodę daje tylko właściciel.

## Decyzja

- `hotd horizon` raz na godzinę sprawdza najnowszy tag `nightly-source/*` (`git ls-remote`). Bramka: `libloading` musi być w `Cargo.lock` tej rewizji (`HOT-E0701`), bo bez niej `hot-host` się nie zbuduje.
- Dalej: co najmniej 8 GiB wolnego dysku, budowa przez `hotd build-engine` (rewizję wybiera przypięcie paczki `hot-host`, ADR 0012), test na kopii świata z 10 botami przez 300 s (`crates/hotd/src/trial.rs`), podmiana przez `hotd engine` (ADR 0022), gdy nikt nie gra, najpóźniej po 24 h z ogłoszeniem na czacie, potem 10 minut obserwacji. Nowy silnik odpada, gdy wejdzie mniej botów, liczba modów jest inna, `hot-core` zgłosi błąd albo p95 MSPT jest gorsze o ponad 20 % i 5 ms (`WORSE_RATIO`, `WORSE_MS`). Awaria albo spowolnienie po podmianie oznacza powrót na stary silnik i odrzucenie rewizji.
- Zgoda: bez `--trust-native-code` `hotd horizon` tylko pokazuje nową wersję w `hot status`. Samodzielne budowanie wymaga osobnej zgody: `--auto-update-horizon` w instalatorze albo `HOT_AUTO_UPDATE_HORIZON=1` w kontenerze (ADR 0013).
- Instalator i obraz Dockera budują `horizon-server` i `horizon-engine` ze źródeł w przypiętej rewizji (`HORIZON_REV`), nigdy z datowanego wydania.
- Kroki, stan i ograniczenia: `docs/aktualizacje.md`.

## Konsekwencje

- Klient gracza musi mieć tę samą wersję co serwer. `hot-play` aktualizuje Horizona przy każdym starcie, więc okno zgodności to czas do najbliższego startu gry.
- Pusty serwer Horizona stoi w pauzie i nie liczy ticków, więc bez graczy obserwacja łapie tylko awarię, nie spowolnienie.
- Nie sprawdzone (brak maszyny i zgody właściciela): pełny przebieg na serwerze (M9.1).
