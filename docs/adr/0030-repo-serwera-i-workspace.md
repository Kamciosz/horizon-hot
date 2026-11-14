# ADR 0030 — układ repo serwera i repo horizon-hot (D22, D23)

- **Data:** 2026-11-14 (decyzje z 2026-11-12, przeniesione z `PLAN.md` w stanie zgodnym z kodem)
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Są dwa repozytoria. Repo serwera należy do właściciela serwera i opisuje, co ma być na serwerze; zmienia się commitami. Repo horizon-hot to narzędzia, które to wykonują. `PLAN.md` opisał oba układy przed budową; kilka elementów wyszło inaczej.

## Decyzja

**D22. Repo serwera** (wzór i opis: `templates/server-repo/`):

```
mods/<id>/Cargo.toml, src/   mod z kodem (Rust, hot-sdk); buduje go CI
mods/<id>/data/              paczka danych moda (ADR 0020)
mods/<id>/assets/            tekstury i nazwy bloków w slotach (ADR 0021)
.github/workflows/hot.yml    kopia z /srv/horizon/server-repo-files/ (instalator)
```

- Brak `mods/` oznacza czysty serwer bez modów.
- `<id>`: małe litery, cyfry, `_` i `-`, do 64 znaków.
- Prywatne repo czyta klucz tylko do odczytu, który tworzy instalator (`/srv/horizon/.ssh/deploy.pub`).
- CI podpisuje wyniki kluczem z sekretu `HOT_SIGNING_KEY` i publikuje je na gałęzi `hot-builds` (ADR 0008, ADR 0024).
- Różnice względem planu: nie ma pliku `mod.toml` (opis moda jest w `hot_mod!` w kodzie), a `hot.toml` i `native/` serwer tylko odnotowuje (ADR 0026).

**D23. Repo horizon-hot** (workspace Cargo):

| Ścieżka | Co to jest |
|---|---|
| `crates/hot-abi` | ABI między `hot-host` a `hot-core` (ADR 0006, ADR 0019) |
| `crates/hot-diag` | logi D27, katalog błędów, raport (ADR 0032) |
| `crates/hot-sdk` | biblioteka dla autorów modów, makro `hot_mod!` |
| `crates/hot-core` | warstwa gorąca w silniku (ADR 0017) |
| `crates/hot-host` | paczka mixinów do silnika Horizona (`horizon-mixin.toml`) i jej API |
| `crates/hotd` | demon serwera: git, CI, dane, sloty, paczka zasobów (`hotd serve-pack`), aktualizacje, raport |
| `crates/hot-proxy` | proxy z kontrolą zakupu (ADR 0022) |
| `crates/hot-play` | aplikacja gracza (ADR 0015) |
| `examples/` | przykładowe mody: `licznik`, `sklep`, `teleport-blok` |
| `templates/server-repo/` | wzór repo serwera |
| `scripts/` | instalator serwera, instalatory `hot-play`, budowa i publikacja w CI |

- Różnice względem planu: nie ma osobnych `crates/hot-pack`, `bundle/hot-host` ani `content/hot-slots`. Paczkę zasobów buduje i serwuje `hotd`, mixiny są w `crates/hot-host`, a pulę slotów `hot_slots.hmod` instalator buduje z `hotd slots-source` (ADR 0021).
- Commity: Conventional Commits, temat w formie bezosobowej, hook w repo to sprawdza.

## Konsekwencje

- Autor modów potrzebuje tylko repo serwera i `hot-sdk`; reszty horizon-hot nie dotyka.
- Nowy składnik to nowy katalog w `crates/` z komentarzem nagłówkowym (do czego służy, kto go używa) i wpisem w mapie w `AGENTS.md`.
