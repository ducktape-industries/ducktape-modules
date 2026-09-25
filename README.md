# modules

The ducktape contract line and the modules written against it, one
repository. Only what compiles to wasm lives here:

```
crates/sdk/     abi store describe ducklink view-wire view-guest view-guest-derive design
crates/system/  module-registry valset identity
crates/app/     chat chat-view forge forge-view members-view node-view explorer-view settings-view
crates/lib/     gitcore
```

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI the host speaks: `GuestCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, the `module_registry` and `valset` genesis contracts. A byte-for-byte copy of ducktape's `crates/kernel/abi` under the kernel's names (`make kernel-abi-check`); a module reads `store` instead |
| `crates/sdk/store` | the module SDK: `Reads`/`Writes` (the store a call runs over), `entrypoint!` (a module's whole wasm32 glue), the typed `Map`/`Set`/`Item` tables with `KeyCodec`, `PageRequest`/`PageResponse`, `Error` and its constructors, the `Env` origin guards (`signer`, `sender_module`, `require_module`), and `testing::MockHost` to run a module natively. `kernel.rs` is the one place the kernel's names become the SDK's |
| `crates/sdk/describe` | what an op means to a person: the pure wasm module a module ships in its `ducktape.describe` section, and the sandbox that runs it |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<module>/<tail…>`, one spelling per name, no module names known here |
| `crates/sdk/view-wire`, `view-guest`, `view-guest-derive`, `design` | the host<->view wire, the runtime a wasm view is written against, the palette |
| `crates/system/module-registry` | the boot set's root: the registry module (its `Op`, `Query`, `Reply`) and `AUTHORITY`. Its `tests/system.rs` founds ducktape's host over the bytes `make wasm-programs` built and drives every system module |
| `crates/system/valset`, `identity` | the other two boot modules, the same shape: types always built, the wasm32 entry points behind `module`, the asks another module makes of them (`identity::account_of`, `valset::role`) |
| `crates/app/chat`, `chat-view` | the reference app module: `chat` is one crate whose types and rules over a `Reads`/`Writes` store are always built (native, tested), and whose wasm32 entry points sit behind its `module` feature. `chat-view` links `chat` with the feature off: the types, no host import, no export |
| `crates/app/forge`, `forge-view` | the git server as a module, the same shape as `chat`: a push is one op whose input is the receive-pack body a client sent, a merge is an op that lands the commit the client built, fetch and the ref advertisement are queries; a git object's blob id is its oid. it links `gitcore` for the git; merging is the client's. The rules run natively over `MemorySandbox`, which is where `fixtures/` comes from; `forge-view` links `forge` with `module` off |
| `crates/app/members-view`, `node-view`, `explorer-view`, `settings-view` | the system views, which link the system crates with `module` off |
| `crates/lib/gitcore` | git as a library over one `Objects` trait: objects, packs (read, delta, write), walks, diffs, and the server side of the wire (receive-pack verification, upload-pack); no merging, that is the git client's. `forge` links it into its wasm |

A view links its module by path and reads its types. A module is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape; the founding suite links it at
the revision `Cargo.toml` pins, patched to compile against `crates/sdk/abi`.
What is not wasm lives elsewhere: the forge smoke (real git against
`forge.wasm` on ducktape's runtime) and `view-pack` (a view into its module)
are in the qa repo, which packs and founds what this repo builds.

`valset` and `module-registry` take their writes from the module named
`module_registry::AUTHORITY` (`governance`); no module in this tree
implements it. The system modules beyond the boot set are archived at
`ducktape-industries/ducktape-system-modules-archive`.

## A module

```rust
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Env, Error, Item, Reads, Writes};

const N: Item<u64> = Item::new("n");

#[derive(BorshSerialize, BorshDeserialize)]
pub enum Op { Set(u64) }
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Query { Get }

pub fn execute(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Error> {
    env.signer()?; // only a signed transaction
    let Op::Set(n) = op;
    N.put(store, &n);
    Ok(())
}

pub fn query(store: &impl Reads, _env: &Env, query: Query) -> Result<u64, Error> {
    let Query::Get = query;
    Ok(N.get(store)?.unwrap_or_default())
}

store::entrypoint! {
    execute: Op => execute,
    query: Query => query,
}
```

`entrypoint!` is the whole wasm32 glue: it decodes the call (bytes that do
not decode are refused as `invalid_input`), runs the typed entry point, and
returns a query's answer. `init: Genesis => init,` adds a genesis entry;
`sender: identity::principal_of,` hands `execute` the resolved sender
(`execute(store, env, sender, op)`, as chat and forge do). An execute's own
return data is `store.set_return_data(bytes)`; messages to other modules are
`send` (no reply) and `call` (its outcome comes back as `Cause::Reply`).

An app module is one crate: its types and rules are always built, and the
exports sit behind a cargo feature `module`, off by default. `make
wasm-programs` builds the crate with `--features module`; its view links the
same crate with the feature off and gets the types with no host import and
no export, which `make wasm-views` checks.

A test runs the same rules over `store::testing::MockHost` in the env
`store::testing::env(origin)` makes. `host.attempt(|h| ...)` runs a write and,
when it fails, checks it left the host untouched; `host.refused(|h| ...)` is
`attempt` for a write that must fail and returns its `Error`. Another module's
queries answer through `host.mock_module::<Query, Reply>(id, handler)`.

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
wasm-programs` itself), clippy, `make program-wasm-check`,
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
