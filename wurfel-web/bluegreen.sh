#!/bin/bash
# Blue/green for local development: you test "blue" while "green" (this working folder, the thing
# you or an agent keep editing, possibly broken) moves on.
#
#   ./bluegreen.sh run        keep blue running: the last promoted build, client and server on one
#                             port (http://127.0.0.1:4000). Switches by itself when you promote.
#   ./bluegreen.sh promote    snapshot the working folder, then build and check it: tests, server,
#                             client, and a smoke run of the new server. Only if all of that passes
#                             does blue switch to it (the old build keeps running until then).
#   ./bluegreen.sh rollback   go back to the previous build.
#   ./bluegreen.sh status     what blue runs now.
#
# Green is ./dev.sh as before (hot reload on its own ports); blue never changes underneath you
# while you test. A promote works from a copy of the files, so editing while it builds is safe.
#
# BLUE_PORT=4000       the port of blue. Its smoke run uses the next one.
# BLUE_MAPS=<folder>   blue's own maps and saves (default <state>/maps), kept apart from green's so
#                      the two never write to the same world. Starts with the default map only.
# BLUEGREEN_DIR=<dir>  where builds live (default ~/.wurfel-bluegreen), with its own cargo cache so
#                      promoting never waits for, or blocks, your normal builds.
# INTRO=1              Caveland story maps start in the crashing spaceship (default: on the ground).
# KEEP=5               how many old builds to keep.
SELF=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
cd "$(dirname "$SELF")/.." || exit 1
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

STATE=${BLUEGREEN_DIR:-$HOME/.wurfel-bluegreen}
BLUE_PORT=${BLUE_PORT:-4000}
BLUE_MAPS=${BLUE_MAPS:-$STATE/maps}
KEEP=${KEEP:-5}
SDK=$(pwd)

die() { echo "bluegreen: $*" >&2; exit 1; }
current() { readlink "$STATE/current" 2>/dev/null; }
previous() { readlink "$STATE/previous" 2>/dev/null; }
label() { [ -n "$1" ] && basename "$1" || echo "(none)"; }

# ----------------------------------------------------------------------------------------- run

stop_server() { # stop_server <pid>: SIGTERM so the world is saved, SIGKILL if it hangs
  local pid=$1 i
  kill "$pid" 2>/dev/null || return 0
  for i in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || { wait "$pid" 2>/dev/null; return 0; }; sleep 0.5; done
  kill -9 "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
}

cmd_run() {
  mkdir -p "$STATE" "$BLUE_MAPS"
  local pid="" release
  trap '[ -n "$pid" ] && stop_server "$pid"; exit' INT TERM
  trap '[ -n "$pid" ] && stop_server "$pid"' EXIT
  echo "blue: http://127.0.0.1:$BLUE_PORT  (maps: $BLUE_MAPS)"
  while true; do
    release=$(current)
    if [ -z "$release" ] || [ ! -x "$release/wurfel-server" ]; then
      echo "blue: no build yet, run ./bluegreen.sh promote"
      while [ -z "$(current)" ]; do sleep 2; done
      continue
    fi
    echo "blue: starting $(label "$release")"
    "$release/wurfel-server" --port "$BLUE_PORT" --maps-dir "$BLUE_MAPS" --static "$release/dist" ${INTRO:+--intro} &
    pid=$!
    while kill -0 "$pid" 2>/dev/null && [ "$(current)" = "$release" ]; do sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then
      echo "blue: switching to $(label "$(current)")"
      stop_server "$pid"
    else
      echo "blue: the server stopped, starting it again in 3 s"
      sleep 3
    fi
    pid=""
  done
}

# ------------------------------------------------------------------------------------- promote

# step "<what>" <command...>: runs in the snapshot, output goes to the log; a failure ends the promote.
step() {
  local what=$1; shift
  printf '  %-34s' "$what"
  if "$@" >>"$LOG" 2>&1; then echo ok; else
    echo FAILED
    echo "---- last lines of the log ($LOG) ----"; tail -n 40 "$LOG"
    fail=1; return 1
  fi
}

smoke() { # start the new server on a spare port with empty maps, run the end-to-end script, stop it
  local port=$((BLUE_PORT + 1)) maps="$REL/smoke-maps" pid ok=1 i
  mkdir -p "$maps"
  "$REL/wurfel-server" --port "$port" --maps-dir "$maps" >"$REL/smoke-server.log" 2>&1 &
  pid=$!
  for i in $(seq 1 40); do curl -s -o /dev/null "http://127.0.0.1:$port/" && break; sleep 0.5; done
  node "$SRC/wurfel-server/smoke.mjs" "http://127.0.0.1:$port" && ok=0
  stop_server "$pid"
  [ "$ok" = 0 ]
}

prune() {
  local keep_cur keep_prev dir n=0
  keep_cur=$(current); keep_prev=$(previous)
  for dir in $(ls -1dt "$STATE"/releases/*/ 2>/dev/null); do
    dir=${dir%/}
    [ "$dir" = "$keep_cur" ] || [ "$dir" = "$keep_prev" ] && continue
    n=$((n + 1))
    [ "$n" -gt "$((KEEP - 2))" ] && rm -rf "$dir"
  done
}

cmd_promote() {
  mkdir -p "$STATE/releases" "$STATE/failed"
  mkdir "$STATE/promote.lock" 2>/dev/null || die "another promote is running ($STATE/promote.lock)"
  trap 'rmdir "$STATE/promote.lock" 2>/dev/null' EXIT

  local sha dirty id fail=0
  sha=$(git rev-parse --short HEAD 2>/dev/null || echo nogit)
  git diff --quiet HEAD 2>/dev/null || dirty=-dirty
  id=$(date +%Y%m%d-%H%M%S)-$sha$dirty
  # The copy lives at one fixed place and is updated in place: tests and the build bake in the path
  # of their sources (CARGO_MANIFEST_DIR), and cargo happily reuses a build made somewhere else, so a
  # copy that moves or is deleted would leave stale paths behind. Updating in place also keeps the
  # file times, so only what changed is rebuilt.
  REL="$STATE/releases/$id"; SRC="$STATE/work"; LOG="$REL/promote.log"
  mkdir -p "$SRC" "$REL"
  rsync -a --delete --exclude target --exclude dist --exclude node_modules \
    Cargo.toml Cargo.lock wurfel-sim caveland-sim wurfel-server wurfel-web "$SRC/" || die "could not copy the working folder"
  export CARGO_TARGET_DIR=${BLUEGREEN_TARGET:-$STATE/target}
  mkdir -p "$CARGO_TARGET_DIR"
  if [ "$(cat "$CARGO_TARGET_DIR/.snapshot-path" 2>/dev/null)" != "$SRC" ]; then
    # Builds of our own crates made from another place: drop them (the downloaded dependencies stay).
    (cd "$SRC" && cargo clean -q -p wurfel-sim -p caveland-sim -p wurfel-server -p wurfel-web) >/dev/null 2>&1
    echo "$SRC" > "$CARGO_TARGET_DIR/.snapshot-path"
  fi

  echo "promote $id (from a copy of $SDK at $SRC)"
  step "tests" bash -c "cd '$SRC' && cargo test -q -p wurfel-sim -p caveland-sim -p wurfel-server -p wurfel-web" &&
  step "server build" bash -c "cd '$SRC' && cargo build -p wurfel-server && cp '$CARGO_TARGET_DIR/debug/wurfel-server' '$REL/wurfel-server'" &&
  step "client build (wasm)" bash -c "cd '$SRC/wurfel-web' && trunk build --dist '$REL/dist'" &&
  step "smoke run of the new server" smoke

  if [ "$fail" != 0 ]; then
    mv "$LOG" "$STATE/failed/$id.log" 2>/dev/null
    rm -rf "$REL"
    echo "NOT promoted: blue still runs $(label "$(current)"). Log: $STATE/failed/$id.log"
    exit 1
  fi
  [ -n "$(current)" ] && ln -sfn "$(current)" "$STATE/previous"
  ln -sfn "$REL" "$STATE/current"
  prune
  echo "promoted: blue now runs $id$([ -n "$(pgrep -f 'bluegreen.sh run')" ] && echo ' (switching now)' || echo ' (start it with ./bluegreen.sh run)')"
}

# ---------------------------------------------------------------------------- rollback / status

cmd_rollback() {
  local cur prev
  cur=$(current); prev=$(previous)
  [ -n "$prev" ] && [ -d "$prev" ] || die "there is no previous build to go back to"
  ln -sfn "$prev" "$STATE/current"
  ln -sfn "$cur" "$STATE/previous"
  echo "rolled back: blue runs $(label "$prev") (was $(label "$cur"))"
}

cmd_status() {
  echo "blue runs : $(label "$(current)")"
  echo "previous  : $(label "$(previous)")"
  echo "url       : http://127.0.0.1:$BLUE_PORT"
  if pgrep -f 'bluegreen.sh run' >/dev/null; then echo "supervisor: running"; else echo "supervisor: not running (./bluegreen.sh run)"; fi
  [ -d "$STATE/failed" ] && [ -n "$(ls -A "$STATE/failed" 2>/dev/null)" ] && echo "last failed promote log: $(ls -1t "$STATE"/failed/* | head -1)"
  return 0
}

case "${1:-}" in
  run) cmd_run ;;
  promote) cmd_promote ;;
  rollback) cmd_rollback ;;
  status) cmd_status ;;
  *) sed -n '2,/^SELF=/p' "$SELF" | sed '$d' | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
