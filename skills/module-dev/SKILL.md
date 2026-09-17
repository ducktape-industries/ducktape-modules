---
name: module-dev
description: Use when writing or changing a ducktape application module in this repository — the native crate, its wasm guest port, its optional index guest, and the committed component.wasm / index.wasm / guest.lock beside it. Also when a module's bytes go stale, a guest build is refused for an uncommitted input, or a module needs registering on a live network.
---

# Module development — the module repository runbook

A module is three layers here: native crate (the logic) → wasm guest (the
packaging) → committed artifacts (`component.wasm`, optional `index.wasm`,
`guest.lock`). Genesis composition, the host that runs a module and the
operator CLI all live in the platform repository
(`https://github.com/ducktape-industries/ducktape`); the module contract, the
module SDK and every module's wire crate live in
`https://github.com/ducktape-industries/ducktape-sdk`.

**REQUIRED BACKGROUND:** `docs/records/architecture/wasm-module-authoring.md`
— the guest contract (host-owned state, sibling reads, live update, the
cutover pattern). This skill is the wiring checklist that record doesn't cover.

## Decide first: genesis registration is a root-hash break

A module that joins the genesis set moves the descriptor's module table and its
`genesis_namespace()` fingerprint, so every existing workspace fails closed and
dev networks re-genesis. A genesis module ⇒ a new genesis — get that agreed
before wiring.

A post-genesis module leaves the genesis unchanged, changes the live root at
activation, and needs no genesis edit — it is one operator command against a
LIVE network:

```
ducktape module register <id> <component.wasm> [--index <index.wasm>] [--after N]  # admit a new id
ducktape module update   <id> <component.wasm> [--index <index.wasm>] [--after N]  # swap live code
ducktape module status                                                             # the registry + open code proposals
```

`register`/`update` drive the governance proposal that schedules the
admission/swap FIRST, then stage the component at this node's owner-gated admin
route (which fans it out to every validator and returns their receipts). That
order is load-bearing: a peer admits only a digest consensus names — an active
`code_hash`, a pending swap, or an OPEN `RegisterModule`/`UpdateModule`
proposal — so a brand-new artifact staged ahead of its proposal collects
refusals. A validator that did not take the bytes is printed as a holdout, not
a refusal to propose: the swap activates at `height + N` (`N > MIN_SWAP_LEAD`,
i.e. `> 3`; default 50 to leave room for the ceremony's own blocks) only once
every validator holds the code and signals ready, and a holdout fetches the
committed artifact off a peer before that boundary. `status` prints one row per
entry — `id  kind  active  pending` (`kind` is `module` or `view`: what the
registry says the artifact is, fixed at admission), a pending swap carrying
`ready k` (validators that signalled) or `ready ✓`. Restore and state sync
compose the wasm set from the registry's roster at the boundary, so an admitted
id composes like a genesis one; a module admitted after the last checkpoint
starts fresh and is rebuilt by replay.

The CLI stages bytes, it never builds them: the component comes from
`guest-builder` (§2).

## 1. Native crate — `crates/modules/apps/<id>`

Clone the `tasks` shape:
- `src/lib.rs` — the struct + `impl sdk::Module` (`root`, `execute`, `query`,
  `commit_block`) + `snapshot()`/`install()`. The wire types are NOT here: a
  module's payload/query/reply shapes and codecs are its own `<id>-wire` crate
  in ducktape-sdk, and the module re-exports them at its crate root.
- `tests/` — happy path, every rejection, snapshot→install root round-trip.
- Root `Cargo.toml`: a `members` entry, plus a `[workspace.dependencies]` line
  for anything new the crate reaches.
- Native-only deps (media engines, unix IO, tokio) must sit behind a `native`
  feature or be absent — the guest builds compile this same crate to wasm32.

A sibling's format crosses the edge as its WIRE crate, never its module crate:
`identity = { workspace = true }` resolves to `identity-wire`. The exception is
a module in THIS repo linked by path with `default-features = false`
(`chat = { path = "../chat", default-features = false }` in `automations` and
`runs`), which cargo cannot express through workspace inheritance.

## 2. Wasm guest — `src/guest.rs` in the module crate, built by guest-builder

The module carries its OWN port (the `tasks`/`chat` shape): a `src/guest.rs`
behind a wasm-only `guest = ["dep:ducktape-module-sdk"]` feature — the doc
header, the id consts, and ONE dispatch-shell macro
(`ducktape_module_sdk::snapshot_guest!` for whole-state `SnapshotBytes`
modules, `store_guest!` for store-backed ones, or a hand-written `Guest` impl
+ `export_module!` for odd tenants). Each macro takes the component's
`shape:` — the host learns everything it needs to run the module from the
`shape` export, never from a table: `store_shape()` / `map_shape()` /
`odb_shape()`, with `config: vec![CHAIN_ID.into()]` (or `INVITE`) on top for a
network-bound module and `committed_queries: true` for a committed-only query
lane. `#[cfg(feature = "guest")] mod guest;` in lib.rs.

No packaging crate is checked in: `guest-builder` builds the module ALONE, out
of this repository at the checkout's HEAD (so push first — uncommitted module,
SDK, sibling, and workspace build inputs are refused, and an unpushed HEAD
fails to fetch), through an ephemeral shell workspace, and writes the canonical
COMMITTED `component.wasm` and `guest.lock` into the module directory. Bytes
move only when something the module compiles moves.

## 2b. Index guest (optional) — the module's derived-tier mapper

A module that wants a materialized view (search, listings — anything qmdb's
point lookups can't serve) ships a SECOND wasm artifact: the index guest. Same
two-file shape in the module crate:
- `src/index.rs` — the pure decision core: `fold_op`/`serve_view` over the
  `index_guest` contract crate (dep `index_guest = { workspace = true }`,
  never `indexer`). Unit-test natively against a `BTreeMap`.
- `src/index_guest.rs` behind `index-guest = ["index_guest/guest"]` — the
  ~15-line engine shell (`EngineRead`, `apply`, `index_guest::fold!`/`view!`).

`guest-builder --index <module-dir>` writes the committed `index.wasm`. The
`src/index_guest.rs` file IS the declaration: the node's build script stages
`<id>.index.wasm` into the founding set for exactly the module crates that
carry it, and `node init` composes the genesis from whatever `<id>.index.wasm`
the set holds. The fold runs ASYNC behind a fluent31 changes-mode trigger —
views trail the op feed observably, never atomically.

The engine's side of that contract — no backfill at registration, at-least-once
invocation with exactly-once effects, a row above the inline cap arriving
key-only, a failing fold holding its queue with backoff — is fluent31's own
`SKILL.md`, at the rev the platform repository pins; read it before writing a
mapper. A guest's `log` output is a `debug` event under
`fluent31::wasm::guest` — turn that one target up to see it.

## 3. A code swap swaps the CODE, never the DATA

`module update` replaces the component and leaves the store untouched. The new
component must therefore be schema-IDENTICAL to the old one — same key
derivation, same value encodings — not merely schema-compatible. A store-backed
module's logical keys are hashed before they touch the store (sha256), so the
store carries no order and no prefix a new component could scan; the
`ducktape:module` WIT world exports `shape`, `initialize`, `execute`, `query`,
and `finalize-block`. Readiness and swap preparation require the replacement to
preserve its declared backing and initialized configuration keys. There is no
migrate/scan import: a new component cannot enumerate the records a key- or
value-shape change would need to rewrite, because the keyspace it would scan is
exactly the sha256 digests it can't invert. A key-layout or value-shape change
is a new module id — a fresh `register`, decided at genesis if it must replace
an existing one — never a `module update`.

The deployment hash covers component and mapper together. A mapper-only change
uses the same proposal, readiness, and activation boundary as a code change.
The global root binds deployment hashes as well as state; checkpoints and
state-sync manifests authenticate the code needed to reopen the registries.

## 4. Gates — ordering is load-bearing

```
cargo test -p <id>                                        # 1. native logic
cargo clippy -p <id> --tests --no-deps                    # 2. lints, this crate only
git push                                                  # 3. the guest build reads HEAD out of the repository
guest-builder crates/modules/apps/<id>                    # 4. rebuild the component (catches native-dep leaks)
guest-builder --index crates/modules/apps/<id>            #    …and its mapper, if it ships one
ops/wasm-repro-check.sh                                   # 5. one guest, two scratch dirs: identical bytes, no host path
```

`guest-builder` bakes its platform root in at compile time. Build it into a
directory of its own — a host config that points `CARGO_TARGET_DIR` at one
directory for every worktree leaves ONE binary at ONE path, owned by whichever
checkout built it last, and running that one refuses every module you own by
name:

```
guest-builder: crates/modules/apps/chat is outside the platform checkout <someone else's worktree>
```

It can happen MID-SWEEP, when a sibling's build lands between two of your
guests.

**A guest's bytes move with EVERY crate it compiles in, a deletion included.**
Five deleted lines shift every panic-path line number below them. Each module's
`guest.lock` records what it actually compiled, so
`grep -l 'name = "<crate>"' crates/modules/apps/*/guest.lock` says which guests
a crate change moves — check the scope by hand before rebuilding anything. A
module that ships an index guest has ONE lock covering both: the builder's
shell workspace holds every guest the module declares, so the lock is their
union. A lock names only what a guest COMPILES, so it can never name the
BUILDER: a change to the module SDK, the WIT world or the toolchain pin moves
every guest at once and that grep finds nothing at all.

## Common mistakes

| Mistake | Reality |
|---|---|
| Building a guest before pushing | guest-builder reads the module out of the repository at HEAD: an unpushed HEAD fails to fetch, an uncommitted edit is refused. Commit, push, then build |
| Moving the rust channel — or the componentizer — for one guest | bytes depend on BOTH pins (`rust-toolchain.toml` here, the componentizer in the platform repository); moving either rebuilds the whole set and commits it as one change |
| Bumping the module SDK without a rebuild | panic locations carry line numbers and every guest expands the SDK's macros, so even a comment line above them moves the set |
| Native-only dep in the module crate | the wasm32 build breaks; gate it behind a `native` feature |
| Linking a sibling's module crate for its types | a module links a sibling's WIRE crate; the module crate is the executor and belongs to nobody else |
| Changing a module's store layout in a `module update` | a swap keeps the store. A key- or value-shape change is a new module id |
