//! The program's own path, run natively: an origin resolved through an
//! identity sibling, a huddle join's node proof, identity's roster paged
//! through chat. Each refusal leaves the store as it was.
use std::cell::Cell;
use std::rc::Rc;

use abi::{Cause, Env, Origin, Refusal};
use identity::{Account, Control, Key};

use super::*;
use crate::{AccountRow, HUDDLE_JOIN_NS, HUDDLE_NODE_KEY_BYTES};

/// Ada's key; she holds account 1.
const ADA_KEY: [u8; 32] = [1; 32];
/// Bo's key: no account holds it until `claimed` says account 2 does.
const LONE_KEY: [u8; 32] = [2; 32];
/// Cy's key; she holds account 3.
const CY_KEY: [u8; 32] = [3; 32];

fn account(number: u64, keys: Vec<Vec<u8>>) -> Account {
    Account {
        number,
        name: format!("user{number}"),
        control: Control::Keys(
            keys.into_iter()
                .map(|key| Key {
                    scheme: abi::Scheme::Ed25519,
                    key,
                    label: None,
                    added_at: 0,
                })
                .collect(),
        ),
        avatar: None,
        bio: None,
        updated_at: 0,
    }
}

/// Identity over three accounts, Ada's first; `List` pages by number.
/// Account 2 holds [`LONE_KEY`] once `claimed` is set.
fn identity(claimed: Rc<Cell<bool>>) -> guest::Sibling {
    Box::new(move |request| {
        let lone = if claimed.get() {
            vec![LONE_KEY.to_vec()]
        } else {
            vec![]
        };
        let roster = [
            account(1, vec![ADA_KEY.to_vec()]),
            account(2, lone),
            account(3, vec![CY_KEY.to_vec()]),
        ];
        let reply = match abi::decode::<identity::Query>(request)? {
            identity::Query::OfKey { key } => identity::Reply::Number(
                roster
                    .iter()
                    .find(|account| account.holds(&key))
                    .map(|account| account.number),
            ),
            identity::Query::List { page } => {
                let from = page.after.as_ref().map_or(0, |after| after[0] as usize);
                let to = (from + page.limit() as usize).min(roster.len());
                identity::Reply::Accounts(PageReply {
                    height: 1,
                    items: roster[from..to].to_vec(),
                    next: (to < roster.len()).then(|| vec![to as u8]),
                })
            }
            other => panic!("chat never asks identity {other:?}"),
        };
        Ok(abi::encode(&reply))
    })
}

/// A store with identity beside it and a verifier that takes `b"signed"`
/// over exactly the join message; `claimed` hands [`LONE_KEY`] account 2.
fn store_claiming(claimed: Rc<Cell<bool>>) -> MockHost {
    let store = MockHost::default();
    store
        .borrow_mut()
        .siblings
        .insert(identity::PROGRAM.into(), identity(claimed));
    store.borrow_mut().verifier = Some(Box::new(|_, _, namespace, message, signature| {
        let expected = [b"general".as_slice(), &ADA_KEY].concat();
        namespace == HUDDLE_JOIN_NS && message == expected && signature == b"signed"
    }));
    store
}

/// [`store_claiming`] where [`LONE_KEY`] stays unclaimed.
fn store() -> MockHost {
    store_claiming(Rc::default())
}

fn env(origin: Origin) -> Env {
    Env {
        network: vec![],
        height: 1,
        time: 1000,
        me: crate::PROGRAM.into(),
        origin,
        cause: Cause::Direct,
    }
}

fn key(bytes: &[u8]) -> Origin {
    Origin::External(bytes.to_vec())
}

/// The refusal's reason; the store is untouched by it.
#[track_caller]
fn refused(store: &MockHost, origin: Origin, op: Op) -> String {
    let ctx = store.exec(env(origin));
    store.refused(|| crate::Chat::execute(&ctx, op)).reason
}

/// A read of `store`.
fn reads(store: &MockHost) -> guest::QueryCtx {
    store.query(env(Origin::System))
}

fn owner(store: &MockHost, id: &str) -> Principal {
    crate::state::channel(&reads(store), id).unwrap().owner
}

#[test]
fn an_origin_acts_as_the_principal_identity_names() {
    let store = store();
    crate::Chat::execute(
        &store.exec(env(key(&ADA_KEY))),
        create("a", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "a"), Principal::Account(1));
    let forge = Origin::Program("forge".into());
    crate::Chat::execute(&store.exec(env(forge)), create("forge:c", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "forge:c"), Principal::Module("forge".into()));
    crate::Chat::execute(
        &store.exec(env(Origin::System)),
        create("d", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "d"), Principal::System);
    assert_eq!(
        refused(&store, key(&[]), create("e", PostPolicy::Open)),
        reason::INVALID_INPUT
    );
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
    let claimed = Rc::new(Cell::new(false));
    let store = store_claiming(claimed.clone());
    crate::Chat::execute(
        &store.exec(env(key(&ADA_KEY))),
        create("general", PostPolicy::Open),
    )
    .unwrap();
    crate::Chat::execute(
        &store.exec(env(key(&ADA_KEY))),
        post("general", "m1", "hello", None),
    )
    .unwrap();
    for op in every_op() {
        let why = format!("{op:?}");
        assert_eq!(
            refused(&store, key(&LONE_KEY), op),
            reason::UNAUTHORIZED,
            "{why}"
        );
    }
    claimed.set(true);
    crate::Chat::execute(
        &store.exec(env(key(&LONE_KEY))),
        create("bo", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "bo"), Principal::Account(2));
    crate::Chat::execute(
        &store.exec(env(key(&LONE_KEY))),
        post("general", "m2", "now I can", None),
    )
    .unwrap();
    let row = crate::state::message(&reads(&store), "general", 2).unwrap();
    assert_eq!(row.author, Principal::Account(2));
}

/// With no identity deployed, no key holds an account, so none writes.
#[test]
fn no_key_writes_until_identity_is_deployed() {
    let store = MockHost::default();
    assert_eq!(
        refused(&store, key(&ADA_KEY), create("a", PostPolicy::Open)),
        reason::UNAUTHORIZED
    );
}

#[test]
fn identity_refusing_refuses_the_op() {
    let store = MockHost::default();
    store.borrow_mut().siblings.insert(
        identity::PROGRAM.into(),
        Box::new(|_| Err(Refusal::new(reason::WRONG_STATE, "identity is halted"))),
    );
    assert_eq!(
        refused(&store, key(&ADA_KEY), create("a", PostPolicy::Open)),
        reason::WRONG_STATE
    );
}

#[test]
fn a_huddle_join_needs_its_nodes_signature() {
    let join = |proof: &[u8]| Op::JoinHuddle {
        channel_id: "general".into(),
        node: vec![7; HUDDLE_NODE_KEY_BYTES],
        node_proof: proof.to_vec(),
    };
    let store = store();
    crate::Chat::execute(
        &store.exec(env(key(&ADA_KEY))),
        create("general", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(
        refused(&store, key(&ADA_KEY), join(b"forged")),
        reason::INVALID_INPUT
    );
    // the proof binds the key: another account's key cannot reuse Ada's
    assert_eq!(
        refused(&store, key(&CY_KEY), join(b"signed")),
        reason::INVALID_INPUT
    );
    assert_eq!(
        refused(&store, Origin::System, join(b"signed")),
        reason::UNAUTHORIZED
    );
    let unverified = MockHost::default();
    unverified
        .borrow_mut()
        .siblings
        .insert(identity::PROGRAM.into(), identity(Rc::default()));
    crate::Chat::execute(
        &unverified.exec(env(Origin::System)),
        create("general", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(
        refused(&unverified, key(&ADA_KEY), join(b"signed")),
        reason::UNSUPPORTED
    );
    crate::Chat::execute(&store.exec(env(key(&ADA_KEY))), join(b"signed")).unwrap();
    let huddle = crate::state::channel(&reads(&store), "general")
        .unwrap()
        .huddle;
    assert_eq!(huddle[0].principal, Principal::Account(1));
}

#[test]
fn the_roster_pages_through_identity() {
    let store = store();
    let page = |after: Option<Vec<u8>>| {
        let asked = Query::Accounts {
            page: Page {
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
    let numbers = |rows: &[AccountRow]| rows.iter().map(|row| row.number).collect::<Vec<_>>();
    assert_eq!(numbers(&first.items), [1, 2]);
    assert_eq!(first.items[0].keys, [crate::hex(&ADA_KEY)]);
    let rest = page(first.next);
    assert_eq!(numbers(&rest.items), [3]);
    assert_eq!(rest.next, None);
}
