#!/bin/bash
# Fast development loop. Starts the game server (port 3000) and the client dev server (port 8080).
# The client rebuilds and reloads on every change to wurfel-web or wurfel-sim (about 1 s); the
# server is restarted by running this script again. Open http://127.0.0.1:8080 in two tabs to
# see two players.
#
# Output is labelled [server] or [client]. A "Build failure" overlay in the browser always means
# the CLIENT (wurfel-web, compiled to wasm) failed; the compiler messages are in the [client]
# lines here, or run ./check.sh for a summary of which part is broken.
#
# NO_OPEN=1 ./dev.sh  starts without opening a browser tab.
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

./check.sh || echo "  (starting anyway: fix the BROKEN part above and it will rebuild on save)"
echo

trap 'kill 0 2>/dev/null' EXIT INT TERM
(cd .. && cargo run -p wurfel-server -- --port 3000 2>&1 | sed -u 's/^/[server] /') &
if [ -z "$NO_OPEN" ]; then OPEN=--open; fi
trunk serve $OPEN "$@" 2>&1 | sed -u 's/^/[client] /'
