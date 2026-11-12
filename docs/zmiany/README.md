# Wpisy do changelogu z pull requestów

Każdy pull request dodaje jeden plik `docs/zmiany/<gałąź>.md`, np. `docs/zmiany/feat-hot-core-generacje.md` dla gałęzi `feat/hot-core-generacje`. `CHANGELOG.md` zmienia tylko właściciel przy wydaniu: przenosi wpisy i usuwa pliki. Dzięki temu kilka osób albo agentów pracuje równolegle bez konfliktów w jednym pliku.

## Wzór

```markdown
## Changelog

### Dodane
- Co doszło, z nazwą składnika i pliku.

### Zmienione
- Co działa inaczej i dlaczego.

### Usunięte
- Co zniknęło.

### Uwagi
- Czego zmiana świadomie nie robi, znane luki, rzeczy do osobnego zadania.
```

Puste sekcje pomijamy.
