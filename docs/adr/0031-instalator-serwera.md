# ADR 0031 — instalacja serwera jednym poleceniem (D25)

- **Data:** 2026-11-14 (decyzja z 2026-11-12, przeniesiona z `PLAN.md` w stanie zgodnym z kodem)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Serwer stoi u kolegi właściciela na ThinkCentre z Ubuntu. Kolega ma `sudo`, ale nie zna projektu. Instalacja ma się dać powtórzyć bez szkody, a każdy błąd ma mówić, co zrobić.

## Decyzja

- Jeden skrypt `scripts/server-install.sh`, uruchamiany z `sudo`. Opcje: `--help`. Każdy krok sprawdza stan maszyny i robi tylko to, czego brakuje, więc kolejne uruchomienie niczego nie psuje.
- Kroki:
  1. sprawdzenie maszyny (Ubuntu, x86_64, pamięć, dysk, wolne porty);
  2. użytkownik `horizon` i katalogi w `/srv/horizon`;
  3. pakiety systemowe;
  4. Horizon (`horizon-server`, `horizon-engine`) ze źródeł w przypiętej rewizji `HORIZON_REV` (ADR 0027);
  5. horizon-hot z `--hot-ref`: `hotd`, `hot-core`, `hot-diag`, `hot-proxy`;
  6. klucze: weryfikacja podpisów CI, klucz tylko do odczytu repo serwera, `hot.yml` w `/srv/horizon/server-repo-files/`;
  7. pliki gry i świat: pliki z Minecrafta właściciela (`--resources DIR`), `init` świata;
  8. proxy z kontrolą zakupu, silnik tylko na `127.0.0.1`;
  9. pula slotów i paczka zasobów;
  10. usługi systemd i slice'y (ADR 0025), kopie co noc z retencją 7 dni;
  11. zapora `ufw`: port graczy i paczki, port SSH;
  12. autotest serwera;
  13. silnik z `hot-host`, potem drugi autotest, już z `hot-core`.
- Dwa kroki czekają na właściciela i kończą skrypt kodem 3 z poleceniem do powtórki: pliki gry (krok 7, tylko z kupionego Minecrafta) i zgoda `--trust-native-code` na budowę silnika z kodem natywnym (krok 13). Instalator nie loguje się do konta Microsoft i nie pobiera plików Mojanga.
- Błędy idą do `/srv/horizon/logs/install.jsonl` w formacie D27, kody `HOT-E01xx`.
- `--uninstall` bez `--yes` pokazuje plan, z `--yes` usuwa horizon-hot i Horizona; świat, kopie, logi i użytkownik `horizon` zostają.
- Skrót dla operatora po instalacji: `docs/operator.md`.
- Różnice względem planu: logowanie Microsoft na serwerze odpadło (pliki gry przenosi się z komputera właściciela), a pierwszy build silnika nie idzie w tle, tylko jest osobnym krokiem za zgodą.

## Konsekwencje

- Właściciel musi raz przygotować pliki gry u siebie i skopiować folder na serwer.
- Kontener (ADR 0011) to druga droga: ten sam układ, ale w obrazie Dockera.
- Nie sprawdzone (brak maszyny x86_64): kroki 4, 7, 9 i 13 w całości (M6.1, M0.10). Kroki 1–3, 5, 6, 8, 10–12 sprawdzono w maszynie wirtualnej aarch64 z `--test-no-horizon`.
