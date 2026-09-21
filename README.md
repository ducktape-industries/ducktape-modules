# ducktape-sdk

The contract line of the ducktape platform: what a wasm program is authored
against, what a wasm view is authored against, and the leaf libraries both are
made of.

This repository depends on no other ducktape repository. Every other one
depends on it.

## What is in here

- **`crates/kernel/abi`** — the bytes ABI a program and the host share, all
  borsh: `GuestCall` (`Init` / `Execute` / `Query`), `HostOp` and `HostReply`
  (reads, writes, scans, blobs, sibling queries, messages, events, `Respond`,
  crypto), `Env`, `Refusal`, and the two fixed contracts the host reads
  (`roster`: the `modules` program; `validators`: the `valset` program).
- **`crates/kernel/guest`** — what a program compiles against: the `Program`
  trait (`init`, `execute`, `query`), the `program!` macro that emits the two
  exports (`alloc`, `call`), and one typed function per host op
  (`guest::get`, `guest::set`, `guest::scan`, `guest::emit`,
  `guest::respond`, …). wasm32 only for the host calls; the types build
  everywhere.
- **`crates/kernel/{refusal-class, keyscheme}`** — the refusal words as bare
  constants, and the key schemes a proof is verified under.
- **`crates/view-wire`**, **`crates/view-guest`**, **`crates/design`** — the
  host<->view wire, the runtime a wasm view is written against (`View`,
  `Cx`, `export_view!`, the host protocol, the composer, test helpers) and
  the palette they draw from. A view links `view-guest` as
  `ducktape-view-guest`.
- **`crates/{duck-address, duckdns, duckfs/core, git-primitives,
  run-envelope}`** — the leaf libraries: the `duck://` grammar, `.duck`
  names, the duckfs paths/objects/wire, git read types, the run envelope.

## A program

```rust
use abi::{Refusal, Scan};
use guest::Program;

struct Counter;

impl Program for Counter {
    fn execute(payload: &[u8]) -> Result<(), Refusal> {
        let n: u64 = abi::decode(payload)?;
        guest::set(b"n".to_vec(), abi::encode(&n));
        Ok(())
    }
    fn query(_request: &[u8]) -> Result<(), Refusal> {
        guest::respond(guest::get(b"n").unwrap_or_default());
        Ok(())
    }
}

guest::program!(Counter);
```

Built with `cargo build --release --target wasm32-unknown-unknown`; the host
loads the `.wasm` by the blob id the `modules` program records for it.

## Repository graph

```
                      ducktape-sdk
                      /     |     \
                     /      |      \
            ducktape   ducktape-modules   ducktape-views
                |
          ducktape-app
```

- `ducktape` — the kernel host (runtime, state, blobs, host, node, consensus,
  statesync), the daemon and the CLI.
- `ducktape-modules` — the programs, and the views that live beside them.
- `ducktape-views` — the remaining wasm views the desktop app renders.
- `ducktape-app` — the native desktop client.

## How it is consumed

Downstream repositories name it as a cargo git dependency in their
`[workspace.dependencies]`, so member crates keep `{ workspace = true }`:

```toml
[workspace.dependencies]
abi = { git = "https://github.com/ducktape-industries/ducktape-sdk", branch = "dev", package = "abi" }
guest = { git = "https://github.com/ducktape-industries/ducktape-sdk", branch = "dev", package = "guest" }
```

## Building

The toolchain is pinned in `rust-toolchain.toml`. `cargo test --workspace`,
`cargo clippy --workspace --tests -- -D warnings`, `make program-wasm-check`
and `make view-wasm-check` are what CI runs.
