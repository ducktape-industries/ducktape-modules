//! The program's own path, run natively: an origin resolved through an
//! identity sibling, a huddle join's node proof, identity's roster paged
//! through chat. Each refusal leaves the store as it was.
use abi::{Cause, Env, Origin, Refusal};
use identity::{Account, Control, Key};

use super::*;
use crate::{AccountRow, HUDDLE_JOIN_NS, HUDDLE_NODE_KEY_BYTES, execute_from};

/// Ada's key; she holds account 1.
const ADA_KEY: [u8; 32] = [1; 32];
/// A key no account holds.
const LONE_KEY: [u8; 32] = [2; 32];

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
fn identity(request: &[u8]) -> Result<Vec<u8>, Refusal> {
    let roster = [
        account(1, vec![ADA_KEY.to_vec()]),
        account(2, vec![]),
        account(3, vec![]),
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
}

/// A store with identity beside it and a verifier that takes `b"signed"`
/// over exactly the join message.
fn store() -> Memory {
    let mut store = Memory::default();
    store
        .siblings
        .insert(identity::PROGRAM.into(), Box::new(identity));
    store.verifier = Some(Box::new(|_, _, namespace, message, signature| {
        let expected = [b"general".as_slice(), &ADA_KEY].concat();
        namespace == HUDDLE_JOIN_NS && message == expected && signature == b"signed"
    }));
    store
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
fn refused(store: &mut Memory, origin: Origin, op: Op) -> String {
    let before = store.state.clone();
    let refusal = execute_from(store, &env(origin), op).expect_err("the op was refused");
    assert_eq!(store.state, before, "a refused op wrote");
    refusal.reason
}

fn owner(store: &Memory, id: &str) -> Party {
    crate::state::channel(store, id).unwrap().owner
}

#[test]
fn an_origin_acts_as_the_party_identity_names() {
    let mut store = store();
    execute_from(
        &mut store,
        &env(key(&ADA_KEY)),
        create("a", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "a"), Party::Account(1));
    execute_from(
        &mut store,
        &env(key(&LONE_KEY)),
        create("b", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "b"), Party::Key(LONE_KEY.to_vec()));
    let forge = Origin::Program("forge".into());
    execute_from(&mut store, &env(forge), create("forge:c", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "forge:c"), Party::Module("forge".into()));
    execute_from(
        &mut store,
        &env(Origin::System),
        create("d", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "d"), Party::System);
    assert_eq!(
        refused(&mut store, key(&[]), create("e", PostPolicy::Open)),
        reason::INVALID_INPUT
    );
}

#[test]
fn a_key_acts_as_itself_until_identity_is_deployed() {
    let mut store = Memory::default();
    execute_from(
        &mut store,
        &env(key(&ADA_KEY)),
        create("a", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(owner(&store, "a"), Party::Key(ADA_KEY.to_vec()));
}

#[test]
fn identity_refusing_refuses_the_op() {
    let mut store = Memory::default();
    store.siblings.insert(
        identity::PROGRAM.into(),
        Box::new(|_| Err(Refusal::new(reason::WRONG_STATE, "identity is halted"))),
    );
    assert_eq!(
        refused(&mut store, key(&ADA_KEY), create("a", PostPolicy::Open)),
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
    let mut store = store();
    execute_from(
        &mut store,
        &env(key(&ADA_KEY)),
        create("general", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(
        refused(&mut store, key(&ADA_KEY), join(b"forged")),
        reason::INVALID_INPUT
    );
    // the proof binds the key: another key cannot reuse Ada's
    assert_eq!(
        refused(&mut store, key(&LONE_KEY), join(b"signed")),
        reason::INVALID_INPUT
    );
    assert_eq!(
        refused(&mut store, Origin::System, join(b"signed")),
        reason::UNAUTHORIZED
    );
    let mut unverified = Memory::default();
    execute_from(
        &mut unverified,
        &env(Origin::System),
        create("general", PostPolicy::Open),
    )
    .unwrap();
    assert_eq!(
        refused(&mut unverified, key(&ADA_KEY), join(b"signed")),
        reason::UNSUPPORTED
    );
    execute_from(&mut store, &env(key(&ADA_KEY)), join(b"signed")).unwrap();
    let huddle = crate::state::channel(&store, "general").unwrap().huddle;
    assert_eq!(huddle[0].party, Party::Account(1));
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
        let Reply::Accounts(page) = query(&store, 1, asked).unwrap() else {
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

/// A key asks after the threads it started as a key and as its account:
/// the one answered last.
#[test]
fn a_keys_attention_counts_the_threads_its_account_started() {
    let mut chat = Chat {
        store: store(),
        height: 0,
    };
    chat.ok(&ADA, create("general", PostPolicy::Open));
    let ada_key = Party::Key(ADA_KEY.to_vec());
    let ada = Party::Account(1);
    chat.post(&ada_key, "as-key", "before the account", None);
    chat.post(&ada, "as-account", "after it", None);
    chat.post(&BO, "r1", "re: key", Some(1));
    chat.post(&BO, "r2", "re: account", Some(2));
    let newest = |chat: &Chat, author: Party| {
        let Reply::Attention(row) = chat.ask(Query::ThreadAttention {
            channel_id: "general".into(),
            author,
        }) else {
            panic!("attention answers attention");
        };
        row.map(|row| row.seq)
    };
    assert_eq!(newest(&chat, ada_key), Some(2));
    assert_eq!(newest(&chat, Party::Key(LONE_KEY.to_vec())), None);
    chat.post(&BO, "r3", "re: key again", Some(1));
    assert_eq!(newest(&chat, Party::Key(ADA_KEY.to_vec())), Some(1));
}
