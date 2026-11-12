#!/bin/bash
# PL: Wariant puli na dwóch serwerach z S12 ($H/s12/a, $H/s12/b) i w instancji klienta;
#     rozgrzewa cache B, startuje A i proxy S8. Potem: MCP `play --server 127.0.0.1:25700`, swaps.sh.
# EN: Pool variant on the two S12 servers and the client instance; warms B's cache,
#     starts A and the S8 proxy. Then: MCP `play --server 127.0.0.1:25700`, swaps.sh.
# Użycie / usage: variant.sh <nazwa> <bloki> <przedmioty>
set -e
source "$(dirname "$0")/env.sh"
name=$1; blocks=$2; items=$3
cd "$H"
[ -d "$name" ] || python3 "$HERE/gen.py" "$name" "$blocks" "$items"
(cd "$name" && cargo horizon-mod build >/dev/null)
hmod="$H/$name/target/horizon-mod/hot_slots.hmod"
for s in a b; do
  mkdir -p "$H/s12/$s/horizon-mods"; cp "$hmod" "$H/s12/$s/horizon-mods/"
  echo "mod.hot_slots:true" > "$H/s12/$s/horizon-mods.txt"
done
cp "$hmod" "$INST/horizon-mods/hot_slots.hmod"
# PL: B musi zbudować cache treści, zanim A zajmie świat (S12: dwa silniki nie otworzą jednego świata).
# EN: B builds its content cache before A takes the world (S12: two engines cannot open one world).
"$SB" --dir "$H/s12/b" start >/dev/null 2>&1; "$SB" --dir "$H/s12/b" stop >/dev/null 2>&1
"$SB" --dir "$H/s12/a" start >/dev/null 2>&1
rm -f "$H/swap"
nohup "$HERE/../s8-podmiana/proxy/target/release/s8-proxy" --listen 127.0.0.1:25700 --server-bin "$SB" \
  --a "25613:$H/s12/a" --b "25566:$H/s12/b" --swap-file "$H/swap" 2> "$H/proxy-$name.log" >/dev/null &
ln -sf "proxy-$name.log" "$H/proxy.log"
sleep 1; echo "ready $name"
