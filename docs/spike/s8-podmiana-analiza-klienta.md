# S8, część 1 — co klient Horizona robi przy przełączeniu serwera (analiza kodu)

- Data: 2026-11-12. Źródła: Horizon `nightly-2026-11-05-66f3981` (`~/Library/Application Support/horizon-launcher/native/stage`, licencja MIT).
- Krok planu: M0.9 (przed prototypem). Wynik: **droga bez ekranu ładowania istnieje tylko jedna** — opis niżej. Prototyp: część 2.

## Pytanie

Wariant A (D10–D12): gracz przechodzi na nowy silnik bez rozłączenia i bez ekranu ładowania; dopuszczalne jest krótkie „mrugnięcie” świata. Proxy trzyma połączenie klienta, a za nim zmienia się silnik. Którymi pakietami da się to zrobić?

## Fakty z kodu klienta

Ścieżki względem `crates/horizon-client/src/`.

| Co proxy wysyła klientowi | Reakcja klienta | Gdzie |
|---|---|---|
| `StartConfiguration` (powrót do konfiguracji) | ekran „connect.reconfiguring” do następnego `Login` | `net.rs:64`, `net.rs:891`, `game/remote.rs:9916` |
| drugi `Login` w trakcie gry | nowy gracz, nowy poziom, `LevelLoadingScreen` (`enter_remote_dimension`, `keep_world = false`) | `game/remote.rs:3213` |
| `Respawn`, także w tym samym wymiarze | `LevelLoadingScreen` (`start_client_load`) | `game/remote.rs:3259`, `game/remote.rs:2209-2254` |
| nic przez dłuższy czas | rozłączenie po 30 s ciszy (`READ_TIMEOUT`, „disconnect.timeout”) | `net.rs:573` |
| ekran ładowania bez chunka pod graczem | czeka do 30 s (`CLIENT_WAIT_TIMEOUT_MS`), potem zamyka się po 0,5 s od gotowości | `crates/horizon-client-render/src/level_loading.rs:57-60` |

Wniosek: `StartConfiguration`, `Login` i `Respawn` zawsze dają ekran ładowania. Proxy, które ma go uniknąć, **nie może wysłać klientowi żadnego z tych trzech pakietów** po pierwszym wejściu.

## Jedyna droga bez ekranu ładowania

1. Proxy samo przechodzi logowanie i konfigurację z nowym silnikiem (B), w imieniu klienta. Odtwarza to, co klient wysłał do starego silnika (A) w swojej konfiguracji: `ClientInformation`, `brand`, `horizon:mod_list`, odpowiedzi na `known_packs`, cookies. Pakiety konfiguracji od B połyka.
2. `Login` od B połyka. Zapamiętuje z niego `player_id` gracza na B (pB). Klient zna gracza jako pA.
3. Klientowi wysyła `RemoveEntities` ze wszystkimi encjami, które A pokazał (proxy je śledzi), oprócz gracza. Chunków prawdopodobnie nie musi zapominać: B przyśle te same chunki, a `LevelChunkWithLight` powinien zastąpić je w miejscu (HIPOTEZA, sprawdzić w prototypie). To jest „mrugnięcie”: encje znikają i wracają, teren zostaje.
4. Od tej chwili każdy pakiet w obie strony z id encji przechodzi przez zamianę pA ↔ pB. Bez tego klient brałby pakiety o własnym graczu za obcą encję i odwrotnie.
5. Pozycję ustawia `PlayerPosition`, który B wysyła po wejściu gracza (z zapisu gracza w świecie); proxy go przepuszcza. Ruch klienta w czasie przerwy przepada, więc gracz może cofnąć się o kilka kratek (PRZEWIDYWANIE).

Pakiety tylko z `Login` (dystans widzenia, `hardcore`, lista wymiarów, `enforces_secure_chat`) muszą mieć te same wartości na A i B — ten sam `server.toml`.

## Czym to zbudować

- `crates/horizon-protocol` (MIT): pełny kodek 26.3 (protokół 777) w obie strony — `Clientbound::decode/encode` i `Serverbound::decode/encode` dla każdego stanu, pakiety bez modelu jako `Unknown(id, bytes)` (`src/packets/mod.rs:17-68`), kompresja, szyfrowanie, RSA.
- Zależności tylko z crates.io i dwa crate'y Horizona (`horizon-core`, `horizon-world`); `horizon-world/build.rs` liczy jedynie hash własnych źródeł. **Nie czyta plików gry**, więc `hot-proxy` z tą zależnością można budować w CI.
- Horizon ma też gotowe przekazanie gracza między dwoma działającymi serwerami: `[transfer]` w `server.toml` + `player-expect`, `player-export … freeze`, `player-import`, `player-release` (`docs/server.md` Horizona, sekcja 2). Wymaga Velocity forwarding (`docs/velocity-forwarding.md`: HMAC-SHA256, kanał `velocity:player_info`). Przy podmianie silnika na tym samym świecie oba silniki nie działają naraz (S12: `session.lock`), więc eksport z A i import do B nie są potrzebne — dane gracza zapisuje A przy wyjściu, a B czyta je ze świata. Moduł `[transfer]` przyda się, jeśli kiedyś będą dwa światy.

## Ryzyka do sprawdzenia w prototypie

1. Zamiana pA ↔ pB musi objąć **każdy** pakiet z id encji, także id w `SetEntityData` (np. właściciel haczyka wędki). Przeoczony pakiet = błąd wyświetlania jednej encji, nie rozłączenie.
2. Czy klient przyjmuje `PlayerPosition`, `SetHealth`, `ContainerSetContent` i `PlayerAbilities` od B bez `Login` — według kodu tak (zwykłe pakiety gry), do potwierdzenia.
3. Pauza = zapis i zatrzymanie A + start B + logowanie i konfiguracja proxy z B + wysłanie chunków. S12 zmierzyło pierwsze dwa: ok. 1 s na Macu.
4. Komunikaty „left the game” / „joined the game” z A i B — proxy je odfiltrowuje.
