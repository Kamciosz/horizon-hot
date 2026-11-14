# ADR 0033 — nowy program `hot-proxy` przy graczach (D19, M13.1)

- **Data:** 2026-11-14
- **Status:** accepted, zmienia mechanizm z ADR 0028
- **Autor:** Szymon Sosnowski

## Kontekst

Restart `hot-proxy` rozłącza wszystkich graczy, więc do tej pory nowa wersja proxy czekała na pusty serwer (ADR 0014, 0028). `PLAN.md` zakładał drugi proces, któremu stary przekazuje gniazda graczy przez `SCM_RIGHTS`. Połączenie gracza to jednak nie tylko gniazdo: szyfrowanie AES/CFB8 ma stan w obu kierunkach, czytelnik mógł już odszyfrować część następnego pakietu, a proxy pamięta profil, nagraną konfigurację, identyfikator gracza i encje silnika. Wszystko to musi przejść razem z gniazdem, bo inaczej klient dostaje śmieci i się rozłącza.

## Rozważane opcje

1. **Drugi proces i `SCM_RIGHTS`.** Nowy proces startuje obok starego i dostaje deskryptory przez gniazdo uniksowe. Przez chwilę działają dwa procesy z tym samym portem, a systemd widzi nowy PID, którego nie uruchomił: usługa wymagałaby `Type=notify` i `NotifyAccess`, a kontener innego procesu nadrzędnego.
2. **`execve` w tym samym procesie.** Stare proxy zapisuje stan do pliku, zdejmuje `FD_CLOEXEC` z potrzebnych deskryptorów i zastępuje swój program nowym. PID się nie zmienia, więc systemd i kontener niczego nie zauważają, a deskryptory przechodzą same.
3. **Bez zmian:** proxy czeka na pusty serwer. Na serwerze, na którym ktoś gra stale, poprawka proxy może nie wejść nigdy.

## Decyzja

Wybieramy **opcję 2**: `hot-proxy upgrade [PLIK]` przez gniazdo sterujące (`crates/hot-proxy/src/upgrade.rs`).

1. Nowy plik musi odpowiedzieć kodem 0 na `PLIK --check-resume 1` w 10 s. Inaczej odmowa `HOT-E0609`, a stare proxy działa dalej.
2. Proxy przestaje przyjmować połączenia. Gniazdo nasłuchu zostaje otwarte, nowe połączenia czekają w kolejce jądra. Proxy czeka do 10 s, aż skończą się trwające logowania i każdy gracz będzie w grze.
3. Każdy wątek czytający gracza zatrzymuje się na granicy pakietu (odczyt co 50 ms sprawdza prośbę) i oddaje swój stan. Gracz w fazie konfiguracji albo wątek, który nie stanie w 2 s, to odmowa: wątki ruszają dalej, a połączenia znowu są przyjmowane.
4. Pisarze graczy zostają zablokowani, a stan idzie do pliku `hot-state/hot-proxy-upgrade-<pid>.json` (0600, usuwany z katalogu zaraz po otwarciu, dalej przechodzi tylko deskryptor). Format 1: sekret, ostatnie 16 bajtów szyfrogramu w obu kierunkach (rejestr CFB8), odczytany, a nieprzetworzony tekst jawny, próg kompresji, profil, adres, nagrana konfiguracja, identyfikator gracza, encje, licznik czatu, stan paczki pakietów.
5. Gniazda graczy, nasłuchu, sterujące, gniazdo z odpowiedzią i plik stanu tracą `FD_CLOEXEC`, potem `execve(PLIK, te same argumenty + --resume-fd N)`. Gdy `execve` się nie uda, flagi wracają, wątki ruszają, odmowa `HOT-E0609`.
6. Nowy program odtwarza połączenia graczy i loguje ich do silnika od nowa tą samą drogą, co po utracie silnika (`Link::rejoin`). Połączeń z silnikiem nie przekazujemy: `execve` je zamyka, a silnik widzi zwykłe wyjście i ponowne wejście gracza. Odpowiedź `{"upgraded": true, "players", "failed", "handover_ms", "pid", "version"}` przychodzi przez to samo gniazdo sterujące, już od nowego programu.

`hotd upgrade` (ADR 0014) ma tryb `proxy` dla wersji, która zmienia tylko `crates/hot-proxy/`: buduje sam `hot-proxy`, kładzie nowy plik w `bin/hot-proxy` (stary jako `.previous`) i wysyła `upgrade` przy graczach. Odmowa albo brak proxy na gnieździe: stary plik wraca, a wersja idzie trybem `full`, czyli czeka na pusty serwer. Polecenie przyjęte bez odpowiedzi znaczy, że nowy program padł: stary plik wraca, usługa proxy się restartuje, wersja trafia na listę `bad` (`HOT-E0708`).

Zależność: `libc` (już w `[workspace.dependencies]`, ADR 0009) także w `hot-proxy`. Biblioteka standardowa nie zdejmuje `FD_CLOEXEC` i nie czeka na gotowość gniazda bez wątku (`poll`). Dwa bloki `unsafe`: `fcntl` na własnych deskryptorach i `poll` na jednym deskryptorze z lokalnej struktury.

## Konsekwencje

- Pomiar (Mac, test `crates/hot-proxy/tests/upgrade.rs`, szyfrowany gracz, udawany silnik): od zatrzymania graczy do gotowości nowego programu 670 ms w buildzie debug i 84 ms w release. Ponowne wejście na działający silnik to dodatkowo mediana 67 ms, maksymalnie 118 ms (pomiar `hot-proxy switch` z M8.2). Nie sprawdzone na ThinkCentre ani z prawdziwym klientem.
- Test sprawdza, że gracz zostaje przy tym samym PID, bez ponownego logowania i bez rozłączenia, a pół pakietu wysłane przed podmianą dochodzi do silnika po niej. Mutacje (zły rejestr CFB8, zgubiony bufor, deskryptor z `FD_CLOEXEC`, wątek, który nie rusza po odmowie) wywracają testy.
- Format stanu ma numer. Nowy program, który go nie zna, odmawia w kroku 1, zanim cokolwiek się zatrzyma. Zmiana formatu wymaga nowego numeru i obsługi starego, jeśli ma działać podmiana w obie strony.
- Na czas przekazania gracze nie dostają pakietów. Klient rozłącza się dopiero po 30 s ciszy (`docs/podmiana.md`), a po przekazaniu proxy wysyła mu własne keep-alive tak samo jak przy utracie silnika.
- Gracz w trakcie zmiany świata albo wejścia blokuje podmianę. `hotd upgrade` przy odmowie przechodzi na tryb `full` i nie ponawia `upgrade` co 10 minut.
- Kontener nie aktualizuje się sam (ADR 0011). PID proxy się nie zmienia, więc proces nadrzędny kontenera nie powinien niczego zauważyć, ale `hot-proxy upgrade` w kontenerze nie był sprawdzany.
