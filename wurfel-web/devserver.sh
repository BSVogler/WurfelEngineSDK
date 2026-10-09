#!/bin/bash
# Runs the game server and restarts it whenever its code changes (wurfel-server, wurfel-sim and
# caveland-sim). Used by ./dev.sh, which only restarts the browser side by itself.
#
#   ./devserver.sh [wurfel-server arguments]     e.g.  ./devserver.sh --port 3000 --intro
#
# A restart disconnects everybody (the world is kept: it is saved on shutdown), so the browser
# shows "connection lost" and you pick the world again. A failed build prints the compiler
# messages and keeps waiting: the next save rebuilds, and the old server is left stopped until
# then. Needs no extra tools, it polls the source files once a second.
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

WATCH=(wurfel-server wurfel-sim caveland-sim Cargo.toml)
marker=$(mktemp)
server_pid=""

stop_server() {
  if [ -n "$server_pid" ] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null
    wait "$server_pid" 2>/dev/null
  fi
  server_pid=""
}
trap 'stop_server; rm -f "$marker"; exit' EXIT INT TERM

# Something under the watched folders is newer than the last build started.
changed() {
  find "${WATCH[@]}" \( -name target -o -name fixtures \) -prune -o -type f \( -name '*.rs' -o -name Cargo.toml \) -newer "$marker" -print -quit 2>/dev/null | grep -q .
}

binary() {
  local target
  target=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])') || return 1
  echo "$target/debug/wurfel-server"
}

while true; do
  touch "$marker" # saves after this point count as new changes
  echo "building the server..."
  if cargo build -p wurfel-server --color never 2>&1 && bin=$(binary) && [ -x "$bin" ]; then
    echo "starting the server"
    "$bin" "$@" &
    server_pid=$!
    while ! changed; do
      if ! kill -0 "$server_pid" 2>/dev/null; then
        echo "the server stopped; waiting for a change to start it again"
        server_pid=""
        while ! changed; do sleep 1; done
        break
      fi
      sleep 1
    done
    [ -n "$server_pid" ] && echo "change detected, restarting the server"
    stop_server
  else
    echo "SERVER BUILD FAILED, fix it and save to try again"
    while ! changed; do sleep 1; done
    echo "change detected"
  fi
done
