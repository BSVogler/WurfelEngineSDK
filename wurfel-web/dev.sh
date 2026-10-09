#!/bin/bash
# Fast development loop. Starts the game server (port 3000) and the client dev server (port 8080).
# The client rebuilds and reloads on every change to wurfel-web or wurfel-sim (about 1 s). The
# server is rebuilt and restarted when wurfel-server, wurfel-sim or caveland-sim change (checked
# once per second, no extra tools needed): the old one gets SIGTERM, so it saves the world and tells
# the connected pages "server updating", which keep their view and rejoin by themselves. Open
# http://127.0.0.1:8080 in two tabs to see two players.
#
# Output is labelled [server] or [client]. A "Build failure" overlay in the browser always means
# the CLIENT (wurfel-web, compiled to wasm) failed; the compiler messages are in the [client]
# lines here, or run ./check.sh for a summary of which part is broken. A server that does not
# compile shows its errors as [server] lines and starts again when the next change is saved.
#
# NO_OPEN=1 ./dev.sh  starts without opening a browser tab.
# INTRO=1 ./dev.sh       Caveland story maps start in the crashing spaceship (default: on the ground).
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

./check.sh || echo "  (starting anyway: fix the BROKEN part above and it will rebuild on save)"
echo

STAMP=$(mktemp)
trap 'rm -f "$STAMP"; kill 0 2>/dev/null' EXIT INT TERM

label() { sed -u 's/^/[server] /'; }

# Prints a changed server-side source (newer than $STAMP), if there is one.
changed_source() {
  find ../wurfel-server ../wurfel-sim ../caveland-sim ../Cargo.lock -type f \
    \( -name '*.rs' -o -name 'Cargo.*' \) -newer "$STAMP" -print -quit 2>/dev/null
}

wait_for_change() {
  until [ -n "$(changed_source)" ]; do sleep 1; done
}

# Build, run, and when a source changes stop the old server with SIGTERM and start over.
server_loop() {
  local bin="${CARGO_TARGET_DIR:-$PWD/../target}/debug/wurfel-server"
  while true; do
    touch "$STAMP" # changes made while building count too: they restart the server right after
    (cd .. && cargo build -p wurfel-server 2>&1) | label
    if [ "${PIPESTATUS[0]}" -ne 0 ]; then
      echo "[server] build failed: fix it and save to try again"
      wait_for_change
      echo "[server] change detected, rebuilding"
      continue
    fi
    (cd .. && exec "$bin" --port 3000 ${INTRO:+--intro}) > >(label) 2>&1 &
    local pid=$!
    while kill -0 "$pid" 2>/dev/null && [ -z "$(changed_source)" ]; do sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then
      echo "[server] $(changed_source | head -1) changed: restarting the server (SIGTERM, it saves first)"
      kill -TERM "$pid"
      # It closes the players' sockets and exits within a few seconds; KILL only if it hangs.
      for _ in $(seq 1 100); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
      kill -KILL "$pid" 2>/dev/null
      wait "$pid" 2>/dev/null
    else
      wait "$pid" 2>/dev/null
      echo "[server] stopped (exit $?): waiting for a change to start it again"
      wait_for_change
    fi
  done
}

server_loop &
if [ -z "$NO_OPEN" ]; then OPEN=--open; fi
trunk serve $OPEN "$@" 2>&1 | sed -u 's/^/[client] /'
