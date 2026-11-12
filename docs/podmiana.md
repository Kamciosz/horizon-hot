# Podmiana silnika bez rozłączania graczy

Jak `hot-proxy` i `hotd` przenoszą graczy na nowy silnik (`horizon-server`), żeby nikt nie zobaczył ekranu ładowania ani nie stracił połączenia. Decyzje: D10–D13 w `PLAN.md`. Dowody: `docs/spike/s8-podmiana-analiza-klienta.md` (kod klienta), `docs/spike/s8-podmiana-prototyp.md` (prototyp i pomiar), `docs/spike/s12-dwa-serwery-jeden-swiat.md` (dwa silniki na jednym świecie).

## Kiedy

- nowy mod natywny (`.hnmod`) albo nowa wersja `hot-host`;
- aktualizacja Horizona (D18);
- odzyskanie pamięci po wielu generacjach modów gorących (D4);
- powrót po awarii silnika (D13).

## Co widzi gracz

Przez ok. 1 s świat stoi. Gracz może chodzić i niszczyć bloki, ale po przerwie wraca na miejsce sprzed niej, a zniszczone bloki wracają: wszystko, co zrobił w tym czasie, przepada (S8). Potem znikają wszystkie moby, gracze i przedmioty na ziemi, a po ok. 0,8 s wracają. Ekran gry zostaje, połączenie też.

## Kolejność

| Krok | Kto | Co | Czas (Mac, pusty świat) |
|---|---|---|---|
| 1 | `hot-proxy` | przestaje przekazywać ruch w obie strony; pakiety klienta odrzuca | — |
| 2 | `hotd` | `stop` starego silnika (zapis świata i graczy, zwolnienie `session.lock`); czeka na **zniknięcie procesu silnika**, nie na koniec komendy | ok. 0,4 s |
| 3 | `hotd` | `start` nowego silnika na tym samym świecie; **bez czekania** na zakończenie komendy | — |
| 4 | `hot-proxy` | co 20 ms próba logowania do nowego silnika jako każdy gracz | ok. 0,55 s od kroku 3 |
| 5 | `hot-proxy` | odtworzenie konfiguracji klienta, połknięcie `Login`, `PlayerLoaded` do silnika | < 20 ms |
| 6 | `hot-proxy` | `RemoveEntities` do klienta z encjami starego silnika; zamiana id gracza; dalej zwykłe przekazywanie | — |

Razem: 0,96–1,07 s w prototypie bez modów. Z pulą slotów start nowego silnika trwa dłużej: 1,16–1,82 s, a z nowym plikiem silnika (zimny cache treści modów) ok. 3,9 s przy pełnej puli (`docs/sloty.md`). Nowy silnik musi startować dopiero po zatrzymaniu starego. Inaczej nie otworzy świata i wpadnie w pętlę restartów (S12).

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
