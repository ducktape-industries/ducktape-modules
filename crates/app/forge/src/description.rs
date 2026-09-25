//! [`describe`]: an op as a person reads it, a title and its fields. The
//! source of the `ducktape.describe` module this module ships
//! (`make wasm-describes`); people show as their accounts.
use describe::{Description, Value, field};

use crate::{Op, Principal, Revision, Verdict};

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this module ships (`make wasm-describes`).
pub fn describe(op: &Op) -> Description {
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
                    store::HashKind::Sha256 => "SHA-256",
                    store::HashKind::Sha1 => "SHA-1",
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
        Op::Grant { repo, principal } => (
            "Grant writer",
            repo,
            vec![field("writer", principal.value())],
        ),
        Op::Revoke { repo, principal } => (
            "Revoke writer",
            repo,
            vec![field("writer", principal.value())],
        ),
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
                    Value::List(reviewers.iter().map(Principal::value).collect()),
                ),
            ],
        ),
        Op::ChangeEdit {
            repo,
            n,
            title,
            reviewers,
            ..
        } => {
            let mut fields = vec![
                change(n),
                field(
                    "title",
                    Value::Text(title.clone().unwrap_or_else(|| "—".into())),
                ),
            ];
            if let Some(reviewers) = reviewers {
                let reviewers = reviewers.iter().map(Principal::value).collect();
                fields.push(field("reviewers", Value::List(reviewers)));
            }
            ("Edit change", repo, fields)
        }
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
    Description {
        title: format!("{verb} · {repo}"),
        fields: all,
    }
}
