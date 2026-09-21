#!/usr/bin/env bash
# make wasm-repro-check — prove a guest's bytes depend on nothing builder-local.
#
# guest-builder compiles a module out of this repository at a revision, never
# out of the checkout: the module, the SDK and every sibling it reads come from
# one git source, whose location is no part of a symbol hash, and every path
# that could reach the bytes as a panic location (the cargo home, the rustup
# home, the scratch shell, the unpacked revision) is remapped to a fixed token.
#
# It builds ONE module (the smallest — every module shares the same prefixes)
# twice, in two scratch directories, and asserts BOTH that the two artifacts
# are byte-identical and that neither carries a host path. Both assertions
# earn their keep: the two builds share $HOME and the same unpacked revision,
# so a dropped remap flag leaves them identical to each other and only the
# host-path scan catches it, while a scratch path leaking into the bytes is
# caught only by the comparison.
#
# Needs the wasm32-unknown-unknown target, a pushed HEAD and network access
# (the builder itself is installed from ducktape-sdk).
#
# GUEST_BUILDER pins an already-built binary and skips the install;
# GUEST_BUILDER_REV pins the revision it is installed from otherwise, unset
# meaning ducktape-sdk's default branch.
set -euo pipefail

MODULE=${MODULE:-crates/directory}
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# under target/, not /tmp: /tmp is memory-backed on some dev boxes and each
# scratch holds its own wasm32 target dir. Wiped on ENTRY, not on exit, so a
# failure leaves both artifacts to compare.
work="$repo/target/wasm-repro"
rm -rf "$work"
mkdir -p "$work"

builder=${GUEST_BUILDER:-}
if [ -z "$builder" ]; then
  builder_dir="$repo/target/guest-builder-bin"
  cargo install -q --locked --root "$builder_dir" \
    --git https://github.com/ducktape-industries/ducktape-sdk \
    ${GUEST_BUILDER_REV:+--rev "$GUEST_BUILDER_REV"} guest-builder
  builder="$builder_dir/bin/guest-builder"
fi

# --platform names this checkout explicitly: the builder resolves the platform
# from the working directory otherwise, and this script may be run from anywhere.
"$builder" "$repo/$MODULE" --platform "$repo" --scratch "$work/here" --out "$work/here.wasm"
"$builder" "$repo/$MODULE" --platform "$repo" --scratch "$work/there" --out "$work/there.wasm"

if ! cmp "$work/here.wasm" "$work/there.wasm"; then
  echo "wasm-repro-check: $MODULE built in two scratch directories differs — a" >&2
  echo "builder-local path reached the artifact. Compare the embedded strings:" >&2
  echo "  strings $work/here.wasm | grep '^/'" >&2
  exit 1
fi

# `grep | head` exits 0 either way, so the emptiness of the capture is the
# verdict — never the pipeline's status.
leak=$(grep -aoE '/(home|Users)/[^ ]*' "$work/here.wasm" | tr -d '\0' | sort -u | head -5 || true)
if [ -n "$leak" ]; then
  echo "wasm-repro-check: host paths embedded in $MODULE:" >&2
  echo "$leak" >&2
  exit 1
fi

echo "wasm-repro-check: $MODULE is byte-identical from two scratch directories, no host path embedded"
