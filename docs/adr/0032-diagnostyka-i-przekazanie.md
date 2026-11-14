# ADR 0032 — logi, raport i dokumentacja dla osoby spoza projektu (D27, D28, D29)

- **Data:** 2026-11-14 (decyzje z 2026-11-12, przeniesione z `PLAN.md` w stanie zgodnym z kodem)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Projekt będą prowadzić i naprawiać ludzie i agenci bez historii rozmów. Błąd ma się dać naprawić z samego logu, a cały projekt ma się tłumaczyć bez `PLAN.md`.

## Decyzja

**D27. Logi.**

- Każdy składnik loguje przez `crates/hot-diag`; instalator pisze ten sam format sam (`scripts/server-install.sh`, funkcja `diag`).
- Format: JSON Lines, klucze i treść po angielsku. Wpis o błędzie ma pola `code`, `component`, `version`, `what`, `expected`, `context`, `cause`, `fix`, `doc`, `source`, `chain`, `state`. Bez `code`, `what` albo `fix` wpis się nie kompiluje (`DiagError::new`).
- `source` to ścieżka w repo z ukośnikami `/` na każdym systemie.
- Pliki: `<HOT_HOME>/logs/<składnik>.jsonl` (na serwerze `/srv/horizon/server/logs/`), instalator `/srv/horizon/logs/install.jsonl`, kontener `/srv/horizon/logs/container.jsonl`, u gracza katalog danych `hot-play`. Rotacja 50 MB × 5 plików.
- Clippy: `unwrap_used` i `expect_used` to błąd.
- Katalog błędów `docs/bledy/HOT-EXXXX.md`. Test `crates/hot-diag/tests/catalog.rs` sprawdza w obie strony: każdy kod w kodzie ma wpis, każdy wpis jest użyty i ma wszystkie sekcje.
- Zakresy kodów w użyciu: `E00` diagnostyka, `E01` instalacja, `E02` git, CI, dane, sloty, paczka i świat, `E03` budowa silnika, `E04` mody gorące, `E06` proxy i podmiana, `E07` aktualizacje, `E09` aplikacja gracza. Zakresy `E05`, `E08` i `E10` z planu są wolne: błędy danych, paczki i świata dostały kody `E02`, a hamulec MSPT nie zgłasza błędów.

**D28. Raport.**

- Na serwerze `hotd report --home /srv/horizon/server [--out PLIK]`, u gracza „Napraw” w `hot-play`, bez instalacji: `hot-diag report`. Jeden plik Markdown poniżej 200 KB.
- Części: czym jest system, wersje, ostatnie błędy z treścią wpisów z `docs/bledy/`, `hot status` z końcówkami logów i konfiguracją, instrukcja dla modelu.
- Maskowane: `forwarding.secret`, klucze prywatne, adresy IPv4 graczy, u gracza katalog domowy i nazwa konta. Testy z podłożonymi sekretami sprawdzono mutacją. Adresy IPv6 nie są maskowane.

**D29. Projekt tłumaczy się sam.**

- Decyzje: `docs/adr/` z indeksem D1–D29 w `docs/adr/README.md`; test sprawdza, że każdy ADR ma wymagane sekcje i że każde odwołanie `D<n>` ma swój ADR.
- Działanie: `docs/ARCHITECTURE.md`. Obsługa: `docs/operator.md`. Błędy: `docs/bledy/`. Typowe prace: `docs/przewodniki/`.
- `AGENTS.md`: czym jest projekt, słownik, mapa repo, komendy, zasady nienaruszalne.
- Każdy crate ma na górze komentarz: do czego służy, kto go używa, z czym rozmawia.
- Zmieniona decyzja to nowy ADR, a stary dostaje „zastąpiony przez”. Dokumentacja zmienia się w tym samym commicie co kod.

## Konsekwencje

- Komunikaty dla gracza są po polsku i osobno od logów.
- Nie sprawdzone: test „nowej osoby w zespole” (M12.2) wymaga świeżego agenta bez historii.
