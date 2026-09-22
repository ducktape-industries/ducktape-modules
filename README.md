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
| `crates/app/chat`, `chat-view` | the reference app module: `chat` is one crate whose types and rules over a `Read`/`Write` store are always built (native, tested), and whose wasm32 program over the host sits behind its `program` feature. `chat-view` links `chat` with the feature off: the types, no host import, no program export |
| `crates/app/gitcore` | git as a `no_std` library over one `Objects` trait: objects, packs, walks, diff, merge, and the server side of the wire protocol (receive-pack v1, upload-pack v2) |
| `crates/app/forge`, `forge-view` | the git server as a program, the same shape as `chat`: a push is one op whose input is the receive-pack body a client sent, a merge is an op, fetch and the ref advertisement are queries; a git object's blob id is its oid. The rules run natively over `MemorySandbox`, which is where `fixtures/` comes from; `forge-view` links `forge` with `program` off |
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

An app program is one crate: its types and rules are always built, and the
glue above (`guest`, `program!`) sits behind a cargo feature `program`, off by
default. `make wasm-programs` builds the crate with `--features program`; its
view links the same crate with the feature off and gets the types with no
host import and no export, which `make wasm-views` checks.

## A view

A view implements `Render::render(window, cx)` and serializable `View::new(window, cx)`.
`Context<V>` dereferences to `App`; `listener` registers typed event callbacks and
returns the ID used by `wire::Node`. Call `cx.notify()` after state changes.
Unnotified frames retain their tree and event routes; native debug builds catch
serialized state changes without notification.

`cx.host()` is the typed host door. Keep the `Task` returned by `cx.spawn`, or
call `.detach()`; dropping it cancels the future and any owned subscription.
Consume host streams with `while let Some(item) = stream.next().await` and update
through `WeakEntity`. `TestAppContext` supplies typed fake handlers and feeds,
input simulation, and tree assertions. See `examples/exported_view.rs` in
`view-guest` and the four app view test modules.

Snapshot/restore transfers the root view's serde state, not entity identities.
Snapshots wait for ordinary work to settle; parked host streams restart in
`View::restored`. Keep independent writes in separate tasks: an opaque joined
future sharing a stream waiter cannot expose whether its other work is pending.
The tree vocabulary, manifests, and five-function Wasm ABI are unchanged.

## Building

`cargo test --workspace`, the same for clippy, `make program-wasm-check`, `make view-wasm-check`, `make
wasm-programs` and `make wasm-views` are what CI runs. The toolchain is
pinned in `rust-toolchain.toml`.

View releases require `wasm-tools`, Python 3, and
[Binaryen wasm-opt 132](https://github.com/WebAssembly/binaryen/releases/tag/version_132).
`make wasm-views` builds, optimizes, and checks the exact guest ABI and decimal
size limits (1,200,000 bytes for Members/Node/Explorer; 2,500,000 for Chat).
`WASM_OPT=/path/to/wasm-opt` can select the pinned optimizer. It preserves the
view manifest and never supplies imports or removes capabilities.

GPUI uses the fork's default-off consumer configuration: `web` is disabled for
guests and remains default-on in the native app. The root patches select the
fork's scheduler, logging, and MessagePack crates as well. MessagePack's `typed`
feature is enabled only for wasm: it follows the wire's Serde shapes and retains
the existing named encoding. Dynamic/ignored values remain supported. Shared
writers and table-driven keyboard enums avoid duplicate generated codec code.
Native and guest roundtrip, limit, and malformed-input tests cover that boundary.
