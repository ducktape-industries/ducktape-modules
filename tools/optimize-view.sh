#!/bin/sh
# Keep the manifest section and real imports/exports; optimize code only.
set -eu
optimizer=${WASM_OPT:-wasm-opt}
case "$("$optimizer" --version)" in
    "wasm-opt version 132 "*) ;;
    *) echo "View builds require Binaryen wasm-opt 132" >&2; exit 1 ;;
esac
"$optimizer" "$1" -Oz --flatten --rereloop -Oz --converge \
    --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int \
    --enable-multivalue --enable-reference-types -o "$1.optimized"
mv "$1.optimized" "$1"
