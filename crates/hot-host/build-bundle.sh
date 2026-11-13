#!/bin/sh
# PL: Składa paczkę `hot-host` do `native-build`: ten katalog + kopia `crates/hot-abi` jako `hot_abi`.
#     Użycie: crates/hot-host/build-bundle.sh <katalog-wyjściowy>
#     Potem:  horizon-launcher native-build <katalog-wyjściowy> --server-output <silnik> \
#               --version <Nightly> --trust-native-code
# EN: Assembles the `hot-host` bundle for `native-build`: this folder + a copy of `crates/hot-abi`
#     as `hot_abi`. Usage: crates/hot-host/build-bundle.sh <output-dir>
set -eu
out=${1:?"Usage: build-bundle.sh <output-dir>"}
here=$(cd "$(dirname "$0")" && pwd)
rm -rf "$out"
mkdir -p "$out/api" "$out/hot_abi"
cp "$here/horizon-mixin.toml" "$out/"
cp "$here/wait_for_world.rs" "$out/"
cp "$here/api/Cargo.toml" "$out/api/"
cp -R "$here/api/src" "$out/api/"
cp "$here/../hot-abi/Cargo.toml" "$out/hot_abi/"
cp -R "$here/../hot-abi/src" "$out/hot_abi/"
echo "hot-host bundle: $out"
