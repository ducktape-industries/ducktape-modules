//! UI queries produce one height-bearing Borsh reply. Git protocol queries stream Git bytes.
use crate::contract::*;
use crate::ops::{PROGRAM, cap, refusal_of, storage};
use crate::repo::{load_bounds, load_refs, load_repo, refs_prefix, repo_hash, writers_prefix};
use crate::store::Store;
use abi::{Env, Refusal};
use gitcore::wire::receive::advertise_refs;
use gitcore::wire::smart_http_service_header;
use gitcore::wire::upload::{
    Command, capability_advertisement, fetch, ls_refs_response, parse_command,
};
use store::{Listing, Reads, decoded, invalid, stale};
const AGENT: &[u8] = b"ducktape-forge";

/// A UI query's response bytes (one `Reply`), or a git protocol query's raw
/// git bytes; a refusal is `Err` through the ABI like any program's.
pub fn query<S: Reads>(sandbox: &S, env: &Env, request: &[u8]) -> Result<Vec<u8>, Refusal> {
    let query: Query = decoded(PROGRAM, "Query", request)?;
    match &query {
        Query::Advertise { repo, service } => advertise(sandbox, repo, *service),
        Query::Upload { repo, request } => upload(sandbox, repo, request),
        _ => Ok(abi::encode(&answer(sandbox, env.height, &query)?)),
    }
}
fn answer<S: Reads>(s: &S, height: u64, q: &Query) -> Result<Reply, Refusal> {
    let bounds = load_bounds(s)?;
    let paging = q
        .page()
        .map(|p| listing(p.bounded(bounds.page_size as u64), q, height))
        .transpose()?;
    let p = || {
        paging
            .as_ref()
            .ok_or_else(|| invalid("this query has no page"))
    };
    Ok(match q {
        Query::Repos { .. } => {
            let page = p()?
                .reply(
                    s.scan(p()?.scan_ahead(b"a/"))
                        .into_iter()
                        .map(|e| (e.key, e.value)),
                )
                .try_map(|value| {
                    let name =
                        String::from_utf8(value).map_err(|_| storage("bad repo activity index"))?;
                    Ok(RepoInfo {
                        repo: load_repo(s, &name)?,
                        name,
                    })
                })?;
            Reply::Repos { height, page }
        }
        Query::Repo { repo, .. } => {
            let record = load_repo(s, repo)?;
            let prefix = writers_prefix(repo);
            let writers = p()?.reply(
                s.scan(p()?.scan_ahead(&prefix))
                    .into_iter()
                    .map(|e| (e.key.clone(), e.key[prefix.len()..].to_vec())),
            );
            Reply::Repo {
                height,
                repo: RepoInfo {
                    name: repo.clone(),
                    repo: record,
                },
                bounds,
                writers,
            }
        }
        Query::Refs { repo, .. } => {
            let record = load_repo(s, repo)?;
            let prefix = refs_prefix(repo);
            let page = p()?
                .reply(
                    s.scan(p()?.scan_ahead(&prefix))
                        .into_iter()
                        .map(|e| (e.key.clone(), e)),
                )
                .try_map(|e| {
                    let oid = gitcore::Oid::from_bytes(repo_hash(&record), &e.value)
                        .map_err(|e| storage(e.to_string()))?;
                    Ok(RefInfo {
                        name: e.key[prefix.len()..].to_vec(),
                        target: oid.to_hex(),
                    })
                })?;
            Reply::Refs { height, page }
        }
        Query::Activity { repo } => Reply::Activity {
            height,
            last_height: load_repo(s, repo)?.last_activity,
        },
        Query::Changes { .. } | Query::Change { .. } | Query::Judgment { .. } => {
            crate::change_queries::answer(s, height, q, p()?)?
        }
        _ => crate::reads::answer(s, height, q, &bounds, paging.as_ref())?,
    })
}

/// A forge listing can be rewritten by a push, so a cursor is good for the
/// height that answered it and no other.
fn listing(page: Page, q: &Query, height: u64) -> Result<Listing, Refusal> {
    let listing = page.listing(q.scope(), height)?;
    if listing.cursor_height.is_some_and(|h| h != height) {
        return Err(stale("cursor height changed; restart the listing"));
    }
    Ok(listing)
}

fn advertise<S: Reads>(sandbox: &S, name: &str, service: Service) -> Result<Vec<u8>, Refusal> {
    let repo = load_repo(sandbox, name)?;
    let hash = repo_hash(&repo);
    let body = match service {
        Service::ReceivePack => {
            let refs = load_refs(sandbox, name, hash)?;
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

fn upload<S: Reads>(sandbox: &S, name: &str, request: &[u8]) -> Result<Vec<u8>, Refusal> {
    let repo = load_repo(sandbox, name)?;
    let bounds = load_bounds(sandbox)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(sandbox, name, hash)?;
    let store = Store::new(sandbox, hash);
    let Some(command) = parse_command(request, hash).map_err(refusal_of)? else {
        return Ok(Vec::new());
    };
    let mut response = Vec::new();
    let served = match command {
        Command::LsRefs(command) => ls_refs_response(
            &store,
            &refs,
            Some(&repo.settings.head),
            &command,
            cap(bounds.fetch_walk),
        )
        .map(|body| response = body),
        Command::Fetch(command) => fetch(
            &store,
            &refs,
            &command,
            cap(bounds.fetch_walk),
            &mut |chunk| response.extend_from_slice(chunk),
        ),
    };
    served.map_err(refusal_of)?;
    Ok(response)
}
