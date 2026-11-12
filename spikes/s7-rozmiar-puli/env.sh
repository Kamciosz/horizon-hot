# PL: Wspólne ścieżki skryptów S7. H = katalog roboczy próby (poza repo), INST = instancja klienta.
# EN: Shared paths for the S7 scripts. H = spike work dir (outside the repo), INST = client instance.
: "${H:?ustaw H na katalog roboczy, np. /tmp/hot-spike}"
: "${INST:=$HOME/Library/Application Support/horizon/instances/hot-spike}"
HZ="$HOME/Library/Application Support/horizon-launcher"
SB="$HZ/versions/nightly-2026-11-05-66f3981/horizon-server"
CTL=("$HZ/versions/nightly-2026-11-05-66f3981/horizon-ctl" --socket "$HZ/game-control.sock")
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
