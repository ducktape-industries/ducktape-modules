//! The module's own path, run natively: the sender the host resolved, a
//! huddle join's node proof, the identity role's profiles paged through chat. Each
//! refusal leaves the store as it was.
use abi::role::identity as role;
use guest::{Cause, Env, Origin};

use super::*;
use crate::{HUDDLE_JOIN_NS, HUDDLE_NODE_KEY_BYTES, Profile};

/// Ada's key; she holds account 1.
const ADA_KEY: [u8; 32] = [1; 32];
/// Bo's key: it holds account 2 once identity seats it.
const LONE_KEY: [u8; 32] = [2; 32];
/// Cy's key; she holds account 3.
const CY_KEY: [u8; 32] = [3; 32];

/// The identity role over three accounts, the third an agent Ada manages;
/// `Profiles` pages by number.
fn identity() -> guest::Sibling {
    Box::new(move |request| {
        let profile = |number: u64| Profile {
            number,
            name: format!("user{number}"),
            category: (number == 3).then_some(crate::Category::Agent),
            manager: (number == 3).then_some(1),
            module: None,
        };
        let role::Query::Profiles { after, limit } =
            abi::decode(request).map_err(guest::kernel::error_from)?
        else {
            panic!("chat asks the identity role only for profiles");
        };
        let from = after.unwrap_or(0) + 1;
        let to = (from + u64::from(limit)).min(4);
        let reply = role::Reply::Profiles {
            profiles: (from..to).map(profile).collect(),
            next: (to < 4).then_some(to - 1),
        };
        Ok(abi::encode(&reply))
    })
}

/// A store with identity beside it and a verifier that takes `b"signed"`
/// over exactly the join message.
fn store() -> MockHost {
    let store = MockHost::default();
    store
        .borrow_mut()
        .siblings
        .insert(guest::MockHost::roles().identity, identity());
    store.borrow_mut().verifier = Some(Box::new(|_, _, namespace, message, signature| {
        let expected = [b"general".as_slice(), &ADA_KEY].concat();
        namespace == HUDDLE_JOIN_NS && message == expected && signature == b"signed"
    }));
    store
}

fn env(origin: Origin, sender: Option<Principal>) -> Env {
    Env {
        chain_id: vec![],
        height: 1,
        time: 1000,
        module: crate::MODULE.into(),
        origin,
        sender,
        roles: guest::MockHost::roles(),
        cause: Cause::Direct,
    }
}

/// A frame signed by `key`, acting as the account it holds (if any), as the
/// host resolves it.
fn key(key: &[u8], account: Option<u64>) -> Env {
    env(
        Origin::Signed(key.to_vec()),
        account.map(Principal::Account),
    )
}

fn ada() -> Env {
    key(&ADA_KEY, Some(1))
}

fn root() -> Env {
    env(Origin::Root, Some(Principal::Root))
}

/// The refusal's reason; the store is untouched by it.
#[track_caller]
fn refused(store: &MockHost, env: Env, op: Op) -> String {
    let ctx = store.exec(env);
    store.refused(|| crate::Chat::execute(&ctx, op)).code
}

/// A read of `store`.
fn reads(store: &MockHost) -> guest::QueryCtx {
    store.query(env(Origin::Root, None))
}

fn owner(store: &MockHost, id: &str) -> Principal {
    crate::state::channel(&reads(store), id).unwrap().owner
}

#[test]
fn a_write_acts_as_its_sender() {
    let store = store();
    crate::Chat::execute(&store.exec(ada()), create("a", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "a"), Principal::Account(1));
    // forge's own frame acts as forge's account
    let forge = env(Origin::Module("forge".into()), Some(FORGE));
    crate::Chat::execute(&store.exec(forge), create("forge:c", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "forge:c"), FORGE);
    crate::Chat::execute(&store.exec(root()), create("d", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "d"), Principal::Root);
}

/// Every op, one each, as a key would send it into `#general`.
fn every_op() -> Vec<Op> {
    let general = || "general".to_string();
    vec![
        create("room", PostPolicy::Open),
        Op::CreateVoiceChannel {
            channel_id: "voice".into(),
            name: "voice".into(),
        },
        Op::CreateDmChannel {
            counterpart: 1,
            name: "ada".into(),
        },
        Op::RenameChannel {
            channel_id: general(),
            name: "renamed".into(),
        },
        Op::SetChannelArchived {
            channel_id: general(),
            archived: true,
        },
        Op::PostMessage {
            channel_id: general(),
            message_id: "m2".into(),
            blocks: parse_message("hi"),
            thread: None,
        },
        Op::EditMessage {
            channel_id: general(),
            seq: 1,
            blocks: parse_message("edited"),
            base_rev: None,
        },
        Op::DeleteMessage {
            channel_id: general(),
            seq: 1,
        },
        Op::AddReaction {
            channel_id: general(),
            seq: 1,
            emoji: "👍".into(),
        },
        Op::RemoveReaction {
            channel_id: general(),
            seq: 1,
            emoji: "👍".into(),
        },
        Op::SetMembership {
            channel_id: general(),
            principal: Principal::Account(3),
            member: true,
        },
        Op::JoinHuddle {
            channel_id: general(),
            node: vec![7; HUDDLE_NODE_KEY_BYTES],
            node_proof: b"signed".to_vec(),
        },
        Op::LeaveHuddle {
            channel_id: general(),
        },
    ]
}

/// A key that holds no account writes nothing, whatever the op; once
/// identity hands it an account, the same key writes as that account.
#[test]
fn a_key_writes_only_once_it_holds_an_account() {
    let store = store();
    crate::Chat::execute(&store.exec(ada()), create("general", PostPolicy::Open)).unwrap();
    crate::Chat::execute(&store.exec(ada()), post("general", "m1", "hello", None)).unwrap();
    for op in every_op() {
        let why = format!("{op:?}");
        assert_eq!(
            refused(&store, key(&LONE_KEY, None), op),
            code::UNAUTHORIZED,
            "{why}"
        );
    }
    let bo = || key(&LONE_KEY, Some(2));
    crate::Chat::execute(&store.exec(bo()), create("bo", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "bo"), Principal::Account(2));
    crate::Chat::execute(&store.exec(bo()), post("general", "m2", "now I can", None)).unwrap();
    let row = crate::state::message(&reads(&store), "general", 2).unwrap();
    assert_eq!(row.author, Principal::Account(2));
}

#[test]
fn a_huddle_join_needs_its_nodes_signature() {
    let join = |proof: &[u8]| Op::JoinHuddle {
        channel_id: "general".into(),
        node: vec![7; HUDDLE_NODE_KEY_BYTES],
        node_proof: proof.to_vec(),
    };
    let store = store();
    crate::Chat::execute(&store.exec(ada()), create("general", PostPolicy::Open)).unwrap();
    assert_eq!(refused(&store, ada(), join(b"forged")), code::INVALID_INPUT);
    // the proof binds the key: another account's key cannot reuse Ada's
    assert_eq!(
        refused(&store, key(&CY_KEY, Some(3)), join(b"signed")),
        code::INVALID_INPUT
    );
    assert_eq!(refused(&store, root(), join(b"signed")), code::UNAUTHORIZED);
    let unverified = MockHost::default();
    crate::Chat::execute(
        &unverified.exec(root()),
        create("general", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(
        refused(&unverified, ada(), join(b"signed")),
        code::UNSUPPORTED
    );
    crate::Chat::execute(&store.exec(ada()), join(b"signed")).unwrap();
    let huddle = crate::state::channel(&reads(&store), "general")
        .unwrap()
        .huddle;
    assert_eq!(huddle[0].principal, Principal::Account(1));
}

#[test]
fn the_roster_pages_through_the_identity_role() {
    let store = store();
    let page = |after: Option<Vec<u8>>| {
        let asked = Query::Accounts {
            page: PageRequest {
                after,
                limit: Some(2),
            },
        };
        let Reply::Accounts(page) = crate::Chat::query(&reads(&store), asked).unwrap() else {
            panic!("accounts answer accounts");
        };
        page
    };
    let first = page(None);
    let numbers = |rows: &[Profile]| rows.iter().map(|row| row.number).collect::<Vec<_>>();
    assert_eq!(numbers(&first.items), [1, 2]);
    assert_eq!(first.items[0].name, "user1");
    let rest = page(first.next);
    assert_eq!(numbers(&rest.items), [3]);
    assert_eq!(rest.items[0].category, Some(crate::Category::Agent));
    assert_eq!(rest.next, None);
}
