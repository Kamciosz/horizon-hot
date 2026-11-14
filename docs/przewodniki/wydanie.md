# Wydanie wersji

Projekt nie ma numerowanych wydań. Wersją jest commit na gałęzi `main`. Co się dzieje po scaleniu do `main`:

| Co | Jak dochodzi do ludzi | Szczegóły |
|---|---|---|
| `hot-play` (aplikacja gracza) | CI publikuje pliki w wydaniu `hot-play` na GitHubie (publiczne), a `hot-play` u graczy aktualizuje się przy najbliższym starcie | ADR 0016, `.github/workflows/hot-play.yml` |
| serwer (`hotd`, `hot-core`, `hot-proxy`, `hot-diag`) | serwery z `--hot-ref main` same pobierają nowy commit raz na dobę | ADR 0014, ADR 0028, `docs/aktualizacje.md` |
| `hot.yml` w repo serwera | CI repo serwera bierze `.hot` z commita przypiętego przy instalacji; nowszy wymaga ponownego uruchomienia instalatora i skopiowania pliku | `templates/server-repo/.github/workflows/hot.yml` |
| obraz Dockera | nie aktualizuje się sam; operator buduje go od nowa | `docs/docker.md` |

## Przed scaleniem

1. Zielone CI na gałęzi: `ci.yml` (format commitów, `fmt`, `clippy`, testy) i `hot-play.yml` (cztery systemy).
2. Wpisy z `docs/zmiany/*.md` przenieś do `CHANGELOG.md`, do sekcji z datą, a pliki z `docs/zmiany/` usuń. To robi tylko właściciel (`CONTRIBUTING.md`).
3. Zmiana w `hot-host`, `hot-abi`, instalatorze albo puli slotów nie wejdzie na serwery sama: serwer czeka na właściciela (ADR 0014). Napisz w opisie zmiany, co operator ma zrobić.

## Scalenie

Scalenie do `main` jest publicznym wydaniem `hot-play`, więc decyduje o nim właściciel. Agent nie scala, nie tworzy tagów ani wydań ręcznie.

## Po scaleniu

- `gh run list --branch main` — oba przebiegi zielone.
- `gh release view hot-play` — nowe pliki i `SHA256SUMS`.
- Na serwerze po dobie: `hotd status` pokazuje nowy commit horizon-hot albo powód czekania.
