# ADR 0011 — serwer w kontenerze Docker

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Właściciel chce móc uruchomić serwer w Dockerze, obok instalatora na Ubuntu (`scripts/server-install.sh`). Instalator opiera się na systemd: osobne usługi dla silnika, `hot-proxy`, `hotd` i serwera paczki, zapora i użytkownik `horizon`. W kontenerze nie ma systemd. Trzy ograniczenia zostają bez zmian:
- każdy gracz musi mieć kupioną grę, więc silnik stoi za `hot-proxy` i nigdy nie słucha na zewnątrz;
- pliki Mojanga nie trafiają do repo, CI ani obrazu;
- kod natywny (silnik z `hot-host`) buduje się tylko za zgodą właściciela.

## Rozważane opcje

1. **Jeden kontener ze skryptem startowym**, który przygotowuje dane jak instalator i pilnuje czterech procesów. Bez nowych zależności.
2. **Kontener na każdy proces** (silnik, proxy, hotd, paczka). `hotd` restartuje silnik i czyta jego folder, a proxy łączy się z `127.0.0.1`: trzeba by dzielić wolumeny, sieć i sygnały między kontenerami.
3. **systemd w kontenerze.** Wymaga uprzywilejowanego kontenera i cgroups hosta.

## Decyzja

Wybieramy **opcję 1**:
- `Dockerfile` buduje `hotd`, `hot-core`, `hot-diag`, `hot-proxy`, pulę slotów i paczkę `hot-host`, a `horizon-server` i `horizon-engine` kompiluje ze źródeł Horizona w pełnej rewizji commita, jak `packaging/server/Dockerfile` Horizona. Gotowego wydania nie pobiera, bo datowane wydania Nightly znikają z GitHuba po kilku dniach (R29 w `PLAN.md`); instalator robi to samo;
- `scripts/container-entrypoint.sh` przy każdym starcie robi kroki instalatora dotyczące danych (klucze, świat, pula slotów, silnik za proxy w tej samej kolejności), potem uruchamia procesy i podnosi każdy po awarii;
- `compose.yaml`: `init: true`, sieć bridge, na zewnątrz tylko porty 25565 i 8131, dane w wolumenie `/srv/horizon`, pliki gry montowane tylko do odczytu w `/game-files`, `stop_grace_period: 2m`;
- tylko `linux/amd64`, bo Horizon wydaje serwer na Linux x86-64;
- silnik z `hot-host` buduje się przy starcie tylko z `HOT_TRUST_NATIVE_CODE=1`, które ustawia właściciel. Bez tego serwer działa bez modów gorących.

## Konsekwencje

- Instalator i skrypt kontenera robią te same kroki w dwóch miejscach. Test `crates/hot-proxy/tests/installer.rs` pilnuje w obu kolejności sekret → adres lokalny → tryb offline i braku `--offline`.
- Obraz nie ma plików gry, więc pierwszy start bez montowania kończy się kodem `HOT-E0107` i czeka na właściciela.
- Zapora hosta nie dotyczy kontenera: o tym, co jest otwarte, decydują porty w `compose.yaml`.
- Obrazu nie publikujemy w rejestrze. Każdy buduje go u siebie z repo (`docker compose build`).
- Pierwsza budowa obrazu jest długa (kompilacja Horizona), kolejne korzystają z pamięci podręcznej BuildKit.
- Silnik z `hot-host` w kontenerze woła `horizon-launcher install <tag>`, a tag może już nie istnieć (R29). Do rozwiązania w M9; ścieżka niesprawdzona.
- Nowych zależności nie ma: obraz bazowy `ubuntu:24.04`, pakiety z apt i Rust z rustup, jak w instalatorze.
