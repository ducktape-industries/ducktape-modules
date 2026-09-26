// The module natively over `guest::MockHost`: what the founding suite checks on the host, without the host.

use guest::{BlobId, Cause, Env, Origin, code};
use guest::{MockHost, Module};
use store::PageRequest;

use crate::{AUTHORITY, Change, Entry, Genesis, Modules, Op, Query, Reply, Scheduled, View};

fn env(height: u64, origin: Origin) -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height,
        time: 0,
        module: crate::MODULE.into(),
        origin,
        // these rules read the origin alone
        sender: None,
        roles: guest::MockHost::roles(),
        cause: Cause::Direct,
    }
}

fn authority(height: u64) -> Env {
    env(height, Origin::Module(AUTHORITY.into()))
}

fn entry(program: &str, code: BlobId) -> Entry {
    Entry {
        program: program.into(),
        code,
        params: vec![],
    }
}

fn founded() -> (MockHost, BlobId) {
    let store = MockHost::default();
    crate::rules::init(
        &store.exec(env(0, Origin::Root)),
        Genesis {
            programs: vec![entry("boot", BlobId::Sha256([1; 32]))],
            views: vec![View {
                name: "lens".into(),
                view: BlobId::Sha256([2; 32]),
            }],
        },
    );
    Modules::execute(
        &store.exec(env(1, Origin::Signed(vec![9]))),
        Op::Publish {
            body: b"wasm".to_vec(),
        },
    )
    .unwrap();
    let code: BlobId = abi::decode(&store.take_output()).unwrap();
    (store, code)
}

fn programs(store: &MockHost, height: u64) -> Vec<String> {
    match Modules::query(&store.query(env(height, Origin::Root)), Query::At(height)).unwrap() {
        Reply::Programs(entries) => entries.into_iter().map(|e| e.program).collect(),
        other => panic!("{other:?}"),
    }
}

fn schedule(store: &MockHost, height: u64, change: Change) -> Result<(), guest::Error> {
    Modules::execute(
        &store.exec(authority(1)),
        Op::Schedule(Scheduled { height, change }),
    )
}

#[test]
fn publishing_stores_the_code_under_its_blob_id() {
    let (store, code) = founded();
    assert_eq!(store.borrow().blobs[&code].kind, crate::CODE_KIND);
    assert_eq!(store.borrow().blobs[&code].body, b"wasm");
}

#[test]
fn only_the_authority_schedules_and_only_published_code_in_the_future() {
    let (store, code) = founded();
    let set = Change::Set(entry("new", code));
    let stranger = Modules::execute(
        &store.exec(env(1, Origin::Signed(vec![9]))),
        Op::Schedule(Scheduled {
            height: 5,
            change: set.clone(),
        }),
    );
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);
    assert_eq!(
        schedule(&store, 1, set.clone()).unwrap_err().code,
        code::INVALID_INPUT
    );
    let unpublished = Change::Set(entry("new", BlobId::Sha256([7; 32])));
    assert_eq!(
        schedule(&store, 5, unpublished).unwrap_err().code,
        code::NOT_FOUND
    );
    schedule(&store, 5, set.clone()).unwrap();
    assert_eq!(
        schedule(&store, 5, set).unwrap_err().code,
        code::ALREADY_EXISTS
    );
}

#[test]
fn a_scheduled_change_is_seen_at_its_height_and_folded_by_the_next_op() {
    let (store, code) = founded();
    schedule(&store, 5, Change::Set(entry("new", code))).unwrap();
    schedule(&store, 6, Change::Remove("boot".into())).unwrap();
    assert_eq!(programs(&store, 4), ["boot"]);
    assert_eq!(programs(&store, 5), ["boot", "new"]);
    assert_eq!(programs(&store, 6), ["new"]);
    Modules::execute(
        &store.exec(env(6, Origin::Signed(vec![9]))),
        Op::Publish { body: vec![1] },
    )
    .unwrap();
    assert_eq!(programs(&store, 6), ["new"]);
    let Reply::Scheduled(page) = Modules::query(
        &store.query(env(6, Origin::Root)),
        Query::Scheduled {
            page: PageRequest::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(page.items.is_empty(), "folded changes leave the schedule");
    let Reply::Program { height, entry } = Modules::query(
        &store.query(env(6, Origin::Root)),
        Query::Program("new".into()),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((height, entry.map(|e| e.code)), (6, Some(code)));
}

#[test]
fn the_schedule_pages_in_height_order_and_a_cancel_removes_one_change() {
    let (store, code) = founded();
    for height in [30u64, 4, 200] {
        schedule(&store, height, Change::Set(entry("p", code))).unwrap();
    }
    let ask = |store: &MockHost, page: PageRequest| match Modules::query(
        &store.query(env(2, Origin::Root)),
        Query::Scheduled { page },
    )
    .unwrap()
    {
        Reply::Scheduled(page) => page,
        other => panic!("{other:?}"),
    };
    let first = ask(&store, PageRequest::first(2));
    assert_eq!(
        first.items.iter().map(|s| s.height).collect::<Vec<_>>(),
        [4, 30],
        "numeric order, not lexical"
    );
    assert_eq!(first.height, 2);
    let rest = ask(
        &store,
        PageRequest {
            after: first.next,
            limit: Some(2),
        },
    );
    assert_eq!(rest.items[0].height, 200);
    assert_eq!(rest.next, None);
    Modules::execute(
        &store.exec(authority(2)),
        Op::Cancel {
            height: 30,
            program: "p".into(),
        },
    )
    .unwrap();
    let gone = Modules::execute(
        &store.exec(authority(2)),
        Op::Cancel {
            height: 30,
            program: "p".into(),
        },
    );
    assert_eq!(gone.unwrap_err().code, code::NOT_FOUND);
    assert_eq!(ask(&store, PageRequest::default()).items.len(), 2);
}

fn views(store: &MockHost, height: u64) -> Vec<(String, BlobId)> {
    match Modules::query(
        &store.query(env(height, Origin::Root)),
        Query::Views(height),
    )
    .unwrap()
    {
        Reply::Views(views) => views.into_iter().map(|v| (v.name, v.view)).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_view_is_listed_apart_from_the_programs_and_scheduled_like_one() {
    let (store, code) = founded();
    assert_eq!(views(&store, 1), [("lens".into(), BlobId::Sha256([2; 32]))]);
    assert_eq!(programs(&store, 1), ["boot"], "a view is never a program");
    let explorer = Change::SetView(View {
        name: "explorer".into(),
        view: code,
    });
    let stranger = Modules::execute(
        &store.exec(env(1, Origin::Signed(vec![9]))),
        Op::Schedule(Scheduled {
            height: 5,
            change: explorer.clone(),
        }),
    );
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);
    let unpublished = Change::SetView(View {
        name: "explorer".into(),
        view: BlobId::Sha256([7; 32]),
    });
    assert_eq!(
        schedule(&store, 5, unpublished).unwrap_err().code,
        code::NOT_FOUND
    );
    let over_a_program = Change::SetView(View {
        name: "boot".into(),
        view: code,
    });
    assert_eq!(
        schedule(&store, 5, over_a_program).unwrap_err().code,
        code::ALREADY_EXISTS
    );
    schedule(&store, 5, explorer).unwrap();
    schedule(&store, 6, Change::RemoveView("lens".into())).unwrap();
    assert_eq!(views(&store, 4).len(), 1);
    assert_eq!(
        views(&store, 5),
        [
            ("explorer".into(), code),
            ("lens".into(), BlobId::Sha256([2; 32]))
        ]
    );
    Modules::execute(
        &store.exec(env(6, Origin::Signed(vec![9]))),
        Op::Publish { body: vec![1] },
    )
    .unwrap();
    assert_eq!(views(&store, 6), [("explorer".into(), code)]);
    assert_eq!(programs(&store, 6), ["boot"]);
}

fn cancel(store: &MockHost, height: u64, program: &str) -> Result<(), guest::Error> {
    Modules::execute(
        &store.exec(authority(1)),
        Op::Cancel {
            height,
            program: program.into(),
        },
    )
}

#[test]
fn a_name_is_one_kind_whatever_order_the_changes_are_scheduled_in() {
    let (store, code) = founded();
    let view = |name: &str| {
        Change::SetView(View {
            name: name.into(),
            view: code,
        })
    };
    schedule(&store, 10, view("x")).unwrap();
    assert_eq!(
        schedule(&store, 5, Change::Set(entry("x", code)))
            .unwrap_err()
            .code,
        code::ALREADY_EXISTS,
        "a program landing before a pending view of its name"
    );
    schedule(&store, 5, Change::Set(entry("y", code))).unwrap();
    assert_eq!(
        schedule(&store, 3, view("y")).unwrap_err().code,
        code::ALREADY_EXISTS,
        "a view landing before a pending program of its name"
    );
    assert_eq!(
        schedule(&store, 20, Change::Set(entry("lens", code)))
            .unwrap_err()
            .code,
        code::ALREADY_EXISTS,
        "a listed view"
    );
    schedule(&store, 6, Change::Remove("boot".into())).unwrap();
    schedule(&store, 8, view("boot")).unwrap();
    assert_eq!(
        cancel(&store, 6, "boot").unwrap_err().code,
        code::ALREADY_EXISTS,
        "cancelling the removal would leave boot both kinds from 8"
    );
    assert_eq!(views(&store, 10).len(), 3);
    assert_eq!(programs(&store, 10), ["y"]);
}

#[test]
fn a_removal_names_one_of_its_own_kind() {
    let (store, code) = founded();
    for (change, what) in [
        (Change::Remove("ghost".into()), "no such program"),
        (Change::RemoveView("ghost".into()), "no such view"),
        (Change::Remove("lens".into()), "a view is not a program"),
        (Change::RemoveView("boot".into()), "a program is not a view"),
    ] {
        assert_eq!(
            schedule(&store, 5, change).unwrap_err().code,
            code::NOT_FOUND,
            "{what}"
        );
    }
    schedule(&store, 5, Change::Set(entry("new", code))).unwrap();
    schedule(&store, 6, Change::Remove("new".into())).unwrap();
    assert_eq!(
        schedule(&store, 4, Change::Remove("new".into()))
            .unwrap_err()
            .code,
        code::NOT_FOUND,
        "not yet seated at 4"
    );
}

#[test]
fn the_host_contract_is_a_prefix_of_the_program_contract() {
    assert_eq!(
        abi::encode(&abi::role::registry::Query::At(9)),
        abi::encode(&super::Query::At(9))
    );
    let entry = abi::role::registry::Entry {
        program: "p".into(),
        code: abi::BlobId::Sha256([1; 32]),
        params: vec![2],
    };
    assert_eq!(
        abi::encode(&abi::role::registry::Reply::Programs(vec![entry.clone()])),
        abi::encode(&super::Reply::Programs(vec![entry]))
    );
}
