// The fixture story, natively: the same objects real git built through the harness (Ada, one fixed second, the same files), pushed as one op, then heights that move like the kernel's.

use super::*;
use forge::{ChangeFilter, Query, ReviewDraft, Revision, Verdict};
use gitcore::Tag;

pub const REPO: &str = "project";
pub const TESTER: &[u8] = b"tester";

/// One program over `MemorySandbox` with the kernel's height discipline: an
/// op lands in a new block, whose queue (forge's chat emissions) is delivered
/// first; a chat write is a block of its own; a query moves nothing.
pub struct Rig {
    pub sandbox: MemorySandbox,
    pub height: u64,
    pub actor: Vec<u8>,
}

impl Rig {
    pub fn start(bounds: Bounds, hash: HashKind) -> Rig {
        let mut sandbox = MemorySandbox::default();
        forge::init(&mut sandbox, &abi::encode(&bounds)).unwrap();
        let mut rig = Rig {
            sandbox,
            height: 0,
            actor: TESTER.to_vec(),
        };
        rig.execute(&Op::Create {
            repo: REPO.into(),
            hash,
        })
        .unwrap();
        rig
    }

    pub fn env(&self) -> Env {
        Env {
            network: b"harness".to_vec(),
            height: self.height,
            time: TIME,
            me: "forge".into(),
            origin: Origin::External(self.actor.clone()),
            cause: Cause::Direct,
        }
    }

    pub fn advance(&mut self) {
        self.height += 1;
        for delivered in self.sandbox.deliver(self.height, TIME) {
            delivered.unwrap();
        }
    }

    // ponytail: no rollback on a refused op; the story never refuses one.
    pub fn execute(&mut self, op: &Op) -> Result<Vec<u8>, abi::Refusal> {
        self.advance();
        let env = self.env();
        forge::execute(&mut self.sandbox, &env, &abi::encode(op))?;
        Ok(self.sandbox.forge.take_output())
    }

    pub fn query(&self, query: &Query) -> Result<Vec<u8>, abi::Refusal> {
        forge::query(&self.sandbox, &self.env(), &abi::encode(query))
    }

    pub fn chat_execute(&mut self, party: chat::Party, msg: chat::ChatMsg) {
        self.advance();
        let frame = chat::Frame {
            party,
            height: self.height,
            time: TIME,
        };
        self.sandbox.chat_execute(&frame, msg).unwrap();
    }
}

pub struct Story {
    pub objects: MemoryObjects,
    pub root: String,
    pub feature: String,
    pub clean: String,
    pub conflict: String,
    pub unrelated: String,
}

fn entry(mode: Mode, name: &str, id: Oid) -> TreeEntry {
    TreeEntry {
        mode,
        name: name.as_bytes().to_vec(),
        id,
    }
}

fn put_tree(store: &mut MemoryObjects, entries: Vec<TreeEntry>) -> Oid {
    store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap()
}

impl Story {
    /// Git's history, object for object: `Base` on main with a `v1` tag,
    /// `feature` editing every kind of file, `clean` and `conflict` off main,
    /// an orphan `unrelated` over `conflict`'s tree.
    pub fn pushed(rig: &mut Rig) -> Story {
        let mut store = MemoryObjects::new(Hash::Sha1);
        let s = &mut store;
        let readme = blob(s, b"# Project\n");
        let gone = blob(s, b"remove me\n");
        let image = blob(s, &[0, 1, 2, 3]);
        let large = blob(s, &[b'x'; 257]);
        let mode_sh = blob(s, b"echo ok\n");
        let base_lib = blob(s, b"one\nkeep\nold\nend\n");
        let base_src = put_tree(s, vec![entry(Mode::Regular, "lib.rs", base_lib)]);
        let base = [
            entry(Mode::Regular, "README.md", readme),
            entry(Mode::Regular, "gone.txt", gone),
            entry(Mode::Regular, "image.bin", image),
            entry(Mode::Regular, "large.txt", large),
            entry(Mode::Regular, "mode.sh", mode_sh),
        ];
        let mut base_entries = base.to_vec();
        base_entries.push(entry(Mode::Directory, "src", base_src));
        let base_tree = put_tree(s, base_entries);
        let root = commit(s, base_tree, &[], TIME as i64, "Base");
        let tag = Tag {
            object: root,
            kind: Kind::Commit,
            name: b"v1".to_vec(),
            tagger: Some(signature(TIME as i64)),
            message: b"Version one\n".to_vec(),
        };
        let v1 = s.put(Kind::Tag, &tag.serialize()).unwrap();
        let feature_lib = blob(s, b"one\nkeep\n++ x\nend\nadded");
        let feature_src = put_tree(s, vec![entry(Mode::Regular, "lib.rs", feature_lib)]);
        let empty = blob(s, b"");
        let image_edited = blob(s, &[0, 9, 2, 3]);
        let large_edited = blob(s, &[b'y'; 258]);
        let new = blob(s, b"new\n");
        let feature_tree = put_tree(
            s,
            vec![
                entry(Mode::Regular, "README.md", readme),
                entry(Mode::Regular, "empty.txt", empty),
                entry(Mode::Regular, "image.bin", image_edited),
                entry(Mode::Regular, "large.txt", large_edited),
                entry(Mode::Executable, "mode.sh", mode_sh),
                entry(Mode::Regular, "new.txt", new),
                entry(Mode::Directory, "src", feature_src),
                entry(Mode::Gitlink, "vendor", root),
            ],
        );
        let feature = commit(
            s,
            feature_tree,
            &[root],
            TIME as i64,
            "Feature\n\nReview these bytes.",
        );
        let ours = blob(s, b"ours\n");
        let mut clean_entries = base.to_vec();
        clean_entries.push(entry(Mode::Regular, "ours.txt", ours));
        clean_entries.push(entry(Mode::Directory, "src", base_src));
        let clean_tree = put_tree(s, clean_entries);
        let clean = commit(s, clean_tree, &[root], TIME as i64, "ours.txt");
        let conflict_lib = blob(s, b"one\nkeep\nconflict\nend\n");
        let conflict_src = put_tree(s, vec![entry(Mode::Regular, "lib.rs", conflict_lib)]);
        let mut conflict_entries = base.to_vec();
        conflict_entries.push(entry(Mode::Directory, "src", conflict_src));
        let conflict_tree = put_tree(s, conflict_entries);
        let conflict = commit(s, conflict_tree, &[root], TIME as i64, "src/lib.rs");
        let unrelated = commit(s, conflict_tree, &[], TIME as i64, "Unrelated root");
        let zero = Oid::Sha1([0; 20]);
        let commands = [
            (zero, root, "refs/heads/main"),
            (zero, feature, "refs/heads/feature"),
            (zero, clean, "refs/heads/clean"),
            (zero, conflict, "refs/heads/conflict"),
            (zero, unrelated, "refs/heads/unrelated"),
            (zero, v1, "refs/tags/v1"),
        ];
        let pack = pack_of(&store, &all_ids(&store));
        rig.execute(&Op::Push {
            repo: REPO.into(),
            request: push_request(&commands, &pack),
        })
        .unwrap();
        Story {
            objects: store,
            root: root.to_hex(),
            feature: feature.to_hex(),
            clean: clean.to_hex(),
            conflict: conflict.to_hex(),
            unrelated: unrelated.to_hex(),
        }
    }

    /// `git rev-parse feature:<path>` for a file at the feature tip.
    pub fn oid(&self, path: &str) -> String {
        let feature = Oid::from_hex(Hash::Sha1, self.feature.as_bytes()).unwrap();
        let commit = Commit::parse(
            &self.objects.get(&feature).unwrap().unwrap().body,
            Hash::Sha1,
        )
        .unwrap();
        let mut tree = commit.tree;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let parsed =
                Tree::parse(&self.objects.get(&tree).unwrap().unwrap().body, Hash::Sha1).unwrap();
            let found = parsed
                .entries
                .iter()
                .find(|e| e.name == part.as_bytes())
                .unwrap_or_else(|| panic!("{path}: no {part}"))
                .id;
            if parts.peek().is_none() {
                return found.to_hex();
            }
            tree = found;
        }
        unreachable!()
    }

    /// `commit_file` then `git push feature`: one more commit on the feature
    /// tip carrying `name`, pushed with only the new objects.
    pub fn push_follow_up(&mut self, rig: &mut Rig, name: &str, content: &[u8]) -> String {
        let old = Oid::from_hex(Hash::Sha1, self.feature.as_bytes()).unwrap();
        let commit_bytes = self.objects.get(&old).unwrap().unwrap().body;
        let parsed = Commit::parse(&commit_bytes, Hash::Sha1).unwrap();
        let mut tree = Tree::parse(
            &self.objects.get(&parsed.tree).unwrap().unwrap().body,
            Hash::Sha1,
        )
        .unwrap();
        let s = &mut self.objects;
        let file = blob(s, content);
        tree.entries.push(entry(Mode::Regular, name, file));
        let new_tree = put_tree(s, tree.entries);
        let tip = commit(s, new_tree, &[old], TIME as i64, name);
        let pack = pack_of(s, &[file, new_tree, tip]);
        rig.execute(&Op::Push {
            repo: REPO.into(),
            request: push_request(&[(old, tip, "refs/heads/feature")], &pack),
        })
        .unwrap();
        self.feature = tip.to_hex();
        tip.to_hex()
    }

    pub fn open(&self, title: &str) -> Op {
        Op::ChangeOpen {
            repo: REPO.into(),
            from: reference("feature"),
            into: b"refs/heads/main".to_vec(),
            title: title.into(),
            body: "The author's body.".into(),
            reviewers: vec![b"reviewer".to_vec()],
        }
    }
}

pub fn reference(name: &str) -> Revision {
    Revision::Ref(format!("refs/heads/{name}").into_bytes())
}

pub fn change(n: u64) -> Query {
    Query::Change {
        repo: REPO.into(),
        n,
        page: Page::first(128),
    }
}

pub fn changes() -> Query {
    Query::Changes {
        repo: REPO.into(),
        filter: ChangeFilter::default(),
        page: Page::first(128),
    }
}

pub fn compare(from: &str, into: &str) -> Query {
    Query::Compare {
        repo: REPO.into(),
        from: reference(from),
        into: reference(into),
    }
}

pub fn review(feature: &str, root: &str, verdict: Verdict) -> Op {
    Op::ReviewSubmit {
        repo: REPO.into(),
        n: 1,
        review: ReviewDraft {
            commit_oid: feature.into(),
            base_oid: Some(root.into()),
            verdict,
            body: "Review body".into(),
            comments: vec![
                forge::LineComment {
                    path: b"src/lib.rs".to_vec(),
                    side: forge::Side::Old,
                    line: 3,
                    body: "Old side".into(),
                },
                forge::LineComment {
                    path: b"src/lib.rs".to_vec(),
                    side: forge::Side::New,
                    line: 2,
                    body: "Context is commentable".into(),
                },
            ],
        },
    }
}

/// The bounds the harness captured with: its defaults, blobs inline to 64 bytes.
pub fn fixture_bounds() -> Bounds {
    Bounds {
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
