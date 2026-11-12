# Standard kodu

Obowiązuje w każdym pliku. Recenzja sprawdza tę listę.

## Komentarze

- Na górze każdego `lib.rs` i `main.rs`: do czego crate służy, kto go używa, z kim rozmawia.
- Komentarz przy funkcji publicznej i w miejscu, którego nie da się wyczytać z kodu: dlaczego, nie co.
- Dwa języki: najpierw `PL:`, potem `EN:`. Angielski pomaga osobom i agentom spoza zespołu. Wzór: `crates/hot-abi/src/lib.rs`.
- Bez zakomentowanego kodu. Stary kod usuwamy, historia gita go pamięta.

## Błędy i logi

Cel: model, który nie zna projektu, naprawia błąd z samego wpisu w logu.

- Logujemy tylko przez `hot-diag`. Bez `println!`/`eprintln!` w kodzie produkcyjnym (clippy ostrzega).
- Każdy błąd widoczny dla operatora albo gracza ma stały kod `HOT-EXXXX` i wpis w `docs/bledy/` (wzór: `docs/bledy/README.md`).
- Wpis o błędzie ma pola: `code`, `component`, `version`, `what`, `expected`, `context`, `cause`, `fix`, `doc`, `source`, `chain`, `state`. Znaczenie pól: `docs/bledy/README.md`.
- `what` i `fix` piszemy pełnymi zdaniami, bez skrótów projektowych. Źle: `gen swap fail`. Dobrze: `Hot mod "sklep" generation 7 failed to load: symbol hot_mod_entry not found in /srv/horizon/hot/mods/sklep/7.so`.
- Zakazane (clippy jako błąd): `unwrap()`, `expect()`, `dbg!` poza testami. Błąd przekazujemy wyżej z kontekstem, nie połykamy go.
- Sekretów nie logujemy. `hot-diag` maskuje znane sekrety, ale nie podawaj ich do logu.
- Treść logów po angielsku (jak logi Horizona). Komunikaty dla gracza po polsku.

## Rozmiar i struktura

- Plik powyżej 600 linii → podział. Funkcja powyżej ~50 linii → rozbicie.
- Nowa funkcja = nowy moduł albo crate, nie doklejka do istniejącego pliku.
- Nazwy po angielsku, opisowe.
- `unsafe` tylko na granicy ABI i w `libloading`, każdy blok z komentarzem `SAFETY:` mówiącym, dlaczego jest poprawny.

## Testy

- Każda zmiana zachowania ma test, który obleje bez niej.
- Testów nie osłabiamy, żeby przeszły. Zły test opisujemy w pull requeście.
- Nowy kod błędu ma test, który go wywołuje i sprawdza wpis w logu.

## Zależności

- Najpierw to, co już jest w workspace, potem biblioteka standardowa, dopiero potem nowa zależność.
- Nowa zależność = uzasadnienie w ADR. Dependabot aktualizuje co tydzień.
