#!/usr/bin/env bash
# Writes one tree JSON per forge screen, light and dark, under
# target/forge-screens/ for the app's node-less renderer
# (`ducktape-app --render-tree <json>`). No node, no window, no network:
# every screen is replayed from the forge program's own fixture bytes.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/home/eddy/dev/ducktape/target-w3} RUSTC_WRAPPER=
export FORGE_SCREEN_EXPORT=1
OUT=target/forge-screens
mkdir -p "$OUT"
log=$OUT/export.log
for attempt in 1 2 3 4 5; do
    set +e
    cargo test -p forge-view export_forge_screens >"$log" 2>&1
    result=$?
    set -e
    echo "forge-screens exit=$result attempt=$attempt"
    if ((result == 0)); then
        ls "$OUT"/*.json
        exit 0
    fi
    tail -40 "$log"
    grep -qE 'SIGSEGV|signal: 11|internal compiler error' "$log" || exit "$result"
done
exit "$result"
