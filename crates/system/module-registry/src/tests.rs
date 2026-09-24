// The rules natively over `store::Memory`: what the founding suite checks on the host, without the host.

use abi::{BlobId, Cause, Env, Origin, reason};
use store::{Memory, Page};

use crate::{AUTHORITY, Change, Entry, Genesis, Op, Query, Reply, Scheduled, View};

fn env(height: u64, origin: Origin) -> Env {
    Env {
        network: b"net".to_vec(),
        height,
        time: 0,
        me: crate::PROGRAM.into(),
        origin,
        cause: Cause::Direct,
    }
}

fn authority(height: u64) -> Env {
    env(height, Origin::Program(AUTHORITY.into()))
}

fn entry(program: &str, code: BlobId) -> Entry {
    Entry {
        program: program.into(),
        code,
        params: vec![],
    }
}

fn founded() -> (Memory, BlobId) {
    let mut store = Memory::default();
    crate::init(
        &mut store,
        Genesis {
            programs: vec![entry("boot", BlobId::Sha256([1; 32]))],
            views: vec![View {
                name: "lens".into(),
                view: BlobId::Sha256([2; 32]),
            }],
        },
    );
    crate::execute(
        &mut store,
        &env(1, Origin::External(vec![9])),
        Op::Publish {
            body: b"wasm".to_vec(),
        },
    )
    .unwrap();
    let code: BlobId = abi::decode(&store.take_output()).unwrap();
    (store, code)
}

fn programs(store: &Memory, height: u64) -> Vec<String> {
    match crate::query(store, &env(height, Origin::System), Query::At(height)).unwrap() {
        Reply::Programs(entries) => entries.into_iter().map(|e| e.program).collect(),
        other => panic!("{other:?}"),
    }
}

fn schedule(store: &mut Memory, height: u64, change: Change) -> Result<(), abi::Refusal> {
    crate::execute(
        store,
        &authority(1),
        Op::Schedule(Scheduled { height, change }),
    )
}

#[test]
fn publishing_stores_the_code_under_its_blob_id() {
    let (store, code) = founded();
    assert_eq!(store.blobs[&code].kind, crate::CODE_KIND);
    assert_eq!(store.blobs[&code].body, b"wasm");
}

#[test]
fn only_the_authority_schedules_and_only_published_code_in_the_future() {
    let (mut store, code) = founded();
    let set = Change::Set(entry("new", code));
    let stranger = crate::execute(
        &mut store,
        &env(1, Origin::External(vec![9])),
        Op::Schedule(Scheduled {
            height: 5,
            change: set.clone(),
        }),
    );
    assert_eq!(stranger.unwrap_err().reason, reason::UNAUTHORIZED);
    assert_eq!(
        schedule(&mut store, 1, set.clone()).unwrap_err().reason,
        reason::INVALID_INPUT
    );
    let unpublished = Change::Set(entry("new", BlobId::Sha256([7; 32])));
    assert_eq!(
        schedule(&mut store, 5, unpublished).unwrap_err().reason,
        reason::NOT_FOUND
    );
    schedule(&mut store, 5, set.clone()).unwrap();
    assert_eq!(
        schedule(&mut store, 5, set).unwrap_err().reason,
        reason::ALREADY_EXISTS
    );
}

#[test]
fn a_scheduled_change_is_seen_at_its_height_and_folded_by_the_next_op() {
    let (mut store, code) = founded();
    schedule(&mut store, 5, Change::Set(entry("new", code))).unwrap();
    schedule(&mut store, 6, Change::Remove("boot".into())).unwrap();
    assert_eq!(programs(&store, 4), ["boot"]);
    assert_eq!(programs(&store, 5), ["boot", "new"]);
    assert_eq!(programs(&store, 6), ["new"]);
    crate::execute(
        &mut store,
        &env(6, Origin::External(vec![9])),
        Op::Publish { body: vec![1] },
    )
    .unwrap();
    assert_eq!(programs(&store, 6), ["new"]);
    let Reply::Scheduled(page) = crate::query(
        &store,
        &env(6, Origin::System),
        Query::Scheduled {
            page: Page::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(page.items.is_empty(), "folded changes leave the schedule");
    let Reply::Program { height, entry } = crate::query(
        &store,
        &env(6, Origin::System),
        Query::Program("new".into()),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((height, entry.map(|e| e.code)), (6, Some(code)));
}

#[test]
fn the_schedule_pages_in_height_order_and_a_cancel_removes_one_change() {
    let (mut store, code) = founded();
    for height in [30u64, 4, 200] {
        schedule(&mut store, height, Change::Set(entry("p", code))).unwrap();
    }
    let ask = |store: &Memory, page: Page| match crate::query(
        store,
        &env(2, Origin::System),
        Query::Scheduled { page },
    )
    .unwrap()
    {
        Reply::Scheduled(page) => page,
        other => panic!("{other:?}"),
    };
    let first = ask(&store, Page::first(2));
    assert_eq!(
        first.items.iter().map(|s| s.height).collect::<Vec<_>>(),
        [4, 30],
        "numeric order, not lexical"
    );
    assert_eq!(first.height, 2);
    let rest = ask(
        &store,
        Page {
            after: first.next,
            limit: Some(2),
        },
    );
    assert_eq!(rest.items[0].height, 200);
    assert_eq!(rest.next, None);
    crate::execute(
        &mut store,
        &authority(2),
        Op::Cancel {
            height: 30,
            program: "p".into(),
        },
    )
    .unwrap();
    let gone = crate::execute(
        &mut store,
        &authority(2),
        Op::Cancel {
            height: 30,
            program: "p".into(),
        },
    );
    assert_eq!(gone.unwrap_err().reason, reason::NOT_FOUND);
    assert_eq!(ask(&store, Page::default()).items.len(), 2);
}

fn views(store: &Memory, height: u64) -> Vec<(String, BlobId)> {
    match crate::query(store, &env(height, Origin::System), Query::Views(height)).unwrap() {
        Reply::Views(views) => views.into_iter().map(|v| (v.name, v.view)).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_view_is_listed_apart_from_the_programs_and_scheduled_like_one() {
    let (mut store, code) = founded();
    assert_eq!(views(&store, 1), [("lens".into(), BlobId::Sha256([2; 32]))]);
    assert_eq!(programs(&store, 1), ["boot"], "a view is never a program");
    let explorer = Change::SetView(View {
        name: "explorer".into(),
        view: code,
    });
    let stranger = crate::execute(
        &mut store,
        &env(1, Origin::External(vec![9])),
        Op::Schedule(Scheduled {
            height: 5,
            change: explorer.clone(),
        }),
    );
    assert_eq!(stranger.unwrap_err().reason, reason::UNAUTHORIZED);
    let unpublished = Change::SetView(View {
        name: "explorer".into(),
        view: BlobId::Sha256([7; 32]),
    });
    assert_eq!(
        schedule(&mut store, 5, unpublished).unwrap_err().reason,
        reason::NOT_FOUND
    );
    let over_a_program = Change::SetView(View {
        name: "boot".into(),
        view: code,
    });
    assert_eq!(
        schedule(&mut store, 5, over_a_program).unwrap_err().reason,
        reason::ALREADY_EXISTS
    );
    schedule(&mut store, 5, explorer).unwrap();
    schedule(&mut store, 6, Change::RemoveView("lens".into())).unwrap();
    assert_eq!(views(&store, 4).len(), 1);
    assert_eq!(
        views(&store, 5),
        [
            ("explorer".into(), code),
            ("lens".into(), BlobId::Sha256([2; 32]))
        ]
    );
    crate::execute(
        &mut store,
        &env(6, Origin::External(vec![9])),
        Op::Publish { body: vec![1] },
    )
    .unwrap();
    assert_eq!(views(&store, 6), [("explorer".into(), code)]);
    assert_eq!(programs(&store, 6), ["boot"]);
}

#[test]
fn the_host_contract_is_a_prefix_of_the_program_contract() {
    assert_eq!(
        abi::encode(&abi::module_registry::Query::At(9)),
        abi::encode(&super::Query::At(9))
    );
    let entry = abi::module_registry::Entry {
        program: "p".into(),
        code: abi::BlobId::Sha256([1; 32]),
        params: vec![2],
    };
    assert_eq!(
        abi::encode(&abi::module_registry::Reply::Programs(vec![entry.clone()])),
        abi::encode(&super::Reply::Programs(vec![entry]))
    );
}
