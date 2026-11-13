# Podmiana silnika bez rozłączania graczy

Jak `hot-proxy` i `hotd` przenoszą graczy na nowy silnik (`horizon-server`), żeby nikt nie zobaczył ekranu ładowania ani nie stracił połączenia. Decyzje: D10–D13 w `PLAN.md`. Dowody: `docs/spike/s8-podmiana-analiza-klienta.md` (kod klienta), `docs/spike/s8-podmiana-prototyp.md` (prototyp i pomiar), `docs/spike/s12-dwa-serwery-jeden-swiat.md` (dwa silniki na jednym świecie), `docs/spike/s13-rozgrzany-silnik.md` (rozgrzany silnik).

## Kiedy

- nowy mod natywny (`.hnmod`) albo nowa wersja `hot-host`;
- aktualizacja Horizona (D18);
- odzyskanie pamięci po wielu generacjach modów gorących (D4);
- powrót po awarii silnika (D13).

## Co widzi gracz

Przez ok. 0,5 s świat stoi (S13, Mac). Gracz może chodzić i niszczyć bloki, ale po przerwie wraca na miejsce sprzed niej, a zniszczone bloki wracają: wszystko, co zrobił w tym czasie, przepada (S8). Potem znikają wszystkie moby, gracze i przedmioty na ziemi, a po ok. 0,8 s wracają. Ekran gry zostaje, połączenie też.

## Kolejność

Rozgrzany silnik (D11, S13): wszystko, co nie wymaga świata, dzieje się przed zamrożeniem.

| Krok | Kto | Co | Czas (Mac, pula 4096/8192) |
|---|---|---|---|
| 1 | `hotd` | wstrzymuje zmiany datapacków; `start` nowego silnika w tle, **bez czekania** na koniec komendy (wraca dopiero w fazie `running`) | — |
| 2 | nowy silnik | zasoby, cache treści modów, port; przed zajęciem świata czeka (mixin `wait_for_world`), w logu „horizon-hot: waiting for the world” | 0,8–0,9 s z ciepłym cache, 4,1–4,4 s z zimnym; **przed zamrożeniem** |
| 3 | `hot-proxy` | przestaje przekazywać ruch w obie strony; pakiety klienta odrzuca. **Początek pauzy** | — |
| 4 | `hotd` | `save-all flush`, potem `stop` starego silnika (zapis świata i graczy, zwolnienie `session.lock`) | 0,4–0,6 s |
| 5 | nowy silnik | zajmuje świat w ciągu 2 ms od zwolnienia, wczytuje go | ok. 0,1 s |
| 6 | `hot-proxy` | co 20 ms próba logowania do nowego silnika jako każdy gracz; odtworzenie konfiguracji klienta, połknięcie `Login`, `PlayerLoaded` do silnika | < 30 ms |
| 7 | `hot-proxy` | `RemoveEntities` do klienta z encjami starego silnika; zamiana id gracza; dalej zwykłe przekazywanie | — |

Razem: 480–549 ms na tym samym silniku i 505–573 ms na nowym pliku silnika z pustym cache (M0.20). Z pulą 1024/2048 było prawie tak samo: 470–541 ms i 509–531 ms. Bez rozgrzania było 1,16–1,34 s i ok. 2,2 s.

Błędy:
- `start` nowej strony z kodem różnym od 0 (wraca po 0,5–1 s) to błąd przed zamrożeniem: gracze zostają na starym silniku, `hotd` zgłasza błąd od razu.
- Przerwana podmiana: `hotd` zatrzymuje czekający silnik przez `horizon-server stop` w tle, a potem jeden SIGTERM do procesu silnika (pid z `engine_pid` w `status --json` tej strony, nie ze ścieżki programu: przy podmianie na ten sam silnik obie strony działają z tego samego pliku). Odwrotnej kolejności nie sprawdzano. Czekający silnik nie czyta konsoli, więc samo `stop` czeka do limitu 120 s; po SIGTERM mixin przestaje czekać i silnik kończy się po ok. 0,5 s (M0.20). Bez zatrzymania silnik po 120 s idzie dalej, dostaje błąd zajętego świata i supervisor wpada w pętlę restartów (S12).
- Każdy silnik pod nową ścieżką: nadpisanie pliku, z którego działa silnik, zabija następny start (macOS, kod 137) albo się nie udaje (Linux, „Text file busy”).

## Zasady dla `hot-proxy`

1. Po pierwszym wejściu gracza proxy **nigdy** nie wysyła klientowi `Login`, `Respawn` ani `StartConfiguration`. Każdy z nich pokazuje ekran ładowania.
2. Proxy zapisuje, co klient wysłał w konfiguracji, i odtwarza to wobec nowego silnika. Konfigurację od nowego silnika połyka.
3. Id gracza u klienta to id z pierwszego `Login`. Każdy pakiet z id encji przechodzi przez zamianę „id u klienta ↔ id w aktywnym silniku”, w obie strony. Lista pakietów: `crates/hot-proxy/src/remap.rs` (`clientbound`, `serverbound`), z testami na prawdziwym kodowaniu pakietów Horizona.
4. Proxy śledzi encje, które pokazało klientowi (`add_entity`, `remove_entities`), i przy podmianie usuwa je wszystkie oprócz gracza.
5. Przez 5 s po wejściu na nowy silnik proxy odrzuca `game_event` 13 (start ładowania poziomu) i komunikaty „joined/left the game”. Samemu graczowi nowy silnik ich nie wysyła (S8), ale przy wielu graczach dostaliby je pozostali.
6. Klient rozłącza się po 30 s bez żadnego pakietu (`READ_TIMEOUT`). W przerwie proxy samo wysyła klientowi keep-alive: pierwszy po 5 s, potem co 10 s. Ich id mają znacznik `0x686f74…` („hot”), a odpowiedzi klienta na nie przepadają (D13). Sprawdzone testem z atrapą silnika, która wstaje po 6 s (`crates/hot-proxy/tests/switch.rs`).
7. Do silnika trafia tylko odpowiedź na keep-alive, który wysłał aktywny silnik. Odpowiedź na keep-alive starego silnika przepada. Horizon i tak nie wyrzuca za złe id, ale silnik Mojanga by wyrzucił.
8. Numer wiadomości czatu (`global_index` w `PlayerChat`) nowy silnik liczy od 0, a klient sprawdza ciągłość i bez przesunięcia rozłączyłby się przy pierwszej wiadomości (`bad_chat_index`). Proxy dodaje do numeru liczbę wiadomości sprzed podmiany.
9. Gdy podmiana trafi w środek paczki pakietów (`bundle_delimiter`), proxy ją zamyka, zanim wyśle `RemoveEntities`.
10. Podpisanego czatu nie ma: silniki działają w trybie offline za proxy, więc klient pokazuje czat jako „Not Secure”, a lista ostatnio widzianych wiadomości nie przechodzi walidacji podpisu. Podmiana tego nie zmienia.
11. Silnik zerwał połączenie bez `Disconnect` (awaria, restart przez supervisor): proxy trzyma gracza do 180 s i co 20 ms próbuje go zalogować pod aktywnym adresem. `Disconnect` od silnika (kick, ban, `stop` bez podmiany) idzie do gracza i kończy sesję. Gracz, którego nie udało się przenieść, dostaje komunikat „The server is restarting. Join again in a minute.”, a w logu jest `HOT-E0605`.

## Sterowanie

### Podmiana silnika: `hotd swap`

Na serwerze silnik prowadzi `hotd engine` (usługa `horizon-hot-server`, w kontenerze proces `engine`). Silnik ma dwie strony:

- `a` — katalog serwera (`/srv/horizon/server`), port silnika (25566);
- `b` — `/srv/horizon/server/engine-b`, port o jeden wyżej (25567). Ma własny `server.toml` (kopia strony `a` przed każdą podmianą na `b`, bez panelu i API), a świat, `backups/`, `horizon-mods/`, `config/`, `resources/`, `forwarding.secret` i listy graczy (`ops.json`, `whitelist.json`, bany, `usercache.json`) to dowiązania do strony `a`.

Aktywną stronę zapisuje `hot-state/engine.json`. Czytają go `hotd engine` po restarcie, `hotd run` (komendy do silnika) i `hot-proxy` przy starcie (zamiast `--engine`).

```sh
sudo -u horizon /srv/horizon/bin/hotd swap --home /srv/horizon/server                      # ten sam plik silnika
sudo -u horizon /srv/horizon/bin/hotd swap --to <nowy plik silnika> --home /srv/horizon/server
# {"ok":true,"report":{"from":"a","to":"b","warm_ms":4210,"stop_ms":512,"freeze_ms":590,"proxy":{...}}}
```

`hotd swap` wysyła komendę do `<HOT_HOME>/hot-engine.sock` i czeka na koniec podmiany (do ok. 2,5 min). Kod wyjścia 1, gdy podmiana się nie udała; błąd ma pola D27:

- `HOT-E0606` — nowa strona nie doszła do linii czekania albo proxy odmówiło; gracze zostali na starym silniku, nic nie stanęło;
- `HOT-E0607` — nowa strona nie zajęła świata w 10 s po zatrzymaniu starej; `hotd` zatrzymuje nową, przestawia proxy (`engine ADRES`) i uruchamia starą, a gracze trzymani w przerwie logują się już na nią.

Nowy plik silnika musi mieć `hot-host` (linię czekania pisze jego mixin) i własną ścieżkę (zob. „Błędy” wyżej). Sygnał „held” od proxy (`{"held":N}`) przychodzi, gdy proxy przestało już przekazywać pakiety starego silnika; dopiero wtedy `hotd` wysyła mu `stop`, bo inaczej `Disconnect` ze `stop` zakończyłby sesje graczy.

Ograniczenie (do sprawdzenia na prawdziwym silniku): jeśli silnik zapisuje listy graczy przez zapis do nowego pliku i zamianę nazwy, na stronie `b` zastąpi dowiązanie zwykłym plikiem i zmiana (np. `/op`) zostanie tylko na tej stronie.

### Proxy: `hot-proxy switch`

`hot-proxy` słucha komend na gnieździe Unix `<HOT_HOME>/hot-proxy.sock` (tylko właściciel, 0600). Jedna komenda na połączenie; odpowiedź to linie JSON, ostatnia jest wynikiem (`hot-proxy switch` wypisuje tylko ją):

```sh
hot-proxy status --home /srv/horizon            # {"engine":"127.0.0.1:25566","players":["Dev"]}
hot-proxy switch --home /srv/horizon            # przeniesienie na ten sam silnik
hot-proxy switch 127.0.0.1:25567 --home /srv/horizon
# {"engine":"127.0.0.1:25567","players":1,"moved":1,"failed":[],"pause_ms":512}
```

`switch` najpierw u wszystkich graczy w grze zamyka stary silnik (krok 3) i wysyła linię `{"held":N}`, potem każdego loguje do nowego równolegle, więc pauza nie rośnie z liczbą graczy. Każda próba logowania idzie pod bieżący adres, więc `engine ADRES` w trakcie przerwy kieruje trzymanych graczy gdzie indziej. Na nowy silnik czeka do 25 s. Kod wyjścia 1, gdy choć jeden gracz nie przeszedł. Druga podmiana czeka, aż skończy się pierwsza.

## Jak sprawdzić, że podmiana zadziałała

- `hot-proxy status`: aktywny silnik i gracze w grze.
- W logu proxy (`<HOT_HOME>/logs/hot-proxy.jsonl`) każdy przeniesiony gracz ma linię „A player moved to another engine” z pauzą, liczbą usuniętych encji i liczbą odrzuconych pakietów, a cała podmiana linię „Engine switch finished” z podsumowaniem jak w odpowiedzi `switch`.
- Po stronie gracza ekran `playing` przez cały czas (`horizon-ctl status`, `horizon-ctl events`). To pole nie widzi nakładki ładowania (S6), więc w testach podmiany potrzebne są też zrzuty klatki (`horizon-ctl screenshot`) w czasie podmiany.
