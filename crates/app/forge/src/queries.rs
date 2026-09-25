//! The repository answers [`Forge::query`](crate::Forge) names, and git's
//! own bytes for a git client.

use abi::Refusal;
use gitcore::wire::receive::advertise_refs;
use gitcore::wire::smart_http_service_header;
use gitcore::wire::upload::{
    Command, capability_advertisement, fetch, ls_refs_response, parse_command,
};
use guest::{QueryCtx, stale};
use store::Listing;

use crate::contract::*;
use crate::objects::ObjectStore;
use crate::ops::{cap, refusal_of};
use crate::state::{ACTIVITY, REFS, load_bounds, load_refs, load_repo, repo_hash, storage};

const AGENT: &[u8] = b"ducktape-forge";

/// Repositories, the most recently active first.
pub(crate) fn repos(ctx: &QueryCtx, listing: &Listing) -> Result<PageReply<RepoInfo>, Refusal> {
    ACTIVITY.page_of(ctx, &(), listing)?.try_map(|(_, name)| {
        Ok(RepoInfo {
            repo: load_repo(ctx, &name)?,
            name,
        })
    })
}

/// A repository's refs in byte-name order.
pub(crate) fn refs(
    ctx: &QueryCtx,
    name: &str,
    listing: &Listing,
) -> Result<PageReply<RefInfo>, Refusal> {
    let hash = repo_hash(&load_repo(ctx, name)?);
    REFS.page_of(ctx, &name.to_owned(), listing)?
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
pub(crate) fn listing(page: Page, scope: Vec<u8>, height: u64) -> Result<Listing, Refusal> {
    let listing = page.listing(scope, height)?;
    if listing.cursor_height.is_some_and(|h| h != height) {
        return Err(stale("cursor height changed; restart the listing"));
    }
    Ok(listing)
}

pub(crate) fn advertise(ctx: &QueryCtx, name: &str, service: Service) -> Result<Vec<u8>, Refusal> {
    let repo = load_repo(ctx, name)?;
    let hash = repo_hash(&repo);
    let body = match service {
        Service::ReceivePack => {
            let refs = load_refs(ctx, name, hash)?;
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

pub(crate) fn upload(ctx: &QueryCtx, name: &str, request: &[u8]) -> Result<Vec<u8>, Refusal> {
    let repo = load_repo(ctx, name)?;
    let bounds = load_bounds(ctx)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(ctx, name, hash)?;
    let objects = ObjectStore::new(ctx, hash);
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
