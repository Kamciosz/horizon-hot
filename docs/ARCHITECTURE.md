# Architektura

Gdzie co leży i dlaczego. Stan: zbudowany jest tylko `hot-abi`; reszta opisuje docelowy kształt i powstaje krok po kroku. Składnik jeszcze nieistniejący ma dopisek „planowany”.

## Składniki

| Składnik | Gdzie działa | Odpowiada za | Rozmawia z |
|---|---|---|---|
| `hot-abi` | biblioteka | stabilna granica C ABI między gospodarzem a modami | — |
| `hot-diag` (planowany) | biblioteka + CLI | logi JSONL z kodami błędów, `hot report` | wszyscy |
| `hot-sdk` (planowany) | biblioteka dla autorów modów | makro `hot_mod!`, bezpieczne opakowania ABI, `catch_unwind` | `hot-abi` |
| `hot-host` (planowany) | w silniku (mod natywny `.hnmod`) | ładuje `hot-core`, przekazuje mu haki Horizona | silnik, `hot-core` |
| `hot-core` (planowany) | w silniku (`cdylib`) | ładuje generacje modów gorących, sloty, komenda `/hot` | `hot-host`, mody |
| `hotd` (planowany) | usługa systemd, tło | repo serwera, artefakty CI, aktualizacje, kopie, hamulec MSPT | GitHub, `horizon-server`, `hot-core` |
| `hot-proxy` (planowany) | usługa systemd, port 25565 | logowanie graczy, trzymanie połączeń przy podmianie i awarii silnika | gracze, silniki |
| `hot-pack` (planowany) | usługa systemd, port 8131 | paczka zasobów z sha1 | gracze |
| `hot-play` (planowany) | komputer gracza | instalacja, aktualizacja i naprawa gry, start na serwerze | `horizon-launcher` |

## Najważniejsze ścieżki

**Zmiana kodu moda gorącego.**
```
push do repo serwera → CI buduje .so i podpisuje → hotd pobiera, sprawdza podpis
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
→ hot-proxy przenosi graczy bez rozłączenia (świat stoi ok. 1–4 s, bez ekranu ładowania)
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

- Podmiana silnika nie jest niewidoczna: świat stoi ok. 1–4 s (zmierzone na Macu w S7 i S8, `docs/adr/0002-spike.md`). Podmiana bez żadnego śladu wymagałaby przeniesienia całego stanu gry między procesami — za drogie i kruche przy częstych wydaniach Horizona.
- Stare generacje modów zostają w pamięci (bez wyładowania biblioteki). Pamięć odzyskuje okresowa podmiana silnika.

## Rozwój

- Nowa funkcja = nowy moduł w odpowiednim crate albo nowy crate w `crates/`, dopisany do tabeli wyżej.
- Kruche miejsca: `hot-abi` (zmiana = nowe `ABI_MAJOR` i przebudowa wszystkich modów), `hot-host` (zależy od API Horizona; używamy tylko nazwanych haków).
