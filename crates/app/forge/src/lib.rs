// forge: a git server as a ducktape program, `gitcore` (objects, packs, walks, diffs, the wire) over `store`: the rules run natively over `store::Memory` (tests, fixtures), and the `program` feature adds the wasm32 program over the host.

// The wire, as a view and a git client see it.
mod contract;
mod read_contract;
mod review_contract;

// The state and the rules over it.
mod change_queries;
mod changes;
mod diffs;
mod discussion;
mod objects;
mod ops;
mod queries;
mod reads;
mod state;

#[cfg(feature = "program")]
mod program;
#[cfg(feature = "view")]
pub mod view;

pub use contract::*;
pub use ops::{PROGRAM, execute, init};
pub use queries::query;

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let text = |bytes: &[u8]| Value::Text(String::from_utf8_lossy(bytes).into_owned());
    let revision = |revision: &Revision| match revision {
        Revision::Ref(name) => text(name),
        Revision::Oid(oid) => Value::text(oid),
    };
    let change = |n: &u64| field("change", Value::Text(format!("#{n}")));
    let (verb, repo, fields) = match op {
        Op::Create { repo, hash } => (
            "Create",
            repo,
            vec![field(
                "hash",
                Value::text(match hash {
                    abi::HashKind::Sha256 => "SHA-256",
                    abi::HashKind::Sha1 => "SHA-1",
                }),
            )],
        ),
        Op::Configure { repo, settings } => (
            "Configure",
            repo,
            vec![
                field("head", text(&settings.head)),
                field("allow force", Value::Text(settings.allow_force.to_string())),
                field(
                    "allow delete",
                    Value::Text(settings.allow_delete.to_string()),
                ),
            ],
        ),
        Op::Grant { repo, key } => ("Grant", repo, vec![field("key", Value::Key(key.clone()))]),
        Op::Revoke { repo, key } => ("Revoke", repo, vec![field("key", Value::Key(key.clone()))]),
        Op::Push { repo, request } => ("Push", repo, vec![field("request", Value::bytes(request))]),
        Op::Merge {
            repo,
            into,
            from,
            result,
            change: n,
            ..
        } => (
            "Merge",
            repo,
            vec![
                field("from", revision(from)),
                field("into", text(into)),
                field("result", Value::text(result)),
                n.as_ref()
                    .map_or_else(|| field("change", Value::text("—")), change),
            ],
        ),
        Op::ChangeOpen {
            repo,
            from,
            into,
            title,
            reviewers,
            ..
        } => (
            "Open change",
            repo,
            vec![
                field("title", Value::text(title)),
                field("from", revision(from)),
                field("into", text(into)),
                field(
                    "reviewers",
                    Value::List(reviewers.iter().cloned().map(Value::Key).collect()),
                ),
            ],
        ),
        Op::ChangeEdit { repo, n, title, .. } => (
            "Edit change",
            repo,
            vec![
                change(n),
                field(
                    "title",
                    Value::Text(title.clone().unwrap_or_else(|| "—".into())),
                ),
            ],
        ),
        Op::ChangeClose { repo, n } => ("Close change", repo, vec![change(n)]),
        Op::ReviewSubmit { repo, n, review } => (
            "Review",
            repo,
            vec![
                change(n),
                field(
                    "verdict",
                    Value::text(match review.verdict {
                        Verdict::Approve => "approve",
                        Verdict::RequestChanges => "request changes",
                        Verdict::Comment => "comment",
                    }),
                ),
                field("commit", Value::text(&review.commit_oid)),
                field("comments", Value::Text(review.comments.len().to_string())),
            ],
        ),
    };
    let mut all = vec![field("repo", Value::text(repo))];
    all.extend(fields);
    describe::Description {
        title: format!("{verb} · {repo}"),
        fields: all,
    }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Create",
            "Configure",
            "Grant",
            "Revoke",
            "Push",
            "Merge",
            "ChangeOpen",
            "ChangeEdit",
            "ChangeClose",
            "ReviewSubmit",
        ]
    );
}
