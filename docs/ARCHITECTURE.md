# Architektura

Gdzie co leży i dlaczego. Stan: zbudowane są `hot-abi`, `hot-diag`, `hot-sdk`, `hot-core`, `hot-host` i przykłady `examples/licznik`, `examples/sklep`, `examples/teleport-blok`; reszta opisuje docelowy kształt i powstaje krok po kroku. Składnik jeszcze nieistniejący ma dopisek „planowany”.

## Składniki

| Składnik | Gdzie działa | Odpowiada za | Rozmawia z |
|---|---|---|---|
| `hot-abi` | biblioteka | stabilna granica C ABI między gospodarzem a modami | — |
| `hot-diag` | biblioteka + CLI | logi JSONL z kodami błędów, panika w logu, `hot report` (szkielet: `hot-diag report`) | wszyscy oprócz `hot-host` |
| `hot-sdk` | biblioteka dla autorów modów | makro `hot_mod!`, bezpieczne opakowania ABI, `catch_unwind` | `hot-abi` |
| `hot-host` | w silniku (mod natywny wkompilowany przez `native-build`) | ładuje `hot-core` przy starcie serwera, przekazuje mu haki Horizona, podmienia rdzeń (`hot core load`) | silnik, `hot-core` |
| `hot-core` | w silniku (`cdylib`) | ładuje generacje modów gorących (`hot load`), stan między generacjami, powrót po panice, `hot-state/status.json`; sloty w M4 | `hot-host`, mody, `hotd` (przez pliki i konsolę) |
| `hotd` | usługa systemd, tło | repo serwera, podpisane mody z gałęzi `hot-builds` (ADR 0008), `hot-state/hotd.json`; w planie: aktualizacje, kopie, hamulec MSPT | GitHub (git), `horizon-server cmd`, `hot-core` (przez `status.json`) |
| `hot-proxy` (planowany) | usługa systemd, port 25565 | logowanie graczy, trzymanie połączeń przy podmianie i awarii silnika | gracze, silniki |
| `hot-pack` (planowany) | usługa systemd, port 8131 | paczka zasobów z sha1 | gracze |
| `hot-play` (planowany) | komputer gracza | instalacja, aktualizacja i naprawa gry, start na serwerze | `horizon-launcher` |

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
hotd buduje nowy silnik w tle → stary zapisuje graczy i świat → nowy startuje
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
- **Nowa treść (bloki, przedmioty) przez sloty** z `hot-slots.hmod`, bo rejestry Horizona są zamrożone po starcie.
- **Silnik budujemy tylko na serwerze.** Wymaga zasobów gry, których nie wolno pobierać w CI. Mody gorące i aplikację gracza buduje CI.
- **Każdy błąd ma kod i opis naprawy** (`docs/bledy/`).

## Decyzje

Pełne uzasadnienia: `docs/adr/`. Kompromisy świadomie przyjęte:

- Podmiana silnika nie jest niewidoczna: świat stoi. Cel: krócej niż 1 s (ADR 0004); bez rozgrzania silnika zmierzono na Macu 1–4 s (S7 i S8, `docs/adr/0002-spike.md`). Podmiana bez żadnego śladu wymagałaby przeniesienia całego stanu gry między procesami — za drogie i kruche przy częstych wydaniach Horizona.
- Stare generacje modów zostają w pamięci (bez wyładowania biblioteki). Pamięć odzyskuje okresowa podmiana silnika.

## Rozwój

- Nowa funkcja = nowy moduł w odpowiednim crate albo nowy crate w `crates/`, dopisany do tabeli wyżej.
- Kruche miejsca: `hot-abi` (zmiana = nowe `ABI_MAJOR` i przebudowa wszystkich modów), `hot-host` (zależy od API Horizona; używamy tylko nazwanych haków).
