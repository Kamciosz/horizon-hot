#!/usr/bin/env bash
# PL: Budowa modów gorących repo serwera (D14, ADR 0008). Uruchamia go workflow hot.yml w repo
# serwera; da się go też odpalić ręcznie. Dla każdego mods/<id>/Cargo.toml buduje bibliotekę
# cdylib, kopiuje ją jako <wyjście>/<sha>/<platforma>/<id>.<so|dylib>, podpisuje kluczem
# z HOT_SIGNING_KEY_FILE (ssh-keygen -Y sign) i zapisuje <wyjście>/<sha>/build.json, który czyta
# hotd. Gdy coś się nie zbuduje, build.json ma "ok": false, nazwę moda i koniec logu kompilacji,
# a skrypt kończy się kodem 1 (czerwone CI).
# EN: Builds the hot mods of a server repository (D14, ADR 0008). Run by the hot.yml workflow in
# the server repository, or by hand. For each mods/<id>/Cargo.toml it builds the cdylib, copies it
# to <out>/<sha>/<platform>/<id>.<so|dylib>, signs it with the key in HOT_SIGNING_KEY_FILE
# (ssh-keygen -Y sign) and writes <out>/<sha>/build.json for hotd. When something fails to build,
# build.json has "ok": false, the mod name and the tail of the build log, and the script exits 1.
#
# Użycie / usage: hot-build.sh <repo-serwera> <katalog-wyjścia>
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: hot-build.sh <server-repo> <out-dir>" >&2
  exit 2
fi
repo=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
sha=$(git -C "$repo" rev-parse HEAD)

# PL: Nazwa platformy taka jak w hotd: std::env::consts::{ARCH}-{OS}.
# EN: The platform name as in hotd: std::env::consts::{ARCH}-{OS}.
arch=$(uname -m)
case "$arch" in arm64) arch=aarch64 ;; esac
case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) echo "hot-build: unsupported system $(uname -s)" >&2; exit 1 ;;
esac
target="$arch-$os"

dir="$out/$sha"
dest="$dir/$target"
log="$dir/build.log"
mkdir -p "$dest"
: > "$log"
files='{}'

fail() {
  # $1 = mod, $2 = jedno zdanie do logu / one sentence for the log
  echo "$2" >> "$log"
  jq -n --arg sha "$sha" --arg mod "$1" --arg target "$target" \
        --arg log "$(tail -n 80 "$log")" \
        '{commit: $sha, ok: false, failed: $mod, target: $target, log: $log}' > "$dir/build.json"
  echo "hot-build: $2" >&2
  exit 1
}

if [ -z "${HOT_SIGNING_KEY_FILE:-}" ] || [ ! -s "$HOT_SIGNING_KEY_FILE" ]; then
  fail "(signing)" "HOT_SIGNING_KEY_FILE is empty: add the HOT_SIGNING_KEY secret to the repository (the installer prints it)"
fi

for manifest in "$repo"/mods/*/Cargo.toml; do
  [ -e "$manifest" ] || continue
  id=$(basename "$(dirname "$manifest")")
  if ! printf '%s' "$id" | grep -Eq '^[a-z0-9_-]{1,64}$'; then
    fail "$id" "mods/$id: a mod folder name may only use a-z, 0-9, _ and - (1-64 characters)"
  fi
  echo "== mods/$id" >> "$log"
  messages="$dir/$id.messages.json"
  if ! cargo build --release --manifest-path "$manifest" --target-dir "$repo/target" \
        --message-format=json-render-diagnostics > "$messages" 2>> "$log"; then
    fail "$id" "mods/$id does not compile"
  fi
  lib=$(jq -r 'select(.reason == "compiler-artifact" and (.target.kind | index("cdylib"))) | .filenames[]' "$messages" \
        | grep -E '\.(so|dylib)$' | tail -n 1 || true)
  rm -f "$messages"
  if [ -z "$lib" ]; then
    fail "$id" "mods/$id builds no cdylib: add [lib] crate-type = [\"cdylib\"] to mods/$id/Cargo.toml"
  fi
  name="$id.${lib##*.}"
  cp "$lib" "$dest/$name"
  ssh-keygen -q -Y sign -f "$HOT_SIGNING_KEY_FILE" -n horizon-hot-mod "$dest/$name" 2>> "$log" \
    || fail "$id" "ssh-keygen cannot sign $name: check that HOT_SIGNING_KEY holds an OpenSSH private key"
  files=$(jq -c --arg id "$id" --arg name "$name" '. + {($id): $name}' <<< "$files")
done

jq -n --arg sha "$sha" --arg target "$target" --argjson files "$files" \
  '{commit: $sha, ok: true, targets: {($target): $files}}' > "$dir/build.json"
echo "hot-build: $sha built for $target: $(jq -r 'keys | join(", ")' <<< "$files")"
