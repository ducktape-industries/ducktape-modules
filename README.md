# modules

The ducktape contract line and the programs written against it, one
repository.

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI a program and the host share: `GuestCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, the `module_registry` and `valset` contracts. A copy of ducktape's `crates/kernel/abi`, like `guest` beside it |
| `crates/sdk/guest` | what a program compiles against: the `Program` trait, `program!`, one typed function per host op |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no program names known here |
| `crates/sdk/view-wire`, `view-guest`, `design` | the host<->view wire, the runtime a wasm view is written against, the palette |
| `crates/modules` | the boot set: the `modules` crate (each program's `Op`, `Query` and `Reply`, `AUTHORITY`, `Page`, the helpers a program builds on), `system/` (the wasm32 workspace of `module-registry`, `valset` and `identity`; `make wasm-programs` rebuilds them into `system/wasm/`), and `tests/system.rs`, which founds ducktape's host over the committed bytes and drives every program |
| `crates/app/chat`, `chat-view` | the reference app module (a `guest::Program`: channels, threads, reactions, search) and its view, which links `chat` for its types |

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
use abi::Refusal;
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

## Building

`cargo test --workspace`, the same for clippy, `make program-wasm-check`, `make view-wasm-check`, `make
wasm-programs` and `make wasm-views` are what CI runs. The toolchain is
pinned in `rust-toolchain.toml`.
