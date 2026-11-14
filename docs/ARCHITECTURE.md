# Architektura

Gdzie co leży i dlaczego. Stan: wszystkie składniki z tabeli istnieją i mają testy. Czego nie sprawdzono na docelowym serwerze (Ubuntu x86_64 z prawdziwym silnikiem), wymienia `PLAN.md` przy krokach z dopiskiem „nie sprawdzone: brak maszyny”.

## Składniki

| Składnik | Gdzie działa | Odpowiada za | Rozmawia z |
|---|---|---|---|
| `hot-abi` | biblioteka | stabilna granica C ABI między gospodarzem a modami | — |
| `hot-diag` | biblioteka + CLI | logi JSONL z kodami błędów, panika w logu, `hot report` (szkielet: `hot-diag report`) | wszyscy oprócz `hot-host` |
| `hot-sdk` | biblioteka dla autorów modów | makro `hot_mod!`, bezpieczne opakowania ABI, `catch_unwind` | `hot-abi` |
| `hot-host` | w silniku (mod natywny wkompilowany przez `native-build`) | ładuje `hot-core` przy starcie serwera, przekazuje mu haki Horizona, podmienia rdzeń (`hot core load`) | silnik, `hot-core` |
| `hot-core` | w silniku (`cdylib`) | ładuje generacje modów gorących (`hot load`), stan między generacjami, powrót po panice, `hot-state/status.json` (z pamięcią starych generacji); nazwy bloków modów ze `slots.json` | `hot-host`, mody, `hotd` (przez pliki i konsolę) |
| `hotd` | usługi systemd `horizon-hot-hotd` (`hotd run`) i `horizon-hot-server` (`hotd engine`), tło | repo serwera, podpisane mody z gałęzi `hot-builds` (ADR 0008), `hot-state/hotd.json`, przydział slotów i paczka zasobów, podmiana silnika (`hotd engine`, `hotd swap`, ADR 0022), aktualizacje Horizona i horizon-hot (`hotd horizon`, `hotd upgrade`, ADR 0027, 0028), hamulec MSPT (ADR 0025), `hotd status` i `hotd report` (ADR 0032) | GitHub (git), `horizon-server cmd`, `hot-core` (przez `status.json`), `hot-proxy` |
| `hot-proxy` | usługa systemd `horizon-hot-proxy`, port 25565 (silnik za nim na 127.0.0.1:25566) | kontrola zakupu gry przy logowaniu, trzymanie połączeń przy podmianie i awarii silnika (`docs/podmiana.md`) | gracze, silniki, `hotd` |
| `hotd serve-pack` | usługa systemd `horizon-hot-pack`, port 8131 | paczka zasobów z `hot-state/pack/pack.zip` (buduje ją `hotd`) i pula `hot_slots.hmod` | gracze |
| kontener (`Dockerfile`, `scripts/container-entrypoint.sh`) | Docker, zamiast systemd (ADR 0011) | te same procesy co usługi wyżej, w jednym kontenerze; przygotowanie danych jak w instalatorze, start, wznawianie po awarii, zatrzymanie z zapisem świata | `horizon-server`, `hot-proxy`, `hotd` |
| `hot-play` | komputer gracza (macOS, Linux, Windows) | instalacja, aktualizacja i naprawa gry, plik bloków serwera, start na serwerze, raport „Napraw” (ADR 0015, 0016) | `horizon-launcher`, serwer (`/hot-play.json`, `hot_slots.hmod` na porcie 8131) |

## Najważniejsze ścieżki

**Zmiana kodu moda gorącego.**
```
push do repo serwera → CI buduje .so, podpisuje, wypycha na hot-builds → hotd pobiera gitem, sprawdza podpis
→ hot-core ładuje nową generację poza wątkiem gry → przełączenie na granicy ticku
```
Gracz niczego nie zauważa. Panika w modzie → powrót do poprzedniej generacji.

**Zmiana danych (przepisy, łupy, tagi).**
```
push → hotd kopiuje data/ do paczki danych świata → /reload
```

**Mod natywny albo nowa wersja Horizona.**
```
nowy silnik (kod natywny tylko za zgodą właściciela, ADR 0027) startuje w tle → stary zapisuje graczy i świat → nowy zajmuje świat
→ hot-proxy przenosi graczy bez rozłączenia (świat stoi krócej niż 1 s, bez ekranu ładowania)
```
Błąd nowego silnika → gracze wracają na stary.

**Awaria silnika.**
```
silnik pada → hot-proxy trzyma połączenia → silnik wstaje z ostatnimi dobrymi modami → gracze wracają
```

## Zasady, które trzymają całość

- **Bez restartu serwera** przy zmianach modów i aktualizacjach.
- **Gra ma pierwszeństwo.** Tło (`hotd`, budowa, kopie) działa w osobnym slice systemd z najniższym priorytetem i zatrzymuje się, gdy MSPT rośnie.
- **Nowa treść (bloki, przedmioty) przez sloty** z `hot_slots.hmod`, bo rejestry Horizona są zamrożone po starcie.
- **Silnik budujemy tylko na serwerze.** Wymaga zasobów gry, których nie wolno pobierać w CI. Mody gorące i aplikację gracza buduje CI.
- **Każdy błąd ma kod i opis naprawy** (`docs/bledy/`).

## Decyzje

Pełne uzasadnienia: `docs/adr/`. Kompromisy świadomie przyjęte:

- Podmiana silnika nie jest niewidoczna: świat stoi. Cel: krócej niż 1 s (ADR 0004). Z rozgrzanym silnikiem zmierzono na Macu 480–573 ms, bez rozgrzania 1,2–2,2 s (`docs/podmiana.md`); na serwerze docelowym jeszcze nie mierzono. Podmiana bez żadnego śladu wymagałaby przeniesienia całego stanu gry między procesami — za drogie i kruche przy częstych wydaniach Horizona.
- Stare generacje modów zostają w pamięci (bez wyładowania biblioteki). Pamięć odzyskuje okresowa podmiana silnika.

## Rozwój

- Nowa funkcja = nowy moduł w odpowiednim crate albo nowy crate w `crates/`, dopisany do tabeli wyżej.
- Kruche miejsca: `hot-abi` (zmiana = nowe `ABI_MAJOR` i przebudowa wszystkich modów), `hot-host` (zależy od API Horizona; używamy tylko nazwanych haków).
