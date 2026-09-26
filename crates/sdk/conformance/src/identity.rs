//! The identity role: `RegisterModule` from the system, and who a key or a
//! module acts as. [`run`] checks every rule below.

use abi::role::identity::{Category, Kind, Op, Profile, Query, Reply, Standing};
use borsh::{BorshDeserialize, BorshSerialize};
use guest::{AccountNumber, Error, MockHost, Module, Origin, code};

use crate::{ask, execute, root, same_bytes};

/// What the suite needs from the module beyond the role.
pub trait Fixture {
    type Module: Module<Op: BorshSerialize, Query: BorshSerialize, Response: BorshDeserialize>;

    /// A founded host: the module's init run, and whatever its own ops
    /// need (a verifier).
    fn host(&self) -> MockHost;

    /// `key` comes to hold a new account that acts; its number.
    fn account(&self, host: &MockHost, key: &[u8]) -> AccountNumber;

    /// `key` comes to hold a new account that does not act (a suspended
    /// agent); its number. `None` for a module whose accounts always act:
    /// that rule is skipped.
    fn stopped(&self, host: &MockHost, key: &[u8]) -> Option<AccountNumber>;
}

/// Every identity rule, each on a fresh host.
pub fn run(fixture: &impl Fixture) {
    the_role_is_the_first_variants(fixture);
    register_module_is_root_only_and_idempotent(fixture);
    an_unknown_key_or_module_holds_nothing(fixture);
    a_held_key_resolves_to_an_acting_account(fixture);
    an_account_that_does_not_act_is_refused(fixture);
    an_absent_profile_is_none(fixture);
    profiles_page_ascending(fixture);
    module_profiles_agree_with_of_module(fixture);
}

fn module() -> String {
    MockHost::roles().identity
}

fn query<F: Fixture>(host: &MockHost, query: Query) -> Result<Reply, Error> {
    ask::<F::Module, Reply>(host, &module(), 1, &query)
}

fn account_of<F: Fixture>(host: &MockHost, query: Query) -> Option<AccountNumber> {
    match self::query::<F>(host, query.clone()) {
        Ok(Reply::Account(number)) => number,
        other => panic!("identity: {query:?} answers Account, not {other:?}"),
    }
}

fn profile<F: Fixture>(host: &MockHost, number: AccountNumber) -> Option<Profile> {
    match query::<F>(host, Query::Profile(number)) {
        Ok(Reply::Profile(profile)) => profile,
        other => panic!("identity: Profile({number}) answers Profile, not {other:?}"),
    }
}

/// Every profile, paged by `limit`, checking each page as it comes.
fn every_profile<F: Fixture>(host: &MockHost, limit: u32) -> Vec<Profile> {
    let mut all: Vec<Profile> = Vec::new();
    let mut after = None;
    loop {
        let asked = Query::Profiles { after, limit };
        let (profiles, next) = match query::<F>(host, asked.clone()) {
            Ok(Reply::Profiles { profiles, next }) => (profiles, next),
            other => panic!("identity: {asked:?} answers Profiles, not {other:?}"),
        };
        assert!(
            profiles.len() <= limit as usize,
            "identity: {asked:?} answered {} profiles, past its limit",
            profiles.len()
        );
        for profile in &profiles {
            let last = all.last().map(|p| p.number).or(after);
            assert!(
                last.is_none_or(|last| profile.number > last),
                "identity: {asked:?} is not ascending past {last:?}: {}",
                profile.number
            );
            all.push(profile.clone());
        }
        match next {
            None => return all,
            Some(next) => {
                assert_eq!(
                    Some(next),
                    profiles.last().map(|p| p.number),
                    "identity: {asked:?}: `next` is the last number of a page that has more"
                );
                after = Some(next);
            }
        }
    }
}

fn register<F: Fixture>(host: &MockHost, origin: Origin, name: &str) -> Result<(), Error> {
    let mut env = root(&module(), 1);
    if origin != Origin::Root {
        env.origin = origin;
        env.sender = None;
    }
    execute::<F::Module>(
        host,
        env,
        &Op::RegisterModule {
            module: name.into(),
        },
    )
}

/// The role's op, queries and replies are the module's first variants,
/// byte for byte.
pub fn the_role_is_the_first_variants<F: Fixture>(_: &F) {
    type M<F> = <F as Fixture>::Module;
    let m = module();
    let op = Op::RegisterModule {
        module: "chat".into(),
    };
    same_bytes::<<M<F> as Module>::Op>(&m, "Op", &op);
    for query in [
        Query::Account(b"key".to_vec()),
        Query::OfModule("chat".into()),
        Query::Profile(4),
        Query::Profiles {
            after: Some(2),
            limit: 5,
        },
    ] {
        same_bytes::<<M<F> as Module>::Query>(&m, "Query", &query);
    }
    let profile = Profile {
        number: 1,
        name: "Scout".into(),
        kind: Kind::Managed {
            manager: 2,
            category: Category::Agent,
            standing: Standing::Suspended,
        },
    };
    for reply in [
        Reply::Account(Some(3)),
        Reply::Account(None),
        Reply::Profile(Some(profile.clone())),
        Reply::Profile(None),
        Reply::Profiles {
            profiles: vec![profile],
            next: Some(1),
        },
    ] {
        same_bytes::<<M<F> as Module>::Response>(&m, "Response", &reply);
    }
}

/// Only the system registers a module: a signed frame and another module
/// are refused `unauthorized` and write nothing. Registering twice keeps
/// the one account, which is the module's.
pub fn register_module_is_root_only_and_idempotent<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    for origin in [
        Origin::Signed(b"someone".to_vec()),
        Origin::Module("chat".into()),
    ] {
        // `attempt` also checks a refusal wrote nothing
        let answer = host.attempt(|| register::<F>(&host, origin.clone(), "probe"));
        assert_eq!(
            answer.map_err(|e| e.code),
            Err(code::UNAUTHORIZED.into()),
            "identity: RegisterModule from {origin:?} is refused unauthorized"
        );
    }
    register::<F>(&host, Origin::Root, "probe")
        .unwrap_or_else(|e| panic!("identity: the system registers a module: {e:?}"));
    let number = account_of::<F>(&host, Query::OfModule("probe".into()))
        .expect("identity: OfModule answers the account RegisterModule gave");
    let before = every_profile::<F>(&host, 50);
    register::<F>(&host, Origin::Root, "probe")
        .unwrap_or_else(|e| panic!("identity: registering a module again is no refusal: {e:?}"));
    assert_eq!(
        account_of::<F>(&host, Query::OfModule("probe".into())),
        Some(number),
        "identity: registering again keeps the module's account"
    );
    assert_eq!(
        every_profile::<F>(&host, 50),
        before,
        "identity: registering again changes no account"
    );
    assert_eq!(
        profile::<F>(&host, number).map(|p| p.kind),
        Some(Kind::Module("probe".into())),
        "identity: a registered module's profile is Kind::Module"
    );
}

/// A key no account holds, and a module never registered, answer `None`.
pub fn an_unknown_key_or_module_holds_nothing<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    assert_eq!(
        account_of::<F>(&host, Query::Account(b"nobody's key".to_vec())),
        None,
        "identity: Account of an unknown key is None"
    );
    assert_eq!(
        account_of::<F>(&host, Query::OfModule("never-registered".into())),
        None,
        "identity: OfModule of an unregistered module is None"
    );
}

/// A key that holds an acting account resolves to it, and its profile is a
/// person or an active managed account.
pub fn a_held_key_resolves_to_an_acting_account<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let number = fixture.account(&host, b"held key");
    assert_eq!(
        account_of::<F>(&host, Query::Account(b"held key".to_vec())),
        Some(number),
        "identity: Account(key) answers the account the key holds"
    );
    let kind = profile::<F>(&host, number).map(|p| p.kind);
    assert!(
        matches!(
            kind,
            Some(Kind::Person)
                | Some(Kind::Managed {
                    standing: Standing::Active,
                    ..
                })
        ),
        "identity: an account a key acts as is a person or an active managed one, not {kind:?}"
    );
}

/// A key whose account does not act is refused `unauthorized`, so the host
/// rejects its frame; its profile says why.
pub fn an_account_that_does_not_act_is_refused<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let Some(number) = fixture.stopped(&host, b"stopped key") else {
        return;
    };
    let asked = Query::Account(b"stopped key".to_vec());
    match query::<F>(&host, asked) {
        Err(refused) => assert_eq!(
            refused.code,
            code::UNAUTHORIZED,
            "identity: Account of a key whose account does not act is refused unauthorized"
        ),
        Ok(reply) => panic!(
            "identity: Account of a key whose account does not act is refused, not {reply:?}"
        ),
    }
    let kind = profile::<F>(&host, number).map(|p| p.kind);
    assert!(
        matches!(kind, Some(Kind::Managed { standing, .. }) if standing != Standing::Active),
        "identity: an account that does not act is managed and not active, not {kind:?}"
    );
}

/// `Profile` of a number no account has is `None`.
pub fn an_absent_profile_is_none<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    fixture.account(&host, b"one");
    let past = every_profile::<F>(&host, 50)
        .last()
        .map_or(1, |p| p.number + 1);
    assert_eq!(
        profile::<F>(&host, past),
        None,
        "identity: Profile({past}), past every account, is None"
    );
}

/// `Profiles` pages ascend by number, hold at most `limit`, and `next`
/// resumes where a page ended; each agrees with `Profile(number)`.
pub fn profiles_page_ascending<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let made: Vec<AccountNumber> = (0..5u8)
        .map(|n| fixture.account(&host, &[b'k', n]))
        .collect();
    let by_two = every_profile::<F>(&host, 2);
    for number in &made {
        assert!(
            by_two.iter().any(|p| p.number == *number),
            "identity: Profiles lists account {number}"
        );
    }
    assert_eq!(
        every_profile::<F>(&host, 1),
        by_two,
        "identity: Profiles lists the same accounts whatever the limit"
    );
    for listed in &by_two {
        assert_eq!(
            profile::<F>(&host, listed.number).as_ref(),
            Some(listed),
            "identity: Profile({}) agrees with Profiles",
            listed.number
        );
    }
}

/// Every `Kind::Module(id)` profile is the account `OfModule(id)` answers.
pub fn module_profiles_agree_with_of_module<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    for name in ["probe", "other"] {
        register::<F>(&host, Origin::Root, name)
            .unwrap_or_else(|e| panic!("identity: the system registers {name}: {e:?}"));
    }
    fixture.account(&host, b"person");
    for listed in every_profile::<F>(&host, 50) {
        if let Kind::Module(id) = &listed.kind {
            assert_eq!(
                account_of::<F>(&host, Query::OfModule(id.clone())),
                Some(listed.number),
                "identity: OfModule({id}) is the account whose profile names it"
            );
        }
    }
}
