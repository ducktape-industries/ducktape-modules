//! The module: the signer resolved to its principal, then every op and
//! every query, each handed to its function (`ops.rs`, `changes.rs`,
//! `queries.rs`, `reads.rs`, `change_queries.rs`).

use std::io::{self, Write};

use borsh::BorshSerialize;
use guest::{Error, ExecCtx, Module, QueryCtx};

use crate::change_queries::{change, changes, judgment};
use crate::changes::{Draft, Edit, MergeRequest, close, edit, merge_heads, open, submit_review};
use crate::contract::*;
use crate::ops::{configure, create, grant, init, push, revoke, signed_account, touch};
use crate::queries::{advertise, listing, refs, repos, upload};
use crate::reads::{blob, comparison, diff, log, tree};
use crate::state::{WRITERS, load_bounds, load_repo};

pub struct Forge;

/// A query's answer as the host takes it, unframed: one `Reply`'s borsh for
/// the screens, or git's own bytes for a git client.
pub struct RawReply(pub Vec<u8>);

impl BorshSerialize for RawReply {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.0)
    }
}

impl Module for Forge {
    type Op = Op;
    type Query = Query;
    type Response = RawReply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, params)
    }

    /// Runs one op as the signer's account (`ctx.sender()`).
    /// Every op names its repository; an accepted one marks it active.
    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let actor = &signed_account(ctx)?;
        let repo = op.repo().to_owned();
        let reply = match op {
            Op::Create { repo, hash } => create(ctx, actor, &repo, hash).map(|()| None),
            Op::Configure { repo, settings } => {
                configure(ctx, actor, &repo, settings).map(|()| None)
            }
            Op::Grant { repo, principal } => grant(ctx, actor, &repo, principal).map(|()| None),
            Op::Revoke { repo, principal } => revoke(ctx, actor, &repo, principal).map(|()| None),
            Op::Push { repo, request } => push(ctx, actor, &repo, &request).map(|()| None),
            Op::Merge {
                repo,
                into,
                from,
                expected_into,
                expected_from,
                result,
                change,
            } => {
                let merge = MergeRequest {
                    into,
                    from,
                    expected_into,
                    expected_from,
                    result,
                    change,
                };
                merge_heads(ctx, actor, &repo, merge).map(Some)
            }
            Op::ChangeOpen {
                repo,
                from,
                into,
                title,
                body,
                reviewers,
            } => {
                let draft = Draft {
                    from,
                    into,
                    title,
                    body,
                    reviewers,
                };
                open(ctx, actor, &repo, draft).map(Some)
            }
            Op::ChangeEdit {
                repo,
                n,
                title,
                body,
                reviewers,
            } => {
                let fields = Edit {
                    title,
                    body,
                    reviewers,
                };
                edit(ctx, actor, &repo, n, fields).map(Some)
            }
            Op::ChangeClose { repo, n } => close(ctx, actor, &repo, n).map(Some),
            Op::ReviewSubmit { repo, n, review } => {
                submit_review(ctx, actor, &repo, n, review).map(Some)
            }
        }?;
        if let Some(reply) = reply {
            ctx.set_return_data(abi::encode(&reply));
        }
        touch(ctx, &repo, ctx.env().height)
    }

    /// One height-bearing `Reply`, or a git protocol query's raw git bytes.
    fn query(ctx: &QueryCtx, query: Query) -> Result<RawReply, Error> {
        let height = ctx.env().height;
        let bounds = load_bounds(ctx)?;
        let scope = query.scope();
        let listing = |page: &PageRequest| {
            listing(page.bounded(bounds.page_size as u64), scope.clone(), height)
        };
        let reply = match &query {
            Query::Advertise { repo, service } => {
                return advertise(ctx, repo, *service).map(RawReply);
            }
            Query::Upload { repo, request } => return upload(ctx, repo, request).map(RawReply),
            Query::Repos { page } => Reply::Repos {
                height,
                page: repos(ctx, &listing(page)?)?,
            },
            Query::Repo { repo, page } => Reply::Repo {
                height,
                repo: RepoInfo {
                    name: repo.clone(),
                    repo: load_repo(ctx, repo)?,
                },
                bounds,
                writers: WRITERS
                    .page_of(ctx, repo, &listing(page)?)?
                    .map(|(_, principal)| principal),
            },
            Query::Refs { repo, page } => Reply::Refs {
                height,
                page: refs(ctx, repo, &listing(page)?)?,
            },
            Query::Activity { repo } => Reply::Activity {
                height,
                last_height: load_repo(ctx, repo)?.last_activity,
            },
            Query::Log { repo, from, page } => {
                log(ctx, height, &bounds, repo, from, &listing(page)?)?
            }
            Query::Tree {
                repo,
                at,
                path,
                page,
            } => tree(ctx, height, &bounds, repo, at, path, &listing(page)?)?,
            Query::Blob { repo, oid, range } => blob(ctx, height, &bounds, repo, oid, *range)?,
            Query::Diff {
                repo,
                base,
                head,
                path,
                page,
            } => diff(
                ctx,
                height,
                &bounds,
                repo,
                base,
                head,
                path.as_deref(),
                &listing(page)?,
            )?,
            Query::Compare { repo, from, into } => {
                comparison(ctx, height, &bounds, repo, from, into)?
            }
            Query::Changes { repo, filter, page } => Reply::Changes {
                height,
                page: changes(ctx, repo, filter, &listing(page)?)?,
            },
            Query::Change { repo, n, page } => change(ctx, height, repo, *n, &listing(page)?)?,
            Query::Judgment { principal, page } => Reply::Judgment {
                height,
                page: judgment(ctx, principal, &listing(page)?)?,
            },
        };
        Ok(RawReply(abi::encode(&reply)))
    }
}

#[cfg(feature = "module")]
guest::export!(Forge);
