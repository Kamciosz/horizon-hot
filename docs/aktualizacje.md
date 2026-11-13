# Aktualizacje Horizona: co zmienia się między wydaniami (S11)

- Data: 2026-11-12. Krok planu: M0.12. Nasza wersja odniesienia: `nightly-2026-11-05` (`66f3981`), na niej działają wszystkie próby M0.
- Wynik: **to, z czego korzysta `hot-host`, nie zmieniło się od 11-03 do najnowszego Nightly**. Plan aktualizacji (D18) wymaga jednak poprawki: mod natywny przypina wersję Horizona, więc bez jawnej flagi serwer nigdy nie przejdzie na nowe Nightly (punkt 7).
- Do sprawdzania przy każdej aktualizacji: sekcja „Powtórzenie” na końcu.

## Zakres

Wydania Nightly są dopiero od 2026-11-01 i jest ich 6. Do dziesięciu dobrane są cztery ostatnie wydania stabilne. `v0.2.97` jest przodkiem `nightly-2026-11-01`, więc to jedna linia historii.

| Wydanie | Commit | Data | Commitów od poprzedniego |
|---|---|---|---|
| `v0.2.94` | `e1b2b0b` | 2026-10-28 | 51 |
| `v0.2.95` | `f66f66c` | 2026-10-28 | 23 |
| `v0.2.96` | `506a9c8` | 2026-10-28 | 107 |
| `v0.2.97` | `a3acb5f` | 2026-10-28 | 53 |
| `nightly-2026-11-01` | `c9d474d` | 2026-11-01 | 6255 |
| `nightly-2026-11-03` | `2944480` | 2026-11-03 | 4491 |
| `nightly-2026-11-04` | `6475f62` | 2026-11-04 | 1334 |
| `nightly-2026-11-05` | `66f3981` | 2026-11-05 | 1707 |
| `nightly-2026-11-07` | `2ebf00a` | 2026-11-07 | 2263 |
| `nightly` (wydanie z 11-08) | `fdc87ae` | 2026-11-07 | 51 |

Tag `nightly` przesuwa się na każde nowe Nightly. W skryptach używaj tagów z datą albo commitów.

Metoda: klon repo Horizona bez zawartości plików (`git clone --bare --filter=blob:none`), porównanie identyfikatorów plików między tagami, katalogu haków `native-hooks.json`, sygnatur funkcji i `Cargo.lock`. Niczego nie kompilowano.

## Wynik

### 1. API haków (`mod_api`)

- `crates/horizon-client/src/mod_api.rs` i katalog `native-hooks.json` pojawiły się w `nightly-2026-11-03`. W `v0.2.97` i `nightly-2026-11-01` ich nie ma, więc API, na którym stoi `hot-host`, ma kilka dni.
- To, czego używamy, jest identyczne od 11-03 do `fdc87ae`:

  | Element | Wpis w katalogu | Funkcja w silniku |
  |---|---|---|
  | hak `player_command` | `version 1`, `contract cancel` | `native_hooks.rs`, `native_player_command(player, name, command, canceled, replies)` |
  | hak `server_tick` | `version 1`, `contract listen` | `native_server_tick` |
  | hak `server_about_to_start` | `version 1`, `contract listen` | `native_server_about_to_start` |
  | `Host::tell` | pomocnik | `native_server.rs`, `native_tell(uuid, line) -> bool` |
  | `Host::run_command` | pomocnik | `native_run_command` |
  | `Flow`, `ServerTickPhase` | — | `mod_api.rs`, tekst bez zmian |

  W 11-07 wpis `server_tick` dostał tylko opis (`neoforge`, `side`), kontrakt się nie zmienił.
- Numer API (`NATIVE_HOOKS_API` w `native_hooks.rs`): 359 w 11-03 i 11-04, 411 od 11-05. Między wydaniami z katalogu nie zniknął żaden hak. Oczekujących fragmentów (haki jeszcze bez numeru, `tools/native-hook-fragments/`) było 20 w 11-05 i 158 w 11-07. Na hak bez numeru nie wolno się powoływać (README tego katalogu).
- **Nowe w 11-07 (PR #10218):** `mod_api::set_mod_on(id, on)` wyłącza i włącza trait mod w działającej grze: dispatch `Hooks` pomija wyłączony mod. Wywołuje to ekran opcji klienta. `RenderHooks` i mixiny nie mają tego przełącznika. FAKT z kodu, nieużyte w próbach.

### 2. `content_identity`

- `crates/horizon-client/src/content_identity.rs` bez zmian od 11-01 do `fdc87ae`. W `v0.2.97` pliku nie ma.
- Co liczy (FAKT z kodu, `identity`):
  - kolejność oraz nazwy i właściwości **wszystkich** stanów bloków z `facts.json`, także zwykłych bloków gry;
  - wpisy modów w rejestrach `item`, `mob_effect`, `entity_type`, `menu`;
  - typy komponentów NBT dodane przez mody natywne i ich wartości domyślne.
- `facts.json` powstaje przy `prepare` z plików gry 26.3. W zakresie 11-01 → `fdc87ae` `horizon-prepare` zmieniał tylko wyszukiwanie plików gry (Prism Launcher), nie zapis `facts.json`.
- Wniosek, HIPOTEZA: przy tych samych modach `content_identity` jest takie samo na różnych Nightly. Sprawdzenie: jeden klient, serwer na innym Nightly, ta sama pula. Wtedy aplikacja gracza nie musi aktualizować Horizona razem z serwerem.
- Wymaganie dla `hot-host`: nie rejestruje typów komponentów NBT, bo zmieniłby `content_identity` i każdy gracz potrzebowałby go u siebie.

### 3. Kanały `horizon:*`

| Kanał | Plik | Jest od | Zmiany po 11-05 |
|---|---|---|---|
| `horizon:block_registry` | `horizon-client/src/content_identity.rs` | ≤ 11-01 | brak |
| `horizon:mod_net` | `horizon-protocol/src/mod_net.rs` | ≤ 11-01 | brak |
| `horizon:quiet_commands` | `horizon-protocol/src/quiet.rs` | ≤ 11-01 | brak |
| `horizon:lod` | `horizon-client/src/lod_net.rs` | ≤ 11-01 | brak |
| `horizon:recipe_content` | `horizon-client/src/lan/recipes.rs` | ≤ 11-01 | plik zmieniany w każdym Nightly |
| `horizon:mod_list` | `horizon-protocol/src/mod_list.rs` | 11-03 | brak |
| `horizon:attachment_sync` | `horizon-protocol/src/attachment_sync.rs` | 11-03 | brak |
| `horizon:menu_extra` | `horizon-protocol/src/menu_extra.rs` | 11-05 | brak |
| `horizon:recipe_transfer` | `horizon-client/src/lan/recipes.rs` | **11-07** | nowy |

- Kanały się dopisuje, żaden nie zniknął. Pliki kanałów konfiguracji, które odtwarza proxy (`mod_list`, `mod_net`, `block_registry`), są bez zmian od 11-03.
- Proxy z S8 przekazuje w grze każdy pakiet bez zmian (poza zamianą id encji), więc nowy kanał w grze przejdzie. W konfiguracji odtwarza wobec nowego silnika tylko kanały, które wcześniej zapisało od klienta. FAKT z kodu `spikes/s8-podmiana/proxy/src/main.rs` (`client_loop`, `configure`).

### 4. Protokół i lista serwerów

- `PROTOCOL_VERSION` = 777 we wszystkich 10 wydaniach. `horizon-protocol/src/ids.rs` (numery pakietów) bez zmian od 11-03.
- `packets/handshake.rs` w 11-07: doszła funkcja `decode_intention` z dłuższym limitem nazwy hosta dla serwera za proxy. Pola pakietu bez zmian.
- Doszły adaptery starszych wersji klienta (`adapter/v1_21_*`, `v26_*`). Nas nie dotyczą: gracze grają klientem Horizon 26.3.
- `servers.rs` (`servers.dat`, pole `acceptTextures`, D9): format bez zmian, doszła funkcja `insert_after`.

### 5. `libloading` w `Cargo.lock`

- Wersja 0.8.9 we wszystkich 10 wydaniach.
- Zależą od niej `wgpu-hal`, `ash`, `dlib`, `khronos-egl`, `x11rb` i `horizon-narrator`, czyli grafika i lektor ekranu. Zniknięcie jest mało prawdopodobne. R1 aktualne.

### 6. Cache treści modów

- Od 11-07 (PR #10217) cache leży w `.horizon-cache/mod-content/<16 znaków hex>/`: osobny katalog na każdy zestaw modów. Katalog nieużywany przez 24 h znika przy budowie nowego.
- Klucz nadal zawiera plik programu (`current_exe`). Wniosek z S7 bez zmian: każdy nowy silnik startuje z zimnym cache.
- Nowe: powrót do zestawu modów użytego w ciągu doby nie buduje cache od nowa. HIPOTEZA dla serwera dedykowanego: ta sama funkcja `resource_root`.

### 7. Przypięcie modu natywnego do wersji Horizona

FAKT z kodu `launcher/src/native_mods.rs` (`pinned_revision`, `mismatch`, `repin`) i `launcher/src/cli.rs` (`native-build`):

- `[compatibility] horizon_revision` w `horizon-mixin.toml` przypina wersję. Bez flag launcher buduje silnik na najnowszej z przypiętych rewizji, nie na najnowszym zainstalowanym Nightly.
- Mod przypięty do innej rewizji niż build → błąd „Made for other Horizon versions”, chyba że podano `--allow-version-mismatch`.
- `--ignore-pins` (od 11-05) buduje na commicie wersji bazowej: `--version <tag>` albo `--update` (najnowsza zainstalowana). Launcher sam przepisuje wtedy `horizon_revision` w kopii modu.
- **Skutek dla D18:** `hot-host` ma `horizon_revision`, więc build aktualizacji musi mieć `--version <nowe Nightly> --ignore-pins`. Inaczej silnik zostaje na starej wersji, choć nowe Nightly jest zainstalowane.
- **Zmiana 2026-11-13 (ADR 0012):** `--version` wymaga zainstalowanego wydania, a datowane wydania znikają po kilku dniach. Zamiast `--ignore-pins` przepinamy `horizon_revision` w kopii paczki na nową rewizję, a bazą jest dowolne zainstalowane wydanie albo Nightly. Przy jednym przypięciu launcher buduje przypiętą rewizję (`agreed` w `newest`).
- Koszt builda według Horizona (`docs/design/mod-hotload.md`, szybki desktop pod obciążeniem): na zimno 812 s, bez zmian 55 s.

### 8. Horizon sam wprowadza mody bez restartu

FAKT, `docs/design/mod-hotload.md` (11-05/06) i PR-y scalone po 11-05:

- #10104: przeglądarka i instalacja modów treści z poziomu gry.
- #10217: włączanie i wyłączanie modu treści w działającej grze, jeśli mod ma tylko zasoby i dane (przepisy, tagi, łupy). Klient pokazuje nakładkę przeładowania, serwer robi to samo co `/reload`. Dotyczy gry z ekranu opcji i serwera zintegrowanego (świat jednoosobowy, LAN), zmiany są w `options_ui.rs`, `integrated.rs`, `game/mods.rs`. `horizon-server` nie dostał takiej funkcji. Mod z blokami albo przedmiotami nadal wymaga restartu.
- #10218: `set_mod_on`, punkt 1.
- Plany Horizona bez terminu: przeładowanie świata przy zmianie rejestrów, mody `dylib` i wasm (wasm z wyładowaniem) od zewnętrznego współtwórcy.

Ryzyko i szansa, HIPOTEZA: Horizon może dojść do przeładowania modów na serwerze dedykowanym, a wtedy część naszej podmiany silnika (D11) będzie zbędna. Mody wasm z wyładowaniem mogłyby zastąpić nasze `cdylib` (D2). Przy każdym przeglądzie aktualizacji czytać ten dokument Horizona.

## Wnioski dla planu

1. Nazwane haki, których używa `hot-host`, przetrwały wszystkie Nightly od powstania API. Zasada z D18 („tylko nazwane haki”) się sprawdza. API jest jednak młode: numer wzrósł o 52 w dwa dni.
2. D18: build aktualizacji z `--version <Nightly> --ignore-pins`. Wpisane w `PLAN.md`. Zastąpione przez ADR 0012: przypięcie w kopii paczki.
3. Okno zgodności klientów (D18): protokół stały, kanały się dopisuje. Niewiadomą pozostaje `content_identity` między Nightly (punkt 2).
4. R23: między `v0.2.97` a `nightly-2026-11-01` było 6255 commitów.

## Czego nie sprawdzono

- Kompilacji `hot-host` ani próby S3/S4 z `fdc87ae`. Porównanie dotyczy katalogu i sygnatur, nie całego builda.
- `content_identity` w praktyce między dwoma Nightly.
- Przełączania modów z #10217 na serwerze dedykowanym.

## Powtórzenie

```bash
git clone --bare --filter=blob:none https://github.com/horizon-rust/horizon hz.git && cd hz.git
gh release list -R horizon-rust/horizon --limit 300 | grep -i nightly        # lista wydań
A=nightly-2026-11-05; B=nightly                                              # stare i nowe
# API haków: wpisy, których używamy (wynik musi być identyczny)
for t in $A $B; do git show "$t:native-hooks.json" | python3 -c '
import json,sys; d=json.load(sys.stdin)
for s in ("hooks","helpers"):
  for e in d[s]:
    if e["name"] in {"player_command","server_tick","server_about_to_start","tell","run_command"}: print(s,e)'; done
git show "$B:crates/horizon-client/src/game/native_hooks.rs" | grep "NATIVE_HOOKS_API: u32 ="
# content_identity, kanały konfiguracji, numery pakietów: ten sam identyfikator pliku = brak zmian
for f in crates/horizon-client/src/content_identity.rs crates/horizon-protocol/src/mod_list.rs \
         crates/horizon-protocol/src/mod_net.rs crates/horizon-protocol/src/ids.rs; do
  echo "$f $(git rev-parse --short "$A:$f") $(git rev-parse --short "$B:$f")"; done
# nowe stałe "horizon:…" (nie każda to kanał: sprawdź, gdzie jej używa CustomPayload)
git diff $A $B -- 'crates/*.rs' | grep -E '^[+-].*const [A-Z_]+: &str = "horizon:'
# libloading
git show "$B:Cargo.lock" | grep -A1 'name = "libloading"'
```
