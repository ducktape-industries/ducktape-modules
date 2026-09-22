//! UI queries produce one height-bearing Borsh reply. Git protocol queries stream Git bytes.
use crate::contract::*;
use crate::git::wire::receive::advertise_refs;
use crate::git::wire::smart_http_service_header;
use crate::git::wire::upload::{
    Command, capability_advertisement, fetch, ls_refs_response, parse_command,
};
use crate::ops::{cap, refusal_of};
use crate::paging::Paging;
use crate::repo::{load_bounds, load_refs, load_repo, refs_prefix, repo_hash, writers_prefix};
use crate::sandbox::Sandbox;
use crate::store::Store;
use abi::{Env, Refusal};
const AGENT: &[u8] = b"ducktape-forge";

pub fn query<S: Sandbox>(sandbox: &S, env: &Env, request: &[u8]) -> Result<(), Refusal> {
    let query: Query = abi::decode(request)?;
    match &query {
        Query::Advertise { repo, service } => return advertise(sandbox, repo, *service),
        Query::Upload { repo, request } => return upload(sandbox, repo, request),
        _ => {}
    }
    let height = env.height;
    let reply = answer(sandbox, height, &query).unwrap_or_else(|r| Reply::Refused {
        height,
        reason: r.reason,
        sentence: r.sentence,
    });
    sandbox.respond(abi::encode(&reply));
    Ok(())
}
fn answer<S: Sandbox>(s: &S, height: u64, q: &Query) -> Result<Reply, Refusal> {
    let bounds = load_bounds(s)?;
    let paging = Paging::for_query(height, &bounds, q)?;
    let p = || paging.as_ref().expect("this query has pagination");
    Ok(match q {
        Query::Repos { .. } => {
            let entries = p().entries(s, b"a/")?;
            let items = entries
                .items
                .into_iter()
                .map(|e| {
                    let name = String::from_utf8(e.value)
                        .map_err(|_| crate::refuse::storage("bad repo activity index"))?;
                    Ok(RepoInfo {
                        repo: load_repo(s, &name)?,
                        name,
                    })
                })
                .collect::<Result<_, Refusal>>()?;
            Reply::Repos {
                height,
                page: Page {
                    items,
                    next: entries.next,
                },
            }
        }
        Query::Repo { repo, .. } => {
            let record = load_repo(s, repo)?;
            let prefix = writers_prefix(repo);
            let entries = p().entries(s, &prefix)?;
            let items = entries
                .items
                .into_iter()
                .map(|e| e.key[prefix.len()..].to_vec())
                .collect();
            Reply::Repo {
                height,
                repo: RepoInfo {
                    name: repo.clone(),
                    repo: record,
                },
                bounds,
                writers: Page {
                    items,
                    next: entries.next,
                },
            }
        }
        Query::Refs { repo, .. } => {
            let record = load_repo(s, repo)?;
            let prefix = refs_prefix(repo);
            let entries = p().entries(s, &prefix)?;
            let items = entries
                .items
                .into_iter()
                .map(|e| {
                    let oid = crate::git::Oid::from_bytes(repo_hash(&record), &e.value)
                        .map_err(|e| crate::refuse::storage(e.to_string()))?;
                    Ok(RefInfo {
                        name: e.key[prefix.len()..].to_vec(),
                        target: oid.to_hex(),
                    })
                })
                .collect::<Result<_, Refusal>>()?;
            Reply::Refs {
                height,
                page: Page {
                    items,
                    next: entries.next,
                },
            }
        }
        Query::Activity { repo } => Reply::Activity {
            height,
            last_height: load_repo(s, repo)?.last_activity,
        },
        Query::Changes { .. } | Query::Change { .. } | Query::Judgment { .. } => {
            crate::change_queries::answer(s, height, q, p())?
        }
        _ => crate::reads::answer(s, height, q, &bounds, paging.as_ref())?,
    })
}

fn advertise<S: Sandbox>(sandbox: &S, name: &str, service: Service) -> Result<(), Refusal> {
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
    sandbox.respond(body);
    Ok(())
}

fn upload<S: Sandbox>(sandbox: &S, name: &str, request: &[u8]) -> Result<(), Refusal> {
    let repo = load_repo(sandbox, name)?;
    let bounds = load_bounds(sandbox)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(sandbox, name, hash)?;
    let store = Store::new(sandbox, hash);
    let Some(command) = parse_command(request, hash).map_err(|error| refusal_of(&store, error))?
    else {
        return Ok(());
    };
    let served = match command {
        Command::LsRefs(command) => ls_refs_response(
            &store,
            &refs,
            Some(&repo.settings.head),
            &command,
            cap(bounds.fetch_walk),
        )
        .map(|body| sandbox.respond(body)),
        Command::Fetch(command) => fetch(
            &store,
            &refs,
            &command,
            cap(bounds.fetch_walk),
            &mut |chunk| sandbox.respond(chunk.to_vec()),
        ),
    };
    served.map_err(|error| refusal_of(&store, error))
}
