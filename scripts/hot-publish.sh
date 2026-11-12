#!/usr/bin/env bash
# PL: Wypycha wynik hot-build.sh (<wyjście>/<sha>/) na gałąź hot-builds repo serwera (ADR 0008).
# Działa w klonie repo serwera, więc używa jego dostępu (w Actions: GITHUB_TOKEN
# z contents: write). Trzyma katalogi tylko dla ostatnich 50 commitów gałęzi serwera.
# EN: Pushes the hot-build.sh result (<out>/<sha>/) to the hot-builds branch of the server
# repository (ADR 0008). Runs inside a clone of the server repository, so it uses that clone's
# access (in Actions: GITHUB_TOKEN with contents: write). Keeps folders only for the last 50
# commits of the server branch.
#
# Użycie / usage: hot-publish.sh <klon-repo-serwera> <katalog-wyjścia>
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: hot-publish.sh <server-repo-clone> <out-dir>" >&2
  exit 2
fi
repo=$(cd "$1" && pwd)
out=$(cd "$2" && pwd)
sha=$(git -C "$repo" rev-parse HEAD)
if [ ! -f "$out/$sha/build.json" ]; then
  echo "hot-publish: $out/$sha/build.json is missing; hot-build.sh did not run" >&2
  exit 1
fi
rm -f "$out/$sha/build.log"

pub=$(mktemp -d)
trap 'git -C "$repo" worktree remove --force "$pub" 2>/dev/null || rm -rf "$pub"' EXIT
gitc() { git -C "$pub" -c user.name="hot-ci" -c user.email="hot-ci@users.noreply.github.com" -c commit.gpgsign=false "$@"; }

for attempt in 1 2 3; do
  rm -rf "$pub"
  git -C "$repo" worktree prune
  if git -C "$repo" fetch --quiet origin "+refs/heads/hot-builds:refs/remotes/origin/hot-builds" 2>/dev/null; then
    git -C "$repo" worktree add --quiet --detach "$pub" origin/hot-builds
    gitc checkout --quiet -B hot-builds
  else
    git -C "$repo" worktree add --quiet --detach "$pub" HEAD
    gitc checkout --quiet --orphan hot-builds
    gitc rm -r --quiet --cached . > /dev/null
    find "$pub" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
  fi
  rm -rf "${pub:?}/$sha"
  cp -R "$out/$sha" "$pub/$sha"
  # PL: Retencja: zostają katalogi commitów z ostatnich 50 na gałęzi serwera.
  # EN: Retention: folders of the last 50 commits on the server branch stay.
  keep=$(git -C "$repo" rev-list --max-count=50 HEAD)
  for d in "$pub"/*/; do
    d=$(basename "$d")
    printf '%s\n' "$keep" | grep -qx "$d" || rm -rf "${pub:?}/$d"
  done
  gitc add -A
  gitc commit --quiet -m "build $sha" || true
  if gitc push --quiet origin hot-builds; then
    echo "hot-publish: $sha pushed to hot-builds"
    exit 0
  fi
  echo "hot-publish: push rejected (attempt $attempt), retrying" >&2
  git -C "$repo" worktree remove --force "$pub"
  pub=$(mktemp -d)
done
echo "hot-publish: cannot push to hot-builds after 3 attempts" >&2
exit 1
