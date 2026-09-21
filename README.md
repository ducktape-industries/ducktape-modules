# modules

The ducktape contract line and the programs written against it, one
repository.

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI a program and the host share: `GuestCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, the `module_registry` and `valset` contracts. A copy of ducktape's `crates/kernel/abi`, like `guest` beside it |
| `crates/sdk/guest` | what a program compiles against: the `Program` trait, the `Execute` and `Query` contexts its entry points receive, `program!` |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no program names known here |
| `crates/sdk/view-wire`, `view-guest`, `design` | the host<->view wire, the runtime a wasm view is written against, the palette |
| `crates/modules` | the boot set: the `modules` crate (each program's `Op`, `Query` and `Reply`, `AUTHORITY`, `Page`, the helpers a program builds on), `system/` (the wasm32 workspace of `module-registry`, `valset` and `identity`; `make wasm-programs` rebuilds them into `system/wasm/`), and `tests/system.rs`, which founds ducktape's host over the committed bytes and drives every program |
| `crates/app/chat`, `chat-program`, `chat-view` | the reference app module: `chat` is its types and rules over a `Read`/`Write` store (native, tested), `chat-program` the wasm32 program that runs them over the host, `chat-view` the view, which links `chat` for its types and never the program |
| `crates/app/gitcore` | git as a `no_std` library over one `Objects` trait: objects, packs, walks, diff, merge, and the server side of the wire protocol (receive-pack v1, upload-pack v2) |
| `crates/app/forge` | the git server as a program: a push is one op whose input is the receive-pack body a client sent, a merge is an op, fetch and the ref advertisement are queries; a git object's blob id is its oid |
| `crates/app/forge-harness` | a dev rig, not product: runs `forge.wasm` on ducktape's `runtime` over an in-memory host and speaks git smart HTTP, so real `git` pushes to and clones from the program without a network |

A view links its module by path and reads its types. A program is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape; the `modules` suite links it at
the revision `Cargo.toml` pins, patched to compile against `crates/sdk/abi`
and `crates/sdk/guest`, so a copy that drifts from the kernel fails to build.

`valset` and `module-registry` take their writes from the program named
`modules::AUTHORITY` (`governance`); no program in this tree implements it.
The eight system modules beyond the boot set are archived at
`ducktape-industries/ducktape-system-modules-archive`.

## A program

```rust
use abi::{Env, Refusal};
use guest::{Execute, Program, Query, Reads};

struct Counter;

impl Program for Counter {
    fn execute(ctx: &mut Execute, _env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        let n: u64 = abi::decode(payload)?;
        ctx.put(b"n", &n);
        Ok(())
    }
    fn query(ctx: &mut Query, _env: &Env, _request: &[u8]) -> Result<(), Refusal> {
        ctx.respond(ctx.get(b"n").unwrap_or_default());
        Ok(())
    }
}

guest::program!(Counter);
```

An entry point receives the context (`Execute` writes, sends and sets
output; `Query` responds; both read through `Reads`) and the `Env` of the
call; nothing reaches the host by any other path.

## Building

`cargo test --workspace`, the same for clippy, `make program-wasm-check`, `make view-wasm-check`, `make
wasm-programs` and `make wasm-views` are what CI runs. The toolchain is
pinned in `rust-toolchain.toml`.
