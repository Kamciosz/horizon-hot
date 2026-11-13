#!/bin/bash
# PL: N podmian na nowy plik silnika z pustym cache (najgorszy przypadek). Przed każdą: kopia
#     silnika pod nową ścieżką (nadpisanie pliku, z którego działa proces, zabija następny start:
#     macOS kod 137), engine.path celu na tę kopię, usunięty .horizon-cache celu. Celem jest
#     zawsze strona, która nie działa, więc nieudana podmiana nie psuje kolejnych.
# EN: N swaps onto a new engine file with an empty cache (worst case); the target is always
#     the side that is not running.
# Użycie / usage: H=<stanowisko z setup.sh> cold-swaps.sh <silnik z hot-host> [N=6]
set -euo pipefail
source "$(dirname "$0")/../s7-rozmiar-puli/env.sh"
engine=$1
results() { grep -c "SWAP RESULT\|swap FAILED" "$H/proxy.log" || true; }
for k in $(seq 1 "${2:-6}"); do
  if "$SB" --dir "$H/a" status 2>/dev/null | grep -q "^Running"; then t=b; else t=a; fi
  e="$H/engines/$(date +%Y%m%d-%H%M%S)-$k"; mkdir -p "$e"; cp "$engine" "$e/horizon-engine"
  "$SB" --dir "$H/$t" config set "engine.path=$e/horizon-engine" >/dev/null
  rm -rf "$H/$t/.horizon-cache"
  n=$(results); touch "$H/swap"
  # PL: Zimne rozgrzanie trwa kilka sekund (przed zamrożeniem), więc czekam do 2 min.
  # EN: A cold warm-up takes seconds (before the freeze), so wait up to 2 min.
  for _ in $(seq 1 1200); do [ "$(results)" -gt "$n" ] && break; sleep 0.1; done
  r=$(grep "SWAP RESULT\|swap FAILED" "$H/proxy.log" | tail -1 | sed 's/.*SWAP RESULT: swap start -> client //')
  w=$(grep -o "waits for the world after [0-9]* ms" "$H/proxy.log" | tail -1)
  sleep 4
  echo "nowy silnik $k -> $t: $r; rozgrzanie: $w; cache: $(cd "$H/$t/.horizon-cache" 2>/dev/null && echo *)"
done
