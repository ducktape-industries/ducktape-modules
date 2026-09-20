# No dependencies between modules (owner decision, 2026-09-20)

A wasm module depends on nothing but the module SDK (the `ducktape:module`
world, host calls, codec). Not on another module's crate, and not on a shared
per-module "wire" or "api" crate either. Whatever a module needs from a
sibling — the shape of a record it reads, a query it sends, a reply it
decodes — it declares itself, in its own tree, as the minimal interface it
actually uses.

## Why

Today `runs` links `chat`, `pages`, `tasks` and `agent`; `automations` links
`chat`, `tasks`, `inbox`; `chat` links `pages` (path dependencies with
`default-features = false`). The SDK repository carries twenty `*-wire`
crates, one per module, that the app, the views, the node and the modules all
track on a moving branch. The result is that one field on one module's record
rebuilds and re-pins every guest that linked it, and one SDK wire change is a
flag day across four repositories (SDK #36 blocked modules PR #28 this way).
At runtime none of this coupling exists: modules only ever exchange encoded
records through the kernel (dispatch, saga), never call each other's code.

## The rule

- A module's `Cargo.toml` has no `path` dependency on a sibling module and no
  dependency on a sibling's wire crate. The only shared crates are the module
  SDK and general-purpose codec/address crates.
- A consumer defines the types it reads or sends (`struct PageQuery { … }`,
  `enum Party { … }`) locally, containing only the fields it uses, encoded with
  the same codec. Duplication of a few structs is the intended cost.
- Compatibility is proven by bytes, not by shared source: the owning module
  commits golden encodings of its records (fixtures), and each consumer's tests
  decode those fixtures with its local types. A producer change that breaks a
  consumer fails that consumer's fixture test, and nothing else.
- The per-module `*-wire` crates leave the SDK repository. The module that owns
  a record keeps its authoritative definition beside its guest; the SDK keeps
  only the platform contract (module world, view wire, codec, addresses,
  design). The app and the views follow the same rule as modules: local
  interface types, fixture-tested.

## Consequences

- A record change moves exactly the module that owns it and the consumers
  whose fixture tests fail — never the whole set.
- Pins: a guest rebuilds only when its own source (or the SDK) changes.
- Migration is a mechanical flag day (no backward compatibility, as always):
  replace each path dependency with local types + a fixture test, delete the
  SDK wire crates once no consumer references them. It lands after the current
  fresh-clone milestone is accepted, but the work starts now in its own
  worktrees.
