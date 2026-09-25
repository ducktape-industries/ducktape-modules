//! The query path: one height-bearing borsh `Reply` for the screens, or
//! git's own bytes for a git client.

use gitcore::wire::receive::advertise_refs;
use gitcore::wire::smart_http_service_header;
use gitcore::wire::upload::{
    Command, capability_advertisement, fetch, ls_refs_response, parse_command,
};
use store::{Env, Error, Raw};
use store::{Listing, Reads, stale};

use crate::contract::*;
use crate::objects::ObjectStore;
use crate::ops::{cap, refusal_of};
use crate::state::{
    ACTIVITY, REFS, WRITERS, load_bounds, load_refs, load_repo, repo_hash, storage,
};
use crate::{change_queries, reads};

const AGENT: &[u8] = b"ducktape-forge";

/// A UI query's response bytes (one `Reply`), or a git protocol query's raw
/// git bytes; a refusal is `Err` through the ABI like any module's.
pub fn query(store: &impl Reads, env: &Env, query: Query) -> Result<Raw, Error> {
    let height = env.height;
    let bounds = load_bounds(store)?;
    let scope = query.scope();
    let listing =
        |page: &PageRequest| listing(page.bounded(bounds.page_size as u64), scope.clone(), height);
    let reply = match &query {
        Query::Advertise { repo, service } => return advertise(store, repo, *service).map(Raw),
        Query::Upload { repo, request } => return upload(store, repo, request).map(Raw),
        Query::Repos { page } => Reply::Repos {
            height,
            page: repos(store, &listing(page)?)?,
        },
        Query::Repo { repo, page } => Reply::Repo {
            height,
            repo: RepoInfo {
                name: repo.clone(),
                repo: load_repo(store, repo)?,
            },
            bounds,
            writers: WRITERS
                .page_of(store, repo, &listing(page)?)?
                .map(|(_, principal)| principal),
        },
        Query::Refs { repo, page } => Reply::Refs {
            height,
            page: refs(store, repo, &listing(page)?)?,
        },
        Query::Activity { repo } => Reply::Activity {
            height,
            last_height: load_repo(store, repo)?.last_activity,
        },
        Query::Log { repo, from, page } => {
            reads::log(store, height, &bounds, repo, from, &listing(page)?)?
        }
        Query::Tree {
            repo,
            at,
            path,
            page,
        } => reads::tree(store, height, &bounds, repo, at, path, &listing(page)?)?,
        Query::Blob { repo, oid, range } => reads::blob(store, height, &bounds, repo, oid, *range)?,
        Query::Diff {
            repo,
            base,
            head,
            path,
            page,
        } => reads::diff(
            store,
            height,
            &bounds,
            repo,
            base,
            head,
            path.as_deref(),
            &listing(page)?,
        )?,
        Query::Compare { repo, from, into } => {
            reads::comparison(store, height, &bounds, repo, from, into)?
        }
        Query::Changes { repo, filter, page } => Reply::Changes {
            height,
            page: change_queries::changes(store, repo, filter, &listing(page)?)?,
        },
        Query::Change { repo, n, page } => {
            change_queries::change(store, height, repo, *n, &listing(page)?)?
        }
        Query::Judgment { principal, page } => Reply::Judgment {
            height,
            page: change_queries::judgment(store, principal, &listing(page)?)?,
        },
    };
    Ok(Raw(store::encode(&reply)))
}

/// Repositories, the most recently active first.
fn repos(store: &impl Reads, listing: &Listing) -> Result<PageResponse<RepoInfo>, Error> {
    ACTIVITY.page_of(store, &(), listing)?.try_map(|(_, name)| {
        Ok(RepoInfo {
            repo: load_repo(store, &name)?,
            name,
        })
    })
}

/// A repository's refs in byte-name order.
fn refs(store: &impl Reads, name: &str, listing: &Listing) -> Result<PageResponse<RefInfo>, Error> {
    let hash = repo_hash(&load_repo(store, name)?);
    REFS.page_of(store, &name.to_owned(), listing)?
        .try_map(|((_, name), bytes)| {
            let oid = gitcore::Oid::from_bytes(hash, &bytes).map_err(|e| storage(e.to_string()))?;
            Ok(RefInfo {
                name,
                target: oid.to_hex(),
            })
        })
}

/// A forge listing can be rewritten by a push, so a cursor is good for the
/// height that answered it and no other.
fn listing(page: PageRequest, scope: Vec<u8>, height: u64) -> Result<Listing, Error> {
    let listing = page.listing(scope, height)?;
    if listing.cursor_height.is_some_and(|h| h != height) {
        return Err(stale("cursor height changed; restart the listing"));
    }
    Ok(listing)
}

fn advertise(store: &impl Reads, name: &str, service: Service) -> Result<Vec<u8>, Error> {
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

fn upload(store: &impl Reads, name: &str, request: &[u8]) -> Result<Vec<u8>, Error> {
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
