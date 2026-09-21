// Real git as the client: pushes to and clones from forge.wasm on the ducktape runtime through the harness. Skips without git on PATH.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use abi::HashKind;
use forge::{Bounds, Op, Settings};
use forge_harness::{Failure, Harness};
use tempfile::TempDir;

const REPO: &str = "project";

fn skipped() -> bool {
    let git_on_path = Command::new("which")
        .arg("git")
        .output()
        .is_ok_and(|output| output.status.success());
    if !git_on_path {
        println!("skipping: git is not on PATH");
    }
    !git_on_path
}

fn wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join("target"));
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let built = Command::new(cargo)
            .current_dir(&workspace)
            .env("CARGO_TARGET_DIR", &target)
            .args([
                "build",
                "-p",
                "forge",
                "--target",
                "wasm32-unknown-unknown",
                "--release",
            ])
            .status()
            .expect("cargo runs");
        assert!(built.success(), "forge.wasm builds");
        std::fs::read(target.join("wasm32-unknown-unknown/release/forge.wasm")).expect("forge.wasm")
    })
}

struct Remote {
    rt: tokio::runtime::Runtime,
    harness: Harness,
    base: String,
    url: String,
}

impl Remote {
    fn start(bounds: Bounds, hash: HashKind) -> Remote {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("a tokio runtime");
        let harness = rt
            .block_on(Harness::new(wasm(), bounds, b"tester".to_vec()))
            .expect("the program founds");
        rt.block_on(harness.create_repo(REPO, hash))
            .expect("the repository is created");
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("a free port");
        let address = listener.local_addr().expect("a bound address");
        rt.spawn(harness.serve(listener));
        Remote {
            rt,
            harness,
            base: format!("http://{address}"),
            url: format!("http://{address}/{REPO}"),
        }
    }

    fn execute(&self, op: &Op) -> Result<Vec<u8>, Failure> {
        self.rt.block_on(self.harness.execute(op))
    }
}

fn git_in(dir: &Path, args: &[&str], traced: bool) -> Output {
    let trace = if traced { "1" } else { "0" };
    Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "protocol.version=2",
            "-c",
            "user.name=Ada",
            "-c",
            "user.email=ada@example.com",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_TRACE_CURL", trace)
        .output()
        .expect("git runs")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_in(dir, args, false);
    assert!(
        output.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git prints text")
}

fn git_fails(dir: &Path, args: &[&str]) -> String {
    let output = git_in(dir, args, false);
    assert!(!output.status.success(), "git {args:?} succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn commit_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("the file is written");
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", name]);
}

fn source_with(commits: usize) -> TempDir {
    let source = tempfile::tempdir().expect("a temp dir");
    git(source.path(), &["init", "-q", "-b", "main"]);
    for index in 0..commits {
        commit_file(
            source.path(),
            &format!("file-{index}"),
            &format!("{index}\n"),
        );
    }
    source
}

fn clone(remote: &Remote, args: &[&str]) -> TempDir {
    let parent = tempfile::tempdir().expect("a temp dir");
    let mut all = vec!["clone", "-q"];
    all.extend_from_slice(args);
    all.push(&remote.url);
    all.push("clone");
    git(parent.path(), &all);
    parent
}

fn clone_path(parent: &TempDir) -> PathBuf {
    parent.path().join("clone")
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn log(dir: &Path) -> String {
    git(dir, &["log", "--format=%H"])
}

#[test]
fn push_then_clone() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(2);
    git(source.path(), &["push", "-q", &remote.url, "main"]);

    let cloned = clone(&remote, &[]);
    let cloned = clone_path(&cloned);
    assert_eq!(log(&cloned), log(source.path()));
    for name in ["file-0", "file-1"] {
        let expected = std::fs::read(source.path().join(name)).expect("source file");
        let actual = std::fs::read(cloned.join(name)).expect("cloned file");
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn incremental_fetch() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(2);
    git(source.path(), &["push", "-q", &remote.url, "main"]);
    let cloned = clone(&remote, &[]);
    let cloned = clone_path(&cloned);

    commit_file(source.path(), "file-2", "2\n");
    git(source.path(), &["push", "-q", &remote.url, "main"]);
    git(&cloned, &["pull", "-q", "--ff-only"]);
    assert_eq!(head(&cloned), head(source.path()));
    assert_eq!(log(&cloned), log(source.path()));

    git(source.path(), &["branch", "side", "main~1"]);
    git(source.path(), &["tag", "-a", "v1", "-m", "v1"]);
    git(source.path(), &["push", "-q", &remote.url, "side", "v1"]);
    let listed = git(source.path(), &["ls-remote", &remote.url]);
    let side = git(source.path(), &["rev-parse", "side"]);
    let tag = git(source.path(), &["rev-parse", "v1"]);
    assert!(listed.contains(&format!("{}\trefs/heads/main", head(source.path()))));
    assert!(listed.contains(&format!("{}\trefs/heads/side", side.trim())));
    assert!(listed.contains(&format!("{}\trefs/tags/v1", tag.trim())));
    assert!(listed.contains(&format!("{}\trefs/tags/v1^{{}}", head(source.path()))));
}

#[test]
fn non_fast_forward_is_rejected() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(2);
    git(source.path(), &["push", "-q", &remote.url, "main"]);
    let pushed = head(source.path());

    git(source.path(), &["reset", "-q", "--hard", "HEAD~1"]);
    commit_file(source.path(), "rewritten", "x\n");
    let rewound = head(source.path());

    let refused = git_fails(source.path(), &["push", &remote.url, "main"]);
    assert!(refused.contains("non-fast-forward"), "{refused}");
    let forced = git_fails(source.path(), &["push", "--force", &remote.url, "main"]);
    assert!(forced.contains("[remote rejected]"), "{forced}");
    assert!(forced.contains("non-fast-forward"), "{forced}");
    let listed = git(
        source.path(),
        &["ls-remote", &remote.url, "refs/heads/main"],
    );
    assert!(listed.starts_with(&pushed), "{listed}");

    remote
        .execute(&Op::Configure {
            repo: REPO.into(),
            settings: Settings {
                allow_force: true,
                ..Settings::default()
            },
        })
        .expect("the owner configures");
    git(
        source.path(),
        &["push", "-q", "--force", &remote.url, "main"],
    );
    let listed = git(
        source.path(),
        &["ls-remote", &remote.url, "refs/heads/main"],
    );
    assert!(listed.starts_with(&rewound), "{listed}");
}

#[test]
fn import_in_steps() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(30);
    for step in ["main~20", "main~10", "main"] {
        let refspec = format!("{step}:refs/heads/main");
        git(source.path(), &["push", "-q", &remote.url, &refspec]);
    }
    let cloned = clone(&remote, &[]);
    assert_eq!(log(&clone_path(&cloned)), log(source.path()));

    let narrow = Remote::start(
        Bounds {
            push_walk: 5,
            ..forge_harness::default_bounds()
        },
        HashKind::Sha1,
    );
    git(
        source.path(),
        &["push", "-q", &narrow.url, "main~20:refs/heads/main"],
    );
    let refused = git_fails(
        source.path(),
        &["push", &narrow.url, "main:refs/heads/main"],
    );
    assert!(refused.contains("push in smaller steps"), "{refused}");
}

#[test]
fn sha256_repository() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha256);
    let source = tempfile::tempdir().expect("a temp dir");
    git(
        source.path(),
        &["init", "-q", "-b", "main", "--object-format=sha256"],
    );
    commit_file(source.path(), "file-0", "0\n");
    git(source.path(), &["push", "-q", &remote.url, "main"]);

    let cloned = clone(&remote, &[]);
    let cloned = clone_path(&cloned);
    let cloned_head = head(&cloned);
    assert_eq!(cloned_head.len(), 64);
    assert_eq!(cloned_head, head(source.path()));
    assert_eq!(
        git(&cloned, &["rev-parse", "--show-object-format"]).trim(),
        "sha256"
    );
}

#[test]
fn gzip_request_bodies() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(1);
    for index in 0..30 {
        let branch = format!("branch-{index}");
        git(source.path(), &["checkout", "-q", "-b", &branch, "main"]);
        commit_file(source.path(), &branch, "x\n");
    }
    git(source.path(), &["checkout", "-q", "main"]);
    git(source.path(), &["push", "-q", &remote.url, "--all"]);

    let cloned = clone(&remote, &["--single-branch", "-b", "main"]);
    let cloned = clone_path(&cloned);
    let fetched = git_in(
        &cloned,
        &[
            "fetch",
            "-q",
            "origin",
            "refs/heads/*:refs/remotes/origin/*",
        ],
        true,
    );
    let trace = String::from_utf8_lossy(&fetched.stderr);
    assert!(fetched.status.success(), "{trace}");
    assert!(trace.contains("Content-Encoding: gzip"), "{trace}");
    let branches = git(&cloned, &["branch", "-r"]);
    assert_eq!(branches.lines().count(), 31, "{branches}");

    let mut request = Vec::new();
    for line in ["command=ls-refs\n", "object-format=sha1\n"] {
        pkt(&mut request, line);
    }
    request.extend_from_slice(b"0001");
    pkt(&mut request, "ref-prefix refs/heads/\n");
    request.extend_from_slice(b"0000");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&request).expect("gzip");
    let gzipped = encoder.finish().expect("gzip");
    let mut response = ureq::post(format!("{}/git-upload-pack", remote.url))
        .header("Content-Type", "application/x-git-upload-pack-request")
        .header("Content-Encoding", "gzip")
        .header("Git-Protocol", "version=2")
        .send(&gzipped[..])
        .expect("the harness answers");
    assert_eq!(response.status().as_u16(), 200);
    let body = response.body_mut().read_to_vec().expect("a body");
    let listing = String::from_utf8_lossy(&body);
    assert!(listing.contains("refs/heads/main\n"), "{listing}");
    assert!(listing.contains("refs/heads/branch-29\n"), "{listing}");
}

#[test]
fn upload_advertisement_requires_protocol_v2() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let response = ureq::get(format!("{}/info/refs?service=git-upload-pack", remote.url))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .expect("the harness answers");
    assert_eq!(response.status().as_u16(), 400);
    let mut with_header = ureq::get(format!(
        "{}.git/info/refs?service=git-upload-pack",
        remote.url
    ))
    .header("Git-Protocol", "version=2")
    .call()
    .expect("the harness answers");
    assert_eq!(with_header.status().as_u16(), 200);
    assert_eq!(
        with_header
            .headers()
            .get("content-type")
            .map(|v| v.as_bytes()),
        Some(b"application/x-git-upload-pack-advertisement".as_slice())
    );
    let body = with_header.body_mut().read_to_vec().expect("a body");
    assert!(body.starts_with(b"001e# service=git-upload-pack\n0000000eversion 2\n"));
    let missing = ureq::get(format!(
        "{}/nope/info/refs?service=git-receive-pack",
        remote.base
    ))
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("the harness answers");
    assert_eq!(missing.status().as_u16(), 404);
}

fn pkt(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(format!("{:04x}", line.len() + 4).as_bytes());
    out.extend_from_slice(line.as_bytes());
}
