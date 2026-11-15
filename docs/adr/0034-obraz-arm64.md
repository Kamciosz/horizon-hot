# ADR 0034 — obraz Dockera także na ARM64, bez modów gorących

- **Data:** 2026-11-15
- **Status:** accepted, zmienia punkt o platformie z ADR 0011
- **Autor:** Szymon Sosnowski

## Kontekst

ADR 0011 ograniczył obraz do `linux/amd64`, bo Horizon wydaje serwer na Linuksa tylko dla x86-64. Obraz nie korzysta jednak z wydań: `horizon-server` i `horizon-engine` kompiluje ze źródeł w przypiętej rewizji. Właściciel może mieć serwer ARM64 (VPS z procesorem Ampere, Raspberry Pi z 64-bitowym systemem), a agent, który wdraża serwer według `docs/przewodniki/wdrozenie-docker.md` bez kontaktu z autorami, musiał się na takiej maszynie zatrzymać.

Na ARM64 nie da się zbudować silnika z `hot-host`. Buduje go `native-build` z `horizon-launcher`, a launcher na Linuksa wychodzi tylko dla x86-64 (wydanie `launcher@0.2.66` z 15.11.2026 ma `horizon-launcher-linux-x86_64.AppImage` i `.tar.gz`, wersji na Linux ARM64 nie ma).

## Rozważane opcje

1. **Bez zmian.** Na ARM64 wdrożenie się zatrzymuje.
2. **Domyślnie `linux/amd64`, ARM64 przez `platform: linux/arm64` w `compose.override.yaml`.** Ten sam `Dockerfile`. Na x86-64 i na Macu z emulacją Rosetta nic się nie zmienia.
3. **Bez `platform` w `compose.yaml`, czyli architektura hosta.** ARM64 działa bez ustawień, ale Mac z Apple Silicon dostaje wtedy obraz arm64 bez modów gorących zamiast dotychczasowej emulacji x86-64.

## Decyzja

Wybieramy **opcję 2**:
- `compose.yaml` zostaje przy `linux/amd64`, a komentarz mówi, kiedy zmienić platformę w `compose.override.yaml`;
- na ARM64 serwer działa jak zwykły serwer Horizona z kontrolą zakupu, bez modów gorących. Według kodu skryptu startowego (tej drogi nie uruchamiano) ze zgodą na kod natywny, ale bez działającego launchera skrypt zgłasza `HOT-E0110` i uruchamia serwer bez `hot-host`;
- przewodnik wdrożenia sprawdza architekturę w kroku 1 (`uname -m`) i dopisuje platformę w kroku 5. Inna architektura niż `x86_64` i `aarch64` (np. 32-bitowy `armv7l`) zatrzymuje wdrożenie.

## Konsekwencje

- Sprawdzone 15.11.2026 w maszynie wirtualnej aarch64 (colima, 4 CPU, 8 GiB). Obraz zbudował się natywnie w 12 min, przy narzędziach Rusta wziętych z pamięci podręcznej. Wszystkie programy w obrazie są dla aarch64 (nagłówek ELF). Kontener na atrapie plików gry przeszedł te same próby co obraz amd64: `HOT-E0115` i `HOT-E0107` z kodem 3, klucze, świat, paczka, tylko opublikowane porty i zatrzymanie w 4 s. Silnik doszedł do sprawdzenia plików gry i odrzucił atrapę. Na prawdziwych plikach gry silnika na ARM64 nikt nie uruchamiał.
- Mody gorące na ARM64 czekają na launcher Horizona dla Linux ARM64. Wtedy wystarczy ustawić `HOT_LAUNCHER`; ta ścieżka nie była sprawdzana.
- Nowych zależności nie ma.
