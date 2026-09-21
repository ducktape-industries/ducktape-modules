# modules

The ducktape contract line and the programs written against it, one
repository.

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI a program and the host share: `GuestCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, the `roster` and `validators` contracts |
| `crates/sdk/guest` | what a program compiles against: the `Program` trait, `program!`, one typed function per host op |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no program names known here |
| `crates/sdk/view-wire`, `view-guest`, `design` | the host<->view wire, the runtime a wasm view is written against, the palette |
| `crates/system/{modules,valset,identity}` | the roster the host reads, the validators it seats, the accounts everything attributes to |
| `crates/app/chat`, `chat-view` | the reference app module and its view. **Red** until chat is rewritten as a `guest::Program`; the source stays as the reference for that rewrite |

A view links its module by path and reads its types. A program is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape.

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

`cargo test --workspace --exclude chat --exclude chat-view`, the same for
clippy, `make program-wasm-check`, `make view-wasm-check`, `make
wasm-programs` and `make wasm-views` are what CI runs. The toolchain is
pinned in `rust-toolchain.toml`.
