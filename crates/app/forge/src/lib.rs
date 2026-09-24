// forge: a git server as a ducktape program, `gitcore` (objects, packs, walks, diffs, the wire) over `store`: the rules run natively over `store::Memory` (tests, fixtures), and the `program` feature adds the wasm32 program over the host.

mod change_queries;
mod changes;
mod contract;
mod diffs;
mod discussion;
mod ops;
#[cfg(feature = "program")]
mod program;
mod queries;
mod read_contract;
mod reads;
mod repo;
mod review_contract;
mod store;
#[cfg(feature = "view")]
pub mod view;

pub use contract::*;
pub use ops::{PROGRAM, execute, init};
pub use queries::query;

/// An op as a person reads it: a title and its fields.
#[cfg(feature = "view")]
pub fn describe(op: &Op) -> (String, Vec<(&'static str, String)>) {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let revision = |revision: &Revision| match revision {
        Revision::Ref(name) => text(name),
        Revision::Oid(oid) => oid.clone(),
    };
    let (verb, repo, fields) = match op {
        Op::Create { repo, hash } => (
            "Create",
            repo,
            vec![(
                "hash",
                match hash {
                    abi::HashKind::Sha256 => "SHA-256",
                    abi::HashKind::Sha1 => "SHA-1",
                }
                .into(),
            )],
        ),
        Op::Configure { repo, settings } => (
            "Configure",
            repo,
            vec![
                ("head", text(&settings.head)),
                ("allow force", settings.allow_force.to_string()),
                ("allow delete", settings.allow_delete.to_string()),
            ],
        ),
        Op::Grant { repo, key } => ("Grant", repo, vec![("key", abi::preview(key))]),
        Op::Revoke { repo, key } => ("Revoke", repo, vec![("key", abi::preview(key))]),
        Op::Push { repo, request } => ("Push", repo, vec![("request", abi::preview(request))]),
        Op::Merge {
            repo,
            into,
            from,
            result,
            change,
            ..
        } => (
            "Merge",
            repo,
            vec![
                ("from", revision(from)),
                ("into", text(into)),
                ("result", result.clone()),
                (
                    "change",
                    change.map_or_else(|| "—".into(), |n| format!("#{n}")),
                ),
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
                ("title", title.clone()),
                ("from", revision(from)),
                ("into", text(into)),
                ("reviewers", reviewers.len().to_string()),
            ],
        ),
        Op::ChangeEdit { repo, n, title, .. } => (
            "Edit change",
            repo,
            vec![
                ("change", format!("#{n}")),
                ("title", title.clone().unwrap_or_else(|| "—".into())),
            ],
        ),
        Op::ChangeClose { repo, n } => ("Close change", repo, vec![("change", format!("#{n}"))]),
        Op::ReviewSubmit { repo, n, review } => (
            "Review",
            repo,
            vec![
                ("change", format!("#{n}")),
                (
                    "verdict",
                    match review.verdict {
                        Verdict::Approve => "approve",
                        Verdict::RequestChanges => "request changes",
                        Verdict::Comment => "comment",
                    }
                    .into(),
                ),
                ("commit", review.commit_oid.clone()),
                ("comments", review.comments.len().to_string()),
            ],
        ),
    };
    let mut all = vec![("repo", repo.clone())];
    all.extend(fields);
    (format!("{verb} · {repo}"), all)
}
