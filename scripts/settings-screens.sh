#!/usr/bin/env bash
# Foreground offline fixtures rendered by the real host; no node required.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=/home/eddy/dev/ducktape/target-w4 RUSTC_WRAPPER=
export SETTINGS_SCREEN_EXPORT=1
OUT=/home/eddy/dev/ducktape/wt/settings-screens
mkdir -p "$OUT/logs"
run() {
    local name=$1 result attempt
    shift
    for attempt in 1 2 3 4 5; do
        set +e
        "$@" >"$OUT/logs/$name.log" 2>&1
        result=$?
        set -e
        echo "$name exit=$result attempt=$attempt"
        if (( result == 0 )); then return; fi
        tail -40 "$OUT/logs/$name.log"
        if ! rg -q 'SIGSEGV|signal: 11|internal compiler error' "$OUT/logs/$name.log"; then return "$result"; fi
    done
    return "$result"
}
if [[ ${1:-} != --capture-only ]]; then
    run settings-tests cargo test -p settings-view
    (cd /home/eddy/dev/ducktape/wt/app-settings && run host-build cargo build -p ducktape-app)
fi
python3 scripts/settings-screens.py
