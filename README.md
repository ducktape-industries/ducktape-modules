# ducktape-modules

The reference module and its view, `crates/app/chat` is the
consensus module, `crates/app/chat-view` the wasm view the desktop renders for it.
Everything else that once lived here (pages, agent, runs, tasks, boards,
automations, inbox, collaboration, call, the reference modules, the remaining
views in ducktape-views) comes back one crate at a time, written against this
pair — and against the program abi in ducktape-sdk (`abi`, `guest`) once
`chat` itself is on it.

| Path | What |
|---|---|
| `crates/app/chat` | the chat module: channels, messages, threads, reactions, huddles; its guest port behind `guest`, its index guest behind `index-guest` |
| `crates/app/chat-view` | the chat view on `view-guest`: the whole screen, on the `View` API |
| `crates/system/modules`, `crates/system/valset` | the two programs the host reads: the roster and the validators (`abi::roster`, `abi::validators`) |
| `crates/system/identity` | numbered accounts held by keys and named; what every other program attributes to |

## Building

`cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`
`make wasm-views` and `make wasm-programs` are what CI runs. The module's
committed `component.wasm` / `index.wasm` / `guest.lock` come out of
`guest-builder` (`make wasm-modules`), built from a pushed HEAD.

## Repositories

```
ducktape-sdk ──┬── ducktape ─── ducktape-app
               └── ducktape-modules
```
