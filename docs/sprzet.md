# Pomiary sprzętowe

Liczby, od których zależą decyzje w `docs/adr/` (np. ADR 0022, ADR 0025). Każdy wpis mówi, na jakiej maszynie i jakim świecie go zmierzono. Docelowy serwer to ThinkCentre (Ubuntu, x86_64); dopóki go nie ma, pomiary są z Maca dewelopera i tak są oznaczone.

## `reload` paczek danych (M3, D7)

**Co blokuje świat.** `reload` buduje od nowa wszystkie tabele treści świata (przepisy, łupy, tagi, osiągnięcia, funkcje) w wątku serwera (`Game::reload_world_tables` w Horizonie). Przez ten czas żaden tick się nie wykonuje. Horizon nie ma przeładowania jednej paczki: każda zmiana w `data/` dowolnego moda kosztuje pełny `reload`. Dlatego `hotd` wysyła jedno `reload` na commit i żadnego, gdy `data/` się nie zmieniło.

**Jak mierzone.** Czas wywołania `horizon-server cmd reload` (zawiera narzut samego wywołania; tania komenda `list` trwa 0,08–0,19 s) i ostrzeżenie „Can't keep up! … Running N ms behind” w `logs/latest.log`. Raport `horizon-server report` nie pomaga: nie liczy czasu komend konsoli w czasie ticku.

**Mac dewelopera** (Apple Silicon, 12 rdzeni, 24 GB; Horizon `nightly-2026-11-05-66f3981`; świat testowy bez graczy, 25 chunków, jedna paczka `hot-*`; 12.11.2026):

| Warunki | `cmd reload` | Uwagi |
|---|---|---|
| Mac mało obciążony (load 3–4, 59% wolnej pamięci) | 0,23–0,40 s, raz 1,0 s | po 40–45 s bezczynności i od razu po sobie |
| Mac mocno obciążony (load 5,5–6,5, 2,5 GB w swapie) | 0,68–0,75 s | kolejne `reload` po sobie |
| to samo, pierwszy `reload` po 30 s – 6 min bezczynności | 3,9–8,2 s | „Can't keep up” 4,1–8,2 s; tick stoi |

Przyczyna wolnych przypadków nie jest ustalona. Obalone: pauza pustego serwera (bez pauzy, `pause-when-empty-seconds = 0`, wynik ten sam) i zimna pamięć podręczna plików gry (odczyt całego `data/` z zasobów tuż przed `reload` nic nie zmienił). Wolne przypadki wystąpiły tylko przy dużym obciążeniu Maca; przy mniejszym tej samej próby nie dało się powtórzyć.

**Z graczem online** (ten sam Mac, klient Horizon na tym samym komputerze, serwer bez pauzy): `hotd` zmierzył `reload` + `datapack list` na 0,25–0,55 s, bez „Can't keep up”.

**ThinkCentre:** nie sprawdzone: brak maszyny. Do zmierzenia tym samym sposobem z graczem na serwerze; jeśli `reload` przekracza 1 s, wrócić do tego wpisu przed M7.
