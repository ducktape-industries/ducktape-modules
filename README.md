# ducktape-modules

The application consensus modules of Ducktape, and the reference modules that
show how one is written.

A module is pure logic over a host-owned store. It holds no durable memory
across dispatches, takes no clock, rng or network import, and ships as a wasm
component the network admits by hash. Each crate under `crates/modules/apps/`
implements `sdk::Module`, carries its own guest port (`src/guest.rs` behind the
`guest` feature) and — where it serves a materialized view — an index guest
(`src/index_guest.rs` behind `index-guest`).

| Path | What |
|---|---|
| `crates/modules/apps/` | chat, pages, agent, runs, tasks, boards, automations, inbox, collaboration |
| `crates/examples/directory` | the first wasm port of a native module; the template every later one followed |
| `crates/examples/greeter` | a consumer module, composed purely out of its siblings' wire types |
| `crates/examples/extension-probe` | module, view and service replacement against fixed native executables (standalone workspaces) |
| `skills/module-dev/SKILL.md` | the runbook: native crate → guest → committed artifacts |
| `docs/records/architecture/wasm-module-authoring.md` | the guest contract |

## The artifacts are the product

`component.wasm`, `index.wasm` and `guest.lock` are committed beside each
module. They are what a network runs: `node init` composes a genesis out of
them, a member verifies its copy against the descriptor's hashes, and the code
registry swaps one at a block. A binary never carries them.

`guest-builder` (in the platform repository) builds a module ALONE, out of this
repository at a revision — so push before you build. Bytes move only when
something the module compiles moves, line numbers included; each `guest.lock`
records exactly what went in.

## Repositories

```
ducktape-sdk ──┬── ducktape ─── ducktape-app
               ├── ducktape-modules
               └── ducktape-views
```

- **ducktape-sdk** — the module contract (`sdk`), the module SDK and its
  `ducktape:module` WIT world, the index contract, and every module's `-wire`
  crate: the types and codecs a sibling, a view or a daemon speaks.
- **ducktape** — the platform: host, consensus, node, the system modules, the
  CLI and `guest-builder`.
- **ducktape-modules** — this repository.
- **ducktape-views** — the desktop views, one per module.
- **ducktape-app** — the desktop application.

## How it is consumed

Nothing links these crates to get a module's format — that is what the `-wire`
crates in ducktape-sdk are for. A consumer takes the **artifact**: the platform
CLI packs `component.wasm` (plus an optional mapper and view) into one
deployment, proposes its hash to governance, and stages the bytes on the blob
plane; validators verify the hash, check the component's shape and activate at
an armed height.

Within this workspace a module names a sibling in this repository by path
(`chat = { path = "../chat", default-features = false }`); everything else
resolves through `[workspace.dependencies]` in the root `Cargo.toml` to a git
dependency on `ducktape-sdk` (the contract and the wire crates) or on
`ducktape` (the host and native system modules a test drives), both tracking
their `dev` branch.
