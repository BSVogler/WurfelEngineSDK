#!/bin/bash
# Compile-checks every part and says which one is broken, with the real compiler messages.
# Trunk's browser overlay only says "cargo build returned a bad status"; this shows why.
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

failed=0
check() {
  local label="$1"; shift
  local output
  if output=$("$@" --message-format short 2>&1); then
    printf '  ok      %s\n' "$label"
  else
    failed=1
    printf '  BROKEN  %s\n' "$label"
    echo "$output" | grep -E "(error|warning)(\[|:)" | sed 's/^/            /'
  fi
}

echo "Checking:"
check "wurfel-sim     game rules shared by client and server" cargo check -p wurfel-sim
check "wurfel-server  game server (native)" cargo check -p wurfel-server
check "wurfel-web     browser client (wasm: this is what trunk builds)" cargo check -p wurfel-web --target wasm32-unknown-unknown
exit $failed
