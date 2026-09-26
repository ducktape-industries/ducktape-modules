# Roles, accounts and updates

The kernel knows three modules by what they do, never by their ids. Genesis
binds each role to a founding module; every call the kernel makes goes to the
module bound to that role, speaking the role's interface in
`crates/sdk/abi` (`abi::role::{registry, validators, identity}`). The
modules in `crates/system/` are one implementation of each.

## The three roles

| Role | Bound here to | What the kernel does with it |
|---|---|---|
| `registry` | `module-registry` | at founding, writes its params (`role::registry::Genesis`: every founding module and view); at each block, before any frame, asks `At(height)` and admits, swaps or drops modules to match the answer |
| `validators` | `valset` | at founding, writes its params (`role::validators::Genesis`: the founding validators); at founding and on the last block of each epoch, asks `Members` and seats them for the next epoch |
| `identity` | `identity` | as it admits a module (at founding, and later off the registry), executes `RegisterModule { module }` with the `System` origin; for each signed frame asks `Account(key)`; for each message and reply asks `OfModule(sender)` |

The founding file binds them (ducktape's `[roles]` table; qa's
`founding.toml` is a working one):

```toml
[roles]
registry = "module-registry"
validators = "valset"
identity = "identity"
```

All three are required. Founding refuses a role bound to a module the
founding does not list, and a node refuses a network that records no roles.
Every frame's `Env` carries the bindings (`env.roles`), so a module that asks
a role asks `ctx.env().roles.identity`, not a name it assumes; chat's roster
does this (`crates/app/chat/src/origin.rs`).

## Who a frame acts as

The host resolves the sender once per frame, through the identity role, and
hands it to the module as `Env.sender`:

- a signed frame acts as the account its key holds (`Account(key)`); a key
  that holds none still runs, as no one (`sender: None`), which is how
  identity's own `Create` works; a key whose account does not act (a
  suspended agent) is refused, and the host rejects the frame;
- a message or a reply from another module acts as that module's account
  (`OfModule(module)`);
- founding and the kernel's own calls act as the chain (`Root`).

A module reads it as `ctx.sender()`, which is `Principal::Account(number)`
or `Principal::Root` and refuses a frame that acts as no one, so no row a
module writes names a bare key. `Env::signer`, `sending_module` and
`sent_by` check the raw origin instead, where a module needs the key or the
module itself (forge requires a signed frame; chat's `:` namespace belongs to
the module its prefix names).

## Accounts

Everything that acts is an account in `identity`:
`Account { number, card, control }`. The `Card` is what it shows (name,
avatar, bio); the `Control` is who acts as it:

- `Person { keys }`: a person and the keys they hold.
- `Managed { manager, category, life }`: an agent. Its manager, a person,
  gives it keys and decides its `Life`: `Active { keys }`,
  `Suspended { keys }` (keys kept, frames refused) or `Revoked` (keys
  dropped, final).
- `Module { module }`: a module's account, registered by the kernel as it
  admits the module. It holds no keys; the module alone acts as it.

Who may do what (the acting account is `ctx.sender()`):

| Op | Who |
|---|---|
| `RegisterModule { module }` | the system (`Root` origin) alone; a module that has an account keeps it |
| `Create { name, scheme }` | a key that holds no account; it becomes a person's first key |
| `CreateAgent { name }` | a person; they become its manager |
| `AddKey` | a person's account: the new key signs the frame and a key already on the account consents. An agent's: its manager signs and the new key consents to itself. Never a module's or a revoked agent's |
| `RemoveKey` | a person removes their own key or a junior one, never their last. An agent's manager removes any of its keys |
| `SetName`, `SetProfile` | a person or a module on its own account; an agent's manager on the agent's (not once revoked) |
| `Suspend`, `Resume`, `Revoke` | the agent's manager |

The role's queries, which views and other modules ask too, are
`Account(key)`, `OfModule(module)`, `Profile(number)` and
`Profiles { after, limit }`. A `Profile` is `{ number, name, kind }`, where
`Kind` is `Person`, `Managed { manager, category, standing }` or
`Module(id)`. `Kind::badge` and `Kind::note` in `abi` turn it into what a view
shows beside a name ("Agent · managed by Dev", "Module · chat", "suspended"),
the one place that happens.

## Updating a module

`module-registry` holds the roster and the changes scheduled to it:

1. `Publish { body }` stores the new code as a blob and returns its id.
2. `Schedule(Scheduled { height, change: Change::Set(Entry { program, code, params }) })`
   lands it at a later height.
3. At that height the kernel's `At(height)` sees the new code. Under an id
   already running, the code is swapped: the state, the account and the id
   stay, and `init` does not run again. Under a new id, the module is
   admitted (its `init` gets `params`) and registered with identity.

`Change::Remove` drops a module; `Cancel` withdraws a scheduled change. A
role's module updates the same way, under its bound id; the binding itself
is fixed at genesis.

Who may schedule is `Env::authority()` in `guest`, which `module-registry`
calls before scheduling or cancelling a change and `valset` before every
write (publishing a blob is open to anyone). It is a stub that admits anyone until
the chain has an authority to ask; the real check replaces that one function.

## Filling a role with another module

A role is an interface, not a module, so any module that speaks it can fill
the role at genesis. To write one:

1. Depend on `abi` and match the role's types byte for byte: the role's
   `Op`, `Query` and `Reply` variants are your enums' **first** variants, in
   the same order and with the same fields (borsh encodes the variant index,
   not its name). Your own variants follow. The simplest way is to reuse the
   role's types (`pub use abi::role::identity::{Kind, Profile, ...}`).
2. Pin it with a test that encodes each role value and your value and
   compares the bytes, as `identity`'s
   `the_identity_role_is_its_first_variants` does.
3. Take the params the kernel writes: a registry's `init` receives
   `role::registry::Genesis`, a validators module's
   `role::validators::Genesis`. An identity module receives its own
   founding params.
4. Keep the kernel's contract. Identity: `RegisterModule` is idempotent
   and only the system sends it; refusing it fails a founding, and later
   undoes the admission until the next height. A refusal of `Account(key)`
   or `OfModule(module)` rejects the frame or the message, so refuse only an
   account that must not act. Registry: `At(height)` answers every module
   that runs at that height; one it leaves out is dropped. Validators: a
   refusal of `Members` stops the node (the host reports its state
   corrupt).
5. Bind it in the founding's `[roles]` and list it under `[[programs]]`.

Modules and views that ask the role go through `env.roles`, so they work
against the new module unchanged; anything that asks a system module by its
id or by its own variants beyond the role's does not.
