#!/bin/bash
# PL: Czeka, aż klient będzie w grze, robi N podmian i wypisuje pauzę oraz ekran po każdej.
# EN: Waits for the client to be in game, does N swaps, prints the pause and the screen after each.
source "$(dirname "$0")/env.sh"
screen() { "$HERE/ctl.sh" status 2>/dev/null | python3 -c 'import sys,json;d=json.load(sys.stdin);print(d.get("screen"),d.get("chunks"))' 2>/dev/null; }
for _ in $(seq 1 60); do s=$(screen); [[ "$s" == playing* && "$s" != "playing 0" ]] && break; sleep 1; done
echo "start: $s"
for k in $(seq 1 "${1:-6}"); do
  r=$("$HERE/doswap.sh" | grep -o "accepted new position: [0-9]* ms")
  sleep 4
  echo "swap $k: $r [$(screen)]"
done
echo "events: $("$HERE/ctl.sh" events)"
