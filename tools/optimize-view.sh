#!/bin/sh
# Keep the manifest section and real imports/exports; optimize code only.
set -eu
optimizer=${WASM_OPT:-wasm-opt}
case "$("$optimizer" --version)" in
    "wasm-opt version 132 "*) ;;
    *) echo "View builds require Binaryen wasm-opt 132" >&2; exit 1 ;;
esac
# Cargo can report Fresh after this script replaces its output. Reuse that
# optimized result instead of optimizing an already optimized module again.
if [ -f "$1.optimized" ] && [ "$1.optimized" -nt "$0" ] && cmp -s "$1" "$1.optimized"; then
    exit 0
fi
# The bytes cargo produced, kept beside the optimized artifact for a size
# comparison (`wasm-tools objdump`); names are stripped in both, so
# `make wasm-why` builds `--profile why` for twiggy instead.
cp "$1" "$1.unoptimized"
"$optimizer" "$1" -Oz --flatten --rereloop -Oz --converge \
    --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int \
    --enable-multivalue --enable-reference-types -o "$1.optimized"
cp "$1.optimized" "$1"
