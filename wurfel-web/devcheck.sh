#!/bin/sh
# Trunk pre-build hook (see Trunk.toml). Trunk's browser overlay only says "cargo exited with 101".
# This runs the same cargo build first (so Trunk's own build is then a no-op), and on failure
# writes the compiler messages to dist/build-error.txt, which devoverlay.js shows in the page.
cd "$(dirname "$0")" || exit 0
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
mkdir -p dist
if out=$(cargo build --target=wasm32-unknown-unknown --manifest-path Cargo.toml --color never 2>&1); then
  rm -f dist/build-error.txt
else
  printf '%s\n' "$out" > dist/build-error.txt
  # The page in dist may predate devoverlay.js (no build has succeeded since it was added), so make
  # sure the stale page loads it too.
  if [ -f dist/index.html ] && ! grep -q devoverlay.js dist/index.html; then
    cp devoverlay.js dist/devoverlay.js
    sed -i.bak 's#</body>#<script src="devoverlay.js"></script></body>#' dist/index.html && rm -f dist/index.html.bak
  fi
  printf '%s\n' "$out" >&2
fi
exit 0
