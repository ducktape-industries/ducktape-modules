#!/bin/sh
# One view's release gate over the artifact cargo built: optimize (keeping
# the pre-wasm-opt bytes beside it), check the bare-WASM ABI, print the
# size against the view's limit and fail over it.
#   tools/view-gate.sh <view> <artifact> <limit-bytes>
set -eu
view=$1
artifact=$2
limit=$3
here=$(dirname "$0")
WASM_OPT="${WASM_OPT:-wasm-opt}" "$here/optimize-view.sh" "$artifact"
python3 "$here/check-view-abi.py" "$artifact"
bytes=$(wc -c < "$artifact")
pct=$(awk "BEGIN { printf \"%.1f\", $bytes * 100 / $limit }")
echo "$view  $bytes / $limit  ($pct%)"
if [ "$bytes" -gt "$limit" ]; then
    echo "$view is over its limit by $((bytes - limit)) bytes: make wasm-why V=$view" >&2
    exit 1
fi
