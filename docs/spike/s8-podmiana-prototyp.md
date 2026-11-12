# S8, część 2 — prototyp podmiany silnika przez proxy

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64 (Apple M4 Pro), klient i serwery na jednej maszynie.
- Krok planu: M0.9. Wynik: **potwierdzone**. Gracz zostaje połączony, ekranu ładowania nie ma. Pauza wynosi 0,97–1,26 s, czyli **czasem powyżej budżetu 1 s** (decyzja użytkownika, PLAN.md sekcja 8).
- Część 1 (dlaczego tylko ta droga): `s8-podmiana-analiza-klienta.md`.

## Co zbudowano

`spikes/s8-podmiana/proxy/` (jeden plik, ok. 700 linii, zależność: `horizon-protocol` z gita Horizona, rewizja `66f3981`):

1. Klient loguje się do proxy (offline, bez kompresji). Proxy loguje się do silnika A jako ten sam gracz i przepuszcza konfigurację i grę w obie strony. Zapisuje przy tym pakiety, które klient wysłał w konfiguracji (`client_information`, `custom_payload`, `select_known_packs`, odpowiedzi na paczkę zasobów, ciasteczka).
2. Gdy pojawia się plik-wyzwalacz, proxy:
   - przestaje przekazywać pakiety od A i do A;
   - wywołuje `horizon-server --dir A stop`, a potem uruchamia `horizon-server --dir B start` (nie czeka na zakończenie tej komendy);
   - co 20 ms próbuje zalogować się do B;
   - odtwarza wobec B zapisaną konfigurację klienta;
   - połyka `Login` B, zapamiętuje z niego id gracza na B i wysyła B `PlayerLoaded`;
   - wysyła klientowi `RemoveEntities` ze wszystkimi encjami pokazanymi przez A;
   - od tej chwili przepuszcza ruch B.
3. Id gracza: zamiana „id u klienta ↔ id na B” na surowych bajtach, bez przekodowania reszty pakietu. Klient → serwer: `interact`, `player_command`. Serwer → klient: pakiety, które zaczynają się od id encji (`add_entity`, `set_entity_data`, `update_mob_effect`, `update_attributes`, ruch i obrót encji, `teleport_entity`, `set_equipment`, `damage_event` itd.), a także `entity_event`, `set_entity_link`, `take_item_entity`, `set_passengers` i `remove_entities`.
4. Od B proxy odrzuca `game_event` 13 (start ładowania poziomu) i komunikat „joined the game”.

Silniki: `a` i `b` z S12 (wspólny świat przez dowiązanie, bez modów, offline). Klient: instancja `hot-spike` z wyłączonym na czas próby `hot_slots`, uruchomiony przez MCP `play --server 127.0.0.1:25700 --offscreen`.

## Jak sprawdzono

- Ekran klienta: `horizon-ctl --socket …/game-control.sock status` co ok. 90 ms przez cały czas podmiany (pole `screen`, liczba chunków i mobów), do tego `horizon-ctl events` (zmiany ekranu) i `logs/latest.log` klienta.
- Czy gracz jest na B: ruch klienta (`horizon-ctl move forward 1`) i `data get entity kamciosz Pos` na B dają tę samą pozycję. `give` z konsoli B trafia do ekwipunku klienta.
- Zamiana id: na aktywnym silniku `effect clear`, potem `effect give … jump_boost`. Klient pokazuje na swoim graczu dokładnie ten efekt.
- Próba kontrolna z wyłączoną zamianą (`S8_NO_REMAP=1`): po podmianie `effect clear` z B **nie** dociera do gracza. Efekt zostaje, a czas trwania spada dalej (590 → 579 ticków), zamiast zacząć się od nowa. Test zamiany potrafi więc oblać.

## Wynik

Pauza od początku podmiany do chwili, gdy klient przyjął pozycję od nowego silnika (`accept_teleportation`):

| Runda | Kierunek | `stop` starego | Pauza |
|---|---|---|---|
| 1 | A → B | 0,40 s | 0,98 s |
| 2 | B → A | 0,42 s | 0,97 s |
| 3 | A → B | 0,63 s | 1,22 s |
| 4 | B → A | 0,64 s | 1,21 s |
| 5 | A → B | 0,42 s | 0,99 s |

W pierwszym przebiegu proxy czekało, aż komenda `start` się zakończy. Pauza wynosiła wtedy 1,19–1,87 s: `start` zgłasza gotowość tylko co ok. 0,5 s (zwraca po 504 albo ok. 1010 ms). Logowanie i konfiguracja z B zajmują 1–2 ms, a `Login` przychodzi ok. 10 ms później.

We wszystkich 10 podmianach:

- klient przez cały czas był na ekranie `playing`, a `events` nie zapisało żadnej zmiany ekranu;
- połączenie zostało, w logu klienta nie ma ostrzeżeń ani błędów, a proxy nie zgłosiło nieudanej zamiany id;
- liczba chunków u klienta została ta sama (329). Chunki z B zastępują stare w miejscu, więc „zapomnij chunki” nie jest potrzebne (HIPOTEZA z części 1 potwierdzona);
- moby znikają przy podmianie (137 → 0) i wracają w ciągu ok. 0,8 s. To jest całe „mrugnięcie”: teren stoi, encje gasną i wracają;
- pakiety klienta wysłane w czasie przerwy (20–39 na podmianę) proxy odrzuca. Gracz stał w miejscu, więc nie było czego cofać.

## Wnioski

1. **Wariant A działa** bez zmian w kliencie i w silniku. Wystarczą proxy i dwa supervisory na jednym świecie.
2. Pauza to prawie w całości zatrzymanie starego silnika (0,4–0,64 s, zapis świata) i start nowego (ok. 0,55 s). Proxy dokłada mniej niż 20 ms. Rozgrzanie nowego silnika przed zwolnieniem świata nie jest możliwe (S12, wniosek 5).
3. `hotd` nie może czekać na `horizon-server start`: musi od razu próbować połączenia z portem silnika. Tak samo przy `stop`: komenda może zgłaszać koniec z opóźnieniem. Czy wolny `session.lock` widać wcześniej, nie sprawdzono (NIEWIADOMA, M8).
4. Proxy musi trzymać pełny stan swoich encji (co pokazał silnik). Wystarczyło śledzić `add_entity` i `remove_entities`.
5. `game_event` 13 i „joined the game” od nowego silnika trzeba odrzucić. Komunikat „left the game” od starego nie dociera, bo proxy przestaje przekazywać ruch starego silnika, zanim ten się zatrzyma.

## Czego prototyp nie sprawdził (do M8)

- **Mody i paczka zasobów.** Serwery próby nie miały modów ani paczki, więc odtwarzanie `horizon:mod_list` i odpowiedzi na `resource_pack_push` nie zostało sprawdzone. Na produkcji oba są obowiązkowe (D8, D9).
- **Tryb online.** Serwery i proxy działały offline. Na produkcji proxy sprawdza konto Microsoft (online), a silniki działają offline na `127.0.0.1` albo z Velocity forwarding (D10). Bez tego zasada „każdy ma kupioną grę” przestaje działać.
- **Wielu graczy.** Zamiana dotyczy tylko id własnego gracza. Inni gracze są zwykłymi encjami i wracają z B jak moby. Wpisy listy graczy (`player_info_update`) od B przychodzą ponownie.
- **Id encji ukryte w środku pakietów:** właściciel w `add_entity.data` (pociski, haczyk wędki), odwołania w `set_entity_data`, `player_look_at`, przyczyna w `damage_event`, `entity_tag_query` i `pick_item_from_entity` od klienta. Przeoczenie psuje wyświetlanie jednej encji, a nie połączenie.
- **Tagi.** Konfigurację B proxy połyka. Jeśli nowy silnik ma inne tagi (np. nowy mod natywny), proxy musi wysłać je klientowi w grze jako `update_tags`, tak jak `/reload`.
- **Gracz w ruchu, w otwartym oknie, w pojeździe albo w innym wymiarze niż świat główny.**
- Pomiar na ThinkCentre z prawdziwym światem i graczami (M0.11).

## Powtórzenie

```bash
cd spikes/s8-podmiana/proxy && cargo build --release
SB=".../versions/nightly-2026-11-05-66f3981/horizon-server"
# a, b: katalogi z S12 (init --offline, world -> ../shared/world); start tylko a
target/release/s8-proxy --listen 127.0.0.1:25700 --server-bin "$SB" \
  --a 25613:<s12>/a --b 25566:<s12>/b --swap-file <katalog>/swap 2> proxy.log
# klient: play --server 127.0.0.1:25700; podmiana: touch <katalog>/swap
# wynik: linia "SWAP RESULT" w proxy.log
```
