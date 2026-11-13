#!/usr/bin/env bash
# PL: Instalator serwera horizon-hot na Ubuntu (D25, M6.1). Uruchamiany jako root, jednym poleceniem:
#       curl -fsSL https://raw.githubusercontent.com/Kamciosz/horizon-hot/main/scripts/server-install.sh \
#         | sudo bash -s -- --repo <adres repo serwera> --eula
#     Kolejne uruchomienie niczego nie psuje: każdy krok sprawdza stan maszyny i robi tylko to,
#     czego brakuje. Kluczy, które już są, nigdy nie generuje od nowa (zerwałoby to podpisy CI).
#     Dwa kroki czekają na operatora i kończą skrypt kodem 3 z poleceniem do powtórki:
#     pliki gry (z jego kupionego Minecrafta) i budowa silnika z hot-host (--trust-native-code).
#     Błędy idą do /srv/horizon/logs/install.jsonl w formacie D27, opisy w docs/bledy/HOT-E01xx.md.
# EN: horizon-hot server installer for Ubuntu (D25, M6.1). Runs as root, in one command (above).
#     Running it again breaks nothing: each step checks the machine and does only what is missing.
#     Existing keys are never regenerated (that would break CI signatures). Two steps wait for the
#     operator and end the script with exit code 3 and the command to rerun: the game files (from
#     their own purchased Minecraft) and building the engine with hot-host (--trust-native-code).
#     Errors go to /srv/horizon/logs/install.jsonl in the D27 format, described in docs/bledy/.
set -euo pipefail

VERSION=0.1.0
ROOT=/srv/horizon
HZ=$ROOT/horizon            # PL: Horizon zbudowany ze źródeł. EN: Horizon built from source.
HZ_SRC=$ROOT/horizon-src    # PL: źródła Horizona w przypiętej rewizji. EN: Horizon source at the pinned revision.
BIN=$ROOT/bin
SRV=$ROOT/server            # PL: folder serwera = HOT_HOME. EN: the server folder = HOT_HOME.
SRC=$ROOT/src/horizon-hot
KEYS=$ROOT/keys             # PL: tylko root. EN: root only.
LOGS=$ROOT/logs
BACKUPS=$ROOT/backups
ENGINES=$ROOT/engines
SHARE=$ROOT/server-repo-files
LOG=$LOGS/install.jsonl

REPO=
HOT_SOURCE=https://github.com/Kamciosz/horizon-hot.git
HOT_REF=main
HORIZON_VERSION=nightly-2026-11-05-66f3981
HORIZON_REV=66f3981f522b24c250b21523a70080965b940424
RESOURCES=
LAUNCHER=
TRUST_NATIVE=0
EULA=0
PORT=25565
ENGINE_PORT=25566
PACK_PORT=8131
ADDRESS=
OP=
TEST_NO_HORIZON=0
UNINSTALL=0
YES=0

USAGE="Użycie: server-install.sh --repo URL --eula [opcje]
       server-install.sh --uninstall [--yes]

  --repo URL              repo serwera na GitHubie (https://… publiczne, git@github.com:… prywatne)
  --eula                  akceptujesz EULA Minecrafta: https://aka.ms/MinecraftEULA
  --resources DIR         folder 26.3 z plikami gry przygotowanymi z Twojego Minecrafta
  --trust-native-code     przeczytałeś kod crates/hot-host i ufasz mu: buduje silnik z hot-host
  --launcher PATH         horizon-launcher dla Linuxa x86-64 (domyślnie z PATH)
  --op NAME               pierwszy operator; włącza whitelistę z tym graczem
  --port N                port dla graczy: tu słucha hot-proxy (domyślnie 25565)
  --engine-port N         port silnika, tylko na 127.0.0.1 (domyślnie 25566)
  --address HOST          adres, pod którym gracze widzą serwer (IP albo nazwa); z niego
                          gra pobiera paczkę zasobów: http://HOST:8131/pack.zip
  --hot-ref REF           wersja horizon-hot: gałąź, tag albo commit (domyślnie main)
  --hot-source URL        skąd pobrać horizon-hot (domyślnie GitHub)
  --horizon-version TAG   nazwa wersji Horizona (domyślnie $HORIZON_VERSION); podaj razem z --horizon-rev
  --horizon-rev HASH      pełny hash commita Horizona, z którego budujemy serwer (domyślnie $HORIZON_REV)
  --test-no-horizon       TYLKO DO TESTÓW: pomija Horizona (pobranie, pliki gry, świat, silnik),
                          żeby sprawdzić resztę na maszynie bez Linuxa x86-64
  --uninstall             pokazuje, co zostanie usunięte; z --yes usuwa horizon-hot i Horizona,
                          a zostawia świat, kopie, logi i użytkownika horizon"

# --- PL: Dziennik D27 i komunikaty. EN: The D27 log and messages. ---------------------------

say() { printf '%s\n' "$*"; }
step() { printf '\n[%s] %s\n' "$1" "$2"; }

# PL: Tekst jako napis JSON: bez znaków sterujących poza \t i \n.
# EN: Text as a JSON string: no control characters except \t and \n.
json_str() {
  printf '"'
  printf '%s' "$1" | LC_ALL=C tr -d '\000-\010\013-\037' \
    | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/\t/\\t/g' \
    | awk 'NR > 1 { printf "\\n" } { printf "%s", $0 }'
  printf '"'
}

# PL: Wpis D27. Argumenty: kod, what, expected, cause, fix, state, potem pary klucz=wartość do context.
#     Ostatnie linie wyjścia polecenia (zmienna OUTPUT) trafiają do chain.
# EN: A D27 entry. Arguments: code, what, expected, cause, fix, state, then key=value context pairs.
#     The last lines of a command's output (variable OUTPUT) go to chain.
diag() {
  local code=$1 what=$2 expected=$3 cause=$4 fix=$5 state=$6
  shift 6
  local ctx="" pair fn=${FUNCNAME[1]:-main}
  case "$fn" in fail|wait_for_operator) fn=${FUNCNAME[2]:-main} ;; esac
  for pair in "hot_ref=$HOT_REF" "horizon_version=$HORIZON_VERSION" "$@"; do
    ctx+="${ctx:+,}$(json_str "${pair%%=*}"):$(json_str "${pair#*=}")"
  done
  mkdir -p "$LOGS" 2>/dev/null || true
  {
    printf '{"ts":%s,"level":"error","component":"server-install","version":%s,"code":%s,' \
      "$(json_str "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)")" "$(json_str "$VERSION")" "$(json_str "$code")"
    printf '"what":%s,"expected":%s,"cause":%s,"fix":%s,"state":%s,' \
      "$(json_str "$what")" "$(json_str "$expected")" "$(json_str "$cause")" "$(json_str "$fix")" "$(json_str "$state")"
    printf '"doc":%s,"source":%s,"context":{%s},"chain":[%s]}\n' \
      "$(json_str "docs/bledy/$code.md")" "$(json_str "scripts/server-install.sh:$fn")" \
      "$ctx" "$([ -n "${OUTPUT:-}" ] && json_str "$(printf '%s' "$OUTPUT" | tail -n 40)")"
  } >> "$LOG" 2>/dev/null || true
  printf '\nBŁĄD %s: %s\nNaprawa: %s\nOpis: docs/bledy/%s.md, wpis w %s\n' "$code" "$what" "$fix" "$code" "$LOG" >&2
}

fail() { diag "$@"; exit 1; }

# PL: Operator musi coś zrobić; kod 3 odróżnia to od błędu. EN: The operator must act; exit 3.
wait_for_operator() {
  diag "$@"
  printf 'Potem uruchom ponownie (z tymi samymi opcjami i tym, czego brakowało):\n  %s\n' "$RERUN" >&2
  exit 3
}

# PL: Polecenie jako użytkownik horizon, z jego katalogiem domowym. EN: As the horizon user.
as_horizon() { runuser -u horizon -- env HOME="$ROOT" PATH="$ROOT/.cargo/bin:$BIN:/usr/local/bin:/usr/bin:/bin" "$@"; }

hz() { as_horizon "$BIN/horizon-server" --dir "$SRV" "$@"; }

# PL: Zapis pliku tylko przy zmianie treści; zwraca 0, gdy coś zapisał.
# EN: Writes a file only when its content changes; returns 0 when it wrote.
write_if_changed() {
  local path=$1 content=$2
  if [ -f "$path" ] && [ "$(cat "$path")" = "$content" ]; then return 1; fi
  printf '%s\n' "$content" > "$path"
}

# --- PL: Argumenty. EN: Arguments. ----------------------------------------------------------

ARGS=("$@")
while [ $# -gt 0 ]; do
  case "$1" in
    --repo) REPO=${2:?"--repo potrzebuje adresu"}; shift 2 ;;
    --eula) EULA=1; shift ;;
    --resources) RESOURCES=${2:?"--resources potrzebuje folderu"}; shift 2 ;;
    --trust-native-code) TRUST_NATIVE=1; shift ;;
    --launcher) LAUNCHER=${2:?"--launcher potrzebuje ścieżki"}; shift 2 ;;
    --op) OP=${2:?"--op potrzebuje nazwy gracza"}; shift 2 ;;
    --port) PORT=${2:?"--port potrzebuje numeru"}; shift 2 ;;
    --engine-port) ENGINE_PORT=${2:?"--engine-port potrzebuje numeru"}; shift 2 ;;
    --address) ADDRESS=${2:?"--address potrzebuje adresu serwera"}; shift 2 ;;
    --hot-ref) HOT_REF=${2:?"--hot-ref potrzebuje gałęzi, tagu albo commita"}; shift 2 ;;
    --hot-source) HOT_SOURCE=${2:?"--hot-source potrzebuje adresu"}; shift 2 ;;
    --horizon-version) HORIZON_VERSION=${2:?"--horizon-version potrzebuje tagu"}; shift 2 ;;
    --horizon-rev) HORIZON_REV=${2:?"--horizon-rev potrzebuje pełnego hasha"}; shift 2 ;;
    --test-no-horizon) TEST_NO_HORIZON=1; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    --yes) YES=1; shift ;;
    -h|--help) say "$USAGE"; exit 0 ;;
    *) say "Nieznana opcja: $1" >&2; say "$USAGE" >&2; exit 2 ;;
  esac
done
[ -n "$REPO" ] || [ "$UNINSTALL" = 1 ] || { say "Brakuje --repo." >&2; say "$USAGE" >&2; exit 2; }
# PL: Strona `b` silnika (podmiana, D11) słucha na porcie silnika + 1. EN: Engine side `b` uses the engine port + 1.
ENGINE_PORT_B=$((ENGINE_PORT + 1))
[ "$PORT" != "$ENGINE_PORT" ] && [ "$PORT" != "$PACK_PORT" ] && [ "$ENGINE_PORT" != "$PACK_PORT" ] \
  && [ "$PORT" != "$ENGINE_PORT_B" ] && [ "$PACK_PORT" != "$ENGINE_PORT_B" ] \
  || { say "--port, --engine-port, port silnika + 1 ($ENGINE_PORT_B) i port paczki ($PACK_PORT) muszą być różne." >&2; exit 2; }

# PL: Ponowne uruchomienie z tymi samymi opcjami i z tej samej wersji skryptu (do komunikatów).
# EN: The rerun command: the same options and the same version of this script.
printf -v QUOTED ' %q' "${ARGS[@]}"
RERUN="curl -fsSL https://raw.githubusercontent.com/Kamciosz/horizon-hot/$HOT_REF/scripts/server-install.sh | sudo bash -s --$QUOTED"

# --- PL: Krok 1: sprawdzenie maszyny. EN: Step 1: checking the machine. -----------------------

port_owner() { ss -Hltnp "sport = :$1" 2>/dev/null | head -n 1; }

preflight() {
  step 1/13 "Sprawdzenie maszyny"
  [ "$(id -u)" = 0 ] || { say "Uruchom przez sudo: … | sudo bash -s -- …" >&2; exit 2; }
  local id="" ver="" arch mem_gb disk_gb
  # shellcheck disable=SC1091
  [ -r /etc/os-release ] && { id=$(. /etc/os-release; echo "${ID:-}"); ver=$(. /etc/os-release; echo "${VERSION_ID:-}"); }
  arch=$(uname -m)
  [ "$id" = ubuntu ] || fail HOT-E0101 "The installer runs only on Ubuntu, this system is '${id:-unknown}'" \
    "/etc/os-release has ID=ubuntu" "another Linux distribution" \
    "Install Ubuntu Server 22.04 or 24.04 LTS on the machine, then rerun the installer" \
    "nothing was changed" "os=$id" "os_version=$ver"
  if [ "$arch" != x86_64 ] && [ "$TEST_NO_HORIZON" = 0 ]; then
    fail HOT-E0101 "Horizon ships its Linux server only for x86_64, this machine is $arch" \
      "uname -m prints x86_64" "an ARM or 32-bit machine" \
      "Use an x86_64 machine (the ThinkCentre is one); Horizon has no Linux $arch release" \
      "nothing was changed" "arch=$arch"
  fi
  mem_gb=$(awk '/MemTotal/ { printf "%d", $2 / 1024 / 1024 + 0.5 }' /proc/meminfo)
  disk_gb=$(df -P -BG "$(dirname "$ROOT")" | awk 'NR == 2 { sub("G", "", $4); print $4 }')
  [ "$mem_gb" -ge 4 ] || fail HOT-E0101 "The machine has ${mem_gb} GB of RAM, the server needs at least 4 GB" \
    "MemTotal in /proc/meminfo >= 4 GB" "too little memory" "Add RAM or use another machine" \
    "nothing was changed" "ram_gb=$mem_gb"
  if [ ! -d "$ROOT" ] && [ "$disk_gb" -lt 10 ]; then
    fail HOT-E0101 "Only ${disk_gb} GB free on the disk holding $ROOT, the installer needs 10 GB" \
      "df shows >= 10 GB free" "a full or small disk" \
      "Free disk space (sudo apt clean, journalctl --vacuum-size=200M) or use a bigger disk" \
      "nothing was changed" "free_gb=$disk_gb"
  fi
  local p units owner ours u
  # PL: Port gracza zajmował silnik przed proxy (M8.1b), więc przy aktualizacji należy do niego.
  # EN: The engine held the players' port before the proxy (M8.1b), so on an update it owns it.
  for p in "$PORT:horizon-hot-proxy horizon-hot-server" "$ENGINE_PORT:horizon-hot-server" "$ENGINE_PORT_B:horizon-hot-server" "$PACK_PORT:horizon-hot-pack"; do
    units=${p#*:} p=${p%%:*}
    owner=$(port_owner "$p")
    # PL: Port zajęty przez naszą własną usługę przy ponownym uruchomieniu jest w porządku.
    # EN: A port held by our own service on a rerun is fine.
    ours=0
    for u in $units; do systemctl is-active -q "$u" 2>/dev/null && ours=1; done
    if [ -n "$owner" ] && [ "$ours" = 0 ]; then
      OUTPUT=$owner fail HOT-E0102 "TCP port $p is already in use by another program" \
        "port $p is free" "another Minecraft server or program listens on it" \
        "Stop that program (ss -ltnp 'sport = :$p' shows it) or rerun with another port (--port N for players, --engine-port N for the engine)" \
        "nothing was changed" "port=$p"
    fi
  done
  say "Ubuntu $ver, $arch, ${mem_gb} GB RAM, ${disk_gb} GB wolnego miejsca, porty $PORT, $ENGINE_PORT, $ENGINE_PORT_B i $PACK_PORT wolne."
}

# --- PL: Krok 2: użytkownik i katalogi. EN: Step 2: user and folders. -----------------------

user_and_dirs() {
  step 2/13 "Użytkownik horizon i katalogi w $ROOT"
  if ! id horizon >/dev/null 2>&1; then
    OUTPUT=$(useradd --system --home-dir "$ROOT" --create-home --shell /usr/sbin/nologin horizon 2>&1) \
      || fail HOT-E0103 "The system user horizon cannot be created" "useradd succeeds" "see chain" \
        "Create it by hand: sudo useradd --system --home-dir $ROOT --create-home --shell /usr/sbin/nologin horizon" \
        "nothing else was changed"
    say "Utworzony użytkownik horizon."
  fi
  mkdir -p "$ROOT" "$BIN" "$SRV" "$SRV/hot-keys" "$SRV/hot-core" "$ROOT/src" "$LOGS" "$BACKUPS" "$ENGINES" "$ROOT/tmp" "$SHARE"
  chown horizon:horizon "$ROOT" "$BIN" "$SRV" "$SRV/hot-keys" "$SRV/hot-core" "$ROOT/src" "$BACKUPS" "$ENGINES" "$ROOT/tmp"
  mkdir -p "$KEYS"
  chmod 0700 "$KEYS"
  say "Katalogi gotowe."
}

# --- PL: Krok 3: pakiety. EN: Step 3: packages. --------------------------------------------

packages() {
  step 3/13 "Pakiety systemowe"
  local want="git curl ca-certificates openssh-client build-essential ufw tar" missing="" p
  for p in $want; do
    dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "install ok installed" || missing+=" $p"
  done
  if [ -z "$missing" ]; then say "Wszystkie są."; return; fi
  say "Instaluję:$missing"
  # shellcheck disable=SC2086
  OUTPUT=$( { apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q $missing; } 2>&1) \
    || fail HOT-E0104 "apt-get cannot install the packages:$missing" "apt-get install succeeds" \
      "no network, a broken mirror or another apt process holding the lock" \
      "Check the network and 'sudo apt-get update', wait for other apt runs to end, then rerun the installer" \
      "the packages already installed stay" "packages=$missing"
}

# --- PL: Krok 4: Horizon. EN: Step 4: Horizon. ---------------------------------------------

# PL: Ze źródeł w przypiętej rewizji, jak packaging/server/Dockerfile Horizona: datowane wydania
#     Nightly znikają z GitHuba po kilku dniach, a commit zostaje. Układ katalogów jak w install.sh
#     Horizona (releases/<wersja>, current, małe skrypty w $BIN). Kompilacja nie pobiera nic od Mojanga.
# EN: From source at the pinned revision, as Horizon's packaging/server/Dockerfile does: dated Nightly
#     releases vanish from GitHub after a few days, the commit stays. The folder layout follows
#     Horizon's install.sh (releases/<version>, current, small scripts in $BIN). Nothing from Mojang.
horizon() {
  step 4/13 "Horizon $HORIZON_VERSION (${HORIZON_REV:0:12}) ze źródeł"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Pominięty (--test-no-horizon)."; return; fi
  local release=$HZ/releases/$HORIZON_VERSION
  if [ "$(readlink "$HZ/current" 2>/dev/null)" = "$release" ] && [ -x "$release/horizon-engine" ] && [ -x "$BIN/horizon-server" ]; then
    say "Już jest."; return
  fi
  rust_ready
  say "Kompiluję serwer Horizona (pierwszy raz kilkadziesiąt minut)…"
  mkdir -p "$HZ_SRC" "$HZ/releases" "$HZ/game-files/horizon-client"
  chown horizon:horizon "$HZ_SRC" "$HZ" "$HZ/releases" "$HZ/game-files" "$HZ/game-files/horizon-client"
  OUTPUT=$( { [ -d "$HZ_SRC/.git" ] || as_horizon git -C "$HZ_SRC" init -q; } 2>&1 \
    && as_horizon git -C "$HZ_SRC" fetch -q --depth 1 https://github.com/horizon-rust/horizon "$HORIZON_REV" 2>&1 \
    && as_horizon git -C "$HZ_SRC" checkout -q --force FETCH_HEAD 2>&1 \
    && [ "$(as_horizon git -C "$HZ_SRC" rev-parse HEAD)" = "$HORIZON_REV" ] \
    && as_horizon sh -c "cd '$HZ_SRC' && nice -n 10 cargo build --release -q --locked -p horizon-client --no-default-features --bin horizon-client && nice -n 10 cargo build --release -q --locked -p horizon-server --bin horizon-server" 2>&1) \
    || fail HOT-E0105 "Horizon $HORIZON_REV cannot be built from source" \
      "cargo build writes horizon-client and horizon-server in $HZ_SRC/target/release" \
      "see chain: no network to GitHub or crates.io, a wrong --horizon-rev, or too little memory for the compiler" \
      "Check the network and free memory (free -h), then rerun the installer; --horizon-rev must be a full commit hash" \
      "the Horizon release already in use (if any) stays" "rev=$HORIZON_REV"
  rm -rf "$release"
  mkdir -p "$release/.local"
  install -m 0755 "$HZ_SRC/target/release/horizon-client" "$release/horizon-engine"
  install -m 0755 "$HZ_SRC/target/release/horizon-server" "$release/horizon-server"
  ln -sfn "$HZ/game-files/horizon-client" "$release/.local/horizon-client"
  chown -R horizon:horizon "$release"
  ln -sfn "$release" "$HZ/current"
  printf '#!/bin/sh\nexec "%s/current/horizon-server" "$@"\n' "$HZ" > "$BIN/horizon-server"
  chmod 0755 "$BIN/horizon-server"
  say "Zbudowany w $release."
}

# --- PL: Krok 5: horizon-hot ze źródeł. EN: Step 5: horizon-hot from source. -----------------

# PL: Rust dla użytkownika horizon; potrzebny w krokach 4, 5 i 9. EN: Rust for the horizon user; steps 4, 5 and 9.
rust_ready() {
  if [ ! -x "$ROOT/.cargo/bin/rustup" ]; then
    say "Instaluję Rusta dla użytkownika horizon (rustup)…"
    OUTPUT=$(curl -fsSL https://sh.rustup.rs -o "$ROOT/tmp/rustup.sh" 2>&1 && chown horizon:horizon "$ROOT/tmp/rustup.sh" \
      && as_horizon sh "$ROOT/tmp/rustup.sh" -y --profile minimal --default-toolchain none --no-modify-path 2>&1) \
      || fail HOT-E0106 "rustup cannot be installed for the horizon user" "rustup ends in $ROOT/.cargo/bin" \
        "no network to static.rust-lang.org" "Check the network, then rerun the installer" \
        "nothing was built" "step=rustup"
  fi
  # PL: Źródła Horizona i `cargo horizon-mod` (krok 9) nie mają rust-toolchain.toml, więc
  #     potrzebują domyślnego toolchainu. EN: Horizon's source and `cargo horizon-mod` (step 9)
  #     have no rust-toolchain.toml, so they need a default toolchain.
  if ! as_horizon rustup default > /dev/null 2>&1; then
    OUTPUT=$(as_horizon rustup default stable 2>&1) \
      || fail HOT-E0106 "rustup cannot set the stable toolchain as default for the horizon user" \
        "rustup default stable succeeds" "see chain; usually no network to static.rust-lang.org" \
        "Check the network, then rerun the installer" "nothing was built" "step=rustup-default"
  fi
}

hot_build() {
  step 5/13 "horizon-hot ($HOT_REF): hotd, hot-core, hot-diag, hot-proxy"
  rust_ready
  if [ ! -d "$SRC/.git" ]; then
    OUTPUT=$(as_horizon git clone -q "$HOT_SOURCE" "$SRC" 2>&1) \
      || fail HOT-E0106 "horizon-hot cannot be cloned from $HOT_SOURCE" "git clone succeeds" "see chain" \
        "Check the address and network, or pass --hot-source URL" "nothing of horizon-hot was built" "step=clone"
  fi
  # PL: Zawsze z --hot-source, nawet gdy klon powstał z innego adresu. EN: Always from --hot-source.
  OUTPUT=$(as_horizon git -C "$SRC" fetch -q --tags "$HOT_SOURCE" "+refs/heads/*:refs/remotes/origin/*" 2>&1) \
    || fail HOT-E0106 "horizon-hot cannot be fetched from $HOT_SOURCE" "git fetch succeeds" "see chain" \
      "Check the network, then rerun the installer" "the version built before stays" "step=fetch"
  local sha
  sha=$(as_horizon git -C "$SRC" rev-parse -q --verify "origin/$HOT_REF^{commit}" 2>/dev/null \
    || as_horizon git -C "$SRC" rev-parse -q --verify "$HOT_REF^{commit}" 2>/dev/null) \
    || fail HOT-E0106 "horizon-hot has no branch, tag or commit '$HOT_REF'" "--hot-ref names an existing ref" \
      "a typo or a branch that was deleted" "Rerun with --hot-ref main (or an existing tag or commit)" \
      "the version built before stays" "step=ref"
  HOT_SHA=$sha
  if [ "$(cat "$ROOT/hot-version" 2>/dev/null)" = "$sha" ] && [ -x "$BIN/hotd" ] && [ -x "$BIN/hot-proxy" ] && [ -f "$SRV/hot-core/libhot_core.so" ]; then
    say "Już zbudowany: ${sha:0:7}."; return
  fi
  as_horizon git -C "$SRC" checkout -q --detach "$sha"
  say "Buduję ${sha:0:7} (kilka minut przy pierwszym razie)…"
  OUTPUT=$(cd "$SRC" && as_horizon rustup toolchain install 2>&1 && as_horizon cargo build --release -q -p hotd -p hot-core -p hot-diag -p hot-proxy 2>&1) \
    || fail HOT-E0106 "horizon-hot ${sha:0:7} does not compile on this server" "cargo build --release succeeds" \
      "see chain: usually a missing system package or not enough memory" \
      "Fix what chain names (sudo apt install build-essential; free memory), then rerun the installer" \
      "the version built before stays" "step=cargo" "commit=$sha"
  install -o horizon -g horizon -m 0755 "$SRC/target/release/hotd" "$SRC/target/release/hot-diag" "$SRC/target/release/hot-proxy" "$BIN/"
  install -o horizon -g horizon -m 0644 "$SRC/target/release/libhot_core.so" "$SRV/hot-core/libhot_core.so"
  printf '%s\n' "$sha" > "$ROOT/hot-version"
  HOT_REBUILT=1
  say "Zbudowany ${sha:0:7}: $BIN/hotd, $SRV/hot-core/libhot_core.so."
}

# --- PL: Krok 6: klucze i pliki do repo serwera. EN: Step 6: keys and server repo files. -----

ssh_repo() { case "$REPO" in git@*|ssh://*) return 0 ;; *) return 1 ;; esac; }

keys() {
  step 6/13 "Klucze i pliki do repo serwera"
  if [ ! -f "$KEYS/hot-ci" ]; then
    ssh-keygen -q -t ed25519 -N '' -C hot-ci -f "$KEYS/hot-ci"
    say "Nowy klucz podpisu CI: $KEYS/hot-ci."
  fi
  if write_if_changed "$SRV/hot-keys/allowed_signers" "hot-ci namespaces=\"horizon-hot-mod\" $(cat "$KEYS/hot-ci.pub")"; then
    chown horizon:horizon "$SRV/hot-keys/allowed_signers"
    say "Zapisany $SRV/hot-keys/allowed_signers."
  fi
  if ssh_repo; then
    mkdir -p "$ROOT/.ssh"
    chmod 0700 "$ROOT/.ssh"
    chown horizon:horizon "$ROOT/.ssh"
    if [ ! -f "$ROOT/.ssh/deploy" ]; then
      as_horizon ssh-keygen -q -t ed25519 -N '' -C "horizon-hot deploy $(hostname)" -f "$ROOT/.ssh/deploy"
      say "Nowy klucz tylko do odczytu repo: $ROOT/.ssh/deploy."
    fi
  fi
  # PL: hot.yml z wersją horizon-hot przypiętą do tego commita. EN: hot.yml pinned to this commit.
  mkdir -p "$SHARE/.github/workflows"
  if write_if_changed "$SHARE/.github/workflows/hot.yml" \
    "$(sed "s|^\(          ref: \)main$|\1$HOT_SHA|" "$SRC/templates/server-repo/.github/workflows/hot.yml")"; then
    say "Zapisany $SHARE/.github/workflows/hot.yml (horizon-hot ${HOT_SHA:0:7})."
  else
    say "Bez zmian."
  fi
}

# --- PL: Krok 7: pliki gry i świat. EN: Step 7: game files and the world. ---------------------

resources_present() {
  [ -f "$SRV/resources/horizon-resources.json" ] || [ -f "$HZ/game-files/horizon-client/26.3/horizon-resources.json" ]
}

world() {
  step 7/13 "Pliki gry i świat"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Pominięte (--test-no-horizon)."; return; fi
  if ! resources_present; then
    if [ -z "$RESOURCES" ]; then
      wait_for_operator HOT-E0107 "The game files prepared from the owner's Minecraft are not on the server yet" \
        "horizon-resources.json in $HZ/game-files/horizon-client/26.3" "a first install: only the owner can prepare them" \
        "On a computer with your Minecraft and the Horizon launcher, copy the prepared 26.3 folder here (scp -r), then rerun with --resources DIR" \
        "everything before this step is installed; nothing after it" "dir=$HZ/game-files/horizon-client"
    fi
    OUTPUT=$("$BIN/horizon-server" --dir "$SRV" import-resources "$RESOURCES" 2>&1) \
      || fail HOT-E0107 "horizon-server import-resources refused the folder $RESOURCES" \
        "the folder holds horizon-resources.json for 26.3 with matching checksums" "see chain: the folder is incomplete or for another version" \
        "Copy the whole 26.3 folder again from the computer that prepared it, then rerun with --resources DIR" \
        "the server's game files are unchanged" "resources=$RESOURCES"
    chown -R horizon:horizon "$HZ/game-files" "$SRV"
    say "Pliki gry zaimportowane."
  fi
  if [ ! -f "$SRV/server.toml" ]; then
    [ "$EULA" = 1 ] || wait_for_operator HOT-E0108 "The Minecraft EULA has not been accepted" \
      "the installer runs with --eula" "only the owner can accept it" \
      "Read https://aka.ms/MinecraftEULA, then rerun the installer with --eula" \
      "everything before the world is installed" "port=$ENGINE_PORT"
    local players=(--no-whitelist)
    [ -n "$OP" ] && players=(--whitelist --player "$OP" --op "$OP")
    OUTPUT=$(hz init --eula --world world --port "$ENGINE_PORT" --yes "${players[@]}" 2>&1) \
      || fail HOT-E0108 "horizon-server init cannot create the server in $SRV" "init writes $SRV/server.toml" \
        "see chain" "Fix what chain says, remove $SRV/server.toml if half written, then rerun the installer" \
        "no world was created" "dir=$SRV"
    say "Świat utworzony w $SRV."
  fi
  OUTPUT=$(hz config set backups.every=1d backups.keep=7 2>&1) \
    || fail HOT-E0108 "The backup settings cannot be written to server.toml" "config set succeeds" "see chain" \
      "Run: sudo -u horizon $BIN/horizon-server --dir $SRV config set backups.every=1d backups.keep=7" \
      "the server runs with its old backup settings" "dir=$SRV"
}

# --- PL: Krok 8: proxy z kontrolą zakupu. EN: Step 8: the proxy with the purchase check. -----

# PL: Gracze łączą się z hot-proxy na porcie $PORT. Proxy sprawdza w usłudze sesji, czy gracz ma
#     kupioną grę, i podpisuje silnikowi jego dane wspólnym sekretem (przekazywanie Velocity,
#     PLAN M8). Silnik słucha tylko na 127.0.0.1 i sam kont nie sprawdza (`online_mode = false`).
#     Kolejność jest celowa: najpierw sekret, potem adres lokalny, tryb offline na końcu. Krok
#     przerwany w połowie nie zostawia silnika otwartego dla każdego: bez sekretu nie ma trybu
#     offline, a silnik z sekretem wpuszcza tylko graczy podpisanych przez proxy.
# EN: Players connect to hot-proxy on port $PORT. The proxy asks the session service whether the
#     player owns the game and signs the player's data for the engine with a shared secret
#     (Velocity forwarding, PLAN M8). The engine listens on 127.0.0.1 only and checks no accounts
#     itself (`online_mode = false`). The order is deliberate: the secret first, then the local
#     address, offline mode last. A step cut short never leaves the engine open to anyone: no
#     secret means no offline mode, and an engine with the secret admits only proxy-signed players.
proxy_setup() {
  step 8/13 "Proxy z kontrolą zakupu"
  local secret=$SRV/forwarding.secret before
  if [ -z "$(tr -d '[:space:]' 2>/dev/null < "$secret")" ]; then
    OUTPUT=$( (umask 077 && head -c 32 /dev/urandom | base64 | tr -d '/+=\n' > "$secret") 2>&1) || true
    ENGINE_CHANGED=1
  fi
  chown horizon:horizon "$secret" 2>/dev/null || true
  chmod 0600 "$secret" 2>/dev/null || true
  [ -n "$(tr -d '[:space:]' 2>/dev/null < "$secret")" ] || fail HOT-E0114 "The forwarding secret $secret cannot be written" \
    "a non-empty $secret owned by horizon, mode 0600" "see chain: a full disk or a read-only file system" \
    "Free disk space (df -h $SRV), then rerun the installer" \
    "server.toml is unchanged: the engine keeps checking accounts itself" "file=$secret"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Sekret w $secret; server.toml pominięty (--test-no-horizon)."; return; fi
  before=$(cksum < "$SRV/server.toml")
  OUTPUT=$(hz config set server.bind=127.0.0.1 "server.port=$ENGINE_PORT" 2>&1) \
    || fail HOT-E0114 "The engine cannot be moved to 127.0.0.1:$ENGINE_PORT" "config set server.bind and server.port succeed" \
      "see chain" "Run: sudo -u horizon $BIN/horizon-server --dir $SRV config set server.bind=127.0.0.1 server.port=$ENGINE_PORT; then rerun" \
      "online_mode is unchanged: the engine keeps checking accounts itself" "engine_port=$ENGINE_PORT"
  OUTPUT=$(hz config 2>&1) || true
  # PL: Tryb offline tylko na adresie lokalnym. EN: Offline mode only on the local address.
  grep -qx 'server.bind = "127.0.0.1"' <<< "$OUTPUT" || fail HOT-E0114 \
    "The engine is not bound to 127.0.0.1 after config set" 'horizon-server config prints server.bind = "127.0.0.1"' \
    "see chain: another horizon-server version spells the key differently" \
    "Set bind = \"127.0.0.1\" under [server] in $SRV/server.toml, then rerun" \
    "online_mode is unchanged: the engine keeps checking accounts itself"
  OUTPUT=$(hz config set server.online_mode=false 2>&1) \
    || fail HOT-E0114 "online_mode cannot be turned off for the engine behind the proxy" "config set server.online_mode=false succeeds" \
      "see chain" "Run: sudo -u horizon $BIN/horizon-server --dir $SRV config set server.online_mode=false; then rerun" \
      "the engine checks accounts itself and refuses players coming through the proxy"
  [ "$(cksum < "$SRV/server.toml")" = "$before" ] || ENGINE_CHANGED=1
  say "Silnik: 127.0.0.1:$ENGINE_PORT, sekret w $secret; gracze: hot-proxy na porcie $PORT."
}

# --- PL: Krok 9: pula slotów i paczka zasobów. EN: Step 9: slot pool and resource pack. ----

# PL: Pula slotów (`hot_slots.hmod`) dla serwera i graczy oraz adres paczki w `server.toml`.
#     Plik buduje `cargo horizon-mod` z tej samej wersji Horizona co serwer; build jest powtarzalny,
#     więc gracz może zbudować go sam albo pobrać z `serve-pack`. Opis: docs/sloty.md.
# EN: The slot pool (`hot_slots.hmod`) for the server and players, and the pack address in
#     `server.toml`. `cargo horizon-mod` from the server's Horizon version builds it; the build is
#     reproducible, so a player can build it or download it from `serve-pack`. See docs/sloty.md.
# PL: Ustawia `resource-pack` i `require-resource-pack` w `[properties]`; reszta pliku bez zmian.
# EN: Sets `resource-pack` and `require-resource-pack` in `[properties]`; the rest stays as it is.
set_pack_url() {
  local file=$1 url=$2 tmp
  tmp=$(mktemp)
  awk -v url="$url" '
    function add() { if (!done) { print "resource-pack = \"" url "\""; print "require-resource-pack = true"; done = 1 } }
    /^\[/ { if (inprops) add(); inprops = ($0 == "[properties]") }
    inprops && /^(resource-pack|require-resource-pack)[ =]/ { next }
    { print }
    inprops && $0 == "[properties]" { add() }
    END { if (!done) { print ""; print "[properties]"; add() } }
  ' "$file" > "$tmp" && cat "$tmp" > "$file"
  local rc=$?
  rm -f "$tmp"
  return $rc
}

slots_pack() {
  step 9/13 "Pula slotów i paczka zasobów"
  local rev=${HORIZON_VERSION##*-} pool=$SRV/horizon-mods/hot_slots.hmod src=$ROOT/tmp/hot-slots
  if [ "$(cat "$ROOT/cargo-horizon-mod-version" 2>/dev/null)" != "$rev" ] || [ ! -x "$ROOT/.cargo/bin/cargo-horizon-mod" ]; then
    say "Instaluję cargo-horizon-mod (Horizon $rev)…"
    OUTPUT=$(as_horizon cargo install -q --locked --force --git https://github.com/horizon-rust/horizon --rev "$rev" cargo-horizon-mod 2>&1) \
      || fail HOT-E0112 "cargo-horizon-mod cannot be installed for the horizon user" "cargo install ends in $ROOT/.cargo/bin" \
        "see chain: no network to github.com or a compile error" "Check the network, then rerun the installer" \
        "the server runs without the slot pool" "rev=$rev"
    printf '%s\n' "$rev" > "$ROOT/cargo-horizon-mod-version"
  fi
  if [ "$HOT_REBUILT" = 1 ] || [ ! -f "$pool" ]; then
    rm -rf "$src"
    OUTPUT=$(as_horizon "$BIN/hotd" slots-source "$src" 2>&1 && cd "$src" && as_horizon cargo horizon-mod build 2>&1) \
      || fail HOT-E0112 "The slot pool hot_slots.hmod cannot be built" "cargo horizon-mod build writes target/horizon-mod/hot_slots.hmod" \
        "see chain" "Fix what chain says, then rerun the installer" "the server keeps its previous pool, if any" "src=$src"
    mkdir -p "$SRV/horizon-mods" "$SRV/hot-state/pack"
    if ! cmp -s "$src/target/horizon-mod/hot_slots.hmod" "$pool"; then
      install -o horizon -g horizon -m 0644 "$src/target/horizon-mod/hot_slots.hmod" "$pool"
      SLOTS_CHANGED=1
    fi
    install -o horizon -g horizon -m 0644 "$pool" "$SRV/hot-state/pack/hot_slots.hmod"
    # PL: Źródła z target/ zajmują setki MB, a następna budowa i tak zaczyna od zera (M7.2).
    # EN: The sources with target/ take hundreds of MB and the next build starts from scratch (M7.2).
    rm -rf "$src"
  fi
  touch "$SRV/horizon-mods.txt"
  grep -qx 'mod.hot_slots:true' "$SRV/horizon-mods.txt" || { printf 'mod.hot_slots:true\n' >> "$SRV/horizon-mods.txt"; SLOTS_CHANGED=1; }
  chown -R horizon:horizon "$SRV/horizon-mods" "$SRV/horizon-mods.txt" "$SRV/hot-state"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Pula zbudowana; server.toml pominięty (--test-no-horizon)."; return; fi
  if [ -z "$ADDRESS" ]; then
    wait_for_operator HOT-E0112 "The address players use to reach the server is not known" \
      "the installer runs with --address HOST" "the game downloads the resource pack from that address; the installer does not guess it" \
      "Rerun with --address and the IP or name players type to join (this machine says: $(hostname -I 2>/dev/null | awk '{ print $1 }'))" \
      "everything before the resource pack is installed" "pack_port=$PACK_PORT"
  fi
  # PL: Bez resource-pack-sha1: klient pobiera paczkę przy każdym wejściu, więc nowy wygląd nie
  #     wymaga restartu silnika (docs/sloty.md). EN: No sha1: the client downloads the pack at
  #     every join, so a new look needs no engine restart.
  local url="http://$ADDRESS:$PACK_PORT/pack.zip" before
  before=$(cksum < "$SRV/server.toml")
  OUTPUT=$(set_pack_url "$SRV/server.toml" "$url" 2>&1) \
    || fail HOT-E0112 "server.toml cannot be updated with the resource pack address" "[properties] has resource-pack and require-resource-pack" \
      "see chain" "Add the two lines under [properties] in $SRV/server.toml by hand (docs/sloty.md), then rerun" \
      "the server runs without the resource pack" "url=$url"
  [ "$(cksum < "$SRV/server.toml")" = "$before" ] || SLOTS_CHANGED=1
  say "Pula $(sed -n 's/^version = "\(.*\)"/\1/p' "$src/horizon-mod.toml" 2>/dev/null || true) w $pool; paczka: $url"
}

# --- PL: Krok 10: usługi systemd. EN: Step 10: systemd units. --------------------------------

units() {
  step 10/13 "Usługi systemd"
  local changed=0 git_ssh=""
  ssh_repo && git_ssh="Environment=\"GIT_SSH_COMMAND=ssh -i $ROOT/.ssh/deploy -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new\""
  # PL: Gra ma pierwszeństwo, tło pracuje z resztek (D16). Myślnik w nazwie buduje drzewo:
  #     horizon-game i horizon-bg leżą w horizon.slice, który waży więcej niż reszta systemu.
  #     Tło ma wagę 100, nie 1: z wagą 1 hotd nie zrobi ani kroku, gdy gra zajmuje wszystkie
  #     rdzenie; z wagą 100 pracuje, a gra dostaje około 99% CPU (M6.2). MemoryLow i MemoryMax
  #     czekają na pomiar S10 (M0.11).
  # EN: The game comes first, background work gets the leftovers (D16). A dash in the name
  #     builds the tree: horizon-game and horizon-bg sit in horizon.slice, which outweighs the
  #     rest of the system. Background weighs 100, not 1: at 1, hotd makes no step at all while
  #     the game uses every core, at 100 it does, with the game at about 99% CPU (M6.2).
  #     MemoryLow/MemoryMax wait for the S10 measurement (M0.11).
  write_if_changed /etc/systemd/system/horizon.slice "[Unit]
Description=horizon-hot: the game server and its background work

[Slice]
CPUWeight=1000" && changed=1
  write_if_changed /etc/systemd/system/horizon-game.slice "[Unit]
Description=horizon-hot: the game (engine, resource pack)

[Slice]
CPUWeight=10000
IOWeight=10000" && changed=1
  write_if_changed /etc/systemd/system/horizon-bg.slice "[Unit]
Description=horizon-hot: background work (hotd, copies)

[Slice]
CPUWeight=100
IOWeight=1" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-server.service "[Unit]
Description=Horizon server with horizon-hot (hotd engine: the engine and its swaps)
After=network-online.target
Wants=network-online.target

[Service]
Slice=horizon-game.slice
User=horizon
Group=horizon
WorkingDirectory=$SRV
Environment=HOT_HOME=$SRV
ExecStart=$BIN/hotd engine --home $SRV --engine $BIN/horizon-server
Restart=on-failure
RestartSec=5
TimeoutStopSec=120
KillMode=mixed

[Install]
WantedBy=multi-user.target" && changed=1
  # PL: Bez trybu offline: proxy zawsze sprawdza zakup gry (test crates/hot-proxy/tests/installer.rs).
  # EN: Never offline: the proxy always checks the purchase (test crates/hot-proxy/tests/installer.rs).
  write_if_changed /etc/systemd/system/horizon-hot-proxy.service "[Unit]
Description=horizon-hot: the players' port with the purchase check (hot-proxy)
After=horizon-hot-server.service network-online.target
Wants=network-online.target

[Service]
Slice=horizon-game.slice
User=horizon
Group=horizon
Environment=HOT_HOME=$SRV
ExecStart=$BIN/hot-proxy --listen 0.0.0.0:$PORT --engine 127.0.0.1:$ENGINE_PORT --secret $SRV/forwarding.secret --home $SRV
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-hotd.service "[Unit]
Description=horizon-hot hotd: mods from the server repository
After=horizon-hot-server.service network-online.target
Wants=network-online.target

[Service]
Slice=horizon-bg.slice
Nice=19
IOSchedulingClass=idle
User=horizon
Group=horizon
Environment=HOT_HOME=$SRV
$git_ssh
ExecStart=$BIN/hotd run --home $SRV --repo $REPO --engine $BIN/horizon-server --engine-dir $SRV
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-pack.service "[Unit]
Description=horizon-hot: resource pack and slot pool for players (hotd serve-pack)
After=network-online.target
Wants=network-online.target

[Service]
Slice=horizon-game.slice
User=horizon
Group=horizon
Environment=HOT_HOME=$SRV
ExecStart=$BIN/hotd serve-pack --home $SRV --port $PACK_PORT
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-backup.service "[Unit]
Description=horizon-hot: copy of hot-state and hot-keys (worlds: horizon-server [backups])

[Service]
Type=oneshot
Slice=horizon-bg.slice
Nice=19
IOSchedulingClass=idle
CPUSchedulingPolicy=idle
User=horizon
ExecStart=/bin/sh -c 'tar -czf $BACKUPS/hot-state-\$\$(date +%%F).tar.gz -C $SRV hot-state hot-keys && find $BACKUPS -name \"hot-state-*.tar.gz\" -mtime +7 -delete'" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-backup.timer "[Unit]
Description=horizon-hot: nightly copy of hot-state

[Timer]
OnCalendar=*-*-* 04:00
Persistent=true

[Install]
WantedBy=timers.target" && changed=1
  mkdir -p "$SRV/hot-state"
  chown horizon:horizon "$SRV/hot-state"
  [ "$changed" = 1 ] && systemctl daemon-reload
  OUTPUT=$( {
    systemctl enable -q --now horizon-hot-backup.timer
    systemctl enable -q horizon-hot-pack
    if [ "$changed" = 1 ] || [ "$HOT_REBUILT" = 1 ]; then systemctl restart horizon-hot-pack; else systemctl start horizon-hot-pack; fi
    if [ "$TEST_NO_HORIZON" = 0 ]; then
      systemctl enable -q horizon-hot-server
      # PL: Pula i adres paczki silnik czyta tylko przy starcie. EN: The engine reads the pool and pack address only at start.
      if [ "$changed" = 1 ] || [ "$SLOTS_CHANGED" = 1 ] || [ "$ENGINE_CHANGED" = 1 ]; then systemctl restart horizon-hot-server; else systemctl start horizon-hot-server; fi
    fi
    # PL: Po silniku: przy aktualizacji silnik zwalnia port graczy dopiero po restarcie.
    # EN: After the engine: on an update the engine frees the players' port only on its restart.
    systemctl enable -q horizon-hot-proxy
    if [ "$changed" = 1 ] || [ "$HOT_REBUILT" = 1 ] || [ "$ENGINE_CHANGED" = 1 ]; then systemctl restart horizon-hot-proxy; else systemctl start horizon-hot-proxy; fi
    # PL: hotd rusza dopiero przy silniku z hot-host; bez niego commity z modami byłyby
    #     oznaczane jako nieudane. EN: hotd starts only with a hot-host engine.
    if engine_has_hot_host || [ "$TEST_NO_HORIZON" = 1 ]; then
      systemctl enable -q horizon-hot-hotd
      # PL: Nowy plik programu działa dopiero po restarcie. EN: A new binary runs only after a restart.
      if [ "$changed" = 1 ] || [ "$HOT_REBUILT" = 1 ]; then systemctl restart horizon-hot-hotd; else systemctl start horizon-hot-hotd; fi
    fi
  } 2>&1) || fail HOT-E0109 "systemd cannot start the horizon-hot services" "systemctl enable --now succeeds" \
    "see chain; journalctl -u horizon-hot-server -n 50 shows why a unit stopped" \
    "Run journalctl -u horizon-hot-server -u horizon-hot-hotd -n 80, fix the cause, then rerun the installer" \
    "the unit files are in /etc/systemd/system" "units=horizon-hot-server,horizon-hot-proxy,horizon-hot-hotd,horizon-hot-pack,horizon-hot-backup"
  say "Włączone: horizon-hot-backup.timer, horizon-hot-pack$([ "$TEST_NO_HORIZON" = 0 ] && printf ', horizon-hot-server'), horizon-hot-proxy$( (engine_has_hot_host || [ "$TEST_NO_HORIZON" = 1 ]) && printf ', horizon-hot-hotd')."
}

# --- PL: Krok 11: zapora. EN: Step 11: firewall. ----------------------------------------------

firewall() {
  step 11/13 "Zapora (ufw)"
  local p
  # PL: Najpierw SSH na każdym porcie, na którym słucha sshd, żeby nie odciąć operatora.
  # EN: SSH first, on every port sshd listens on, so the operator is never locked out.
  # PL: Ubuntu 24.04 słucha przez ssh.socket (w ss widać systemd), więc port też z `sshd -T`.
  # EN: Ubuntu 24.04 listens via ssh.socket (ss shows systemd), so the port also comes from `sshd -T`.
  for p in $( { ss -Hltnp 2>/dev/null | awk '/"sshd"/ { n = split($4, a, ":"); print a[n] }'; sshd -T 2>/dev/null | awk '/^port / { print $2 }'; echo 22; } | sort -u); do
    ufw allow "$p/tcp" >/dev/null
  done
  ufw allow "$PORT/tcp" >/dev/null
  ufw allow "$PACK_PORT/tcp" >/dev/null
  OUTPUT=$(ufw --force enable 2>&1) || fail HOT-E0109 "ufw cannot be enabled" "ufw --force enable succeeds" \
    "see chain: often a kernel without netfilter in a container" "Run: sudo ufw status verbose; fix the cause, then rerun" \
    "the firewall rules are added but may be inactive"
  say "Otwarte: SSH, $PORT (gracze, przez proxy), $PACK_PORT (paczka zasobów). Silnik ($ENGINE_PORT) tylko lokalnie."
}

# PL: Czeka do 120 s na fazę running silnika. EN: Waits up to 120 s for the engine's running phase.
wait_running() {
  for _ in $(seq 1 60); do
    hz status --json 2>/dev/null | grep -q '"phase": *"running"' && return 0
    sleep 2
  done
  return 1
}

# --- PL: Krok 13: silnik z hot-host. EN: Step 13: the engine with hot-host. ------------------

engine_has_hot_host() {
  [ -f "$SRV/server.toml" ] && grep -q "^path = \"$ENGINES/" "$SRV/server.toml" 2>/dev/null
}

engine() {
  step 13/13 "Silnik z hot-host"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Pominięty (--test-no-horizon)."; return; fi
  local out=$ENGINES/${HOT_SHA:0:12}-${HORIZON_REV:0:12}
  if grep -q "^path = \"$out/horizon-engine\"" "$SRV/server.toml" 2>/dev/null && [ -x "$out/horizon-engine" ]; then
    say "Już jest: $out."; return
  fi
  if [ "$TRUST_NATIVE" = 0 ]; then
    wait_for_operator HOT-E0110 "The engine with hot-host is not built: the owner has not confirmed --trust-native-code" \
      "the installer runs with --trust-native-code" "native code needs the owner's decision (Horizon's rule)" \
      "Read $SRC/crates/hot-host, then rerun the installer with --trust-native-code" \
      "the server runs without hot mods; hotd is not started" "source=$SRC/crates/hot-host"
  fi
  [ -n "$LAUNCHER" ] || LAUNCHER=$(command -v horizon-launcher || true)
  [ -n "$LAUNCHER" ] && [ -x "$LAUNCHER" ] || fail HOT-E0110 "horizon-launcher is not installed on the server" \
    "--launcher PATH or horizon-launcher in PATH" "native-build lives in the launcher, not in horizon-server" \
    "Install the Horizon launcher for Linux x86-64 (github.com/horizon-rust/horizon/releases?q=launcher), then rerun with --launcher PATH" \
    "the server runs without hot mods" "launcher=${LAUNCHER:-none}"
  # PL: Paczka zostaje: z niej buduje też aktualizacja Horizona (M9). Rewizję wybiera przypięcie
  #     w paczce, nie wydanie, które znika z GitHuba (ADR 0012).
  # EN: The bundle stays: the Horizon update (M9) builds from it too. The bundle's pin chooses the
  #     revision, not a release that vanishes from GitHub (ADR 0012).
  local bundle=$ROOT/hot-host-bundle
  as_horizon sh "$SRC/crates/hot-host/build-bundle.sh" "$bundle" >/dev/null
  say "Buduję silnik (kilka minut)…"
  OUTPUT=$(as_horizon "$BIN/hotd" build-engine --launcher "$LAUNCHER" --bundle "$bundle" \
      --rev "$HORIZON_REV" --out "$out" --trust-native-code 2>&1) \
    || { rm -rf "$out"; fail HOT-E0110 "horizon-launcher native-build cannot build the engine with hot-host" \
      "native-build writes $out/horizon-engine" \
      "see chain (HOT-E0301): often the launcher's version is not prepared (no game files for the horizon user)" \
      "Prepare once as the horizon user: sudo -u horizon -H $LAUNCHER account sign-in, then sudo -u horizon -H $LAUNCHER prepare --download; rerun with --trust-native-code" \
      "the server runs on its old engine without hot mods" "out=$out"; }
  OUTPUT=$(hz config set "engine.path=$out/horizon-engine" 2>&1 && systemctl restart horizon-hot-server 2>&1) \
    || fail HOT-E0110 "The server cannot switch to the new engine" "config set engine.path and restart succeed" \
      "see chain" "Run: sudo -u horizon $BIN/horizon-server --dir $SRV config set engine.path=$out/horizon-engine" \
      "the engine was built but the server may still run the old one" "out=$out"
  # PL: hotd dopiero przy działającym silniku. EN: hotd only once the engine runs.
  wait_running || OUTPUT=$(journalctl -u horizon-hot-server -n 60 --no-pager 2>&1) fail HOT-E0110 \
    "The server does not run on the new engine within 120 s" "horizon-server status --json says running" \
    "see chain (the service log); the engine with hot-host may not start" \
    "Read journalctl -u horizon-hot-server -n 200; to go back: config set engine.path= (empty) and systemctl restart horizon-hot-server" \
    "the server may be down; hotd is not started" "out=$out"
  systemctl enable -q --now horizon-hot-hotd
  say "Silnik z hot-host: $out."
}

# --- PL: Krok 12: autotest. EN: Step 12: self test. -----------------------------------------

autotest() {
  step "$1" "$2"
  local ok=0
  [ -f "$SRV/hot-keys/allowed_signers" ] || fail HOT-E0111 "allowed_signers is missing" "the file exists" \
    "it was deleted after the keys step" "Rerun the installer" "hotd loads no mod without it"
  as_horizon touch "$SRV/hot-state/.write-test" && rm -f "$SRV/hot-state/.write-test" \
    || fail HOT-E0111 "The horizon user cannot write $SRV/hot-state" "hot-state is owned by horizon" \
      "wrong owner" "Run: sudo chown -R horizon:horizon $SRV" "hot-core and hotd cannot save state"
  OUTPUT=$(as_horizon "$BIN/hotd" status --home "$SRV" 2>&1) || fail HOT-E0111 "hotd status does not run" \
    "hotd status prints the state" "a broken build" "Rerun the installer" "hot mods are not managed"
  # PL: Działający hotd musi odczytać repo serwera (adres, klucz tylko do odczytu).
  # EN: A running hotd must read the server repository (address, read-only key).
  if systemctl is-active -q horizon-hot-hotd; then
    local since state updated
    since=$(date -u +%Y-%m-%dT%H:%M:%S)
    ok=0
    for _ in $(seq 1 20); do
      sleep 2
      state=$(cat "$SRV/hot-state/hotd.json" 2>/dev/null || true)
      updated=$(printf '%s' "$state" | sed -n 's/.*"updated": *"\([^"]*\)".*/\1/p')
      # PL: Krok hotd po starcie testu i bez błędu pobierania. EN: A step after the test began, no fetch error.
      if [[ "$updated" > "$since" ]] && printf '%s' "$state" | grep -q '"fetch_error": *null'; then ok=1; break; fi
    done
    [ "$ok" = 1 ] || OUTPUT=$(as_horizon "$BIN/hotd" status --home "$SRV" 2>&1) fail HOT-E0111 \
      "hotd cannot read the server repository $REPO" "hotd.json is updated with fetch_error null within 40 s" \
      "a wrong --repo address, or the read-only deploy key is not added to the repository" \
      "Check --repo; for a private repository add $ROOT/.ssh/deploy.pub as a read-only deploy key, then rerun" \
      "the server runs; hotd retries every 10 s" "repo=$REPO"
  fi
  if [ "$TEST_NO_HORIZON" = 0 ]; then
    wait_running || OUTPUT=$(journalctl -u horizon-hot-server -n 60 --no-pager 2>&1) fail HOT-E0111 \
      "The server did not reach the running phase within 120 s" "horizon-server status --json says running" \
      "see chain (the service log)" "Read journalctl -u horizon-hot-server -n 200, fix the cause, then rerun the installer" \
      "the services stay installed"
  fi
  # PL: Gracze dochodzą przez proxy, a silnik słucha tylko lokalnie. EN: Players get through the proxy; the engine is local only.
  ok=0
  for _ in $(seq 1 15); do [[ "$(port_owner "$PORT")" == *'"hot-proxy"'* ]] && { ok=1; break; }; sleep 1; done
  [ "$ok" = 1 ] || OUTPUT=$(journalctl -u horizon-hot-proxy -n 40 --no-pager 2>&1) fail HOT-E0111 \
    "hot-proxy does not listen on the players' port $PORT" "ss shows hot-proxy on port $PORT" \
    "see chain (the service log); often the port is still held by another program" \
    "Read journalctl -u horizon-hot-proxy -n 80 and $SRV/logs/hot-proxy.jsonl, fix the cause, then rerun the installer" \
    "players cannot join" "port=$PORT"
  if [ "$TEST_NO_HORIZON" = 0 ]; then
    local open
    open=$(ss -Hltn "( sport = :$ENGINE_PORT or sport = :$ENGINE_PORT_B )" 2>/dev/null | awk '$4 !~ /^127\.0\.0\.1:/ { print $4 }')
    [ -z "$open" ] || OUTPUT=$open fail HOT-E0111 "The engine listens on $open, not only on 127.0.0.1" \
      "the engine port $ENGINE_PORT is bound to 127.0.0.1 only" "server.toml lost bind = \"127.0.0.1\"" \
      "Rerun the installer (step 8 sets it again)" "the engine may be reachable without the purchase check" "engine_port=$ENGINE_PORT"
    OUTPUT=$(as_horizon "$BIN/hot-proxy" ping "127.0.0.1:$PORT" 2>&1) || fail HOT-E0111 \
      "The engine does not answer through hot-proxy" "hot-proxy ping 127.0.0.1:$PORT prints the server status" \
      "see chain: the engine port in the proxy unit differs from server.toml, or the engine is down" \
      "Compare --engine in /etc/systemd/system/horizon-hot-proxy.service with server.port in $SRV/server.toml, then rerun the installer" \
      "players cannot join" "port=$PORT" "engine_port=$ENGINE_PORT"
  fi
  if engine_has_hot_host; then
    local before after
    before=$(stat -c %Y "$SRV/hot-state/status.json" 2>/dev/null || echo 0)
    ok=0
    for _ in $(seq 1 15); do
      hz cmd "hot status" >/dev/null 2>&1 || true
      sleep 2
      after=$(stat -c %Y "$SRV/hot-state/status.json" 2>/dev/null || echo 0)
      [ "$after" -gt "$before" ] && { ok=1; break; }
    done
    [ "$ok" = 1 ] || fail HOT-E0111 "hot-core in the engine does not answer 'hot status'" \
      "hot-state/status.json is rewritten after 'hot status'" "the engine does not load hot-core ($SRV/hot-core/libhot_core.so)" \
      "Read $SRV/logs/ and journalctl -u horizon-hot-server; check engine.path in $SRV/server.toml" \
      "the server runs without hot mods"
    systemctl is-active -q horizon-hot-hotd || fail HOT-E0111 "hotd is not running" "horizon-hot-hotd is active" \
      "see journalctl -u horizon-hot-hotd" "Read journalctl -u horizon-hot-hotd -n 80, fix it, then rerun" \
      "mods from the repository are not loaded"
  fi
  as_horizon "$BIN/hotd" status --home "$SRV" | sed 's/^/  /'
}

summary() {
  local ip
  ip=$(hostname -I 2>/dev/null | awk '{ print $1 }')
  say ""
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Test bez Horizona przeszedł."; return; fi
  say "Działa: ${ip:-<adres serwera>}:$PORT"
  say ""
  say "Repo serwera ($REPO), raz:"
  say "  1. Skopiuj $SHARE/.github/workflows/hot.yml do repo jako .github/workflows/hot.yml."
  say "  2. Sekret HOT_SIGNING_KEY w repo = treść klucza: sudo cat $KEYS/hot-ci"
  say "     (z gh: sudo cat $KEYS/hot-ci | gh secret set HOT_SIGNING_KEY -R <właściciel>/<repo>)"
  if ssh_repo; then
    say "  3. Klucz tylko do odczytu (Settings → Deploy keys, bez zapisu): cat $ROOT/.ssh/deploy.pub"
  fi
  say ""
  say "Gracze, raz: pobierz http://$ADDRESS:$PACK_PORT/hot_slots.hmod do folderu horizon-mods/ swojej instancji,"
  say "  dopisz mod.hot_slots:true do horizon-mods.txt obok, a przy wejściu przyjmij paczkę zasobów serwera."
  say "Stan: sudo -u horizon $BIN/hotd status --home $SRV"
}

main() {
  preflight
  user_and_dirs
  packages
  horizon
  hot_build
  keys
  world
  proxy_setup
  slots_pack
  units
  firewall
  autotest 12/13 "Autotest serwera"
  engine
  # PL: Po zbudowaniu silnika test jeszcze raz, już z hot-core. EN: Test again, now with hot-core.
  if engine_has_hot_host; then autotest 13/13 "Autotest z hot-core"; fi
  summary
}

# --- PL: Odinstalowanie. EN: Uninstalling. ----------------------------------------------------

# PL: Usuwa to, co zainstalował ten skrypt, oprócz świata, kopii, logów i użytkownika horizon
#     (jest właścicielem świata). Stan modów (hot-state, hot-keys) trafia najpierw do kopii
#     w backups/. Pakietów systemowych nie rusza, bo mogą służyć czemuś innemu. Bez --yes tylko
#     wypisuje plan. Drugie uruchomienie niczego nie psuje.
# EN: Removes what this script installed, except the world, backups, logs and the horizon user
#     (it owns the world). The mod state (hot-state, hot-keys) first goes into a copy in
#     backups/. System packages stay, as they may serve something else. Without --yes it only
#     prints the plan. Running it again breaks nothing.
uninstall() {
  [ "$(id -u)" = 0 ] || { say "Uruchom przez sudo: … | sudo bash -s -- --uninstall" >&2; exit 2; }
  local units="horizon-hot-hotd.service horizon-hot-proxy.service horizon-hot-pack.service horizon-hot-server.service horizon-hot-backup.timer horizon-hot-backup.service"
  local slices="horizon-bg.slice horizon-game.slice horizon.slice"
  local game_port pack_port path u left=""
  pack_port=$(sed -n 's/.*serve-pack .*--port \([0-9]*\).*/\1/p' /etc/systemd/system/horizon-hot-pack.service 2>/dev/null || true)
  pack_port=${pack_port:-$PACK_PORT}
  # PL: Port graczy ma proxy; przed M8.1b miał go silnik. EN: The proxy holds the players' port; before M8.1b the engine did.
  game_port=$(sed -n 's/.*hot-proxy --listen [^ ]*:\([0-9]*\) .*/\1/p' /etc/systemd/system/horizon-hot-proxy.service 2>/dev/null || true)
  [ -n "$game_port" ] || game_port=$(sed -n 's/^port *= *\([0-9]*\).*/\1/p' "$SRV/server.toml" 2>/dev/null | head -n 1 || true)
  game_port=${game_port:-$PORT}
  local remove=("$BIN" "$HZ" "$HZ_SRC" "$ROOT/src" "$ROOT/tmp" "$ENGINES" "$SHARE" "$KEYS" "$ROOT/.cargo" "$ROOT/.rustup"
    "$ROOT/.ssh" "$ROOT/hot-version" "$ROOT/cargo-horizon-mod-version" "$SRV/hot-core" "$SRV/hot-state"
    "$SRV/hot-keys" "$SRV/horizon-mods/hot_slots.hmod")
  say "Usunę:"
  say "  usługi i slice'y systemd: $units $slices"
  say "  reguły zapory dla portów $game_port i $pack_port (SSH zostaje)"
  for path in "${remove[@]}"; do [ -e "$path" ] && say "  $path"; done
  say "  mod.hot_slots z $SRV/horizon-mods.txt i adres paczki zasobów z $SRV/server.toml"
  [ -e "$SRV/forwarding.secret" ] && say "  $SRV/forwarding.secret"
  [ -e "$SRV/forwarding.secret" ] && [ -f "$SRV/server.toml" ] && say "  silnik za proxy: w server.toml wraca port $game_port, wszystkie adresy i online_mode = true"
  say "Zostawię: świat i ustawienia w $SRV, kopie w $SRV/backups i $BACKUPS, logi w $LOGS, użytkownika horizon."
  if [ "$YES" != 1 ]; then
    say "Nic nie zmienione. Żeby usunąć, uruchom jeszcze raz z --uninstall --yes."
    exit 2
  fi
  if [ -d "$SRV/hot-state" ] || [ -d "$SRV/hot-keys" ]; then
    mkdir -p "$BACKUPS"
    local copy parts=() p
    copy=$BACKUPS/hot-state-przed-odinstalowaniem-$(date +%Y-%m-%d_%H-%M-%S).tar.gz
    for p in hot-state hot-keys; do [ -d "$SRV/$p" ] && parts+=("$p"); done
    OUTPUT=$(tar -czf "$copy" -C "$SRV" "${parts[@]}" 2>&1) \
      || fail HOT-E0113 "The mod state cannot be copied before uninstalling" "tar writes $copy" \
        "see chain; usually a full disk" "Free disk space, then rerun with --uninstall --yes" "nothing was removed"
    chown horizon:horizon "$copy" 2>/dev/null || true
    say "Kopia stanu modów: $copy"
  fi
  for u in $units; do systemctl disable -q --now "$u" 2>/dev/null || true; done
  # PL: Silnik bez proxy sprawdza konta sam; najpierw online_mode, potem adres. Zanim zniknie $BIN.
  # EN: Without the proxy the engine checks accounts itself; online_mode first, then the address. Before $BIN goes.
  if [ -e "$SRV/forwarding.secret" ] && [ -f "$SRV/server.toml" ] && [ -x "$BIN/horizon-server" ]; then
    OUTPUT=$(hz config set server.online_mode=true 2>&1 && hz config set server.bind= "server.port=$game_port" 2>&1) \
      || fail HOT-E0113 "server.toml cannot be switched back to an engine without the proxy" "config set succeeds" \
        "see chain" "Run: sudo -u horizon $BIN/horizon-server --dir $SRV config set server.online_mode=true server.bind= server.port=$game_port; then rerun with --uninstall --yes" \
        "the services are stopped; nothing was removed"
  fi
  rm -f "$SRV/forwarding.secret"
  # PL: Slice bez pliku zostałby aktywny do restartu. EN: A slice without its file stays active until reboot.
  for u in $slices; do systemctl stop "$u" 2>/dev/null || true; done
  for u in $units $slices; do rm -f "/etc/systemd/system/$u"; done
  systemctl daemon-reload
  systemctl reset-failed 2>/dev/null || true
  if command -v ufw >/dev/null; then
    ufw delete allow "$game_port/tcp" >/dev/null 2>&1 || true
    ufw delete allow "$pack_port/tcp" >/dev/null 2>&1 || true
  fi
  for path in "${remove[@]}"; do rm -rf -- "$path" || true; done
  [ -f "$SRV/horizon-mods.txt" ] && sed -i '/^mod\.hot_slots:true$/d' "$SRV/horizon-mods.txt"
  # PL: Tylko nasz adres paczki: bez serwera paczki wymagana paczka nie wpuściłaby graczy.
  # EN: Only our pack address: without the pack server a required pack would keep players out.
  if [ -f "$SRV/server.toml" ] && grep -q "^resource-pack *= *\".*:$pack_port/pack.zip\"" "$SRV/server.toml"; then
    sed -i -e '/^resource-pack *= *".*:'"$pack_port"'\/pack\.zip"/d' -e '/^require-resource-pack *=/d' "$SRV/server.toml"
  fi
  for u in $units $slices; do [ -e "/etc/systemd/system/$u" ] && left+=" /etc/systemd/system/$u"; done
  for path in "${remove[@]}"; do [ -e "$path" ] && left+=" $path"; done
  [ -z "$left" ] || OUTPUT=$left fail HOT-E0113 "Some horizon-hot files cannot be removed" "every listed path is gone" \
    "see chain: a file in use or a read-only file system" "Remove the paths in chain by hand (sudo rm -rf …), then rerun with --uninstall --yes" \
    "the services are stopped; the world and backups are untouched"
  say "Usunięte. Świat i kopie zostały w $ROOT."
  say "Całkiem (to usuwa też świat): sudo userdel horizon && sudo rm -rf $ROOT"
}

if [ "$UNINSTALL" = 1 ]; then uninstall; exit 0; fi

HOT_SHA=
HOT_REBUILT=0
SLOTS_CHANGED=0
ENGINE_CHANGED=0
# PL: Przy `curl | bash` skrypt płynie przez stdin; polecenia w środku nie mogą go czytać.
# EN: With `curl | bash` the script comes through stdin; commands inside must not read it.
main </dev/null
