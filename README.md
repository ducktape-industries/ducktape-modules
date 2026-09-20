# ducktape-sdk

The contract line of the ducktape platform: everything a module is authored
against, and everything a program that talks to a module needs in order to
speak its format without linking the module itself.

This repository depends on no other ducktape repository. Every other one
depends on it.

## What is in here

- **`crates/kernel/sdk`** — the module contract: the `Module` trait, `Ctx`,
  `MerkleStore`, the ids and origins, and `sdk::wire` (the encode/decode pair
  every wire surface delegates to). The only crate a module may depend on
  besides its own wire surfaces.
- **`crates/module-sdk`** — the wasm authoring surface: the `ducktape:module`
  WIT world, the generated bindings, the adapter that presents host imports as
  an `sdk::Ctx`, and the wasm32 patch crates under `stubs/` a guest graph
  needs. A standalone workspace — it compiles for `wasm32-unknown-unknown`
  only.
- **`crates/kernel/sdk-testkit`** — dev-only test doubles for the `sdk`
  boundary traits. `[dev-dependencies]` only.
- **`crates/modules/**/wire`** — one wire crate per module: its messages,
  queries, replies and records, with their codecs, and no module behind them.
  A module's name resolves to its wire crate, so `identity = { workspace =
  true }` links `identity-wire` under the name `identity`.
- **`crates/kernel/{wasm-host, module-artifact, index-guest, keyscheme,
  blobstore, node-work}`** and the leaf libraries
  (`duckfs/{core,disk}`, `duckdns`, `git-primitives`, `run-envelope`,
  `view-wire`, `design`) the surfaces above are made of.
- **`crates/view-guest`** — the runtime a wasm view is written against
  (`App`, `Driver`, `export_app!`, the host protocol, editor primitives, test
  helpers). A view links it as `ducktape-view-guest` (`package =
  "view-guest"`). It pins `wit-bindgen` for the view WIT world on its own,
  apart from `crates/module-sdk`'s pin for the module world.
- **`bin/guest-builder`** — the componentizer that turns a module crate into a
  `component.wasm`, with `wit-component` pinned exactly: the same cdylib at
  another version is another hash.
- **`crates/guests`** — the standalone fixture guests (hello, its replacement,
  noop, sibling, object) the host suites run.

## Repository graph

```
                      ducktape-sdk
                      /     |     \
                     /      |      \
            ducktape   ducktape-modules   ducktape-views
                |
          ducktape-app
```

- `ducktape` — the node, the kernel host, consensus, networking, the CLI.
- `ducktape-modules` — the module implementations behind these wire surfaces.
- `ducktape-views` — the wasm views the desktop app renders.
- `ducktape-app` — the native desktop client, on top of `ducktape`.

## How it is consumed

Downstream repositories name it as a cargo git dependency in their
`[workspace.dependencies]`, so member crates keep `{ workspace = true }`:

```toml
[workspace.dependencies]
sdk = { git = "https://github.com/ducktape-industries/ducktape-sdk", branch = "dev" }
identity = { git = "https://github.com/ducktape-industries/ducktape-sdk", branch = "dev", package = "identity-wire" }
chat = { git = "https://github.com/ducktape-industries/ducktape-sdk", branch = "dev", package = "chat-wire" }
```

A wasm module pins `ducktape-module-sdk` out of this repository by revision and
patches in the `stubs/` crates for its `wasm32-unknown-unknown` build; that
revision is what its committed component bytes came out of.

## Building

The toolchain is pinned in `rust-toolchain.toml`. The committed wasm component
bytes are rustc-dependent, so a floating channel means two operators hash
different bundles into the same descriptor.
