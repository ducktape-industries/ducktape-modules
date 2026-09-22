use super::*;
use forge::{ChangeFilter, Query, Revision};

pub struct Story {
    pub source: TempDir,
    pub root: String,
    pub feature: String,
    pub clean: String,
    pub conflict: String,
    pub unrelated: String,
}
impl Story {
    pub fn pushed(remote: &Remote) -> Self {
        let source = tempfile::tempdir().unwrap();
        let dir = source.path();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "core.filemode", "false"]);
        std::fs::create_dir(dir.join("src")).unwrap();
        for (name, content) in [
            ("README.md", "# Project\n"),
            ("src/lib.rs", "one\nkeep\nold\nend\n"),
            ("gone.txt", "remove me\n"),
            ("mode.sh", "echo ok\n"),
        ] {
            std::fs::write(dir.join(name), content).unwrap();
        }
        std::fs::write(dir.join("image.bin"), [0, 1, 2, 3]).unwrap();
        std::fs::write(dir.join("large.txt"), vec![b'x'; 257]).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "Base"]);
        let root = head(dir);
        git(dir, &["tag", "-a", "v1", "-m", "Version one"]);
        git(dir, &["checkout", "-qb", "feature"]);
        std::fs::write(dir.join("src/lib.rs"), b"one\nkeep\n++ x\nend\nadded").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();
        std::fs::write(dir.join("new.txt"), b"new\n").unwrap();
        std::fs::write(dir.join("empty.txt"), b"").unwrap();
        std::fs::write(dir.join("image.bin"), [0, 9, 2, 3]).unwrap();
        std::fs::write(dir.join("large.txt"), vec![b'y'; 258]).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["update-index", "--chmod=+x", "mode.sh"]);
        git(
            dir,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{root},vendor"),
            ],
        );
        git(dir, &["commit", "-qm", "Feature\n\nReview these bytes."]);
        let feature = head(dir);
        git(dir, &["checkout", "-qb", "clean", "main"]);
        commit_file(dir, "ours.txt", "ours\n");
        let clean = head(dir);
        git(dir, &["checkout", "-qb", "conflict", "main"]);
        commit_file(dir, "src/lib.rs", "one\nkeep\nconflict\nend\n");
        let conflict = head(dir);
        git(dir, &["checkout", "--orphan", "unrelated"]);
        git(dir, &["commit", "-qm", "Unrelated root"]);
        let unrelated = head(dir);
        git(dir, &["checkout", "-q", "feature"]);
        git(
            dir,
            &[
                "push",
                "-q",
                &remote.url,
                "main",
                "feature",
                "clean",
                "conflict",
                "unrelated",
                "v1",
            ],
        );
        Self {
            source,
            root,
            feature,
            clean,
            conflict,
            unrelated,
        }
    }
    pub fn oid(&self, object: &str) -> String {
        git(self.source.path(), &["rev-parse", object])
            .trim()
            .into()
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
        cursor: None,
        limit: 128,
    }
}
pub fn changes() -> Query {
    Query::Changes {
        repo: REPO.into(),
        filter: ChangeFilter::default(),
        cursor: None,
        limit: 128,
    }
}
pub fn compare(from: &str, into: &str) -> Query {
    Query::Compare {
        repo: REPO.into(),
        from: reference(from),
        into: reference(into),
        cursor: None,
        limit: 128,
    }
}
pub fn review(story: &Story, verdict: forge::Verdict) -> Op {
    Op::ReviewSubmit {
        repo: REPO.into(),
        n: 1,
        review: forge::ReviewDraft {
            commit_oid: story.feature.clone(),
            base_oid: Some(story.root.clone()),
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
