# ADR 0003 — `serde_json` w `hot-diag` (D27)

- **Data:** 2026-11-12
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

`hot-diag` zapisuje logi jako JSON Lines (D27), a `hot report` (D28) czyta je z powrotem, żeby wybrać ostatnie błędy. Biblioteka standardowa Rusta nie ma JSON-a. Zasada repo: nowa zależność wymaga ADR.

## Rozważane opcje

1. **`serde_json`** — standard w ekosystemie Rusta. Horizon też go używa (w `Cargo.lock` wersji `66f3981` jest 1.0.151). Daje zapis z poprawnym escapowaniem i odczyt do `Value` bez makr `derive`.
2. **Własny zapis i odczyt JSON** — bez zależności, ale escapowanie i parser to własny kod do utrzymania. Błąd w nim psuje każdy wpis w logu.
3. **Inny format niż JSON** — sprzeczny z D27. Logi mają być czytelne dla dowolnego narzędzia i modelu.

## Decyzja

Wybieramy **opcję 1**: `serde_json` bez dodatkowych funkcji, deklarowany w `[workspace.dependencies]`. Wersja minimalna 1.0.151, ta sama co w Horizonie.

`hot-diag` nie trafia do paczki `hot-host` (D2), więc ograniczenie paczek modów natywnych (zależności tylko z `Cargo.lock` Horizona, S1) go nie dotyczy.

## Konsekwencje

- Pola wpisu w JSON-ie są posortowane alfabetycznie (`serde_json::Map` bez `preserve_order`). Kolejność nie ma znaczenia dla czytelnika ani dla parsera, więc zostaje domyślna.
- Kolejne składniki, które czytają albo piszą JSON, biorą tę samą zależność z workspace.
