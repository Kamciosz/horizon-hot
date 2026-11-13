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
- Przerwana podmiana: `hotd` zatrzymuje czekający silnik przez `horizon-server stop` i jeden SIGTERM do procesu silnika. Czekający silnik nie czyta konsoli, więc samo `stop` czeka do limitu 120 s; po SIGTERM mixin przestaje czekać i silnik kończy się po ok. 0,5 s (M0.20). Bez zatrzymania silnik po 120 s idzie dalej, dostaje błąd zajętego świata i supervisor wpada w pętlę restartów (S12).
- Każdy silnik pod nową ścieżką: nadpisanie pliku, z którego działa silnik, zabija następny start (macOS, kod 137) albo się nie udaje (Linux, „Text file busy”).

## Zasady dla `hot-proxy`

1. Po pierwszym wejściu gracza proxy **nigdy** nie wysyła klientowi `Login`, `Respawn` ani `StartConfiguration`. Każdy z nich pokazuje ekran ładowania.
2. Proxy zapisuje, co klient wysłał w konfiguracji, i odtwarza to wobec nowego silnika. Konfigurację od nowego silnika połyka.
3. Id gracza u klienta to id z pierwszego `Login`. Każdy pakiet z id encji przechodzi przez zamianę „id u klienta ↔ id w aktywnym silniku”, w obie strony. Lista pakietów: `spikes/s8-podmiana/proxy/src/main.rs` (`remap_clientbound`, `remap_serverbound`), znane braki: `s8-podmiana-prototyp.md`.
4. Proxy śledzi encje, które pokazało klientowi (`add_entity`, `remove_entities`), i przy podmianie usuwa je wszystkie oprócz gracza.
5. Od nowego silnika proxy odrzuca `game_event` 13 (start ładowania poziomu). Komunikatu „joined the game” nowy silnik samemu graczowi nie wysyła (S8), ale proxy też go odrzuca: przy wielu graczach dostaną go pozostali.
6. Klient rozłącza się po 30 s bez żadnego pakietu (`READ_TIMEOUT`). W każdej przerwie dłuższej niż kilka sekund proxy samo wysyła klientowi keep-alive co ok. 10 s i nie przekazuje odpowiedzi silnikowi (D13). To jest projekt, jeszcze niesprawdzony.

## Jak sprawdzić, że podmiana zadziałała

- `hot status`: aktywny silnik, czas ostatniej podmiany, pauza.
- W logu proxy na każdą podmianę jest jedna linia z pauzą, liczbą usuniętych encji i liczbą odrzuconych pakietów. Błąd zamiany id ma osobną linię: `could not remap <pakiet>`.
- Po stronie gracza ekran `playing` przez cały czas (`horizon-ctl status`, `horizon-ctl events`). To pole nie widzi nakładki ładowania (S6), więc w testach podmiany potrzebne są też zrzuty klatki (`horizon-ctl screenshot`) w czasie podmiany.
