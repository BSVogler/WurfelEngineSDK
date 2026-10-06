#!/bin/bash
# Fast development loop. Starts the game server (port 3000) and the client dev server (port 8080).
# The client rebuilds and reloads on every change to wurfel-web or wurfel-sim (about 1 s); the
# server rebuilds and restarts on every change to wurfel-server, wurfel-sim or caveland-sim
# (see devserver.sh; everybody is disconnected, pick the world again). Open
# http://127.0.0.1:8080 in two tabs to see two players.
#
# Output is labelled [server] or [client]. A "Build failure" overlay in the browser always means
# the CLIENT (wurfel-web, compiled to wasm) failed; the compiler messages are in the [client]
# lines here, or run ./check.sh for a summary of which part is broken.
#
# NO_OPEN=1 ./dev.sh  starts without opening a browser tab.
# SKIP_INTRO=1 ./dev.sh  Caveland story maps start on the ground instead of in the crashing spaceship.
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

./check.sh || echo "  (starting anyway: fix the BROKEN part above and it will rebuild on save)"
echo

trap 'kill 0 2>/dev/null' EXIT INT TERM
./devserver.sh --port 3000 ${SKIP_INTRO:+--skip-intro} 2>&1 | sed -u 's/^/[server] /' &
if [ -z "$NO_OPEN" ]; then OPEN=--open; fi
trunk serve $OPEN "$@" 2>&1 | sed -u 's/^/[client] /'
