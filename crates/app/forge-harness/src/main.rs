// forge-harness: serves one forge.wasm over smart HTTP on localhost for a real git client. A dev tool, not product.

use std::net::SocketAddr;
use std::process::ExitCode;

use abi::HashKind;
use forge_harness::Harness;

const USAGE: &str = "forge-harness --wasm <path> --listen 127.0.0.1:PORT --repo NAME [--sha256]";
const ACTOR: &[u8] = b"harness";

struct Args {
    wasm: String,
    listen: SocketAddr,
    repo: String,
    hash: HashKind,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut wasm = None;
    let mut listen = None;
    let mut repo = None;
    let mut hash = HashKind::Sha1;
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--wasm" => wasm = args.next(),
            "--listen" => listen = args.next(),
            "--repo" => repo = args.next(),
            "--sha256" => hash = HashKind::Sha256,
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    let (Some(wasm), Some(listen), Some(repo)) = (wasm, listen, repo) else {
        return Err(USAGE.into());
    };
    let listen = listen
        .parse()
        .map_err(|error| format!("--listen {listen}: {error}"))?;
    Ok(Args {
        wasm,
        listen,
        repo,
        hash,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(sentence) => {
            eprintln!("{sentence}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    let wasm = std::fs::read(&args.wasm).map_err(|error| format!("{}: {error}", args.wasm))?;
    let harness = Harness::new(&wasm, forge_harness::default_bounds(), ACTOR.to_vec())
        .await
        .map_err(|failure| failure.to_string())?;
    harness
        .create_repo(&args.repo, args.hash)
        .await
        .map_err(|failure| failure.to_string())?;
    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .map_err(|error| format!("{}: {error}", args.listen))?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?;
    println!("http://{bound}/{}", args.repo);
    harness
        .serve(listener)
        .await
        .map_err(|error| error.to_string())
}
