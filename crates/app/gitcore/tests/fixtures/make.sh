#!/usr/bin/env bash
set -euo pipefail
out="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

export GIT_AUTHOR_NAME="Ada Lovelace" GIT_AUTHOR_EMAIL="ada@example.com" GIT_AUTHOR_DATE="1700000000 +0900"
export GIT_COMMITTER_NAME="Charles Babbage" GIT_COMMITTER_EMAIL="charles@example.com" GIT_COMMITTER_DATE="1700000100 -0500"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null

build_repo() {
  local dir="$1" format="$2"
  git init -q --object-format="$format" -b main "$dir"
  (
    cd "$dir"
    git config gc.auto 0
    for i in $(seq 1 300); do echo "line $i of a moderately long file"; done > long.txt
    printf 'hello\n' > hello.txt
    mkdir -p dir
    printf 'nested\n' > dir/nested.txt
    git add .
    git commit -q -m "base"
    git tag -a -m "first release" v1
    sed -i 's/line 150 of/line 150 (changed) of/' long.txt
    printf 'second\n' > dir/second.txt
    git add .
    git commit -q -m "second"
    sed -i 's/line 20 of/line 20 (changed again) of/' long.txt
    git commit -q -am "third"
  )
}

build_repo "$work/sha1" sha1
build_repo "$work/sha256" sha256

(
  cd "$work/sha1"
  git rev-parse HEAD~2 > "$out/base.oid"
  git rev-parse HEAD > "$out/tip.oid"
  git rev-parse HEAD~2 | git pack-objects --revs --stdout > "$out/base.pack"
  printf '%s\n^%s\n' "$(git rev-parse HEAD)" "$(git rev-parse HEAD~2)" | git pack-objects --revs --delta-base-offset --stdout > "$out/ofs.pack"
  printf '%s\n^%s\n' "$(git rev-parse HEAD)" "$(git rev-parse HEAD~2)" | git pack-objects --revs --stdout > "$out/ref.pack"
  printf '%s\n^%s\n' "$(git rev-parse HEAD)" "$(git rev-parse HEAD~2)" | git pack-objects --revs --thin --stdout > "$out/thin.pack"
  git rev-list --objects HEAD~2 | cut -d' ' -f1 | sort > "$out/base.oids"
  git rev-list --objects HEAD ^HEAD~2 | cut -d' ' -f1 | sort > "$out/delta.oids"
  git rev-parse HEAD^{tree} > "$out/tree.oid"
  git cat-file tree HEAD > "$out/tree.bin"
  git rev-parse v1 > "$out/tag.oid"
  git cat-file tag v1 > "$out/tag.bin"
)

(
  cd "$work/sha256"
  git rev-parse HEAD > "$out/sha256.tip.oid"
  git rev-parse HEAD | git pack-objects --revs --delta-base-offset --stdout > "$out/sha256.pack"
  git rev-list --objects HEAD | cut -d' ' -f1 | sort > "$out/sha256.oids"
)

(
  cd "$work/sha1"
  tree="$(git rev-parse HEAD^{tree})"
  p1="$(git rev-parse HEAD)"
  p2="$(git rev-parse HEAD~1)"
  {
    printf 'tree %s\n' "$tree"
    printf 'parent %s\n' "$p1"
    printf 'parent %s\n' "$p2"
    printf 'author Ada Lovelace <ada@example.com> 1700000200 +0900\n'
    printf 'committer Charles Babbage <charles@example.com> 1700000300 -0500\n'
    printf 'encoding UTF-8\n'
    printf 'gpgsig -----BEGIN PGP SIGNATURE-----\n'
    printf ' \n'
    printf ' iQEzBAABCAAdFiEEfakefakefakefakefakefakefakefakefakeFAmVRZ2AACgkQ\n'
    printf ' fakefakefakefakefakefakefakefakefakefakefakefakefakefakefakefakeFAKE=\n'
    printf ' =abcd\n'
    printf ' -----END PGP SIGNATURE-----\n'
    printf '\n'
    printf 'Merge with a signature\n\nBody line.\n'
  } > "$out/commit.bin"
  git hash-object -t commit --literally "$out/commit.bin" > "$out/commit.oid"
)

(
  cd "$work/sha1"
  blob="$(git rev-parse HEAD:hello.txt)"
  sub="$(git rev-parse HEAD:dir)"
  printf '100644 blob %s\tb.txt\n100755 blob %s\ta.sh\n120000 blob %s\tlink\n040000 tree %s\tb-dir\n040000 tree %s\tb\n160000 commit %s\tmodule\n' \
    "$blob" "$blob" "$blob" "$sub" "$sub" "$(git rev-parse HEAD)" \
    | git mktree > "$out/mixed-tree.oid"
  git cat-file tree "$(cat "$out/mixed-tree.oid")" > "$out/mixed-tree.bin"
)

for pack in ofs ref thin; do
  if [ "$pack" = thin ]; then
    git -C "$work/sha1" index-pack --stdin --fix-thin "$work/$pack.pack" < "$out/$pack.pack" >/dev/null
  else
    cp "$out/$pack.pack" "$work/$pack.pack"
    git -C "$work/sha1" index-pack "$work/$pack.pack" >/dev/null
  fi
  git -C "$work/sha1" verify-pack -v "$work/$pack.idx" | grep -qE '^[0-9a-f]{40} (blob|tree|commit) +[0-9]+ [0-9]+ [0-9]+ [0-9]+ [0-9a-f]{40}$' || { echo "no deltas in $pack.pack"; exit 1; }
  git -C "$work/sha1" verify-pack -v "$work/$pack.idx" | grep -v "^/" > "$out/$pack.verify"
done
echo done
