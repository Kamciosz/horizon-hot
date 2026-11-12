# S6 — czy proxy może zmienić wygląd w trakcie gry (`ResourcePackPush`)

- Data: 2026-11-12. Horizon `nightly-2026-11-05-66f3981`, macOS arm64 (Apple M4 Pro), klient, serwer i proxy na jednej maszynie, paczki z lokalnego `http.server`.
- Krok planu: M0.7. Wynik: **działa, ale z nakładką**. Wygląd zmienia się bez rozłączenia i bez ponownego wejścia, tyle że przez ok. 1,2–1,4 s cały ekran zasłania nakładka ładowania. Hipoteza z D9 („bez żadnego mrugnięcia”) obalona.

## Co sprawdzono

Proxy z S8 (`spikes/s8-podmiana/proxy/`) dostało opcję `--pack-file <plik>`. Gdy plik się pojawia, proxy czyta z niego `<url> <sha1>` i wysyła klientowi w grze `resource_pack_push` (`required: true`, bez tekstu pytania). Odpowiedzi klienta na tę paczkę zapisuje w logu jako `PACK RESULT` i nie przekazuje ich silnikowi, bo silnik o paczce nie wie.

1. Serwer z pulą 1 slotu (S7), bez paczki w `server.properties`. Klient wchodzi przez proxy, przed graczem ściana z `hot_slots:slot_0`.
2. Paczka 1: paczka z S5 (`spikes/s2-s5-pula-slotow/paczka/`).
3. Paczka 2: ta sama z teksturą slotu w innym kolorze (pliki PNG 16×16 wygenerowane skryptem).
4. Co widzi gracz: seria zrzutów klatki (`horizon-ctl screenshot`) co ok. 45 ms przez 6,6 s. Zrzuty zostały na maszynie próby, bo pokazują tekstury i logo gry.
5. Ten sam sposób zastosowany do podmiany silnika z S8 (80 zrzutów przez 5,4 s, podmiana 1,08 s).

## Wynik

| | Paczka 1 | Paczka 2 |
|---|---|---|
| zgoda gracza → pobrana (`accepted` 3 → `downloaded` 4) | 7 ms | 18 ms |
| pobrana → wczytana (`successfully_loaded` 0) | 1,39 s | 1,39 s |
| nakładka ładowania na zrzutach | — | ok. 1,2–1,4 s |

- Po wczytaniu ściana ma nowy kolor, a gracz nadal jest połączony.
- **Nakładka.** Przez cały czas przeładowania klatkę zasłania pełnoekranowa nakładka z paskiem postępu („Preparing resources”, „Loading assets”, „Loading pack tables”), a klient ignoruje klawisze i mysz. FAKT z kodu: `crates/horizon-client/src/game.rs`, `self.pack_reload` rysuje `draw_loading_overlay` zamiast świata i odrzuca wejście.
- **Pytanie o zgodę.** Klient pytał o zgodę przy obu paczkach, mimo że pierwszą gracz już przyjął. FAKT z kodu: `handle_pack_push` sprawdza za każdym razem `accept_textures` z listy serwerów (`server_packs.rs`). Bez wpisu na liście pytanie pojawia się przy każdej paczce. `hot-play` musi więc dopisać nasz serwer do listy z `accept_textures = true` (D9).
- **Ta sama paczka, nowe id.** Paczka o id, które klient już ma, zastępuje poprzednią (`server_packs.rs`, `push`: `Removal::ServerReplaced`). Próba wysyłała za każdym razem nowe id, więc paczki się nakładały. `hot-proxy` musi używać jednego stałego id dla paczki serwera.
- **`horizon-ctl status` nie widzi nakładki ani pytania.** Pole `screen` przez cały czas miało wartość `playing`. Wniosek dla S8: dopiero zrzuty klatki dowodzą, że podmiana silnika nie pokazuje nakładki. Seria 80 zrzutów obejmująca podmianę: na każdym widać świat, nakładki nie ma.

## Wnioski

1. Wygląd można zmienić bez rozłączenia i ponownego wejścia, ale każda zmiana paczki zasłania ekran na ok. 1,3 s (Mac, mała paczka) i blokuje sterowanie. Jest to bardziej widoczne niż podmiana silnika, w której świat stoi, ale widać go cały czas.
2. Podmiana silnika nie dostarcza nowego wyglądu: proxy połyka konfigurację nowego silnika razem z jego `resource_pack_push` (S8). Nowy wygląd dociera więc tylko przez `resource_pack_push` w grze (nakładka) albo przy następnym wejściu gracza.
3. Do `hot-proxy` (M8): paczkę wysyła proxy ze stałym id. Odpowiedzi klienta zatrzymuje u siebie, a przy logowaniu nowego gracza podaje aktualny adres i sha1.
4. Kiedy wysyłać paczkę: od razu po zmianie wyglądu (nakładka u wszystkich) czy dopiero przy następnym wejściu (bez przerwy, ale do tego czasu nowe sloty bez wyglądu). Decyzja użytkownika: `PLAN.md`, sekcja 8.

## Czego nie sprawdzono

- Czasu przeładowania z dużą paczką (modele i tekstury dla 4096 slotów) i na ThinkCentre (M0.11).
- Pobierania przez sieć z `hot-pack`. W próbie paczka miała 2 KB i leżała na tej samej maszynie.

## Powtórzenie

```bash
# serwer z hot_slots (S7, srv-one), paczki w katalogu serwowanym przez: python3 -m http.server 8137
s8-proxy --listen 127.0.0.1:25700 --server-bin "$SB" --a 25621:<srv-one> --b 25566:<s12>/b \
  --swap-file <katalog>/swap --pack-file <katalog>/pack 2> proxy.log
# klient: MCP play --server 127.0.0.1:25700 --offscreen
echo "http://127.0.0.1:8137/pack.zip <sha1>" > <katalog>/pack   # klient pyta o zgodę: przycisk „Proceed”
grep "PACK RESULT" proxy.log                                    # 3 accepted, 4 downloaded, 0 loaded
```
