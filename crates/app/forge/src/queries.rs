// The query path: one height-bearing borsh `Reply` for the screens, or git's own bytes for a git client.

use abi::{Env, Refusal};
use gitcore::wire::receive::advertise_refs;
use gitcore::wire::smart_http_service_header;
use gitcore::wire::upload::{
    Command, capability_advertisement, fetch, ls_refs_response, parse_command,
};
use store::{Listing, Reads, decoded, invalid, stale};

use crate::contract::*;
use crate::objects::ObjectStore;
use crate::ops::{PROGRAM, cap, refusal_of};
use crate::state::{
    ACTIVITY, REFS, RefName, WRITERS, load_bounds, load_refs, load_repo, repo_hash, storage,
};

const AGENT: &[u8] = b"ducktape-forge";

/// A UI query's response bytes (one `Reply`), or a git protocol query's raw
/// git bytes; a refusal is `Err` through the ABI like any program's.
pub fn query(store: &impl Reads, env: &Env, request: &[u8]) -> Result<Vec<u8>, Refusal> {
    let query: Query = decoded(PROGRAM, "Query", request)?;
    match &query {
        Query::Advertise { repo, service } => advertise(store, repo, *service),
        Query::Upload { repo, request } => upload(store, repo, request),
        _ => Ok(abi::encode(&answer(store, env.height, &query)?)),
    }
}

fn answer(store: &impl Reads, height: u64, query: &Query) -> Result<Reply, Refusal> {
    let bounds = load_bounds(store)?;
    let listing = query
        .page()
        .map(|page| listing(page.bounded(bounds.page_size as u64), query, height))
        .transpose()?;
    let paged = || {
        listing
            .as_ref()
            .ok_or_else(|| invalid("this query has no page"))
    };
    Ok(match query {
        Query::Repos { .. } => Reply::Repos {
            height,
            page: repos(store, paged()?)?,
        },
        Query::Repo { repo, .. } => Reply::Repo {
            height,
            repo: RepoInfo {
                name: repo.clone(),
                repo: load_repo(store, repo)?,
            },
            bounds,
            writers: WRITERS
                .page_of(store, &repo.clone(), paged()?)?
                .map(|(_, key)| key),
        },
        Query::Refs { repo, .. } => Reply::Refs {
            height,
            page: refs(store, repo, paged()?)?,
        },
        Query::Activity { repo } => Reply::Activity {
            height,
            last_height: load_repo(store, repo)?.last_activity,
        },
        Query::Changes { .. } | Query::Change { .. } | Query::Judgment { .. } => {
            crate::change_queries::answer(store, height, query, paged()?)?
        }
        _ => crate::reads::answer(store, height, query, &bounds, listing.as_ref())?,
    })
}

/// Repositories, the most recently active first.
fn repos(store: &impl Reads, listing: &Listing) -> Result<PageReply<RepoInfo>, Refusal> {
    ACTIVITY.page_of(store, &(), listing)?.try_map(|(_, name)| {
        Ok(RepoInfo {
            repo: load_repo(store, &name)?,
            name,
        })
    })
}

/// A repository's refs in byte-name order.
fn refs(store: &impl Reads, name: &str, listing: &Listing) -> Result<PageReply<RefInfo>, Refusal> {
    let hash = repo_hash(&load_repo(store, name)?);
    REFS.page_of(store, &name.to_owned(), listing)?
        .try_map(|((_, RefName(name)), bytes)| {
            let oid = gitcore::Oid::from_bytes(hash, &bytes).map_err(|e| storage(e.to_string()))?;
            Ok(RefInfo {
                name,
                target: oid.to_hex(),
            })
        })
}

/// A forge listing can be rewritten by a push, so a cursor is good for the
/// height that answered it and no other.
fn listing(page: Page, query: &Query, height: u64) -> Result<Listing, Refusal> {
    let listing = page.listing(query.scope(), height)?;
    if listing.cursor_height.is_some_and(|h| h != height) {
        return Err(stale("cursor height changed; restart the listing"));
    }
    Ok(listing)
}

fn advertise(store: &impl Reads, name: &str, service: Service) -> Result<Vec<u8>, Refusal> {
    let repo = load_repo(store, name)?;
    let hash = repo_hash(&repo);
    let body = match service {
        Service::ReceivePack => {
            let refs = load_refs(store, name, hash)?;
            let object_format = format!("object-format={}", hash.name());
            let agent = format!("agent={}", String::from_utf8_lossy(AGENT));
            advertise_refs(
                &refs,
                hash,
                &[
                    b"report-status",
                    b"delete-refs",
                    b"side-band-64k",
                    b"ofs-delta",
                    object_format.as_bytes(),
                    agent.as_bytes(),
                ],
            )
        }
        Service::UploadPack => {
            let mut body = smart_http_service_header(b"git-upload-pack");
            body.extend_from_slice(&capability_advertisement(hash, AGENT));
            body
        }
    };
    Ok(body)
}

fn upload(store: &impl Reads, name: &str, request: &[u8]) -> Result<Vec<u8>, Refusal> {
    let repo = load_repo(store, name)?;
    let bounds = load_bounds(store)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(store, name, hash)?;
    let objects = ObjectStore::new(store, hash);
    let Some(command) = parse_command(request, hash).map_err(refusal_of)? else {
        return Ok(Vec::new());
    };
    let mut response = Vec::new();
    let served = match command {
        Command::LsRefs(command) => ls_refs_response(
            &objects,
            &refs,
            Some(&repo.settings.head),
            &command,
            cap(bounds.fetch_walk),
        )
        .map(|body| response = body),
        Command::Fetch(command) => fetch(
            &objects,
            &refs,
            &command,
            cap(bounds.fetch_walk),
            &mut |chunk| response.extend_from_slice(chunk),
        ),
    };
    served.map_err(refusal_of)?;
    Ok(response)
}
