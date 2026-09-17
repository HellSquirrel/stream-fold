#!/usr/bin/env bash
# Build every app bundle into www/pkg/<app>/. One wasm module per app, so a
# page loads only what it uses. Apps named *-raw are built without
# wasm-bindgen: plain cargo, then wasm-opt.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
apps=${*:-like-app counter-app studio-app todo-app bench-app like-raw}
wasm_opt=$( { find "$HOME/Library/Caches/.wasm-pack" "$HOME/.cache/.wasm-pack" -path "*bin/wasm-opt" -type f 2>/dev/null || true; } | head -1)
[[ -x $wasm_opt ]] || { echo "wasm-opt not found; run wasm-pack once to fetch it" >&2; exit 1; }
size() { printf "%-12s wasm %7d bytes, brotli %6d\n" "$1" "$(wc -c < "$2")" "$(brotli -c -q 11 "$2" | wc -c)"; }
for app in $apps; do
  if [[ $app == *-raw ]]; then
    (cd "$root" && cargo build -q -p "$app" --release --target wasm32-unknown-unknown)
    mkdir -p "$root/www/pkg/$app"
    # Same passes as the wasm-pack metadata: measured together at about half a kilobyte.
    "$wasm_opt" --detect-features -Oz --converge --strip-producers --strip-target-features --low-memory-unused --zero-filled-memory \
      "$root/target/wasm32-unknown-unknown/release/${app//-/_}.wasm" -o "$root/www/pkg/$app/${app//-/_}.wasm"
    size "$app" "$root/www/pkg/$app/${app//-/_}.wasm"
  else
    # Quiet on success; on failure print everything wasm-pack said and stop,
    # so a broken crate never leaves yesterday's bundle looking freshly built.
    if ! out=$(wasm-pack build "$root/examples/apps/$app" --target web --out-dir "$root/www/pkg/$app" 2>&1); then
      printf '%s\n' "$out" >&2; exit 1
    fi
    size "$app" "$root/www/pkg/$app/${app//-/_}_bg.wasm"
  fi
done
