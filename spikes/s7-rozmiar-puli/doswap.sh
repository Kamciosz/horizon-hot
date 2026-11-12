#!/bin/bash
# PL: Wyzwala jedną podmianę w proxy S8 i wypisuje jej linię SWAP RESULT (log: $H/proxy.log).
# EN: Triggers one swap in the S8 proxy and prints its SWAP RESULT line (log: $H/proxy.log).
source "$(dirname "$0")/env.sh"
count() { grep -c "SWAP RESULT\|swap FAILED" "$H/proxy.log"; }
n=$(count)
touch "$H/swap"
for _ in $(seq 1 200); do [ "$(count)" -gt "$n" ] && break; sleep 0.05; done
grep "SWAP RESULT\|swap FAILED" "$H/proxy.log" | tail -1 | sed 's/.*SWAP RESULT: //'
