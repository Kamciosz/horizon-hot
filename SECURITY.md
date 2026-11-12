# Bezpieczeństwo

## Jak zgłosić lukę

Nie zakładaj publicznego zgłoszenia. Użyj prywatnego zgłoszenia na GitHubie:

1. Otwórz zakładkę **Security** w repozytorium.
2. Wybierz **Report a vulnerability**.
3. Opisz, co się dzieje, jak to powtórzyć i co jest zagrożone.

Odpowiadamy w ciągu 7 dni.

## Co obejmuje

- serwer i jego składniki (`hotd`, `hot-core`, `hot-proxy`, `hot-pack`),
- instalator serwera i aplikację gracza (`hot-play`, skrypty instalacyjne),
- ładowanie modów: podpisy artefaktów z CI, klucze, `forwarding.secret`.

Mody gorące to natywny kod z pełnym dostępem do serwera. Ochroną jest podpis artefaktu z CI i dostęp do repo serwera. Kto może pushować do repo serwera, ten może uruchomić kod na serwerze.

## Których wersji pilnujemy

Tylko najnowszej wersji z gałęzi `main`.
