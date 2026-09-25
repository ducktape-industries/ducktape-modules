# modules

The ducktape contract line and the modules written against it, one
repository. Only what compiles to wasm lives here:

```
crates/sdk/     abi guest store describe ducklink view-wire view-guest view-guest-derive design
crates/system/  module-registry valset identity
crates/app/     chat chat-view forge forge-view members-view node-view explorer-view settings-view
crates/lib/     gitcore
```

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI a module and the host share: `GuestCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, the `module_registry` and `valset` contracts, under the kernel's names. A copy of ducktape's `crates/kernel/abi` |
| `crates/sdk/guest` | the minimal module SDK, enough alone: the `Module` trait, the `ExecCtx` and `QueryCtx` contexts its entry points receive (env, raw state, blobs, `send`/`call`, events, `set_return_data`, sibling queries, `verify`), `export!`, `Error` and its constructors and `decoded`, and `MockHost`, the native host the same contexts run over in a test. `src/kernel.rs` is the one place the kernel's names (`Refusal`, `ProgramId`, `ItemRef`, `Scan`, …) become the SDK's (`Error`, `ModuleId`, `MessageId`, `Range`, …), byte for byte. `examples/counter.rs` is a module written with it alone |
| `crates/sdk/store` | optional typed storage over `guest`'s contexts: the `Map`/`Set`/`Item` descriptors with `KeyCodec`, and `PageRequest`/`PageResponse`. A read takes `&QueryCtx` (an `&ExecCtx` serves it), a write `&ExecCtx`. A view links it and calls none of it |
| `crates/sdk/describe` | what an op means to a person: the pure wasm module a program ships in its `ducktape.describe` section, and the sandbox that runs it |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no program names known here |
| `crates/sdk/view-wire`, `view-guest`, `view-guest-derive`, `design` | the host<->view wire, the runtime a wasm view is written against, the palette |
| `crates/system/module-registry` | the boot set's root: the registry module (its `Op`, `Query`, `Reply`), `AUTHORITY` and the origin/key/error `helpers` every system module links. Its `tests/system.rs` founds ducktape's host over the bytes `make wasm-programs` built and drives every system module |
| `crates/system/valset`, `identity` | the other two boot modules, the same shape: types always built, the wasm exports behind `module`, the asks another module makes of them (`identity::account_of`, `valset::role`) behind `guest` |
| `crates/app/chat`, `chat-view` | the reference app module: `chat` is one crate whose types, rules and `Chat` module are always built (native, tested over `MockHost`), and whose wasm exports sit behind its `module` feature. `chat-view` links `chat` with the feature off: the types, no host import, no module export |
| `crates/app/forge`, `forge-view` | the git server as a module, the same shape as `chat`: a push is one op whose input is the receive-pack body a client sent, a merge is an op that lands the commit the client built, fetch and the ref advertisement are queries; a git object's blob id is its oid. it links `gitcore` for the git; merging is the client's. The module runs natively over `MemorySandbox` (forge's and chat's `MockHost`), which is where `fixtures/` comes from; `forge-view` links `forge` with `module` off |
| `crates/app/members-view`, `node-view`, `explorer-view`, `settings-view` | the system views, which link the system crates with `module` off |
| `crates/lib/gitcore` | git as a library over one `Objects` trait: objects, packs (read, delta, write), walks, diffs, and the server side of the wire (receive-pack verification, upload-pack); no merging, that is the git client's. `forge` links it into its wasm |

A view links its module by path and reads its types. A module is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape; the founding suite links it at
the revision `Cargo.toml` pins, patched to compile against `crates/sdk/abi`,
so a copy that drifts from the kernel fails to build.
What is not wasm lives elsewhere: the forge smoke (real git against
`forge.wasm` on ducktape's runtime) and `view-pack` (a view into its module)
are in the qa repo, which packs and founds what this repo builds.

`valset` and `module-registry` take their writes from the module named
`module_registry::AUTHORITY` (`governance`); no module in this tree
implements it. The system modules beyond the boot set are archived at
`ducktape-industries/ducktape-system-modules-archive`.

## A module

```rust
use guest::{Error, ExecCtx, Module, QueryCtx};

pub struct Counter;

impl Module for Counter {
    type Op = u64;
    type Query = ();
    type Response = u64;

    fn execute(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
        let n: u64 = ctx.record("n")?.unwrap_or(0);
        ctx.put("n", &(n + by));
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<u64, Error> {
        Ok(ctx.record("n")?.unwrap_or(0))
    }
}

guest::export!(Counter);
```

An entry point receives its context (`ExecCtx` reads, writes, sends and
sets return data; `QueryCtx` reads) and nothing else: `ctx.env()` is the call's
`Env`, and nothing reaches the host by any other path. `export!` only emits
the `alloc`/`call` exports, which decode the invocation and encode the
answer through `guest::execute`/`guest::query`. The same contexts run over a
`MockHost` natively, so a module's test runs its real code
(`crates/sdk/guest/examples/counter.rs`).

An app module is one crate: its types, rules and module are always built,
and its `export!` sits behind a cargo feature `module`, off by default.
`make wasm-programs` builds the crate with `--features module`; its view
links the same crate with the feature off and gets the types with no host
import and no export, which `make wasm-views` checks.

## A view

A view implements `Render::render(window, cx)` and serializable `View::new(window, cx)`.
`Context<V>` dereferences to `App`; `listener` registers typed event callbacks and
returns the ID used by `wire::Node`. Call `cx.notify()` after state changes.
Unnotified frames retain their tree and event routes; native debug builds catch
serialized state changes without notification.

`cx.host()` is the typed host method. Keep the `Task` returned by `cx.spawn`, or
call `.detach()`; dropping it cancels the future and any owned subscription.
Consume host streams with `while let Some(item) = stream.next().await` and update
through `WeakEntity`. `TestAppContext` supplies typed fake handlers and feeds,
input simulation, and tree assertions. See `examples/exported_view.rs` in
`view-guest` and each app view's `src/tests.rs`.

Snapshot/restore transfers the root view's serde state, not entity identities.
Snapshots wait for ordinary work to settle; parked host streams restart in
`View::restored`. Keep independent writes in separate tasks: an opaque joined
future sharing a stream waiter cannot expose whether its other work is pending.
The tree vocabulary, manifests, and five-function Wasm ABI are unchanged.

## Loop

1. Edit a module or a view.
2. `make dev`: rebuilds what cargo finds stale (one line per artifact:
   `name  1,181,498 B` or `unchanged`), gates the rebuilt
   views (ABI) and runs the native tests of the crates cargo rebuilt.
   `P=forge` / `V=forge-view` narrow it to one.
3. `kit build NAME && kit up NAME` in qa packs and founds these artifacts,
   then the app opens on them.
4. Where a view's bytes go: `make wasm-why V=members-view` (`twiggy top`, `cargo install twiggy`).
5. A new module: `make new-module NAME=x`, then `make new-view NAME=x-view`;
   each prints what to do next. `make test` runs everything; the founding
   suite builds the boot set itself.

## Building

`make test` (`cargo test --workspace`; the founding suite runs `make
wasm-programs` itself), clippy, `make module-wasm-check`,
`make view-wasm-check`, `make wasm-views` and `make wasm-reproducible` are
what CI runs. The toolchain is pinned in `rust-toolchain.toml`.

Every wasm artifact is a build output: `make wasm-modules` builds every
module and every view under `$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/`;
nothing built is committed, and nothing is packed here (qa's `make pack` embeds
each view in its module for a founding). `make wasm-reproducible` proves the
bytes do not depend on the checkout.

View releases require `wasm-tools`, Python 3, and
[Binaryen wasm-opt 132](https://github.com/WebAssembly/binaryen/releases/tag/version_132).
`make wasm-views` builds, optimizes, and checks the exact guest ABI, printing
`name  bytes`. The bytes before wasm-opt stay beside each
artifact as `<name>.wasm.unoptimized`. `WASM_OPT=/path/to/wasm-opt` can
select the pinned optimizer. It preserves the view manifest and never
supplies imports or removes capabilities. `make wasm-why V=<view>` builds the
view with `--profile why` (release plus names, its own output dir) and runs
`twiggy top -n 25` over it; the release profile strips names, which is why
twiggy cannot read the shipped bytes.

GPUI uses the fork's default-off consumer configuration: `web` is disabled for
guests and remains default-on in the native app. The root patches select the
fork's scheduler, logging, and MessagePack crates as well. MessagePack's `typed`
feature is enabled only for wasm: it follows the wire's Serde shapes and retains
the existing named encoding. Dynamic/ignored values remain supported. Shared
writers and table-driven keyboard enums avoid duplicate generated codec code.
Native and guest roundtrip, limit, and malformed-input tests cover that boundary.
