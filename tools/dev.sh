#!/bin/sh
# The edit loop, `make dev`: rebuild the programs and views cargo says are
# stale (one cargo invocation per program, the feature-unification rule),
# gate the rebuilt views, run the native tests of the crates whose test
# binaries cargo rebuilt, one line per artifact. Freshness is cargo's own
# (`"fresh"` in its JSON messages), never an mtime computed here.
#   env: CARGO, BUILD_TARGET, RELEASE, WASM_BUILD (the Makefile's)
#   tools/dev.sh "<program ...>" "<view ...>"
set -eu
here=$(dirname "$0")
programs=$1
views=$2
mkdir -p "$RELEASE"
log=$RELEASE/dev.log

# <package>: true when cargo rebuilt an artifact of this path package.
rebuilt() {
    grep -q '"reason":"compiler-artifact","package_id":"path+file://[^"]*/'"$1"'#[^"]*".*"fresh":false' "$log"
}
artifact() { echo "$RELEASE/$(echo "$1" | tr - _).wasm"; }
commas() {
    echo "$1" | awk '{ s = $1; r = ""; while (length(s) > 3) { r = "," substr(s, length(s) - 2) r; s = substr(s, 1, length(s) - 3) } print s r }'
}

# A program builds in its own target dir (see the Makefile) and its
# artifact is copied beside the views.
for p in $programs; do
    eval "$WASM_BUILD" --target-dir "$BUILD_TARGET/programs/$p" -p "$p" --features module --message-format=json-render-diagnostics > "$log"
    built="$BUILD_TARGET/programs/$p/wasm32-unknown-unknown/release/$(echo "$p" | tr - _).wasm"
    cmp -s "$built" "$(artifact "$p")" || cp "$built" "$(artifact "$p")"
    if rebuilt "$p"; then
        echo "$p  $(commas "$(wc -c < "$(artifact "$p")")") B"
    else
        echo "$p  unchanged"
    fi
done

for v in $views; do
    a=$(artifact "$v")
    eval "$WASM_BUILD" --target-dir "$BUILD_TARGET" -p "$v" --message-format=json-render-diagnostics > "$log"
    # Fresh, and the artifact is the optimized one a finished gate left.
    if ! rebuilt "$v" && cmp -s "$a" "$a.optimized"; then
        echo "$v  unchanged"
        continue
    fi
    if ! "$here/view-gate.sh" "$v" "$a" > "$log" 2>&1; then
        cat "$log"
        exit 1
    fi
    echo "$v  $(commas "$(wc -c < "$a")") B"
done

# Native tests of the crates whose test binaries cargo rebuilt: a lib edit
# rebuilds them, and so does an edit under tests/ that no wasm build sees.
crates="$programs $views"
packages=$(for c in $crates; do printf -- ' -p %s' "$c"; done)
# shellcheck disable=SC2086
$CARGO test --no-run --message-format=json-render-diagnostics $packages > "$log"
stale=$(for c in $crates; do rebuilt "$c" && printf -- ' -p %s' "$c"; done || true)
if [ -n "$stale" ]; then
    echo "tests:$stale"
    # shellcheck disable=SC2086
    $CARGO test $stale
else
    echo "tests  unchanged"
fi
