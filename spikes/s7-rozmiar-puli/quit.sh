#!/bin/bash
# PL: Zamyka klienta i czeka, aż jego proces zniknie (silniki serwera mają w linii `engine.json`).
# EN: Quits the client and waits for its process to exit (server engines carry `engine.json`).
source "$(dirname "$0")/env.sh"
"$HERE/ctl.sh" quit >/dev/null 2>&1
for _ in $(seq 1 150); do
  pgrep -fl horizon-client | grep -v engine.json >/dev/null || { echo "client gone"; exit 0; }
  sleep 0.2
done
echo "client still running"; exit 1
