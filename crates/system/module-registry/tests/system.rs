use std::collections::BTreeMap;

use abi::reason;
use abi::{BlobId, HostOp, Message, Origin, Outcome, Scheme};
use borsh::{BorshDeserialize, BorshSerialize};
use commonware_cryptography::{Signer as _, ed25519};
use commonware_runtime::{Runner as _, deterministic};
use fixture_probe::Step;
use host::{
    Applied, Block, BlockId, Founding, FoundingView, Genesis, Host, Layer, Limits, Receipt,
    Submission,
};
use identity::AccountNumber;
use keyscheme::testkit;
use module_registry::AUTHORITY;
use store::PageRequest;

/// Where `make wasm-programs` left the boot set: the bytes are a build
/// output, never committed.
fn release_dir() -> std::path::PathBuf {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"))
        .join("wasm32-unknown-unknown/release")
}

/// Builds the boot set once per test binary: `make wasm-programs` at the
/// repo root, which is cargo's own freshness (a missing or stale artifact
/// rebuilds, a fresh one costs a fingerprint check). The suite never skips;
/// a failed build fails every test that reads the bytes.
fn build() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let status = std::process::Command::new("make")
            .arg("-C")
            .arg(&root)
            .arg("wasm-programs")
            .status()
            .expect("make is on PATH");
        assert!(status.success(), "`make wasm-programs` failed: {status}");
    });
}

fn program(name: &str) -> Vec<u8> {
    build();
    let path = release_dir().join(format!("{name}.wasm"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The kernel's probe fixture, copied from ducktape by `make probe-fixture`
/// (the probe is a dev-dependency only, which cargo cannot build for wasm32
/// from here).
const PROBE: &[u8] = include_bytes!("fixture_probe.wasm");
const NETWORK: &[u8] = b"net";
const EPOCH_LENGTH: u64 = 4;
const TIME: u64 = 1_700_000_000_000;
const PROBE_SIGNER: u64 = 9;

type Ctx = deterministic::Context;

fn key(seed: u64) -> ed25519::PrivateKey {
    ed25519::PrivateKey::from_seed(seed)
}

fn public(seed: u64) -> Vec<u8> {
    key(seed).public_key().as_ref().to_vec()
}

fn member(seed: u64) -> valset::Member {
    valset::Member {
        key: public(seed),
        address: format!("v{seed}:1"),
    }
}

fn membership(seed: u64, role: valset::Role) -> valset::Membership {
    valset::Membership {
        key: public(seed),
        address: format!("v{seed}:1"),
        role,
    }
}

fn founding(program: &str, code: &[u8]) -> Founding {
    Founding {
        program: program.to_owned(),
        code: code.to_vec(),
        params: Vec::new(),
    }
}

fn probe(program: &str) -> Founding {
    Founding {
        program: program.to_owned(),
        code: PROBE.to_vec(),
        params: abi::encode(&Vec::<Step>::new()),
    }
}

fn block_id(height: u64) -> BlockId {
    let mut id = [0u8; 32];
    id[..8].copy_from_slice(&height.to_be_bytes());
    id
}

struct Net {
    host: Host<Ctx>,
    height: u64,
    sequences: BTreeMap<Vec<u8>, u64>,
}

impl Net {
    async fn found(context: Ctx, dir: &std::path::Path) -> Net {
        let genesis = Genesis {
            network: NETWORK.to_vec(),
            module_registry: program("module_registry"),
            valset: program("valset"),
            validators: vec![member(1), member(2)],
            programs: vec![
                founding(identity::MODULE, &program("identity")),
                probe(AUTHORITY),
                probe("probe"),
            ],
            views: vec![FoundingView {
                name: "lens".into(),
                view: b"a view".to_vec(),
            }],
            limits: Limits::default(),
            epoch_length: EPOCH_LENGTH,
            time: TIME,
        };
        let (host, applied) = Host::found(context, "net", dir, block_id(0), genesis)
            .await
            .unwrap();
        for receipt in &applied.admissions {
            assert!(
                matches!(receipt.outcome, Outcome::Applied { .. }),
                "{} did not admit: {:?}",
                receipt.program,
                receipt.outcome
            );
        }
        Net {
            host,
            height: 0,
            sequences: BTreeMap::new(),
        }
    }

    fn time(&self) -> u64 {
        TIME + self.height * 1000
    }

    async fn block(&mut self, submissions: Vec<Submission>) -> Applied {
        self.height += 1;
        self.host
            .apply(Block {
                height: self.height,
                id: block_id(self.height),
                time: self.time(),
                submissions,
            })
            .await
            .unwrap()
    }

    async fn tick(&mut self) -> Applied {
        self.block(Vec::new()).await
    }

    async fn ticks(&mut self, blocks: u64) {
        for _ in 0..blocks {
            self.tick().await;
        }
    }

    fn submission(&self, signer: &[u8], target: &str, payload: Vec<u8>) -> Submission {
        Submission {
            signer: signer.to_vec(),
            seq: self.sequences.get(signer).copied().unwrap_or_default(),
            target: target.to_owned(),
            payload,
        }
    }

    fn consumed(&mut self, signer: &[u8], receipt: &Receipt) {
        if let Outcome::Applied { .. } = receipt.outcome {
            *self.sequences.entry(signer.to_vec()).or_default() += 1;
        }
    }

    async fn submit<T: BorshSerialize>(&mut self, signer: &[u8], target: &str, op: &T) -> Receipt {
        let submission = self.submission(signer, target, abi::encode(op));
        let applied = self.block(vec![submission]).await;
        let receipt = applied.submissions.into_iter().next().unwrap();
        self.consumed(signer, &receipt);
        receipt
    }

    async fn apply<T: BorshSerialize>(&mut self, signer: &[u8], target: &str, op: &T) -> Vec<u8> {
        let receipt = self.submit(signer, target, op).await;
        match receipt.outcome {
            Outcome::Applied { output } => output,
            Outcome::Rejected(refusal) => panic!("{target} rejected the op: {refusal}"),
        }
    }

    async fn refuse<T: BorshSerialize>(&mut self, signer: &[u8], target: &str, op: &T) -> String {
        let receipt = self.submit(signer, target, op).await;
        match receipt.outcome {
            Outcome::Applied { .. } => panic!("{target} applied the op"),
            Outcome::Rejected(refusal) => refusal.reason,
        }
    }

    async fn sent_by<T: BorshSerialize>(&mut self, program: &str, target: &str, op: &T) -> Receipt {
        let script = vec![Step::Op(HostOp::Emit(Message {
            target: target.to_owned(),
            payload: abi::encode(op),
            reply: false,
        }))];
        let signer = public(PROBE_SIGNER);
        let submission = self.submission(&signer, program, abi::encode(&script));
        let applied = self.block(vec![submission]).await;
        self.consumed(&signer, &applied.submissions[0]);
        match &applied.submissions[0].outcome {
            Outcome::Applied { .. } => {}
            Outcome::Rejected(refusal) => panic!("{program} rejected the script: {refusal}"),
        }
        let applied = self.tick().await;
        applied
            .deliveries
            .into_iter()
            .map(|delivered| delivered.receipt)
            .find(|receipt| receipt.program == target)
            .unwrap()
    }

    async fn as_authority<T: BorshSerialize>(&mut self, target: &str, op: &T) -> Receipt {
        self.sent_by(AUTHORITY, target, op).await
    }

    /// A query the module refuses.
    async fn refused<Q: BorshSerialize>(&self, program: &str, query: &Q) -> abi::Refusal {
        self.host
            .query(
                Layer::Confirmed,
                self.time(),
                Origin::External(public(1)),
                program,
                abi::encode(query),
            )
            .await
            .unwrap()
            .unwrap_err()
    }

    async fn ask<Q: BorshSerialize, R: BorshDeserialize>(&self, program: &str, query: &Q) -> R {
        let answer = self
            .host
            .query(
                Layer::Confirmed,
                self.time(),
                Origin::External(public(1)),
                program,
                abi::encode(query),
            )
            .await
            .unwrap()
            .unwrap();
        abi::decode(&answer).unwrap()
    }

    async fn memberships(&self) -> Vec<valset::Membership> {
        match self
            .ask(
                valset::MODULE,
                &valset::Query::Memberships {
                    page: PageRequest::default(),
                },
            )
            .await
        {
            valset::Reply::Memberships(memberships) => memberships.items,
            other => panic!("{other:?}"),
        }
    }

    async fn validators(&self) -> Vec<Vec<u8>> {
        match self.ask(valset::MODULE, &valset::Query::Validators).await {
            valset::Reply::Validators(validators) => validators,
            other => panic!("{other:?}"),
        }
    }
}

fn output_of(receipt: &Receipt) -> &[u8] {
    match &receipt.outcome {
        Outcome::Applied { output } => output,
        Outcome::Rejected(refusal) => panic!("{} rejected: {refusal}", receipt.program),
    }
}

fn refusal_of(receipt: &Receipt) -> &str {
    match &receipt.outcome {
        Outcome::Applied { .. } => panic!("{} applied the op", receipt.program),
        Outcome::Rejected(refusal) => &refusal.reason,
    }
}

#[path = "system/founding.rs"]
mod founding_tests;
#[path = "system/identity.rs"]
mod identity_tests;
#[path = "system/module_registry.rs"]
mod module_registry_tests;
#[path = "system/valset.rs"]
mod valset_tests;
