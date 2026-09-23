// The rules natively over `store::Memory`: what the founding suite checks on the host, without the host.

use std::cell::RefCell;
use std::rc::Rc;

use abi::{Cause, Env, Origin, reason};
use store::Memory;
use valset::{MAX_MEMBERS, Membership, Standing};

use crate::{Grant, INVITE_NAMESPACE, Invite, Motion, Op, Voted};

const NETWORK: &[u8] = b"net";
const TIME: u64 = 1_000;

fn key(n: u8) -> Vec<u8> {
    vec![n; 32]
}

fn env(origin: Origin) -> Env {
    Env {
        network: NETWORK.to_vec(),
        height: 3,
        time: TIME,
        me: crate::PROGRAM.into(),
        origin,
        cause: Cause::Direct,
    }
}

fn membership(n: u8, standing: Standing) -> Membership {
    Membership {
        key: key(n),
        address: format!("node-{n}:9000"),
        standing,
    }
}

type Seated = Rc<RefCell<Vec<Membership>>>;

fn with_valset(seated: Vec<Membership>) -> (Memory, Seated) {
    let seated = Rc::new(RefCell::new(seated));
    let answers = seated.clone();
    let mut store = Memory {
        verifier: Some(Box::new(|_, key, namespace, message, signature| {
            namespace == INVITE_NAMESPACE && signature == [key, message].concat()
        })),
        ..Memory::default()
    };
    store.siblings.insert(
        valset::PROGRAM.into(),
        Box::new(move |request| {
            let seated = answers.borrow();
            let reply = match abi::decode::<valset::Query>(request)? {
                valset::Query::Membership { key } => {
                    valset::Reply::Membership(seated.iter().find(|m| m.key == key).cloned())
                }
                valset::Query::Validators => valset::Reply::Validators(
                    seated
                        .iter()
                        .filter(|m| m.standing == Standing::Validator)
                        .map(|m| m.key.clone())
                        .collect(),
                ),
                valset::Query::Members => {
                    valset::Reply::Members(seated.iter().map(Membership::member).collect())
                }
                other => panic!("admission asked valset {other:?}"),
            };
            Ok(abi::encode(&reply))
        }),
    );
    (store, seated)
}

fn founded() -> (Memory, Seated) {
    with_valset(vec![
        membership(1, Standing::Validator),
        membership(2, Standing::Validator),
    ])
}

fn invite(issuer: u8, nonce: u8, expires: u64) -> Invite {
    let grant = Grant {
        network: NETWORK.to_vec(),
        nonce: vec![nonce; 16],
        expires,
    };
    let signature = [key(issuer), grant.preimage()].concat();
    Invite {
        issuer: key(issuer),
        grant,
        signature,
    }
}

fn run(store: &mut Memory, signer: u8, op: Op) -> Result<Vec<valset::Op>, abi::Refusal> {
    crate::execute(store, &env(Origin::External(key(signer))), op)?;
    let sent = store.take_emissions();
    assert!(sent.iter().all(|message| message.target == valset::PROGRAM));
    Ok(sent
        .iter()
        .map(|message| abi::decode(&message.payload).unwrap())
        .collect())
}

fn enroll(
    store: &mut Memory,
    signer: u8,
    invite: Option<Invite>,
) -> Result<valset::Op, abi::Refusal> {
    let op = Op::Enroll {
        address: format!("node-{signer}:9000"),
        invite,
    };
    let mut written = run(store, signer, op)?;
    assert_eq!(written.len(), 1, "one write to valset");
    Ok(written.remove(0))
}

fn vote(
    store: &mut Memory,
    voter: u8,
    motion: Motion,
) -> Result<(Voted, Vec<valset::Op>), abi::Refusal> {
    let written = run(store, voter, Op::Vote(motion))?;
    let voted = abi::decode(&store.take_output()).unwrap();
    Ok((voted, written))
}

fn refusal<T: std::fmt::Debug>(result: Result<T, abi::Refusal>) -> String {
    result.unwrap_err().reason
}

#[test]
fn the_door_starts_closed_and_an_invite_admits_one_node() {
    let (mut store, _) = founded();
    assert_eq!(refusal(enroll(&mut store, 3, None)), reason::UNAUTHORIZED);
    let ticket = invite(1, 7, TIME + 1);
    let written = enroll(&mut store, 3, Some(ticket.clone())).unwrap();
    assert_eq!(written, valset::Op::Set(membership(3, Standing::Resident)));
    assert_eq!(
        refusal(enroll(&mut store, 4, Some(ticket))),
        reason::WRONG_STATE
    );
}

#[test]
fn a_member_moves_its_address_and_keeps_its_standing() {
    let (mut store, _) = founded();
    let op = Op::Enroll {
        address: "new:9000".into(),
        invite: None,
    };
    let written = run(&mut store, 1, op).unwrap();
    let moved = Membership {
        address: "new:9000".into(),
        ..membership(1, Standing::Validator)
    };
    assert_eq!(written, vec![valset::Op::Set(moved)]);
}

#[test]
fn only_a_signed_frame_enrolls() {
    let (mut store, _) = founded();
    let refused = crate::execute(
        &mut store,
        &env(Origin::Program("probe".into())),
        Op::Enroll {
            address: "x".into(),
            invite: None,
        },
    )
    .unwrap_err();
    assert_eq!(refused.reason, reason::UNAUTHORIZED);
    assert!(store.take_emissions().is_empty());
}

#[test]
fn an_invite_is_refused_expired_forged_foreign_or_from_a_resident() {
    let (mut store, seated) = founded();
    let expired = invite(1, 1, TIME);
    assert_eq!(
        refusal(enroll(&mut store, 3, Some(expired))),
        reason::INVALID_INPUT
    );
    let mut forged = invite(1, 2, TIME + 1);
    forged.signature = invite(9, 2, TIME + 1).signature;
    assert_eq!(
        refusal(enroll(&mut store, 3, Some(forged))),
        reason::UNAUTHORIZED
    );
    let mut foreign = invite(1, 3, TIME + 1);
    foreign.grant.network = b"elsewhere".to_vec();
    foreign.signature = [key(1), foreign.grant.preimage()].concat();
    assert_eq!(
        refusal(enroll(&mut store, 3, Some(foreign))),
        reason::INVALID_INPUT
    );
    seated.borrow_mut().push(membership(3, Standing::Resident));
    let from_a_resident = invite(3, 4, TIME + 1);
    assert_eq!(
        refusal(enroll(&mut store, 4, Some(from_a_resident))),
        reason::UNAUTHORIZED
    );
}

#[test]
fn a_motion_passes_at_the_quorum_of_the_current_validators() {
    let (mut store, seated) = founded();
    seated.borrow_mut().push(membership(3, Standing::Resident));
    let promote = Motion::Promote { key: key(3) };
    assert_eq!(
        refusal(vote(&mut store, 3, promote.clone())),
        reason::UNAUTHORIZED
    );
    let counted = Voted::Counted {
        votes: 1,
        needed: 2,
    };
    assert_eq!(
        vote(&mut store, 1, promote.clone()).unwrap(),
        (counted.clone(), vec![])
    );
    assert_eq!(
        vote(&mut store, 1, promote.clone()).unwrap(),
        (counted, vec![])
    );
    let (voted, written) = vote(&mut store, 2, promote).unwrap();
    assert_eq!(voted, Voted::Enacted);
    assert_eq!(
        written,
        vec![valset::Op::Set(membership(3, Standing::Validator))]
    );
}

#[test]
fn a_motion_on_the_wrong_standing_or_the_last_validator_is_refused() {
    let (mut store, seated) = founded();
    let promote_a_validator = Motion::Promote { key: key(1) };
    assert_eq!(
        refusal(vote(&mut store, 1, promote_a_validator)),
        reason::WRONG_STATE
    );
    let remove_a_stranger = Motion::Remove { key: key(8) };
    assert_eq!(
        refusal(vote(&mut store, 1, remove_a_stranger)),
        reason::NOT_FOUND
    );
    seated.borrow_mut().retain(|m| m.key != key(2));
    let demote_the_last = Motion::Demote { key: key(1) };
    assert_eq!(
        refusal(vote(&mut store, 1, demote_the_last)),
        reason::WRONG_STATE
    );
    assert_eq!(refusal(run(&mut store, 1, Op::Leave)), reason::WRONG_STATE);
}

#[test]
fn the_door_opens_and_closes_by_vote() {
    let (mut store, _) = founded();
    let open = Motion::Door { open: true };
    vote(&mut store, 1, open.clone()).unwrap();
    assert_eq!(vote(&mut store, 2, open.clone()).unwrap().0, Voted::Enacted);
    enroll(&mut store, 3, None).unwrap();
    assert_eq!(refusal(vote(&mut store, 1, open)), reason::WRONG_STATE);
    let close = Motion::Door { open: false };
    vote(&mut store, 1, close.clone()).unwrap();
    assert_eq!(vote(&mut store, 2, close).unwrap().0, Voted::Enacted);
    assert_eq!(refusal(enroll(&mut store, 4, None)), reason::UNAUTHORIZED);
}

#[test]
fn a_member_leaves_and_a_stranger_cannot() {
    let (mut store, seated) = founded();
    seated.borrow_mut().push(membership(3, Standing::Resident));
    let written = run(&mut store, 3, Op::Leave).unwrap();
    assert_eq!(written, vec![valset::Op::Remove { key: key(3) }]);
    assert_eq!(refusal(run(&mut store, 4, Op::Leave)), reason::NOT_FOUND);
}

#[test]
fn a_full_network_refuses_the_enroll() {
    let (mut store, seated) = founded();
    let residents = (0..MAX_MEMBERS - 2).map(|n| Membership {
        key: (n as u64).to_be_bytes().repeat(4),
        address: format!("node-{n}:9000"),
        standing: Standing::Resident,
    });
    seated.borrow_mut().extend(residents);
    let ticket = invite(1, 1, TIME + 1);
    assert_eq!(
        refusal(enroll(&mut store, 3, Some(ticket))),
        reason::CAPACITY
    );
}
