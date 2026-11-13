#!/usr/bin/env bash
# PL: Start serwera horizon-hot w kontenerze (M8.1c, ADR 0011). Przy każdym starcie robi to, co
#     instalator na Ubuntu (scripts/server-install.sh), ale tylko kroki, które dotyczą danych
#     w wolumenie /srv/horizon: klucze, świat, pula slotów, silnik za proxy. Potem pilnuje
#     procesów zamiast systemd: silnik (horizon-server run), hot-proxy na porcie graczy,
#     serwer paczki i hotd; każdy po awarii wraca po kilku sekundach. SIGTERM (docker stop)
#     zatrzymuje najpierw hotd, potem silnik, który zapisuje świat, na końcu resztę.
#     Ustawienia przychodzą w zmiennych środowiska (compose.yaml). Błędy idą w formacie D27
#     do /srv/horizon/logs/container.jsonl i na wyjście kontenera, opisy w docs/bledy/.
# EN: Starts the horizon-hot server in a container (M8.1c, ADR 0011). On every start it does what
#     the Ubuntu installer does (scripts/server-install.sh), limited to data in the /srv/horizon
#     volume: keys, the world, the slot pool, the engine behind the proxy. Then it watches the
#     processes instead of systemd: the engine (horizon-server run), hot-proxy on the players'
#     port, the pack server and hotd; each comes back a few seconds after a crash. SIGTERM
#     (docker stop) stops hotd first, then the engine, which saves the world, then the rest.
#     Settings come from environment variables (compose.yaml). Errors go in the D27 format to
#     /srv/horizon/logs/container.jsonl and to the container output, described in docs/bledy/.
set -euo pipefail

VERSION=0.1.0
ROOT=/srv/horizon
SRV=$ROOT/server            # PL: folder serwera = HOT_HOME. EN: the server folder = HOT_HOME.
KEYS=$ROOT/keys
LOGS=$ROOT/logs
SHARE=$ROOT/server-repo-files
ENGINES=$ROOT/engines
LOG=$LOGS/container.jsonl
HOT=/opt/horizon-hot
HOTBIN=$HOT/bin
HZBIN=/opt/horizon          # PL: horizon-server i horizon-engine. EN: horizon-server and horizon-engine.
GAME_FILES=/game-files      # PL: folder 26.3 właściciela, tylko do odczytu. EN: the owner's 26.3 folder, read-only.

REPO=${HOT_REPO:-}
ADDRESS=${HOT_ADDRESS:-}
EULA=${HOT_EULA:-}
OP=${HOT_OP:-}
TRUST_NATIVE=${HOT_TRUST_NATIVE_CODE:-0}
LAUNCHER=${HOT_LAUNCHER:-}
PORT=25565
ENGINE_PORT=25566
PACK_PORT=8131
HORIZON_VERSION=${HORIZON_VERSION:-unknown}
HOT_SHA=${HOT_SHA:-unknown}

# --- PL: Dziennik D27 (jak w instalatorze). EN: The D27 log (as in the installer). ------------

say() { printf '[horizon-hot] %s\n' "$*"; }

json_str() {
  printf '"'
  printf '%s' "$1" | LC_ALL=C tr -d '\000-\010\013-\037' \
    | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/\t/\\t/g' \
    | awk 'NR > 1 { printf "\\n" } { printf "%s", $0 }'
  printf '"'
}

# PL: Argumenty: kod, what, expected, cause, fix, state, potem pary klucz=wartość. OUTPUT → chain.
# EN: Arguments: code, what, expected, cause, fix, state, then key=value pairs. OUTPUT → chain.
diag() {
  local code=$1 what=$2 expected=$3 cause=$4 fix=$5 state=$6
  shift 6
  local ctx="" pair fn=${FUNCNAME[1]:-main}
  case "$fn" in fail|wait_for_operator|warn_operator) fn=${FUNCNAME[2]:-main} ;; esac
  for pair in "horizon_version=$HORIZON_VERSION" "hot_sha=$HOT_SHA" "$@"; do
    ctx+="${ctx:+,}$(json_str "${pair%%=*}"):$(json_str "${pair#*=}")"
  done
  local line
  line=$(printf '{"ts":%s,"level":"error","component":"container","version":%s,"code":%s,"what":%s,"expected":%s,"cause":%s,"fix":%s,"state":%s,"doc":%s,"source":%s,"context":{%s},"chain":[%s]}' \
    "$(json_str "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)")" "$(json_str "$VERSION")" "$(json_str "$code")" \
    "$(json_str "$what")" "$(json_str "$expected")" "$(json_str "$cause")" "$(json_str "$fix")" "$(json_str "$state")" \
    "$(json_str "docs/bledy/$code.md")" "$(json_str "scripts/container-entrypoint.sh:$fn")" \
    "$ctx" "$([ -n "${OUTPUT:-}" ] && json_str "$(printf '%s' "$OUTPUT" | tail -n 40)")")
  mkdir -p "$LOGS" 2>/dev/null && printf '%s\n' "$line" >> "$LOG" 2>/dev/null || true
  printf '%s\n' "$line"
  printf '\nBŁĄD %s: %s\nNaprawa: %s\nOpis: docs/bledy/%s.md\n' "$code" "$what" "$fix" "$code" >&2
}

fail() { diag "$@"; exit 1; }

# PL: Właściciel musi coś ustawić; kod 3 odróżnia to od błędu. EN: The owner must set something; exit 3.
wait_for_operator() { diag "$@"; exit 3; }

# PL: Brak czegoś, bez czego serwer i tak działa (np. mody gorące). EN: Something the server runs without.
warn_operator() { diag "$@"; }

as_horizon() {
  setpriv --reuid=horizon --regid=horizon --init-groups \
    env HOME="$ROOT" HOT_HOME="$SRV" PATH="$HOTBIN:$HZBIN:/usr/local/bin:/usr/bin:/bin" "$@"
}

hz() { as_horizon "$HZBIN/horizon-server" --dir "$SRV" "$@"; }

write_if_changed() {
  local path=$1 content=$2
  if [ -f "$path" ] && [ "$(cat "$path")" = "$content" ]; then return 1; fi
  printf '%s\n' "$content" > "$path"
}

ssh_repo() { case "$REPO" in git@*|ssh://*) return 0 ;; *) return 1 ;; esac; }

# --- PL: Ustawienia i katalogi. EN: Settings and folders. -------------------------------------

settings() {
  mkdir -p "$SRV/hot-keys" "$SRV/hot-core" "$SRV/hot-state/pack" "$SRV/horizon-mods" "$LOGS" "$ROOT/backups" "$ENGINES" "$SHARE/.github/workflows"
  chown horizon:horizon "$ROOT" "$SRV" "$SRV/hot-keys" "$SRV/hot-core" "$SRV/hot-state" "$SRV/hot-state/pack" "$SRV/horizon-mods" "$LOGS" "$ROOT/backups" "$ENGINES"
  mkdir -p "$KEYS"
  chmod 0700 "$KEYS"
  [ -n "$REPO" ] || wait_for_operator HOT-E0115 "HOT_REPO is not set: the container does not know the server repository" \
    "HOT_REPO holds the server repository address" "a first start with the example compose.yaml" \
    "Set HOT_REPO in compose.yaml (https://github.com/<owner>/<repo> or git@github.com:<owner>/<repo>.git), then docker compose up -d" \
    "nothing was started" "setting=HOT_REPO"
  [ "$EULA" = true ] || wait_for_operator HOT-E0108 "The Minecraft EULA has not been accepted" \
    "HOT_EULA=true" "only the owner can accept it" \
    "Read https://aka.ms/MinecraftEULA, then set HOT_EULA=true in compose.yaml and docker compose up -d" \
    "nothing was started" "setting=HOT_EULA"
  [ -n "$ADDRESS" ] || wait_for_operator HOT-E0112 "HOT_ADDRESS is not set: the address players use to reach the server is not known" \
    "HOT_ADDRESS holds the IP or name players type" "the game downloads the resource pack from that address; the container does not guess it" \
    "Set HOT_ADDRESS in compose.yaml to the IP or name players type to join, then docker compose up -d" \
    "nothing was started" "setting=HOT_ADDRESS"
  [ -f "$GAME_FILES/horizon-resources.json" ] || wait_for_operator HOT-E0107 \
    "The game files prepared from the owner's Minecraft are not mounted at $GAME_FILES" \
    "$GAME_FILES/horizon-resources.json exists" "a first start: only the owner can prepare them, and the image never holds them" \
    "On a computer with your Minecraft and the Horizon launcher, prepare 26.3, copy that folder to this machine and mount it read-only at $GAME_FILES (compose.yaml), then docker compose up -d" \
    "nothing was started" "dir=$GAME_FILES"
}

# --- PL: Klucze i pliki do repo serwera (instalator, krok 6). EN: Keys and server repo files (installer step 6). --

keys() {
  if [ ! -f "$KEYS/hot-ci" ]; then
    ssh-keygen -q -t ed25519 -N '' -C hot-ci -f "$KEYS/hot-ci"
    say "Nowy klucz podpisu CI: $KEYS/hot-ci."
  fi
  write_if_changed "$SRV/hot-keys/allowed_signers" "hot-ci namespaces=\"horizon-hot-mod\" $(cat "$KEYS/hot-ci.pub")" || true
  chown horizon:horizon "$SRV/hot-keys/allowed_signers"
  if ssh_repo; then
    mkdir -p "$ROOT/.ssh"
    chmod 0700 "$ROOT/.ssh"
    chown horizon:horizon "$ROOT/.ssh"
    [ -f "$ROOT/.ssh/deploy" ] || as_horizon ssh-keygen -q -t ed25519 -N '' -C "horizon-hot deploy container" -f "$ROOT/.ssh/deploy"
  fi
  write_if_changed "$SHARE/.github/workflows/hot.yml" "$(cat "$HOT/hot.yml")" || true
}

# --- PL: Świat (instalator, krok 7). EN: The world (installer step 7). -----------------------

world() {
  if [ ! -f "$SRV/server.toml" ]; then
    local players=(--no-whitelist)
    [ -n "$OP" ] && players=(--whitelist --player "$OP" --op "$OP")
    OUTPUT=$(hz init --eula --world world --port "$ENGINE_PORT" --yes "${players[@]}" 2>&1) \
      || fail HOT-E0108 "horizon-server init cannot create the server in $SRV" "init writes $SRV/server.toml" \
        "see chain" "Fix what chain says, remove $SRV/server.toml if half written, then docker compose restart" \
        "no world was created" "dir=$SRV"
    say "Świat utworzony w $SRV."
  fi
  # PL: Pliki gry czyta prosto z montowania, bez kopii. EN: The game files are read from the mount, not copied.
  OUTPUT=$(hz config set "engine.resources=$GAME_FILES" backups.every=1d backups.keep=7 2>&1) \
    || fail HOT-E0108 "The game files path and backup settings cannot be written to server.toml" "config set succeeds" \
      "see chain" "Fix what chain says, then docker compose restart" "the server did not start" "dir=$SRV"
  install -o horizon -g horizon -m 0644 "$HOT/lib/libhot_core.so" "$SRV/hot-core/libhot_core.so"
}

# --- PL: Pula slotów i paczka (instalator, krok 9). EN: Slot pool and pack (installer step 9). ----

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
  install -o horizon -g horizon -m 0644 "$HOT/hot_slots.hmod" "$SRV/horizon-mods/hot_slots.hmod"
  install -o horizon -g horizon -m 0644 "$HOT/hot_slots.hmod" "$SRV/hot-state/pack/hot_slots.hmod"
  touch "$SRV/horizon-mods.txt"
  grep -qx 'mod.hot_slots:true' "$SRV/horizon-mods.txt" || printf 'mod.hot_slots:true\n' >> "$SRV/horizon-mods.txt"
  chown horizon:horizon "$SRV/horizon-mods.txt"
  OUTPUT=$(set_pack_url "$SRV/server.toml" "http://$ADDRESS:$PACK_PORT/pack.zip" 2>&1) \
    || fail HOT-E0112 "server.toml cannot be updated with the resource pack address" "[properties] has resource-pack and require-resource-pack" \
      "see chain" "Add the two lines under [properties] in $SRV/server.toml by hand (docs/sloty.md), then docker compose restart" \
      "the server did not start" "address=$ADDRESS"
}

# --- PL: Silnik za proxy (instalator, krok 8; ta sama kolejność). EN: The engine behind the proxy (installer step 8; same order). --

# PL: Najpierw sekret, potem adres lokalny i jego kontrola, tryb offline na końcu: przerwany
#     start nie zostawia silnika bez kontroli zakupu (test crates/hot-proxy/tests/installer.rs).
# EN: The secret first, then the local address and its check, offline mode last: a start cut short
#     never leaves the engine without the purchase check (test crates/hot-proxy/tests/installer.rs).
proxy_setup() {
  local secret=$SRV/forwarding.secret
  if [ -z "$(tr -d '[:space:]' 2>/dev/null < "$secret")" ]; then
    OUTPUT=$( (umask 077 && head -c 32 /dev/urandom | base64 | tr -d '/+=\n' > "$secret") 2>&1) || true
  fi
  chown horizon:horizon "$secret" 2>/dev/null || true
  chmod 0600 "$secret" 2>/dev/null || true
  [ -n "$(tr -d '[:space:]' 2>/dev/null < "$secret")" ] || fail HOT-E0114 "The forwarding secret $secret cannot be written" \
    "a non-empty $secret owned by horizon, mode 0600" "see chain: a full disk or a read-only volume" \
    "Free disk space on the volume, then docker compose restart" \
    "server.toml is unchanged and nothing was started" "file=$secret"
  OUTPUT=$(hz config set server.bind=127.0.0.1 "server.port=$ENGINE_PORT" 2>&1) \
    || fail HOT-E0114 "The engine cannot be moved to 127.0.0.1:$ENGINE_PORT" "config set server.bind and server.port succeed" \
      "see chain" "Fix what chain says, then docker compose restart" "nothing was started" "engine_port=$ENGINE_PORT"
  OUTPUT=$(hz config 2>&1) || true
  grep -qx 'server.bind = "127.0.0.1"' <<< "$OUTPUT" || fail HOT-E0114 \
    "The engine is not bound to 127.0.0.1 after config set" 'horizon-server config prints server.bind = "127.0.0.1"' \
    "see chain: another horizon-server version spells the key differently" \
    "Set bind = \"127.0.0.1\" under [server] in $SRV/server.toml, then docker compose restart" "nothing was started"
  OUTPUT=$(hz config set server.online_mode=false 2>&1) \
    || fail HOT-E0114 "online_mode cannot be turned off for the engine behind the proxy" "config set server.online_mode=false succeeds" \
      "see chain" "Fix what chain says, then docker compose restart" "nothing was started"
}

# --- PL: Silnik z hot-host (instalator, krok 13). EN: The engine with hot-host (installer step 13). --

engine_has_hot_host() { grep -q "^path = \"$ENGINES/" "$SRV/server.toml" 2>/dev/null; }

# PL: Kod natywny tylko za zgodą właściciela (HOT_TRUST_NATIVE_CODE=1); bez niej serwer działa bez
#     modów gorących. EN: Native code only with the owner's consent; without it, no hot mods.
hot_engine() {
  engine_has_hot_host && return 0
  if [ "$TRUST_NATIVE" != 1 ]; then
    warn_operator HOT-E0110 "The engine with hot-host is not built: the owner has not set HOT_TRUST_NATIVE_CODE=1" \
      "HOT_TRUST_NATIVE_CODE=1 and HOT_LAUNCHER in compose.yaml" "native code needs the owner's decision (Horizon's rule)" \
      "Read crates/hot-host in horizon-hot, then set HOT_TRUST_NATIVE_CODE=1 and HOT_LAUNCHER in compose.yaml and docker compose up -d" \
      "the server runs without hot mods; hotd is not started"
    return 0
  fi
  if [ -z "$LAUNCHER" ] || [ ! -x "$LAUNCHER" ]; then
    warn_operator HOT-E0110 "horizon-launcher is not available in the container" \
      "HOT_LAUNCHER points at an executable horizon-launcher for Linux x86-64" "native-build lives in the launcher, not in horizon-server" \
      "Put the Linux x86-64 launcher in the volume (for example /srv/horizon/launcher/horizon-launcher) and set HOT_LAUNCHER to it" \
      "the server runs without hot mods; hotd is not started" "launcher=${LAUNCHER:-none}"
    return 0
  fi
  local out=$ENGINES/${HOT_SHA:0:12}-$HORIZON_VERSION bundle=$ROOT/tmp/hot-host-bundle
  rm -rf "$bundle" && mkdir -p "$ROOT/tmp" && chown horizon:horizon "$ROOT/tmp"
  say "Buduję silnik z hot-host (kilka minut)…"
  OUTPUT=$(as_horizon cp -R "$HOT/hot-host-bundle" "$bundle" 2>&1 \
    && as_horizon "$LAUNCHER" install "$HORIZON_VERSION" 2>&1 \
    && as_horizon "$LAUNCHER" native-build "$bundle" --server-output "$out" --version "$HORIZON_VERSION" --trust-native-code 2>&1 \
    && hz config set "engine.path=$out/horizon-engine" 2>&1) \
    || { rm -rf "$out"; warn_operator HOT-E0110 "horizon-launcher native-build cannot build the engine with hot-host" \
      "native-build writes $out/horizon-engine" "see chain: often the launcher has no game files prepared for the horizon user" \
      "Read chain; fix it, then docker compose restart" "the server runs without hot mods; hotd is not started" "out=$out"; }
  rm -rf "$bundle"
}

# --- PL: Pilnowanie procesów. EN: Watching the processes. ------------------------------------

declare -A PID=() CMD=()
STOPPING=0

# PL: exec w podpowłoce: zapisany PID to sam program, więc SIGTERM trafia do niego, nie do basha.
# EN: exec in the subshell: the saved PID is the program itself, so SIGTERM reaches it, not bash.
start() {
  local name=$1
  # shellcheck disable=SC2086
  ( exec setpriv --reuid=horizon --regid=horizon --init-groups \
      env HOME="$ROOT" HOT_HOME="$SRV" PATH="$HOTBIN:$HZBIN:/usr/local/bin:/usr/bin:/bin" ${CMD[$name]} ) </dev/null &
  PID[$name]=$!
  say "$name: pid ${PID[$name]}"
}

stop_one() {
  local name=$1 limit=$2 pid=${PID[$1]:-}
  [ -n "$pid" ] || return 0
  kill -TERM "$pid" 2>/dev/null || return 0
  for _ in $(seq 1 "$limit"); do kill -0 "$pid" 2>/dev/null || return 0; sleep 1; done
  say "$name: brak końca po $limit s, SIGKILL"
  kill -KILL "$pid" 2>/dev/null || true
}

# PL: hotd pierwszy (nie wyśle nic do silnika w trakcie), potem silnik zapisuje świat, na końcu reszta.
# EN: hotd first (it sends nothing to the engine meanwhile), then the engine saves the world, then the rest.
shutdown() {
  STOPPING=1
  say "Zatrzymuję."
  stop_one hotd 20
  stop_one engine 100
  stop_one proxy 10
  stop_one pack 10
  wait 2>/dev/null || true
  say "Zatrzymane."
  exit 0
}

supervise() {
  CMD[engine]="$HZBIN/horizon-server run --dir $SRV"
  CMD[proxy]="$HOTBIN/hot-proxy --listen 0.0.0.0:$PORT --engine 127.0.0.1:$ENGINE_PORT --secret $SRV/forwarding.secret --home $SRV"
  CMD[pack]="$HOTBIN/hotd serve-pack --home $SRV --port $PACK_PORT"
  local git_ssh=""
  ssh_repo && git_ssh="ssh -i $ROOT/.ssh/deploy -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
  export GIT_SSH_COMMAND=$git_ssh
  [ -n "$git_ssh" ] || unset GIT_SSH_COMMAND
  local names=(engine proxy pack)
  if engine_has_hot_host; then
    CMD[hotd]="$HOTBIN/hotd run --home $SRV --repo $REPO --engine $HZBIN/horizon-server --engine-dir $SRV"
    names+=(hotd)
  fi
  trap shutdown TERM INT
  local name done_pid
  for name in "${names[@]}"; do start "$name"; done
  while :; do
    done_pid=
    wait -n -p done_pid || true
    [ "$STOPPING" = 1 ] && break
    [ -n "$done_pid" ] || continue
    for name in "${names[@]}"; do
      if [ "${PID[$name]}" = "$done_pid" ]; then
        say "$name zakończył się; wraca za 5 s."
        sleep 5
        [ "$STOPPING" = 1 ] || start "$name"
      fi
    done
  done
}

main() {
  settings
  keys
  world
  slots_pack
  proxy_setup
  hot_engine
  say "Gracze: $ADDRESS:$PORT (hot-proxy, kontrola zakupu). Silnik: 127.0.0.1:$ENGINE_PORT w kontenerze. Paczka: http://$ADDRESS:$PACK_PORT/pack.zip"
  if ssh_repo; then say "Klucz tylko do odczytu repo (Settings → Deploy keys): $(cat "$ROOT/.ssh/deploy.pub")"; fi
  say "Repo serwera, raz: $SHARE/.github/workflows/hot.yml do .github/workflows/ i sekret HOT_SIGNING_KEY = $KEYS/hot-ci"
  supervise
}

main
