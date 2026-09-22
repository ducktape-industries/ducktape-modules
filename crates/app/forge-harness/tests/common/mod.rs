#![allow(dead_code, unused_imports)]
// Real git as the client: pushes to and clones from forge.wasm on the ducktape runtime through the harness. Skips without git on PATH.

pub use std::io::Write as _;
pub use std::path::{Path, PathBuf};
pub use std::process::{Command, Output};
pub use std::sync::OnceLock;

pub use abi::HashKind;
pub use forge::{Bounds, Op, Settings};
pub use forge_harness::{Failure, Harness};
pub use tempfile::TempDir;

pub const REPO: &str = "project";

pub fn skipped() -> bool {
    let git_on_path = Command::new("which")
        .arg("git")
        .output()
        .is_ok_and(|output| output.status.success());
    if !git_on_path {
        println!("skipping: git is not on PATH");
    }
    !git_on_path
}

pub fn wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join("target"));
        let log = workspace.join("target/forge-a/wasm-test.log");
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        for attempt in 0..3 {
            let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .current_dir(&workspace)
                .env("CARGO_TARGET_DIR", &target)
                .env("RUSTC_WRAPPER", "")
                .args([
                    "build",
                    "-p",
                    "forge",
                    "--features",
                    "program",
                    "--target",
                    "wasm32-unknown-unknown",
                    "--release",
                ])
                .output()
                .expect("cargo runs");
            let stderr = String::from_utf8_lossy(&output.stderr);
            std::fs::write(&log, format!("exit={}\n{stderr}", output.status)).unwrap();
            if output.status.success() {
                break;
            }
            let transient = stderr.contains("SIGSEGV") || stderr.contains("signal: 11");
            assert!(
                transient && attempt < 2,
                "wasm build exit={}; see {}",
                output.status,
                log.display()
            );
        }
        std::fs::read(target.join("wasm32-unknown-unknown/release/forge.wasm")).expect("forge.wasm")
    })
}

pub struct Remote {
    pub rt: tokio::runtime::Runtime,
    pub harness: Harness,
    pub base: String,
    pub url: String,
}

impl Remote {
    pub fn start(bounds: Bounds, hash: HashKind) -> Remote {
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

    pub fn query(&self, query: &forge::Query) -> forge::Reply {
        abi::decode(
            &self
                .rt
                .block_on(self.harness.query(query))
                .expect("query runs"),
        )
        .expect("Borsh reply")
    }
    pub fn advance(&self) {
        self.rt
            .block_on(self.harness.advance())
            .expect("chat deliveries apply");
    }
    pub fn actor(&self, actor: &[u8]) {
        self.rt.block_on(self.harness.set_actor(actor.to_vec()));
    }
    pub fn snapshot(&self) -> forge_harness::MemoryHost {
        self.rt.block_on(self.harness.snapshot())
    }
    pub fn execute(&self, op: &Op) -> Result<Vec<u8>, Failure> {
        self.rt.block_on(self.harness.execute(op))
    }
}

pub fn git_in(dir: &Path, args: &[&str], traced: bool) -> Output {
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
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_TRACE_CURL", trace)
        .output()
        .expect("git runs")
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_in(dir, args, false);
    assert!(
        output.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git prints text")
}

pub fn git_fails(dir: &Path, args: &[&str]) -> String {
    let output = git_in(dir, args, false);
    assert!(!output.status.success(), "git {args:?} succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn commit_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("the file is written");
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", name]);
}

pub fn source_with(commits: usize) -> TempDir {
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

pub fn clone(remote: &Remote, args: &[&str]) -> TempDir {
    let parent = tempfile::tempdir().expect("a temp dir");
    let mut all = vec!["clone", "-q"];
    all.extend_from_slice(args);
    all.push(&remote.url);
    all.push("clone");
    git(parent.path(), &all);
    parent
}

pub fn clone_path(parent: &TempDir) -> PathBuf {
    parent.path().join("clone")
}

pub fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

pub fn log(dir: &Path) -> String {
    git(dir, &["log", "--format=%H"])
}

pub fn pkt(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(format!("{:04x}", line.len() + 4).as_bytes());
    out.extend_from_slice(line.as_bytes());
}

pub mod story;
