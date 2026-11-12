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
HZ=$ROOT/horizon            # PL: Horizon z oficjalnego install.sh. EN: Horizon from its install.sh.
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
RESOURCES=
LAUNCHER=
TRUST_NATIVE=0
EULA=0
PORT=25565
PACK_PORT=8131
OP=
TEST_NO_HORIZON=0

USAGE="Użycie: server-install.sh --repo URL --eula [opcje]

  --repo URL              repo serwera na GitHubie (https://… publiczne, git@github.com:… prywatne)
  --eula                  akceptujesz EULA Minecrafta: https://aka.ms/MinecraftEULA
  --resources DIR         folder 26.3 z plikami gry przygotowanymi z Twojego Minecrafta
  --trust-native-code     przeczytałeś kod crates/hot-host i ufasz mu: buduje silnik z hot-host
  --launcher PATH         horizon-launcher dla Linuxa x86-64 (domyślnie z PATH)
  --op NAME               pierwszy operator; włącza whitelistę z tym graczem
  --port N                port gry (domyślnie 25565)
  --hot-ref REF           wersja horizon-hot: gałąź, tag albo commit (domyślnie main)
  --hot-source URL        skąd pobrać horizon-hot (domyślnie GitHub)
  --horizon-version TAG   Nightly Horizona (domyślnie $HORIZON_VERSION)
  --test-no-horizon       TYLKO DO TESTÓW: pomija Horizona (pobranie, pliki gry, świat, silnik),
                          żeby sprawdzić resztę na maszynie bez Linuxa x86-64"

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

while [ $# -gt 0 ]; do
  case "$1" in
    --repo) REPO=${2:?"--repo potrzebuje adresu"}; shift 2 ;;
    --eula) EULA=1; shift ;;
    --resources) RESOURCES=${2:?"--resources potrzebuje folderu"}; shift 2 ;;
    --trust-native-code) TRUST_NATIVE=1; shift ;;
    --launcher) LAUNCHER=${2:?"--launcher potrzebuje ścieżki"}; shift 2 ;;
    --op) OP=${2:?"--op potrzebuje nazwy gracza"}; shift 2 ;;
    --port) PORT=${2:?"--port potrzebuje numeru"}; shift 2 ;;
    --hot-ref) HOT_REF=${2:?"--hot-ref potrzebuje gałęzi, tagu albo commita"}; shift 2 ;;
    --hot-source) HOT_SOURCE=${2:?"--hot-source potrzebuje adresu"}; shift 2 ;;
    --horizon-version) HORIZON_VERSION=${2:?"--horizon-version potrzebuje tagu"}; shift 2 ;;
    --test-no-horizon) TEST_NO_HORIZON=1; shift ;;
    -h|--help) say "$USAGE"; exit 0 ;;
    *) say "Nieznana opcja: $1" >&2; say "$USAGE" >&2; exit 2 ;;
  esac
done
[ -n "$REPO" ] || { say "Brakuje --repo." >&2; say "$USAGE" >&2; exit 2; }

# PL: Ponowne uruchomienie z tymi samymi opcjami (do komunikatów). EN: The rerun command.
RERUN="curl -fsSL https://raw.githubusercontent.com/Kamciosz/horizon-hot/main/scripts/server-install.sh | sudo bash -s -- --repo $REPO --eula${OP:+ --op $OP}"

# --- PL: Krok 1: sprawdzenie maszyny. EN: Step 1: checking the machine. -----------------------

port_owner() { ss -Hltnp "sport = :$1" 2>/dev/null | head -n 1; }

preflight() {
  step 1/11 "Sprawdzenie maszyny"
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
  local p owner
  for p in "$PORT" "$PACK_PORT"; do
    owner=$(port_owner "$p")
    # PL: Port zajęty przez nasz własny serwer przy ponownym uruchomieniu jest w porządku.
    # EN: A port held by our own server on a rerun is fine.
    if [ -n "$owner" ] && ! systemctl is-active -q horizon-hot-server 2>/dev/null; then
      OUTPUT=$owner fail HOT-E0102 "TCP port $p is already in use by another program" \
        "port $p is free" "another Minecraft server or program listens on it" \
        "Stop that program (ss -ltnp 'sport = :$p' shows it) or rerun with --port N" \
        "nothing was changed" "port=$p"
    fi
  done
  say "Ubuntu $ver, $arch, ${mem_gb} GB RAM, ${disk_gb} GB wolnego miejsca, porty $PORT i $PACK_PORT wolne."
}

# --- PL: Krok 2: użytkownik i katalogi. EN: Step 2: user and folders. -----------------------

user_and_dirs() {
  step 2/11 "Użytkownik horizon i katalogi w $ROOT"
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
  step 3/11 "Pakiety systemowe"
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

horizon() {
  step 4/11 "Horizon $HORIZON_VERSION"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Pominięty (--test-no-horizon)."; return; fi
  if [ "$(readlink "$HZ/current" 2>/dev/null)" = "$HZ/releases/$HORIZON_VERSION" ] && [ -x "$BIN/horizon-server" ]; then
    say "Już jest."; return
  fi
  local script=$ROOT/tmp/horizon-install.sh
  OUTPUT=$(curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.sh -o "$script" 2>&1 \
    && chown horizon:horizon "$script" \
    && as_horizon sh "$script" --version "$HORIZON_VERSION" --dir "$HZ" --bin "$BIN" 2>&1) \
    || fail HOT-E0105 "Horizon's official install.sh cannot install $HORIZON_VERSION" \
      "install.sh puts horizon-server into $BIN" "no network, a tag that does not exist, or a failed checksum" \
      "Check the tag on https://github.com/horizon-rust/horizon/releases, then rerun with --horizon-version TAG" \
      "the Horizon release already in use (if any) stays" "dir=$HZ"
  say "Zainstalowany w $HZ."
}

# --- PL: Krok 5: horizon-hot ze źródeł. EN: Step 5: horizon-hot from source. -----------------

hot_build() {
  step 5/11 "horizon-hot ($HOT_REF): hotd, hot-core, hot-diag"
  if [ ! -x "$ROOT/.cargo/bin/rustup" ]; then
    say "Instaluję Rusta dla użytkownika horizon (rustup)…"
    OUTPUT=$(curl -fsSL https://sh.rustup.rs -o "$ROOT/tmp/rustup.sh" 2>&1 && chown horizon:horizon "$ROOT/tmp/rustup.sh" \
      && as_horizon sh "$ROOT/tmp/rustup.sh" -y --profile minimal --default-toolchain none --no-modify-path 2>&1) \
      || fail HOT-E0106 "rustup cannot be installed for the horizon user" "rustup ends in $ROOT/.cargo/bin" \
        "no network to static.rust-lang.org" "Check the network, then rerun the installer" \
        "nothing of horizon-hot was built" "step=rustup"
  fi
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
  if [ "$(cat "$ROOT/hot-version" 2>/dev/null)" = "$sha" ] && [ -x "$BIN/hotd" ] && [ -f "$SRV/hot-core/libhot_core.so" ]; then
    say "Już zbudowany: ${sha:0:7}."; return
  fi
  as_horizon git -C "$SRC" checkout -q --detach "$sha"
  say "Buduję ${sha:0:7} (kilka minut przy pierwszym razie)…"
  OUTPUT=$(cd "$SRC" && as_horizon rustup toolchain install 2>&1 && as_horizon cargo build --release -q -p hotd -p hot-core -p hot-diag 2>&1) \
    || fail HOT-E0106 "horizon-hot ${sha:0:7} does not compile on this server" "cargo build --release succeeds" \
      "see chain: usually a missing system package or not enough memory" \
      "Fix what chain names (sudo apt install build-essential; free memory), then rerun the installer" \
      "the version built before stays" "step=cargo" "commit=$sha"
  install -o horizon -g horizon -m 0755 "$SRC/target/release/hotd" "$SRC/target/release/hot-diag" "$BIN/"
  install -o horizon -g horizon -m 0644 "$SRC/target/release/libhot_core.so" "$SRV/hot-core/libhot_core.so"
  printf '%s\n' "$sha" > "$ROOT/hot-version"
  say "Zbudowany ${sha:0:7}: $BIN/hotd, $SRV/hot-core/libhot_core.so."
}

# --- PL: Krok 6: klucze i pliki do repo serwera. EN: Step 6: keys and server repo files. -----

ssh_repo() { case "$REPO" in git@*|ssh://*) return 0 ;; *) return 1 ;; esac; }

keys() {
  step 6/11 "Klucze i pliki do repo serwera"
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
  step 7/11 "Pliki gry i świat"
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
      "everything before the world is installed" "port=$PORT"
    local players=(--no-whitelist)
    [ -n "$OP" ] && players=(--whitelist --player "$OP" --op "$OP")
    OUTPUT=$(hz init --eula --world world --port "$PORT" --yes "${players[@]}" 2>&1) \
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

# --- PL: Krok 8: usługi systemd. EN: Step 8: systemd units. ----------------------------------

units() {
  step 8/11 "Usługi systemd"
  local changed=0 git_ssh=""
  ssh_repo && git_ssh="Environment=\"GIT_SSH_COMMAND=ssh -i $ROOT/.ssh/deploy -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new\""
  write_if_changed /etc/systemd/system/horizon-hot-server.service "[Unit]
Description=Horizon server with horizon-hot (horizon-server run)
After=network-online.target
Wants=network-online.target

[Service]
User=horizon
Group=horizon
WorkingDirectory=$SRV
Environment=HOT_HOME=$SRV
ExecStart=$BIN/horizon-server run --dir $SRV
Restart=on-failure
RestartSec=5
TimeoutStopSec=120

[Install]
WantedBy=multi-user.target" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-hotd.service "[Unit]
Description=horizon-hot hotd: mods from the server repository
After=horizon-hot-server.service network-online.target
Wants=network-online.target

[Service]
User=horizon
Group=horizon
Environment=HOT_HOME=$SRV
$git_ssh
ExecStart=$BIN/hotd run --home $SRV --repo $REPO --engine $BIN/horizon-server --engine-dir $SRV
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target" && changed=1
  write_if_changed /etc/systemd/system/horizon-hot-backup.service "[Unit]
Description=horizon-hot: copy of hot-state and hot-keys (worlds: horizon-server [backups])

[Service]
Type=oneshot
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
    if [ "$TEST_NO_HORIZON" = 0 ]; then
      systemctl enable -q horizon-hot-server
      if [ "$changed" = 1 ]; then systemctl restart horizon-hot-server; else systemctl start horizon-hot-server; fi
    fi
    # PL: hotd rusza dopiero przy silniku z hot-host; bez niego commity z modami byłyby
    #     oznaczane jako nieudane. EN: hotd starts only with a hot-host engine.
    if engine_has_hot_host || [ "$TEST_NO_HORIZON" = 1 ]; then
      systemctl enable -q horizon-hot-hotd
      if [ "$changed" = 1 ]; then systemctl restart horizon-hot-hotd; else systemctl start horizon-hot-hotd; fi
    fi
  } 2>&1) || fail HOT-E0109 "systemd cannot start the horizon-hot services" "systemctl enable --now succeeds" \
    "see chain; journalctl -u horizon-hot-server -n 50 shows why a unit stopped" \
    "Run journalctl -u horizon-hot-server -u horizon-hot-hotd -n 80, fix the cause, then rerun the installer" \
    "the unit files are in /etc/systemd/system" "units=horizon-hot-server,horizon-hot-hotd,horizon-hot-backup"
  say "Włączone: horizon-hot-backup.timer$([ "$TEST_NO_HORIZON" = 0 ] && printf ', horizon-hot-server')$( (engine_has_hot_host || [ "$TEST_NO_HORIZON" = 1 ]) && printf ', horizon-hot-hotd')."
}

# --- PL: Krok 9: zapora. EN: Step 9: firewall. ----------------------------------------------

firewall() {
  step 9/11 "Zapora (ufw)"
  local p
  # PL: Najpierw SSH na każdym porcie, na którym słucha sshd, żeby nie odciąć operatora.
  # EN: SSH first, on every port sshd listens on, so the operator is never locked out.
  for p in $(ss -Hltnp 2>/dev/null | awk '/"sshd"/ { n = split($4, a, ":"); print a[n] }' | sort -u) 22; do
    ufw allow "$p/tcp" >/dev/null
  done
  ufw allow "$PORT/tcp" >/dev/null
  ufw allow "$PACK_PORT/tcp" >/dev/null
  OUTPUT=$(ufw --force enable 2>&1) || fail HOT-E0109 "ufw cannot be enabled" "ufw --force enable succeeds" \
    "see chain: often a kernel without netfilter in a container" "Run: sudo ufw status verbose; fix the cause, then rerun" \
    "the firewall rules are added but may be inactive"
  say "Otwarte: SSH, $PORT (gra), $PACK_PORT (paczka zasobów)."
}

# --- PL: Krok 10: silnik z hot-host. EN: Step 10: the engine with hot-host. ------------------

engine_has_hot_host() {
  [ -f "$SRV/server.toml" ] && grep -q "^path = \"$ENGINES/" "$SRV/server.toml" 2>/dev/null
}

engine() {
  step 11/11 "Silnik z hot-host"
  if [ "$TEST_NO_HORIZON" = 1 ]; then say "Pominięty (--test-no-horizon)."; return; fi
  local out=$ENGINES/${HOT_SHA:0:12}-$HORIZON_VERSION
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
  local bundle=$ROOT/tmp/hot-host-bundle
  rm -rf "$bundle"
  as_horizon sh "$SRC/crates/hot-host/build-bundle.sh" "$bundle" >/dev/null
  say "Buduję silnik (kilka minut)…"
  OUTPUT=$(as_horizon "$LAUNCHER" install "$HORIZON_VERSION" 2>&1 \
    && as_horizon "$LAUNCHER" native-build "$bundle" --server-output "$out" --version "$HORIZON_VERSION" --trust-native-code 2>&1) \
    || { rm -rf "$out"; fail HOT-E0110 "horizon-launcher native-build cannot build the engine with hot-host" \
      "native-build writes $out/horizon-engine" \
      "see chain: often the launcher's version is not prepared (no game files for the horizon user)" \
      "Prepare once as the horizon user: sudo -u horizon -H $LAUNCHER account sign-in, then sudo -u horizon -H $LAUNCHER prepare --download; rerun with --trust-native-code" \
      "the server runs on its old engine without hot mods" "out=$out"; }
  OUTPUT=$(hz config set "engine.path=$out/horizon-engine" 2>&1 && systemctl restart horizon-hot-server 2>&1) \
    || fail HOT-E0110 "The server cannot switch to the new engine" "config set engine.path and restart succeed" \
      "see chain" "Run: sudo -u horizon $BIN/horizon-server --dir $SRV config set engine.path=$out/horizon-engine" \
      "the engine was built but the server may still run the old one" "out=$out"
  systemctl enable -q --now horizon-hot-hotd
  say "Silnik z hot-host: $out."
}

# --- PL: Krok 11: autotest. EN: Step 11: self test. -----------------------------------------

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
    for _ in $(seq 1 60); do
      hz status --json 2>/dev/null | grep -q '"phase": *"running"' && { ok=1; break; }
      sleep 2
    done
    [ "$ok" = 1 ] || OUTPUT=$(journalctl -u horizon-hot-server -n 60 --no-pager 2>&1) fail HOT-E0111 \
      "The server did not reach the running phase within 120 s" "horizon-server status --json says running" \
      "see chain (the service log)" "Read journalctl -u horizon-hot-server -n 200, fix the cause, then rerun the installer" \
      "the services stay installed"
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
  units
  firewall
  autotest 10/11 "Autotest serwera"
  engine
  # PL: Po zbudowaniu silnika test jeszcze raz, już z hot-core. EN: Test again, now with hot-core.
  if engine_has_hot_host; then autotest 11/11 "Autotest z hot-core"; fi
  summary
}

HOT_SHA=
# PL: Przy `curl | bash` skrypt płynie przez stdin; polecenia w środku nie mogą go czytać.
# EN: With `curl | bash` the script comes through stdin; commands inside must not read it.
main </dev/null
