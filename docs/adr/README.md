# Decyzje architektoniczne (ADR)

Każda decyzja, która kształtuje projekt, ma tu plik `NNNN-temat.md` z częściami Kontekst, Decyzja i Konsekwencje oraz polem **Status**. Decyzji się nie przepisuje: zmiana to nowy ADR, a stary dostaje status „zastąpiony przez ADR NNNN”. Test `crates/hot-diag/tests/adr.rs` pilnuje tych części, tego indeksu i odwołań `D<n>` w kodzie i dokumentacji.

Nowy ADR: następny wolny numer, nagłówek jak w ADR 0032, wpis w tabeli „Wszystkie ADR” niżej i, jeśli zmienia decyzję `D<n>`, poprawka w tabeli „Decyzje D1–D29”. Każda nowa zależność w `Cargo.toml` wymaga ADR (jak ADR 0003, 0005, 0009, 0010).

## Decyzje D1–D29

Numery `D<n>` pochodzą z planu budowy (`PLAN.md`) i stoją w komentarzach w kodzie. Tu jest, gdzie każda decyzja jest opisana dziś.

| Decyzja | Temat | ADR |
|---|---|---|
| D1 | gospodarz w silniku, mody gorące ładowane w locie | [0017](0017-warstwa-goraca.md) |
| D2 | `hot-host` jako cienki loader, logika w `hot-core` | [0017](0017-warstwa-goraca.md) |
| D3 | granica to stabilne C ABI (`hot-abi`) | [0017](0017-warstwa-goraca.md), [0006](0006-ksztalt-abi-v1.md) |
| D4 | cykl życia moda, generacje | [0018](0018-cykl-zycia-moda.md) |
| D5 | haki v1 | [0019](0019-haki-i-funkcje-gospodarza.md) |
| D6 | funkcje gospodarza v1 | [0019](0019-haki-i-funkcje-gospodarza.md) |
| D6a | komendy gracza | [0019](0019-haki-i-funkcje-gospodarza.md) |
| D7 | dane modów jako paczka danych świata | [0020](0020-dane-modow.md) |
| D8 | pula slotów `hot_slots.hmod` | [0021](0021-sloty-i-wyglad.md) |
| D9 | wygląd z paczki zasobów serwera | [0021](0021-sloty-i-wyglad.md) |
| D10 | `hot-proxy` przed silnikiem | [0022](0022-proxy-i-podmiana-silnika.md), [0010](0010-hot-proxy-zaleznosci.md) |
| D11 | podmiana silnika bez rozłączania graczy | [0022](0022-proxy-i-podmiana-silnika.md), [0004](0004-pauza-podmiany-ponizej-1-s.md) |
| D12 | dwa katalogi silnika | [0022](0022-proxy-i-podmiana-silnika.md) |
| D13 | ochrona przed pętlą awarii | [0023](0023-samonaprawa.md) |
| D14 | mody budowane w CI | [0024](0024-budowa-modow-i-silnika.md), [0008](0008-hotd-git-hot-builds-podpis-ssh.md) |
| D15 | silnik budowany na serwerze | [0024](0024-budowa-modow-i-silnika.md), [0012](0012-rewizja-horizona-z-przypiecia.md) |
| D16 | gra ma pierwszeństwo | [0025](0025-gra-ma-pierwszenstwo.md) |
| D17 | aktualizacja modów | [0026](0026-aktualizacja-modow.md) |
| D18 | aktualizacja Horizona | [0027](0027-aktualizacja-horizona.md), [0013](0013-osobna-zgoda-na-aktualizacje-horizona.md) |
| D19 | aktualizacja horizon-hot | [0028](0028-aktualizacja-horizon-hot.md), [0014](0014-samoaktualizacja-horizon-hot.md), [0033](0033-podmiana-proxy-przy-graczach.md) |
| D20 | samonaprawa | [0023](0023-samonaprawa.md) |
| D21 | stan i komunikaty | [0029](0029-stan-i-komunikaty.md) |
| D22 | repo serwera | [0030](0030-repo-serwera-i-workspace.md) |
| D23 | repo horizon-hot | [0030](0030-repo-serwera-i-workspace.md), [0001](0001-struktura-projektu.md) |
| D24 | klasyfikacja zmian | [0026](0026-aktualizacja-modow.md) |
| D25 | instalator serwera | [0031](0031-instalator-serwera.md), [0011](0011-kontener-docker.md) |
| D26 | aplikacja gracza `hot-play` | [0015](0015-aplikacja-gracza-hot-play.md), [0016](0016-wydanie-i-samoaktualizacja-hot-play.md) |
| D27 | logi i katalog błędów | [0032](0032-diagnostyka-i-przekazanie.md), [0003](0003-serde-json-w-hot-diag.md) |
| D28 | `hot report` | [0032](0032-diagnostyka-i-przekazanie.md) |
| D29 | dokumentacja bez planu | [0032](0032-diagnostyka-i-przekazanie.md) |

## Wszystkie ADR

| ADR | Temat |
|---|---|
| [0001](0001-struktura-projektu.md) | struktura projektu: workspace Rust z osobnymi crate na składnik |
| [0002](0002-spike.md) | wyniki prób S1–S12 i poprawki decyzji D1–D29 |
| [0003](0003-serde-json-w-hot-diag.md) | `serde_json` w `hot-diag` |
| [0004](0004-pauza-podmiany-ponizej-1-s.md) | pauza podmiany silnika krótsza niż 1 s |
| [0005](0005-libloading-w-hot-core-i-hot-host.md) | `libloading` w `hot-core` i `hot-host` |
| [0006](0006-ksztalt-abi-v1.md) | kształt ABI v1 (`hot-abi`) |
| [0007](0007-hot-host-logi-komendy-pauza.md) | `hot-host`: logi, komendy i pauza pustego serwera |
| [0008](0008-hotd-git-hot-builds-podpis-ssh.md) | `hotd`: wyniki CI na gałęzi `hot-builds`, podpis `ssh-keygen` |
| [0009](0009-libc-w-hotd.md) | `libc` w `hotd` |
| [0010](0010-hot-proxy-zaleznosci.md) | zależności `hot-proxy` |
| [0011](0011-kontener-docker.md) | serwer w kontenerze Docker |
| [0012](0012-rewizja-horizona-z-przypiecia.md) | rewizję Horizona w silniku wybiera przypięcie paczki `hot-host` |
| [0013](0013-osobna-zgoda-na-aktualizacje-horizona.md) | samodzielne aktualizacje Horizona wymagają osobnej zgody |
| [0014](0014-samoaktualizacja-horizon-hot.md) | serwer sam aktualizuje horizon-hot |
| [0015](0015-aplikacja-gracza-hot-play.md) | aplikacja gracza `hot-play` |
| [0016](0016-wydanie-i-samoaktualizacja-hot-play.md) | wydanie, instalacja i samoaktualizacja `hot-play` |
| [0017](0017-warstwa-goraca.md) | warstwa gorąca: gospodarz w silniku, `hot-core` i C ABI |
| [0018](0018-cykl-zycia-moda.md) | cykl życia moda gorącego i generacje |
| [0019](0019-haki-i-funkcje-gospodarza.md) | haki i funkcje gospodarza v1, komendy gracza |
| [0020](0020-dane-modow.md) | dane modów jako paczka danych świata |
| [0021](0021-sloty-i-wyglad.md) | sloty na nowe bloki i wygląd z paczki zasobów serwera |
| [0022](0022-proxy-i-podmiana-silnika.md) | `hot-proxy` i podmiana silnika bez rozłączania graczy |
| [0023](0023-samonaprawa.md) | ochrona przed pętlą awarii i samonaprawa |
| [0024](0024-budowa-modow-i-silnika.md) | mody gorące budowane w CI, silnik na serwerze |
| [0025](0025-gra-ma-pierwszenstwo.md) | gra ma pierwszeństwo, tło pracuje z resztek |
| [0026](0026-aktualizacja-modow.md) | aktualizacja modów i klasyfikacja zmian |
| [0027](0027-aktualizacja-horizona.md) | aktualizacja Horizona z kanału Nightly |
| [0028](0028-aktualizacja-horizon-hot.md) | aktualizacja samego horizon-hot |
| [0029](0029-stan-i-komunikaty.md) | stan i komunikaty bez zewnętrznych powiadomień |
| [0030](0030-repo-serwera-i-workspace.md) | układ repo serwera i repo horizon-hot |
| [0031](0031-instalator-serwera.md) | instalacja serwera jednym poleceniem |
| [0032](0032-diagnostyka-i-przekazanie.md) | logi, raport i dokumentacja dla osoby spoza projektu |
| [0033](0033-podmiana-proxy-przy-graczach.md) | nowy program `hot-proxy` przy graczach (`execve`, ten sam PID) |

## Inne oznaczenia w kodzie i dokumentacji

Wszystkie pochodzą z `PLAN.md`, który zostaje w repo jako historia budowy.

| Oznaczenie | Znaczenie | Gdzie szukać |
|---|---|---|
| `M<n>`, `M<n>.<k>` | etap i krok planu budowy (np. M8.3: podmiana silnika przez `hotd engine`) | `PLAN.md`, sekcja etapów; wyniki w dzienniku na końcu |
| `S<n>` | próba (spike) przed budową | ADR 0002, `docs/spike/` |
| `R<n>` | ustalenie z researchu przed budową (fakt ze źródłem) | `PLAN.md`, sekcja 3 |
| `W<n>` | wymaganie właściciela | `PLAN.md`, sekcja 1 |
| `HOT-E<nnnn>` | kod błędu | `docs/bledy/` |
