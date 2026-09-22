# Forge replay fixtures

These are the actual `Respond` or `Output` bytes captured while executing
`forge.wasm` on the runtime in `tests/fixtures.rs`. Histories are created and
pushed by real Git through the HTTP rig. They are not Rust-constructed replies.

Each basename below has `.bin` and `.json` files. The compact JSON sidecar records
the exact request (including its Borsh hex), decoded reply when applicable,
response length and SHA-256. Byte arrays preserve Git names/text without lossy
conversion. Rust consumers decode UI binaries as `forge::Reply` and operation
outputs as `forge::OpReply`. The three smart-HTTP fixtures deliberately retain
Git framing (`codec: git`); they are not Borsh and are not UI query replies.

Determinism: fixed actor keys, author/committer names, timestamps/timezones,
content, branch/tag names and harness height progression. Ephemeral paths and
HTTP ports never enter the records or sidecars. The inline blob bound is 64 bytes
so header-only states are small fixtures. The fixture test asserts decoded bytes
re-encode exactly and, by default, compares both files to committed content.

## Regenerate and verify

From the workspace root, with Git and the wasm32 Rust target installed:

```sh
mkdir -p target/forge-a
FORGE_REGENERATE_FIXTURES=1 cargo test -p forge-harness --test fixtures > target/forge-a/fixtures.log 2>&1
fixture_status=$?
printf 'fixture test exit=%s\n' "$fixture_status"
tail -40 target/forge-a/fixtures.log
```

Require exit 0. The test builds the real forge wasm itself, capturing its build
log under `target/forge-a/wasm-test.log`. Commit intended changes, run the same
regeneration command twice, then require a clean `git status --porcelain`.
Without `FORGE_REGENERATE_FIXTURES=1`, the test checks goldens and changes nothing.
A missing fixture fails; there is no ignored test or fallback response.

## Inventory

| Basename (`.bin` + `.json`) | Wire type | State |
| --- | --- | --- |
| `activity` | `Reply` | Repository last activity and answering height. |
| `advertise-receive` | `git smart HTTP` | Raw receive-pack advertisement for a populated repository. |
| `advertise-upload` | `git smart HTTP` | Raw upload-pack v2 capabilities. |
| `blob-binary` | `Reply` | Binary header with no bytes. |
| `blob-empty` | `Reply` | Zero-length text blob. |
| `blob-oversize` | `Reply` | Oversize header with no bytes (inline bound is 64 bytes). |
| `blob-range` | `Reply` | Byte range crossing a newline. |
| `blob` | `Reply` | Complete UTF-8 text bytes, including an unterminated last line. |
| `change-closed` | `Reply` | Closed record. |
| `change-merged` | `Reply` | Merged record and result OID. |
| `change-outdated` | `Reply` | Pinned reviews plus a moved source head. |
| `change-reviewed` | `Reply` | First two submitted reviews with a continuation. |
| `change-reviews-next` | `Reply` | Final review page, including an approval. |
| `change` | `Reply` | Open record with body, channel, current endpoints and empty review page. |
| `changes-empty` | `Reply` | Empty list before any change exists. |
| `changes-filtered` | `Reply` | Closed-state filter. |
| `changes` | `Reply` | Change summary list. |
| `compare-clean` | `Reply` | Clean divergent three-way comparison. |
| `compare-conflicts` | `Reply` | Content conflict with path and kind. |
| `compare-unrelated` | `Reply` | Unrelated histories. |
| `compare-up-to-date` | `Reply` | Source already contained in target. |
| `compare` | `Reply` | Fast-forward comparison. |
| `diff-added` | `Reply` | Added text file. |
| `diff-binary` | `Reply` | Binary file header. |
| `diff-deleted` | `Reply` | Deleted text file and old-side lines. |
| `diff-empty` | `Reply` | Identical endpoints produce no files. |
| `diff-gitlink` | `Reply` | Gitlink entry without a blob read. |
| `diff-mode` | `Reply` | Executable-bit change with no text edits. |
| `diff-next` | `Reply` | Next page, including binary and oversize headers. |
| `diff-oversize` | `Reply` | Oversize file header. |
| `diff-root` | `Reply` | Root commit against the empty tree, including zero-count old range. |
| `diff-text` | `Reply` | Typed context/deleted/added lines, literal ++ x, and no final newline. |
| `diff` | `Reply` | First page of changed file headers/hunks. |
| `judgment-conversation` | `Reply` | Chat-authored thread with a reply; no forge review ID. |
| `judgment-empty` | `Reply` | No outstanding work. |
| `judgment-head-moved` | `Reply` | Requested review becomes pending again after a push. |
| `judgment-replies` | `Reply` | A reply under a submitted review root. |
| `judgment` | `Reply` | Outstanding requested review. |
| `log-next` | `Reply` | Root commit on the final history page. |
| `log-unborn` | `Reply` | Typed not_found refusal for an unborn branch. |
| `log` | `Reply` | First history page, full message/signatures/parents and cursor. |
| `op-change-close` | `OpReply` | Change close receipt. |
| `op-change-edit` | `OpReply` | Change edit receipt. |
| `op-change-open-second` | `OpReply` | The next shared item number. |
| `op-change-open` | `OpReply` | Assigned first item number. |
| `op-merge` | `OpReply` | CAS merge receipt linked to a change. |
| `op-review-approve` | `OpReply` | Batched approval receipt. |
| `op-review-comment` | `OpReply` | Batched comment review receipt. |
| `op-review-request-changes` | `OpReply` | Batched request-changes review receipt. |
| `refs-before-update` | `Reply` | One-ref page whose cursor is later invalidated. |
| `refs-empty` | `Reply` | Unborn repository with no refs. |
| `refs` | `Reply` | First two refs with continuation. |
| `refused-capacity` | `Reply` | Complete commit walk exceeds the founded bound. |
| `refused-invalid-input` | `Reply` | Zero list limit is refused. |
| `refused-not-found` | `Reply` | Repository does not exist. |
| `refused-object-not-held` | `Reply` | Serving node cannot return the requested object. |
| `refused-stale` | `Reply` | Continuation from an older answering height. |
| `repo` | `Reply` | Settings, owner, bounds, counts and granted writer page. |
| `repos-empty` | `Reply` | Empty founded program before Create. |
| `repos` | `Reply` | Repository list with activity/ref count. |
| `tree-directory` | `Reply` | Lazy child-directory page. |
| `tree` | `Reply` | First root directory page with continuation. |
| `upload-refs` | `git smart HTTP` | Raw v2 ls-refs response, including HEAD. |

Loading is view-local and has no program reply bytes. Empty/refused/populated,
continuation, binary/oversize, mergeability and review lifecycle states above
provide the program-owned inputs for a node-less view harness. Screenshots and
host rendering are intentionally outside this crate.
