//! Forge and chat over the host: what forge emits to chat runs in forge's
//! frame, so a change and its discussion channel land in one block.

use super::*;
use abi::HashKind;
use gitcore::wire::pktline;
use gitcore::{
    Commit, Hash, Kind, MemoryObjects, Mode, Objects as _, Oid, Signature, Tree, TreeEntry, pack,
};

const REPO: &str = "project";
const AUTHOR: u64 = 1;

fn bounds() -> forge::Bounds {
    forge::Bounds {
        max_objects: 1_000_000,
        max_delta_depth: 64,
        max_object_size: 256 << 20,
        push_walk: 10_000,
        fetch_walk: 1_000_000,
        merge_cost: 1024,
        page_size: 128,
        log_walk: 10_000,
        tree_walk: 1024,
        diff_bytes: 32 << 20,
        blob_bytes: 64,
        record_bytes: 64 << 10,
    }
}

fn apps() -> Vec<Founding> {
    vec![
        founding(chat::MODULE, &program("chat")),
        Founding {
            program: forge::MODULE.to_owned(),
            code: program("forge"),
            params: abi::encode(&bounds()),
        },
    ]
}

/// One commit holding `files`, on top of `parents`.
fn commit(store: &mut MemoryObjects, parents: &[Oid], files: &[(&str, &[u8])]) -> Oid {
    let entries = files
        .iter()
        .map(|(name, content)| TreeEntry {
            mode: Mode::Regular,
            name: name.as_bytes().to_vec(),
            id: store.put(Kind::Blob, content).unwrap(),
        })
        .collect();
    let tree = store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap();
    let who = Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time: TIME as i64 / 1000,
        offset_minutes: 0,
    };
    let commit = Commit {
        tree,
        parents: parents.to_vec(),
        author: who.clone(),
        committer: who,
        extra: Vec::new(),
        message: b"step\n".to_vec(),
    };
    store.put(Kind::Commit, &commit.serialize()).unwrap()
}

/// A receive-pack request: `refs` set from nothing, every object packed.
fn push_request(store: &MemoryObjects, refs: &[(Oid, &str)]) -> Vec<u8> {
    let zero = Oid::Sha1([0; 20]);
    let mut request = Vec::new();
    for (index, (new, name)) in refs.iter().enumerate() {
        let mut line = format!("{zero} {new} {name}").into_bytes();
        if index == 0 {
            line.extend_from_slice(b"\0report-status side-band-64k");
        }
        pktline::push(&mut request, &line);
    }
    request.extend_from_slice(pktline::flush());
    let objects = store.ids().map(|id| store.get(id).unwrap().unwrap());
    request.extend_from_slice(&pack::write(objects.collect::<Vec<_>>(), store.hash()).unwrap());
    request
}

#[test]
fn a_change_and_its_channel_land_in_one_block() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps()).await;
        let author = public(AUTHOR);
        let create = identity::Op::Create {
            name: "Ada".into(),
            scheme: Scheme::Ed25519,
        };
        net.apply(&author, identity::MODULE, &create).await;
        net.apply(
            &author,
            forge::MODULE,
            &forge::Op::Create {
                repo: REPO.into(),
                hash: HashKind::Sha1,
            },
        )
        .await;
        let mut store = MemoryObjects::new(Hash::Sha1);
        let main = commit(&mut store, &[], &[("README.md", b"# Project\n")]);
        let feature = commit(
            &mut store,
            &[main],
            &[("README.md", b"# Project\n\nMore.\n")],
        );
        let push = forge::Op::Push {
            repo: REPO.into(),
            request: push_request(
                &store,
                &[(main, "refs/heads/main"), (feature, "refs/heads/feature")],
            ),
        };
        net.apply(&author, forge::MODULE, &push).await;

        let open = forge::Op::ChangeOpen {
            repo: REPO.into(),
            from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
            into: b"refs/heads/main".to_vec(),
            title: "Feature".into(),
            body: String::new(),
            reviewers: Vec::new(),
        };
        let receipt = net.submit(&author, forge::MODULE, &open).await;
        assert!(
            matches!(receipt.outcome, Outcome::Applied { .. }),
            "{receipt:?}"
        );
        // chat created the channel in forge's frame
        let ran: Vec<(&str, bool)> = receipt
            .nested
            .iter()
            .map(|nested| {
                (
                    nested.program.as_str(),
                    matches!(nested.outcome, Outcome::Applied { .. }),
                )
            })
            .collect();
        assert_eq!(
            ran,
            [(chat::MODULE, true), (chat::MODULE, true)],
            "{receipt:?}"
        );
        let channel = format!("forge:{REPO}:1");
        let chat::Reply::Channel(Some(info)) = net
            .ask(
                chat::MODULE,
                &chat::Query::Channel {
                    channel_id: channel.clone(),
                },
            )
            .await
        else {
            panic!("no channel {channel} at height {}", net.height);
        };
        assert_eq!(info.channel.id, channel);
    });
}
