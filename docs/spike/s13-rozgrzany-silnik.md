# S13 — rozgrzany silnik: podmiana krótsza niż 1 s

- Data: 2026-11-13. Horizon `nightly-2026-11-05-66f3981`, macOS arm64 (Apple M4 Pro, 24 GB), klient i serwery na jednej maszynie, świat z S12 (seed 7), jeden gracz, pula slotów 1024/2048.
- Krok planu: M0.19. Decyzja: `docs/adr/0004-pauza-podmiany-ponizej-1-s.md`.
- Wynik: **kryterium spełnione na Macu.** Każda z 18 podmian trwała krócej niż 1 s, także 6 podmian na nowy plik silnika z pustym cache: 509–531 ms. Wcześniej było to 1,16–1,34 s na tym samym silniku i ok. 2,2 s na nowym. M0.20: z pulą 4096/8192 480–573 ms (12/12).

## Pomysł

Nowy silnik startuje, **zanim** stary dostanie `stop`. Wszystko, co nie wymaga świata (zasoby gry, cache treści modów, paczka, port, dane generatora), robi przed zamrożeniem. Tuż przed zajęciem świata czeka, aż stary silnik zwolni `session.lock`. W zamrożeniu zostają tylko trzy rzeczy: zapis i zatrzymanie starego silnika, wczytanie świata przez nowy i logowanie proxy.

Czekanie robi mixin `wait_for_world` w `hot-host` (`crates/hot-host/wait_for_world.rs`):
- hak HEAD w `horizon_client::game::world_loading::WorldRequest::prepare_world`;
- działa tylko w serwerze dedykowanym (`options.dedicated`), klienta gry nie dotyczy;
- gdy świat jest zajęty (`SessionLock::is_locked`), sprawdza blokadę co 2 ms;
- po 120 s albo po pierwszym SIGTERM (`horizon_threads::stop_requests`, dodane w M0.20) przestaje czekać i silnik idzie dalej zwykłą drogą, czyli dostaje błąd zajętego świata albo się zatrzymuje.

FAKT z kodu Horizona (`crates/horizon-client/src/save.rs`, `is_locked`): blokada to `File::try_lock`, czyli blokada systemu na otwartym pliku. Znika razem z procesem, także po `kill -9`, więc po awarii starego silnika nowy nie czeka 120 s (D13).

Stary silnik serwera zwalnia blokadę dopiero wtedy, gdy kończy się jego proces, czyli po `stop_dedicated` (`game/dedicated.rs`: zapis graczy i świata), więc nowy czyta kompletny świat.

## Co sprawdzono

Proxy z S8 (`spikes/s8-podmiana/proxy`) dostało dwie flagi:
- `--warm` uruchamia `horizon-server start` celu w tle i czeka w jego `logs/latest.log` na „horizon-hot: waiting for the world”; dopiero wtedy zamraża świat i wysyła `stop` staremu;
- `--flush` tuż przed zamrożeniem wysyła staremu `save-all flush`.

Pauza jak w S7 i S8: od początku zamrożenia do `accept_teleportation` od klienta.

1. Sam serwer, bez gracza: `stop` A → „Done” na B.
2. Gracz, ten sam plik silnika: 6 podmian z `--warm`, potem 6 z `--warm --flush` (`spikes/s7-rozmiar-puli/swaps.sh`).
3. Gracz, nowy plik silnika z pustym cache (najgorszy przypadek): 6 podmian z `--warm --flush` (`spikes/s13-rozgrzany-silnik/cold-swaps.sh`). Przed każdą podmianą skrypt kopiuje silnik do nowego katalogu, ustawia na niego `engine.path` celu i kasuje `.horizon-cache` celu. W cache są dwie rzeczy:
   - cache treści modów (`mod-content`), którego klucz zawiera plik programu (S7);
   - migawki tabel (`behaviours-v2.bin`, `registry-v1.bin`), których klucz to hash kodu wkompilowany w silnik.

   Kopia tego samego pliku unieważnia tylko pierwszą, więc skasowanie obu odpowiada nowemu silnikowi, który zmienia kod świata.
4. Świat po podmianie: na starym silniku `setblock 208 70 530 minecraft:diamond_block`, zaraz potem podmiana, na nowym `execute if block 208 70 530 minecraft:diamond_block`.
5. Pamięć czekającego silnika: RSS procesu silnika w trakcie czekania.

## Wynik

| Przypadek | Podmian | Pauza | Ostatni pakiet starego → pierwszy nowego |
|---|---|---|---|
| sam serwer, `stop` A → „Done” na B | 1 | 0,49 s | — |
| gracz, ten sam silnik, `--warm` | 6 | 501–703 ms | 516–735 ms |
| gracz, ten sam silnik, `--warm --flush` | 6 | 470–541 ms | 508–609 ms |
| gracz, nowy plik silnika, pusty cache, `--warm --flush` | 6 | **509–531 ms** | 510–766 ms |

- Rozgrzanie trwało ok. 0,53 s z ciepłym cache i 2,8–3,0 s z pustym. To czas przed zamrożeniem, gracz go nie odczuwa.
- Klient przez wszystkie podmiany na ekranie `playing` z 329 chunkami, bez rozłączenia.
- Test bloku: na nowym silniku `Test passed`, a klient widzi `minecraft:diamond_block` (`horizon-ctl block 208 70 530`). Bez gracza ten sam test z `gold_block` też przeszedł.
- Czekający silnik zajmuje 363 MB RSS.
- Gdy nowy silnik czeka 120 s, idzie dalej i dostaje zwykły błąd zajętego świata. Supervisor go wznawia i silnik znowu czeka.

Na co idzie pauza (log proxy, podmiana 1 z `--warm`): `stop` starego silnika 405–617 ms, w tym stałe `sleep(100 ms)` w `stop_dedicated` po wyrzuceniu graczy (`game/dedicated.rs`). Od zwolnienia blokady do „Done” na nowym ok. 0,1 s (`world.make_streaming` 81,5 ms w trace startu). Logowanie proxy, konfiguracja i połknięcie `Login` zajmują < 30 ms. `--flush` skraca zapis przy `stop` tylko trochę, bo na małym świecie jest mało do zapisania.

## Co poszło źle po drodze

- Pierwsza wersja `--warm` czekała na koniec `horizon-server start`. Ta komenda wraca dopiero w fazie `running`, a silnik czeka na świat, więc wszystko stanęło. Proxy uruchamia teraz `start` w tle.
- Dwie podmiany na nowy plik silnika padły przez błąd w skrypcie pomiaru: skopiował silnik na plik, z którego właśnie działał drugi serwer. Na macOS następne uruchomienie takiego pliku kończy się SIGKILL (kod 137). Sprawdzone osobno na kopii `sleep` podpisanej ad-hoc jak silnik: nadpisana w trakcie działania daje 137, świeża kopia działa. Na Linuksie `cp` na działający plik kończy się błędem „Text file busy”.
  - Przydatny skutek uboczny: nieudany start nowego silnika zdarzył się przed zamrożeniem, więc gracz grał dalej na starym silniku.
  - Przy okazji wyszło, że silnik szuka plików gry obok siebie (`.local/horizon-client/26.3`), gdy `engine.resources` jest pusty. Skrypt ustawia więc `engine.resources` jawnie.

## Wnioski

1. **Rozgrzany silnik spełnia budżet D11 na Macu z zapasem ok. 0,3 s.** Najgorsza pauza to 703 ms bez `--flush` i 541 ms z nim. `save-all flush` przed zamrożeniem zostaje domyślne.
2. **Osobne uruchomienie rozgrzewające nie jest potrzebne.** Nowy silnik startuje normalnie, jeszcze przed `stop` starego, i sam buduje cache w swoim katalogu. Krok 1 rozgrzanego silnika z D11 i ADR 0004 odpada.
3. **Wniosek 3 z S12 („`hotd` nie może startować nowego supervisora, zanim stary się zatrzyma”) nie dotyczy silnika z `hot-host`.** Taki silnik czeka na świat w miejscu i nie wpada w pętlę restartów. Wniosek zostaje ważny w dwóch przypadkach:
   - podmiana przerwana przed zamrożeniem: `hotd` sam zatrzymuje czekający silnik (`stop` i SIGTERM, niżej „Pula 4096/8192 i zatrzymanie czekającego silnika”), inaczej po 120 s trafi on na błąd i pętlę restartów z S12;
   - silnik bez `hot-host`, np. przy powrocie do czystego Horizona.
4. **Nieudany start nowego silnika wykrywa się po kodzie wyjścia `horizon-server start`, a nie po czasie.** `start` wrócił z kodem 1 po 0,5–1 s, a proxy w próbie czekało na log 60 s. Świat w tym czasie nie stoi, ale `hotd` ma od razu zgłosić błąd i zostawić graczy na starym silniku.
5. **Każdy silnik pod nową ścieżką, nigdy nadpisany w miejscu.** Instalator już tak robi (`engines/<sha>-<wersja>`); to reguła także dla `hotd` w M8.
6. **Między startem nowego silnika a zamrożeniem świat się zmienia.** Nowy silnik czyta datapacki świata, zanim zajmie blokadę. `hotd` wstrzymuje więc zmiany datapacków od startu nowego silnika do końca podmiany.
7. Supervisor czekającego silnika jest w fazie `starting`, więc nie robi automatycznych kopii.
8. Dalsze skracanie, gdyby ThinkCentre nie zmieściło się w 1 s: stałe 100 ms w `stop_dedicated` (mixin w `hot-host`), zapis na wolnym dysku. Czas `stop` zależy od dysku, więc ThinkCentre trzeba zmierzyć (M0.11).

## Czego nie sprawdzono

- ThinkCentre (M0.11): wolniejszy procesor i dysk, czasy będą dłuższe.
- Duży świat i wielu graczy: więcej do zapisania przy `stop`.
- Prawdziwy nowy silnik z innym kodem. Próba używała kopii tego samego pliku z pustym cache.
- Podmiana przerwana po zamrożeniu (D11, krok 5).

## Pula 4096/8192 i zatrzymanie czekającego silnika (M0.20)

Ta sama próba na nowym stanowisku z pulą 4096/8192 u serwerów i klienta (`setup.sh`, `swaps.sh 6`, `cold-swaps.sh … 6`), z `--warm --flush`:

| Przypadek | Podmian | Pauza | Ostatni pakiet starego → pierwszy nowego |
|---|---|---|---|
| gracz, ten sam silnik | 6 | 480–549 ms | 535–621 ms |
| gracz, nowy plik silnika, pusty cache | 6 | 505–573 ms | 547–644 ms |

- Rozgrzanie: 0,8–0,9 s z ciepłym cache, 4,1–4,4 s z pustym (pierwsza podmiana 3,1 s, bo cel nie miał jeszcze cache).
- Czekający silnik: ok. 515 MB RSS (jeden pomiar).
- Wniosek: pauza prawie nie zależy od puli, bo pula wydłuża tylko rozgrzanie. Pula zostaje 4096/8192 (`docs/sloty.md`).

Przy sprzątaniu wyszło, że `horizon-server stop` czekającego silnika trwa 2 minuty. Supervisor pisze `stop` na stdin silnika (`send_stop`), a silnik w mixinie nie czyta konsoli. Pierwszy SIGTERM tylko zwiększa licznik `STOP_REQUESTS`, drugi kończy proces przez `_exit(130)`. Pętla mixina sprawdza teraz `horizon_threads::stop_requests()`. Wynik na silniku z poprawką (B trzyma świat, A czeka):
- samo `stop`: po 10 s nadal czeka, silnik żyje;
- jeden `kill -TERM` silnika A: w logu „stop requested while waiting for the world”, `stop` wraca po 0,5 s, A w stanie `Stopped`, B dalej `Running`.

Drugi przebieg z pidem z `horizon-server --dir a status --json` (`engine_pid`, faza `starting`): `stop` w tle, po 1 s SIGTERM na ten pid, wynik ten sam. `hotd` przerywa więc podmianę w tej kolejności: `horizon-server stop` w tle, potem jeden SIGTERM na `engine_pid` strony. Nie szuka silnika po ścieżce programu, bo przy podmianie na ten sam silnik obie strony działają z jednego pliku. Odwrotnej kolejności nie sprawdzano. Zatrzymanie przez systemd (SIGTERM do całej grupy) działa bez zmian.

## Powtórzenie

```bash
export H=/tmp/hot-spike/s13   # nowy katalog poza repo
# silnik z hot-host: horizon-launcher native-build <paczka z crates/hot-host/build-bundle.sh> --server-output $E --version nightly-2026-11-05-66f3981 --trust-native-code
# pula: spikes/s7-rozmiar-puli/gen.py, potem cargo horizon-mod build
spikes/s13-rozgrzany-silnik/setup.sh $E/horizon-engine <pula>/hot_slots.hmod <zasoby gry 26.3>
# MCP play --instance hot-spike --server 127.0.0.1:25700 --offscreen
spikes/s7-rozmiar-puli/swaps.sh 6                          # ten sam silnik
spikes/s13-rozgrzany-silnik/cold-swaps.sh $E/horizon-engine 6  # nowy plik silnika, pusty cache
```
