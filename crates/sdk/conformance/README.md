# conformance

Proof that a module fills a role the kernel calls: `registry`,
`validators` or `identity` (`abi::role`). The suite runs your module's
native `guest::Module` over `MockHost` and speaks only the role's bytes:
it encodes `abi::role` values, hands them to your module's own decoding,
and reads the answers back as the role's replies. It checks the contract,
not how you keep your state.

## Filling a role

1. Implement `guest::Module`. The role's `Op`, `Query` and `Reply`
   variants are your enums' **first** variants, in the same order and with
   the same fields (borsh encodes the variant index, not its name); your
   own follow. A registry's or validators module's `init` takes the role's
   `Genesis`.
2. Add `conformance` as a dev-dependency and implement the role's
   `Fixture`: the few things the role leaves to your module's own ops.
3. Call the role's `run` in a test.

```rust
use guest::{AccountNumber, MockHost};

struct Mine;

impl conformance::identity::Fixture for Mine {
    type Module = my_identity::MyIdentity;
    fn host(&self) -> MockHost { MockHost::default() }
    /// `key` comes to hold a new account that acts.
    fn account(&self, host: &MockHost, key: &[u8]) -> AccountNumber { todo!() }
    /// `key` comes to hold an account that does not act; `None` if yours always act.
    fn stopped(&self, host: &MockHost, key: &[u8]) -> Option<AccountNumber> { None }
    /// `account` (one `account` made) stops holding `key`.
    fn drop_key(&self, host: &MockHost, account: AccountNumber, key: &[u8]) { todo!() }
    /// `key` comes to hold an active managed account; `None` if yours has none.
    fn managed(&self, host: &MockHost, key: &[u8]) -> Option<AccountNumber> { None }
    /// `account` (one `managed` made) is revoked; unused while `managed` is `None`.
    fn revoke(&self, host: &MockHost, account: AccountNumber) {}
}

#[test]
fn fills_the_identity_role() {
    conformance::identity::run(&Mine);
}
```

`conformance::env(module, height, origin, sender)` builds the env your
fixture's ops run in. `tests/reference.rs` has the fixtures of the three
reference modules (`identity`, `valset`, `module-registry`).

## What each role is held to

Each rule is a public function of its role's module, so a failing one
names itself; `run` calls them all, each on a fresh host.

**identity**: the role's `Op`, `Query` and `Reply` are the module's first
variants, byte for byte; `RegisterModule` is refused `unauthorized` from a
signed frame or a module and writes nothing, and from the system it is
idempotent (one account, `Kind::Module`); a signed or module frame is
refused though the kernel gives it a sender (its key's account, the
module's account): only the system's origin registers; `Account` of an unknown key and
`OfModule` of an unregistered module are `None`; a key that holds an
acting account resolves to it (a person or an active managed account); a
key whose account does not act is refused `unauthorized`, and its profile
is managed and not active; a key dropped from its account (the fixture's
`drop_key`) resolves to `None`; a revoked managed account's keys (`managed`,
`revoke`) resolve to `None` or are refused, and its profile is `Revoked`;
`Profile` past every account is `None`;
`Profiles` ascends, holds at most `limit`, `next` is the last number of a
page that has more, the listing is the same whatever the limit and agrees
with `Profile`; `Profiles { limit: 0 }` is a page, empty with no `next` or
the module's capped first page with `next` as any page has it; every `Kind::Module(id)` is the account `OfModule(id)`
answers.

**validators**: the role's `Query` and `Reply` are the first variants;
init with the role's `Genesis` seats exactly its validators, each a member
at its address; every key is 32 bytes and every validator a member;
seating and unseating (the fixture's `seat`/`unseat`) change the answers.

**registry**: `Query::At` and `Reply::Programs` are the first variants;
`At` answers the founding programs as founded and no view, which the
module lists apart (the fixture's `views`); published code scheduled for a
height (the fixture's `publish`/`set`) runs from that height and not
before, admitting a new program or swapping a running one's code; a
program scheduled to stop (`remove`) runs until its height and not from
it, and stays dropped once the module has run past it.
