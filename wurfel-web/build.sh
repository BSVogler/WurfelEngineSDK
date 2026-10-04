#!/bin/bash
# Production build: runs the tests, then writes an optimised static site to wurfel-web/dist/.
# Run the result together with the server, which serves dist/ itself:
#     cargo run --release -p wurfel-server -- --port 3000 --static wurfel-web/dist
set -e
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cargo test -p wurfel-sim -p wurfel-web -p wurfel-server
trunk build --release
du -sh dist/*.wasm
