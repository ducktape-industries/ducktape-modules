# Forge read and review contract

The source of truth is `src/contract.rs`, `src/read_contract.rs`, and
`src/review_contract.rs`, re-exported by `forge`. All forge requests, records,
UI replies, and new operation outputs use Borsh. Serde is for fixture sidecars
only. No legacy wire/layout conversion exists. Found this version with its new
`Bounds`; do not seat it over an older forge layout.

## Decisions in one screen

- `gitcore` reads loose commit/tree/blob objects through the sandbox; queries never
  parse packs. No gix, host Git mirror, or forge-specific host storage is needed.
- Reads run off consensus. Every change/review mutation is an operation. Merge
  objects are computed and published by the client; `Merge` checks both endpoint
  OIDs and atomically changes the target ref and optional change record. It does
  not inspect locally held objects or enforce review verdicts.
- Changes keep their title, body, author key, and lifecycle on the forge record.
  A repository's number counter allocates the shared future issue/change number space.
- Every table is declared once in `src/state.rs` as a typed `store::Map`/`Set`/`Item`:
  records (repos, refs, changes, reviews), the indexes over them (activity,
  involvement, authored reviews) and the counters. A remove path takes its index
  rows with it. Git objects are the one exception: an object is the blob whose
  id is its oid, with no table between them (`src/objects.rs`).
- Forge queues chat creation for `forge:<repo>:<n>` and system lines for opening,
  closing, merging, and submitting a review. Chat owns all conversation replies.
  The host commits the record and emitted queue items atomically; delivery is in
  the **next block**, per the present ABI, not synchronous cross-program execution.
- A review is one immutable operation: verdict, body, pinned head and base, and
  up to `MAX_REVIEW_COMMENTS` line comments. An anchor is `(path, side, line)`;
  `Old` addresses `base_oid`, `New` addresses `commit_oid`. Both are retained.
  Compare the review's head to `Change.source_head` to show outdated; never move
  positions. A stale draft may be submitted against its original pin.
- All UI lists take `cursor` and `limit`; all UI replies, including refusals,
  carry the answering `height`. No unbounded nested review history is returned.
- Chat's colon namespace is reserved to its exact program prefix (or System).
  External keys/accounts cannot create these channels or reserve system-message
  IDs. Hide colon channels in the view's rail/search; they are not private rooms.

## Common types and pagination

`Revision = Ref(Vec<u8>) | Oid(String)`. Refs are full names such as
`refs/heads/main`. OIDs are full hexadecimal IDs of the repo's hash algorithm;
replies normalize them to lowercase. Paths, Git names, messages and source lines
are byte vectors, never lossy UTF-8 conversions. Tree root is `[]`, not `/`.

`Page<T> = { items: Vec<T>, next: Option<Cursor> }`. Pass `next` back unchanged.
The cursor is bound to the original query arguments and height; changing only
`limit` is allowed. A changed height returns `stale`, so restart the list.
Filtered change/judgment pages can be empty **with a next cursor**: continue until
`next == None`. Limits must be `1..=Bounds.page_size`; they are not silently clamped.

Repos sort by descending activity height then name. Refs use byte-name order,
trees use Git entry order, diffs use path-byte order, changes use ascending item
number, and reviews use submission order. Judgment scans repo then item number.
Log covers all parents, ordered by descending committer time then OID, matching
`gitcore`'s walk (not a promise of topological order under clock skew).

## Queries and examples

In these Rust-shaped examples, `r = "project".into()`, `main` and `feature` are
`Revision::Ref(b"refs/heads/main".to_vec())` and the corresponding feature ref;
`a` and `b` are real full commit IDs; `blob` is a tree entry's blob ID. `None, 20`
means a first page; continue with the returned cursor. Encode with `abi::encode`
and decode a `forge::Reply` from the concatenated `Respond` bytes.

| Query example | Reply data besides `height` |
| --- | --- |
| `Repos { cursor: None, limit: 20 }` | `page<RepoInfo>`: name, hash, owner, settings, ref count, last activity |
| `Repo { repo: r, cursor: None, limit: 20 }` | repo record, founded bounds, `writers: Page<Vec<u8>>` (owner is separate) |
| `Refs { repo: r, cursor: None, limit: 20 }` | `page<RefInfo>`: full ref name and target OID |
| `Log { repo: r, from: feature, cursor: None, limit: 20 }` | resolved tip, `page<CommitInfo>`: OID, tree, all parents, full message, author and committer with time/timezone |
| `Tree { repo: r, at: b, path: b"src".to_vec(), cursor: None, limit: 20 }` | resolved tree OID and `page<TreeInfo>`: name, OID, kind/mode; `at` accepts commit or tree |
| `Blob { repo: r, oid: blob, range: Some(ByteRange { offset: 0, len: 100 }) }` | OID, total size, classification, actual byte range and bytes |
| `Diff { repo: r, base: Some(a), head: b, path: None, cursor: None, limit: 20 }` | normalized endpoints, total matching file count, `page<FileDiff>` |
| `Compare { repo: r, from: feature, into: main }` | resolved endpoints, merge base, ahead/behind, mergeability |
| `Activity { repo: r }` | `last_height`: last successful forge operation in the repo |
| `Changes { repo: r, filter: ChangeFilter { state: Some(ChangeState::Open), ..Default::default() }, cursor: None, limit: 20 }` | `page<ChangeSummary>` with author, endpoints, counts and historical verdict totals |
| `Change { repo: r, n: 1, cursor: None, limit: 20 }` | full change record/body/channel, optional current source/target heads, `reviews: Page<Review>` |
| `Judgment { key: reviewer_key, cursor: None, limit: 20 }` | `page<Judgment>`: open changes requesting this key at the current head or containing an answered thread it authored |
| `Advertise { repo: r, service: Service::UploadPack }` | raw Git protocol v2 advertisement |
| `Upload { repo: r, request: git_v2_request_bytes }` | streamed raw Git protocol response/pack |

`Advertise` and `Upload` are the existing smart-HTTP protocol, with Git's own
framing. They cannot carry a Borsh height envelope without breaking real Git.
They are not UI list APIs. Fetch/walk bounds continue to apply to them.

An unborn ref returns `not_found`; an empty directory/list is a successful empty
page. Missing current change refs are `None` on the detail, so a deleted branch
does not make the conversation or review history unreadable.

`ChangeFilter` combines optional state, author key, and involvement key with AND.
Involvement means author, requested reviewer, or submitted reviewer. A reviewer
taken off the request stays involved only if they submitted a review. Chat-only participation is represented by chat itself.
Judgment additionally joins ordinary chat-authored threads and **all** reviews
by the key, not just its latest review. The newest answered root is returned as
`ReplyAttention { review: Option<u64>, root_seq, last_reply_seq }`; `None` identifies
a conversation root. Requested judgment clears when that key submits any verdict
at the current source head, and returns when the head moves. Read/unread markers
and dismissals are local view state, not notification records or new operations.
Chat resolves a requested key to its current identity account as well as its raw
key handle. Forge author/filter keys remain exact signing keys.

## Blobs, diffs, and mergeability

`Blob` returns `Text`, `Binary` (NUL or invalid UTF-8), or `Oversize` (total size
above `Bounds.blob_bytes`). Binary/oversize replies have a header and no content
bytes, even when a small range was requested. Text ranges use byte offsets,
may split a UTF-8 code point, and clamp their end at EOF. Zero-length text is valid.

`Diff` compares trees directly; use `base: None` for a root commit. For a change's
merge-base diff, first use `Compare.base`, then `Diff { base, head: from, ... }`.
An optional path selects one exact changed file. Each file carries old/new paths,
OIDs, modes, sizes, status, content classification, additions/deletions and hunks.
Renames appear as delete/add, with no heuristic rename tracking. Gitlinks have
headers, not blob reads. Binary/oversize files have no hunks; line stats are zero
and not applicable to those classifications.

Hunks have three context lines on each edge. A range starts at a **one-based**
line, except a zero-length range names the preceding line (0 at file start).
`DiffLine` is typed `Context | Added | Deleted`, with optional old/new line numbers
and exact bytes including the original newline, if any. Literal `++ x` is data,
not a patch header. Use endpoint trees/blobs to expand context or comment on any
line of a changed file. No text-patch parsing is needed in the view or host.

`Compare` reports ancestry facts only: `UpToDate`, `FastForward`, `Diverged`
or `Unrelated`, with the merge base (the deterministic lowest-OID choice among
several) and ahead/behind counts. Whether diverged endpoints merge cleanly is
the git client's to find out: it merges, pushes the result and the both-head
CAS of `Merge` decides whether that result still applies. Approvals and
requested changes never block that CAS.

## Operations and examples

Authors are always derived from the signed external origin, never the payload.
Any authenticated member key may open/review. Only the author edits title/body/
review requests; author or repository writer closes; repository writers merge.
Reviews remain appendable on closed/merged changes, as in the previous tracker.
Closing is terminal in this v1; there is no reopen operation.

| Operation example | Effect/output |
| --- | --- |
| `Create { repo: r, hash: HashKind::Sha1 }` | actor owns a new repo; empty output |
| `Configure { repo: r, settings: Settings::default() }` | owner configures default ref and force/delete policy; empty output |
| `Grant { repo: r, key: writer_key }` | owner grants writes; empty output |
| `Revoke { repo: r, key: writer_key }` | owner revokes writes; empty output |
| `Push { repo: r, request: receive_pack_bytes }` | existing Git receive-pack operation/report |
| `ChangeOpen { repo: r, from: feature, into: b"refs/heads/main".to_vec(), title: "Fix parser".into(), body: "Why this changes".into(), reviewers: vec![reviewer_key] }` | assigns `n`, stores body, queues channel/opened line; `OpReply::Change { height, n }` |
| `ChangeEdit { repo: r, n: 1, title: None, body: Some("Updated rationale".into()), reviewers: None }` | `None` leaves a field unchanged; `Some(vec![])` clears requests; `OpReply::Change` |
| `ChangeClose { repo: r, n: 1 }` | open to closed, queues system line; `OpReply::Change` |
| `ReviewSubmit { repo: r, n: 1, review: ReviewDraft { commit_oid: b, base_oid: Some(a), verdict: Verdict::RequestChanges, body: "Please check this".into(), comments: vec![LineComment { path: b"src/lib.rs".to_vec(), side: Side::New, line: 12, body: "Check this bound".into() }] } }` | one immutable review and all comments, one chat root; `OpReply::Review { height, n, id }` |
| `Merge { repo: r, into: b"refs/heads/main".to_vec(), from: feature, expected_into: a, expected_from: b, result: client_result_oid, change: Some(1) }` | both-head CAS and open to merged together; `OpReply::Merged { height, oid, change }` |

A merge requires a nonzero result that advances the target, correct endpoint
hash kinds, writer authority and matching current heads. It does not validate
ancestry or possession of the client result; the writer is responsible for
publishing it first (e.g. push a prepared result ref). `change: None` changes only
the target ref. The prior message-only guest-computed merge operation is removed.

Review OIDs and anchors receive syntactic validation, not object reads in
consensus. Paths must be relative, lines positive, bodies nonblank, and anchors
distinct within a batch. Old-side comments require a base. Empty approve/request
verdicts are valid; a comment verdict needs body text or line comments. Clients
validate positions against the displayed endpoint bytes, stage one draft per
anchor, read the exported cap, and preserve drafts on failed submission.

## Bounds, refusals, and the current host seam

New bounds are `page_size`, `log_walk`, `tree_walk`, `diff_bytes`, `blob_bytes`,
and `record_bytes`. Defaults live in qa's `forge_smoke::default_bounds()` (which also writes a founding's `forge.params`); founding
chooses deployment values. `log_walk` bounds a complete DAG walk and the total
review lookup count in one judgment query. A log beyond that complete-walk bound
refuses rather than inventing partial ancestry; each page repeats the bounded
walk. `tree_walk` bounds object reads in tree/blob/diff queries; log and compare
have derived read budgets (`2 * log_walk + 1` and `8 * log_walk + tree_walk`).
`diff_bytes` bounds aggregate object bytes read, including metadata; existing
`merge_cost` bounds Myers edit distance per file. `record_bytes` bounds each
encoded change/review; `MAX_REVIEW_COMMENTS`, `MAX_REVIEWERS`, `MAX_TITLE_BYTES`,
`MAX_PATH_BYTES`, `MAX_KEY_BYTES` (a granted or requested key) and `MAX_REPO_NAME`
are exported contract constants; a configured head is at most `MAX_PATH_BYTES`. Repo names
are at most 37 bytes so even `forge:<repo>:<u64::MAX>` fits chat's channel ID.

A UI query failure is an ABI `Refusal { reason, sentence }`. Stable reasons
include `object_not_held`, `capacity`, `invalid_input`, `not_found`, `stale`, and
`unexpected_reply`. Operation failures use ABI `Refusal`, including `unauthorized`
and `wrong_state`; no record/send survives a rejected operation. Malformed Borsh
requests and Git protocol failures use the ABI refusal path.

The current kernel revision `5d1d1f61d89960164cab919dfbb16a1d8cdb3b36` distinguishes
unrostered blobs (`None`) from rostered blobs absent locally (`BlobUnavailable`).
The latter is a host error before the guest receives a usable reply: `BlobStat`
is not a local-availability probe either. The guest therefore returns
`object_not_held` on every observable missing-object result, with **no retry or
wait loop in a query**, but cannot intercept the host's required-blob failure.
Core/SDK must provide a query-local nonblocking blob read/stat, or convert
`BlobUnavailable` at the query boundary. Do not change required reads in execution
into refusals. This limitation is not proven away by the in-process smoke in qa.

Likewise, `send` commits an outbox item, not the receiver's execution. The guarded
chat namespace and validated payloads prevent ordinary delivery refusals, but
chat must be seated as `chat` and forge as `forge`. Truly simultaneous record+
channel creation would require a host transaction spanning both programs; this
version proves atomic record+queue and next-block delivery, including restart.

## View and SDK handoff

- Decode Borsh using these exact types. Carry opaque cursors and answering heights;
  show typed refusals and restart stale pages. Do not embed a Git pack parser.
- Provide virtual lists for all paged collections, an identity picker/display
  resolver for keys, and the existing Markdown surface for bodies/README.
- The host code/diff surface needs byte text, typed hunk lines, old/new gutters,
  line/range selection, unified/side-by-side modes, anchors, and lazy context from
  Blob. Retain endpoint IDs and each line's side through event callbacks.
- Compose chat's conversation/thread component. `MessageById` turns a review's
  `message_id` into its root `seq`; replies use chat `PostMessage { thread }`.
  Different anchors share a review root; include the anchor when composing a reply.
  Subscribe to both forge and chat state: a chat reply does not change forge's
  root, and newly opened channels become available after outbox delivery.
- Supply a client-side Git merge/object publication path. `Compare` does not return
  a merge pack, and forge never constructs the submitted merge in consensus.
- Keep pending drafts, viewed-file checks, unread heights/reply sequences and
  last-reviewed filtering in view state. Do not silently retarget draft anchors.
- Deploy the matching chat adapter: it exposes `MessageById` and `ThreadAttention`
  and resolves key/account attention. Screenshots and live app walks are outside
  this program task.

See [the fixture manifest](fixtures/FIXTURES.md) for the program's real
response bytes and the regeneration command.
