// The smart HTTP surface: four routes mapped onto the program's Advertise, Upload and Push. Used by Harness::serve.

use std::io::Read as _;

use abi::reason;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use forge::{Query, Service};

use crate::{Failure, Harness};

const RECEIVE_ADVERTISEMENT: &str = "application/x-git-receive-pack-advertisement";
const UPLOAD_ADVERTISEMENT: &str = "application/x-git-upload-pack-advertisement";
const RECEIVE_RESULT: &str = "application/x-git-receive-pack-result";
const UPLOAD_RESULT: &str = "application/x-git-upload-pack-result";
const PROTOCOL_HEADER: &str = "git-protocol";

pub fn router(harness: Harness) -> Router {
    Router::new()
        .route("/{repo}/info/refs", get(advertise))
        .route("/{repo}/git-receive-pack", post(receive_pack))
        .route("/{repo}/git-upload-pack", post(upload_pack))
        .with_state(harness)
}

async fn advertise(
    State(harness): State<Harness>,
    Path(repo): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let repo = repo_name(&repo);
    let service = query.as_deref().and_then(service_param);
    match service {
        Some(Service::ReceivePack) => {
            let asked = harness
                .query(&Query::Advertise {
                    repo,
                    service: Service::ReceivePack,
                })
                .await;
            reply(asked, RECEIVE_ADVERTISEMENT)
        }
        Some(Service::UploadPack) => {
            let speaks_v2 = headers
                .get(PROTOCOL_HEADER)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.split(':').any(|part| part == "version=2"));
            if !speaks_v2 {
                return plain(
                    StatusCode::BAD_REQUEST,
                    "this server speaks git protocol v2 only; send Git-Protocol: version=2",
                );
            }
            let asked = harness
                .query(&Query::Advertise {
                    repo,
                    service: Service::UploadPack,
                })
                .await;
            reply(asked, UPLOAD_ADVERTISEMENT)
        }
        None => plain(
            StatusCode::BAD_REQUEST,
            "info/refs takes service=git-upload-pack or service=git-receive-pack",
        ),
    }
}

async fn receive_pack(
    State(harness): State<Harness>,
    Path(repo): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match request_body(&headers, &body) {
        Ok(request) => request,
        Err(sentence) => return plain(StatusCode::BAD_REQUEST, &sentence),
    };
    let pushed = harness
        .execute(&forge::Op::Push {
            repo: repo_name(&repo),
            request,
        })
        .await;
    reply(pushed, RECEIVE_RESULT)
}

async fn upload_pack(
    State(harness): State<Harness>,
    Path(repo): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match request_body(&headers, &body) {
        Ok(request) => request,
        Err(sentence) => return plain(StatusCode::BAD_REQUEST, &sentence),
    };
    let served = harness
        .query(&Query::Upload {
            repo: repo_name(&repo),
            request,
        })
        .await;
    reply(served, UPLOAD_RESULT)
}

fn repo_name(segment: &str) -> String {
    segment.strip_suffix(".git").unwrap_or(segment).to_owned()
}

fn service_param(query: &str) -> Option<Service> {
    let value = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "service")
        .map(|(_, value)| value)?;
    match value {
        "git-receive-pack" => Some(Service::ReceivePack),
        "git-upload-pack" => Some(Service::UploadPack),
        _ => None,
    }
}

fn request_body(headers: &HeaderMap, body: &[u8]) -> Result<Vec<u8>, String> {
    let gzipped = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("gzip"));
    if !gzipped {
        return Ok(body.to_vec());
    }
    let mut inflated = Vec::new();
    flate2::read::MultiGzDecoder::new(body)
        .read_to_end(&mut inflated)
        .map_err(|error| format!("the request body is not gzip: {error}"))?;
    Ok(inflated)
}

fn reply(answer: Result<Vec<u8>, Failure>, content_type: &str) -> Response {
    match answer {
        Ok(bytes) => with_body(StatusCode::OK, content_type, bytes),
        Err(Failure::Refused(refusal)) => {
            let status = match refusal.reason.as_str() {
                reason::NOT_FOUND => StatusCode::NOT_FOUND,
                reason::UNAUTHORIZED => StatusCode::FORBIDDEN,
                _ => StatusCode::BAD_REQUEST,
            };
            plain(status, &refusal.sentence)
        }
        Err(Failure::Faulted(fault)) => {
            plain(StatusCode::INTERNAL_SERVER_ERROR, &fault.to_string())
        }
    }
}

fn plain(status: StatusCode, sentence: &str) -> Response {
    with_body(
        status,
        "text/plain; charset=utf-8",
        format!("{sentence}\n").into_bytes(),
    )
}

fn with_body(status: StatusCode, content_type: &str, bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("a fixed set of valid headers")
}
