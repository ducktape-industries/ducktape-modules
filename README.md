# modules

The ducktape contract line and the programs written against it, one
repository. Only what compiles to wasm lives here, in three folders:

```
crates/sdk/     abi guest program program-derive ducklink view-wire view-guest view-guest-derive design
crates/system/  module-registry valset identity
crates/app/     chat chat-view forge forge-view members-view node-view explorer-view settings-view
crates/lib/     gitcore
```

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI a program and the host share: `GuestCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, the `module_registry` and `valset` contracts. A copy of ducktape's `crates/kernel/abi`, like `guest` beside it |
| `crates/sdk/guest` | what a program compiles against: the `Program` trait, the `Execute` and `Query` contexts its entry points receive, `program!` |
| `crates/sdk/program`, `program-derive` | a program as a state struct plus methods: `Map`/`Set`/`Item` fields under their own prefixes, scalars in one root record, `#[program]` deriving `Op`/`Query`/`Reply` and the guest glue from the methods; `MemoryStore` runs the same methods natively |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no program names known here |
| `crates/sdk/view-wire`, `view-guest`, `view-guest-derive`, `design` | the host<->view wire, the runtime a wasm view is written against, the palette |
| `crates/system/module-registry` | the boot set's root: the registry program (its `Op`, `Query`, `Reply`), `AUTHORITY`, `Page`/`PageReply` and the origin/key/refusal `helpers` every system program links. Its `tests/system.rs` founds ducktape's host over the bytes `make wasm-programs` built and drives every system program |
| `crates/system/valset`, `identity` | the other two boot programs: types always built, the wasm32 program behind `program`, the asks another program makes of them (`identity::account_of`, `valset::standing`) behind `guest`. `identity` is written in the `#[program]` shape over `crates/sdk/program` |
| `crates/app/chat`, `chat-view` | the reference app module: `chat` is one crate whose types and rules over a `Read`/`Write` store are always built (native, tested), and whose wasm32 program over the host sits behind its `program` feature. `chat-view` links `chat` with the feature off: the types, no host import, no program export |
| `crates/app/forge`, `forge-view` | the git server as a program, the same shape as `chat`: a push is one op whose input is the receive-pack body a client sent, a merge is an op that lands the commit the client built, fetch and the ref advertisement are queries; a git object's blob id is its oid. it links `gitcore` for the git; merging is the client's. The rules run natively over `MemorySandbox`, which is where `fixtures/` comes from; `forge-view` links `forge` with `program` off |
| `crates/app/members-view`, `node-view`, `explorer-view`, `settings-view` | the system views, which link the system crates with `program` off |
| `crates/lib/gitcore` | git as a library over one `Objects` trait: objects, packs (read, delta, write), walks, diffs, and the server side of the wire (receive-pack verification, upload-pack); no merging, that is the git client's. `forge` links it into its wasm |

A view links its module by path and reads its types. A program is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape; the founding suite links it at
the revision `Cargo.toml` pins, patched to compile against `crates/sdk/abi`
and `crates/sdk/guest`, so a copy that drifts from the kernel fails to build.
What is not wasm lives elsewhere: the forge smoke (real git against
`forge.wasm` on ducktape's runtime) and `view-pack` (a view into its program)
are in the qa repo, which packs and founds what this repo builds.

`valset` and `module-registry` take their writes from the program named
`module_registry::AUTHORITY` (`governance`); no program in this tree
implements it. The eight system modules beyond the boot set are archived at
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

`make test` (`make wasm-programs`, then `cargo test --workspace`), clippy,
`make program-wasm-check`, `make view-wasm-check`, `make wasm-views` and
`make wasm-reproducible` are what CI runs. The toolchain is pinned in
`rust-toolchain.toml`.

Every wasm artifact is a build output: `make wasm-modules` builds every
program and every view under `$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/`;
nothing built is committed, and nothing is packed here (qa's `make pack` embeds
each view in its program for a founding). `make wasm-reproducible` proves the
bytes do not depend on the checkout.

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
