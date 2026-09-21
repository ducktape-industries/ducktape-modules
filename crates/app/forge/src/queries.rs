// The query path: listings for a UI, and the two git smart-HTTP bodies a client reads, streamed through the sandbox's response.

use abi::{Refusal, Scan};
use gitcore::wire::receive::advertise_refs;
use gitcore::wire::smart_http_service_header;
use gitcore::wire::upload::{
    Command, capability_advertisement, fetch, ls_refs_response, parse_command,
};

use crate::contract::{Query, RefInfo, Reply, RepoInfo, Service};
use crate::ops::{cap, refusal_of};
use crate::repo::{load_bounds, load_refs, load_repo, repo_hash, repo_name, repos_prefix};
use crate::sandbox::Sandbox;
use crate::store::Store;

const AGENT: &[u8] = b"ducktape-forge";

pub fn query<S: Sandbox>(sandbox: &S, request: &[u8]) -> Result<(), Refusal> {
    match abi::decode(request)? {
        Query::Repos => repos(sandbox),
        Query::Refs { repo } => refs(sandbox, &repo),
        Query::Advertise { repo, service } => advertise(sandbox, &repo, service),
        Query::Upload { repo, request } => upload(sandbox, &repo, &request),
    }
}

fn repos<S: Sandbox>(sandbox: &S) -> Result<(), Refusal> {
    let listed = sandbox
        .scan(Scan::prefix(repos_prefix()))
        .into_iter()
        .filter_map(|entry| {
            let name = repo_name(&entry.key)?;
            let repo = abi::decode(&entry.value).ok()?;
            Some(RepoInfo { name, repo })
        })
        .collect();
    sandbox.respond(abi::encode(&Reply::Repos(listed)));
    Ok(())
}

fn refs<S: Sandbox>(sandbox: &S, name: &str) -> Result<(), Refusal> {
    let repo = load_repo(sandbox, name)?;
    let listed = load_refs(sandbox, name, repo_hash(&repo))?
        .into_iter()
        .map(|(reference, target)| RefInfo {
            name: reference,
            target: target.to_hex(),
        })
        .collect();
    sandbox.respond(abi::encode(&Reply::Refs(listed)));
    Ok(())
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
