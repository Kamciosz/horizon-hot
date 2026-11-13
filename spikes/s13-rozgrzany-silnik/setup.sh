#!/bin/bash
# PL: Stanowisko S13. Dwa serwery, a (port 25613) i b (25566), na wspólnym świecie $H/shared/world,
#     oba z silnikiem z hot-host (mixin wait_for_world) i z tą samą pulą slotów, także u klienta.
#     Uruchamia b i proxy S8 z --warm --flush. Potem: MCP `play --server 127.0.0.1:25700`,
#     swaps.sh z s7-rozmiar-puli (ten sam silnik) i cold-swaps.sh (nowy plik silnika).
# EN: S13 stand: servers a and b on one shared world, both on the hot-host engine and the same
#     slot pool (the client too). Starts b and the S8 proxy with --warm --flush.
# Użycie / usage: H=<nowy katalog> setup.sh <silnik z hot-host> <pula .hmod> [zasoby gry]
set -euo pipefail
source "$(dirname "$0")/../s7-rozmiar-puli/env.sh"
engine=$1; hmod=$2; res=${3:-}
[ ! -e "$H/shared" ] || { echo "$H już jest stanowiskiem; podaj nowy katalog H" >&2; exit 2; }
mkdir -p "$H/shared/world"
for s in a b; do
  port=25613; [ "$s" = b ] && port=25566
  mkdir -p "$H/$s"
  "$SB" --dir "$H/$s" init --eula --offline --seed 7 --port "$port" --no-whitelist --yes >/dev/null
  "$SB" --dir "$H/$s" config set "engine.path=$engine" "engine.resources=$res" "panel.bind=" >/dev/null
  # PL: Świat tworzy pierwszy start w pustym katalogu; dowiązanie do nieistniejącego daje „File exists”.
  # EN: The first start fills the empty folder; a link to a missing one fails with "File exists".
  ln -s ../shared/world "$H/$s/world"
  mkdir -p "$H/$s/horizon-mods"; cp "$hmod" "$H/$s/horizon-mods/hot_slots.hmod"
  echo "mod.hot_slots:true" > "$H/$s/horizon-mods.txt"
done
cp "$hmod" "$INST/horizon-mods/hot_slots.hmod"
"$SB" --dir "$H/b" start >/dev/null
rm -f "$H/swap"
nohup nice "$HERE/../s8-podmiana/proxy/target/release/s8-proxy" --listen 127.0.0.1:25700 --server-bin "$SB" \
  --a "25566:$H/b" --b "25613:$H/a" --swap-file "$H/swap" --warm --flush 2> "$H/proxy.log" >/dev/null &
sleep 1; echo "gotowe: b działa, proxy na 127.0.0.1:25700"
