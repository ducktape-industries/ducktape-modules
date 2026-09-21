use std::collections::BTreeMap;

use abi::reason;
use abi::{BlobId, HostOp, Message, Origin, Outcome, Scheme};
use borsh::{BorshDeserialize, BorshSerialize};
use commonware_cryptography::{Signer as _, ed25519};
use commonware_runtime::{Runner as _, deterministic};
use fixture_probe::Step;
use host::{Applied, Block, BlockId, Founding, Genesis, Host, Layer, Limits, Receipt, Submission};
use keyscheme::testkit;
use modules::{AUTHORITY, AccountNumber, Page, admission, identity, module_registry, valset};

macro_rules! program {
    ($name:literal) => {
        include_bytes!(concat!("../system/wasm/", $name, ".wasm"))
    };
}

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

fn membership(seed: u64, standing: valset::Standing) -> valset::Membership {
    valset::Membership {
        key: public(seed),
        address: format!("v{seed}:1"),
        standing,
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
        let admission = founding(admission::PROGRAM, program!("admission"));
        Net::found_with(context, dir, admission).await
    }

    async fn found_with_puppet_admission(context: Ctx, dir: &std::path::Path) -> Net {
        Net::found_with(context, dir, probe(admission::PROGRAM)).await
    }

    async fn found_with(context: Ctx, dir: &std::path::Path, admission: Founding) -> Net {
        let genesis = Genesis {
            network: NETWORK.to_vec(),
            module_registry: program!("module_registry").to_vec(),
            valset: program!("valset").to_vec(),
            validators: vec![member(1), member(2)],
            programs: vec![
                founding(identity::PROGRAM, program!("identity")),
                admission,
                probe(AUTHORITY),
                probe("probe"),
            ],
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

    async fn as_admission(&mut self, op: &valset::Op) -> Receipt {
        self.sent_by(admission::PROGRAM, valset::PROGRAM, op).await
    }

    async fn enroll(&mut self, seed: u64, address: &str) -> Receipt {
        let signer = public(seed);
        let op = admission::Op::Enroll {
            address: address.to_owned(),
        };
        let submission = self.submission(&signer, admission::PROGRAM, abi::encode(&op));
        let applied = self.block(vec![submission]).await;
        self.consumed(&signer, &applied.submissions[0]);
        match &applied.submissions[0].outcome {
            Outcome::Applied { .. } => {}
            Outcome::Rejected(refusal) => panic!("admission rejected the enroll: {refusal}"),
        }
        let applied = self.tick().await;
        applied
            .deliveries
            .into_iter()
            .map(|delivered| delivered.receipt)
            .find(|receipt| receipt.program == valset::PROGRAM)
            .unwrap()
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
        match self.ask(valset::PROGRAM, &valset::Query::Memberships).await {
            valset::Reply::Memberships(memberships) => memberships,
            other => panic!("{other:?}"),
        }
    }

    async fn validators(&self) -> Vec<Vec<u8>> {
        match self.ask(valset::PROGRAM, &valset::Query::Validators).await {
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

#[test]
fn founding_seats_the_validators_and_every_program_answers() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let net = Net::found(context, dir.path()).await;
        let programs = net.host.programs().unwrap();
        for program in [
            module_registry::PROGRAM,
            valset::PROGRAM,
            admission::PROGRAM,
            identity::PROGRAM,
            AUTHORITY,
        ] {
            assert!(programs.contains_key(program), "{program} is not rostered");
        }
        let memberships = net.memberships().await;
        assert_eq!(memberships.len(), 2);
        assert!(
            memberships
                .iter()
                .all(|membership| membership.standing == valset::Standing::Validator)
        );
        let seated = net.host.epoch_seating(0).unwrap().unwrap();
        assert_eq!(seated.validators.len(), 2);
        assert_eq!(seated.members.len(), 2);
        assert!(seated.members.contains(&member(1)));
        assert!(seated.members.contains(&member(2)));
        let identity::Reply::Accounts(accounts) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::List { page: Page::all() },
            )
            .await
        else {
            panic!()
        };
        assert!(accounts.is_empty());
    });
}

#[test]
fn admission_seats_members_and_the_next_epoch_reads_them() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with_puppet_admission(context, dir.path()).await;
        let stranger = net
            .refuse(
                &public(1),
                valset::PROGRAM,
                &valset::Op::Set(membership(3, valset::Standing::Resident)),
            )
            .await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        let other_program = net
            .as_authority(
                valset::PROGRAM,
                &valset::Op::Set(membership(3, valset::Standing::Resident)),
            )
            .await;
        assert_eq!(refusal_of(&other_program), reason::UNAUTHORIZED);
        let admitted = net
            .as_admission(&valset::Op::Set(membership(3, valset::Standing::Resident)))
            .await;
        output_of(&admitted);
        assert_eq!(net.memberships().await.len(), 3);
        assert_eq!(net.validators().await.len(), 2);
        let valset::Reply::Members(members) =
            net.ask(valset::PROGRAM, &valset::Query::Members).await
        else {
            panic!()
        };
        assert_eq!(members.len(), 3);
        let promoted = net
            .as_admission(&valset::Op::Set(membership(3, valset::Standing::Validator)))
            .await;
        output_of(&promoted);
        assert_eq!(net.validators().await.len(), 3);
        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_seating(epoch).unwrap().is_none() {
            net.tick().await;
        }
        let seating = net.host.epoch_seating(epoch).unwrap().unwrap();
        assert_eq!(seating.validators.len(), 3);
        assert_eq!(seating.members.len(), 3);
        for seed in [1, 2] {
            let removed = net
                .as_admission(&valset::Op::Remove { key: public(seed) })
                .await;
            output_of(&removed);
        }
        assert_eq!(net.validators().await, vec![public(3)]);
        let last = net
            .as_admission(&valset::Op::Remove { key: public(3) })
            .await;
        assert_eq!(refusal_of(&last), reason::WRONG_STATE);
        let demoted = net
            .as_admission(&valset::Op::Set(membership(3, valset::Standing::Resident)))
            .await;
        assert_eq!(refusal_of(&demoted), reason::WRONG_STATE);
        let valset::Reply::Membership(Some(standing)) = net
            .ask(
                valset::PROGRAM,
                &valset::Query::Membership { key: public(3) },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(standing.standing, valset::Standing::Validator);
    });
}

#[test]
fn a_stranger_enrolls_as_a_resident_and_a_validator_keeps_its_seat() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let from_a_program = net
            .sent_by(
                "probe",
                admission::PROGRAM,
                &admission::Op::Enroll {
                    address: "203.0.113.9:9000".to_owned(),
                },
            )
            .await;
        assert_eq!(refusal_of(&from_a_program), reason::UNAUTHORIZED);
        let enrolled = net.enroll(3, "203.0.113.9:9000").await;
        output_of(&enrolled);
        let valset::Reply::Membership(Some(resident)) = net
            .ask(
                valset::PROGRAM,
                &valset::Query::Membership { key: public(3) },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(resident.standing, valset::Standing::Resident);
        assert_eq!(resident.address, "203.0.113.9:9000");
        assert_eq!(net.validators().await.len(), 2);
        let moved = net.enroll(1, "203.0.113.1:9001").await;
        output_of(&moved);
        let valset::Reply::Membership(Some(validator)) = net
            .ask(
                valset::PROGRAM,
                &valset::Query::Membership { key: public(1) },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(validator.standing, valset::Standing::Validator);
        assert_eq!(validator.address, "203.0.113.1:9001");
        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_seating(epoch).unwrap().is_none() {
            net.tick().await;
        }
        let seating = net.host.epoch_seating(epoch).unwrap().unwrap();
        assert_eq!(seating.validators.len(), 2);
        assert!(!seating.validators.contains(&public(3)));
        assert_eq!(seating.members.len(), 3);
        assert!(seating.members.iter().any(|member| member.key == public(3)));
    });
}

#[test]
fn a_published_program_is_scheduled_by_the_authority_and_seated_at_its_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let output = net
            .apply(
                &public(7),
                module_registry::PROGRAM,
                &module_registry::Op::Publish {
                    body: program!("identity").to_vec(),
                },
            )
            .await;
        let code: BlobId = abi::decode(&output).unwrap();
        let entry = module_registry::Entry {
            program: "identity2".into(),
            code,
            params: Vec::new(),
        };
        let stranger = net
            .refuse(
                &public(7),
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: net.height + 3,
                    change: module_registry::Change::Set(entry.clone()),
                }),
            )
            .await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        let unpublished = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: net.height + 3,
                    change: module_registry::Change::Set(module_registry::Entry {
                        program: "ghost".into(),
                        code: BlobId::Sha256([9; 32]),
                        params: Vec::new(),
                    }),
                }),
            )
            .await;
        assert_eq!(refusal_of(&unpublished), reason::NOT_FOUND);
        let past = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: net.height,
                    change: module_registry::Change::Set(entry.clone()),
                }),
            )
            .await;
        assert_eq!(refusal_of(&past), reason::INVALID_INPUT);
        let lands_at = net.height + 6;
        let scheduled = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: lands_at,
                    change: module_registry::Change::Set(entry.clone()),
                }),
            )
            .await;
        output_of(&scheduled);
        let taken = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: lands_at,
                    change: module_registry::Change::Remove("identity2".into()),
                }),
            )
            .await;
        assert_eq!(refusal_of(&taken), reason::ALREADY_EXISTS);
        let module_registry::Reply::Scheduled(pending) = net
            .ask(module_registry::PROGRAM, &module_registry::Query::Scheduled)
            .await
        else {
            panic!()
        };
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].height, lands_at);
        assert_eq!(
            pending[0].change,
            module_registry::Change::Set(entry.clone())
        );
        let module_registry::Reply::Programs(later) = net
            .ask(
                module_registry::PROGRAM,
                &module_registry::Query::At(lands_at),
            )
            .await
        else {
            panic!()
        };
        assert!(later.iter().any(|entry| entry.program == "identity2"));
        while net.height + 1 < lands_at {
            let applied = net.tick().await;
            assert!(applied.admissions.is_empty());
        }
        let applied = net.tick().await;
        assert_eq!(applied.height, lands_at);
        assert_eq!(applied.admissions.len(), 1);
        assert_eq!(applied.admissions[0].program, "identity2");
        assert!(net.host.programs().unwrap().contains_key("identity2"));
        net.apply(
            &public(7),
            "identity2",
            &identity::Op::Create {
                name: "Seven".into(),
                scheme: Scheme::Ed25519,
            },
        )
        .await;
        let module_registry::Reply::Program(Some(seated)) = net
            .ask(
                module_registry::PROGRAM,
                &module_registry::Query::Program("identity2".into()),
            )
            .await
        else {
            panic!()
        };
        assert_eq!(seated, entry);
        let removal_at = net.height + 5;
        let removal = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: removal_at,
                    change: module_registry::Change::Remove("identity2".into()),
                }),
            )
            .await;
        output_of(&removal);
        let cancelled = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Cancel {
                    height: removal_at,
                    program: "identity2".into(),
                },
            )
            .await;
        output_of(&cancelled);
        net.ticks(3).await;
        assert!(net.host.programs().unwrap().contains_key("identity2"));
        let removal_at = net.height + 3;
        let removal = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: removal_at,
                    change: module_registry::Change::Remove("identity2".into()),
                }),
            )
            .await;
        output_of(&removal);
        net.ticks(2).await;
        assert!(!net.host.programs().unwrap().contains_key("identity2"));
    });
}

fn consent(
    authorizer: &ed25519::PrivateKey,
    account: AccountNumber,
    new_key: &[u8],
    generation: u64,
    expires_at: u64,
) -> identity::Consent {
    let admission = identity::Admission {
        network: NETWORK.to_vec(),
        scheme: Scheme::Ed25519,
        key: new_key.to_vec(),
        generation,
        account,
        expires_at,
    };
    identity::Consent {
        key: authorizer.public_key().as_ref().to_vec(),
        account,
        expires_at,
        proof: testkit::ed25519_proof(
            authorizer,
            identity::CONSENT_NAMESPACE,
            &admission.preimage(),
        ),
    }
}

#[test]
fn identity_founds_accounts_admits_keys_by_consent_and_provisions_programs() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let output = net
            .apply(
                &public(1),
                identity::PROGRAM,
                &identity::Op::Create {
                    name: " Alice ".into(),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        assert_eq!(abi::decode::<AccountNumber>(&output).unwrap(), 1);
        let twice = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::Create {
                    name: "Alice again".into(),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        assert_eq!(twice, reason::ALREADY_EXISTS);
        let phone = public(11);
        let expires_at = TIME + 600_000;
        net.apply(
            &phone,
            identity::PROGRAM,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone".into()),
                consent: consent(&key(1), 1, &phone, 0, expires_at),
            },
        )
        .await;
        let identity::Reply::Account(Some(account)) = net
            .ask(identity::PROGRAM, &identity::Query::Get { number: 1 })
            .await
        else {
            panic!()
        };
        assert_eq!(account.name, "Alice");
        assert_eq!(account.keys().len(), 2);
        let identity::Reply::Number(of_phone) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::OfKey { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(of_phone, Some(1));
        let replayed = net
            .refuse(
                &public(12),
                identity::PROGRAM,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), 1, &phone, 0, expires_at),
                },
            )
            .await;
        assert_eq!(replayed, reason::UNAUTHORIZED);
        let expired = net
            .refuse(
                &public(12),
                identity::PROGRAM,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), 1, &public(12), 0, TIME - 1),
                },
            )
            .await;
        assert_eq!(expired, reason::UNAUTHORIZED);
        let senior = net
            .refuse(
                &phone,
                identity::PROGRAM,
                &identity::Op::RemoveKey { key: public(1) },
            )
            .await;
        assert_eq!(senior, reason::UNAUTHORIZED);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::RemoveKey { key: phone.clone() },
        )
        .await;
        let last = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::RemoveKey { key: public(1) },
            )
            .await;
        assert_eq!(last, reason::WRONG_STATE);
        let identity::Reply::Generation(generation) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Generation { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(generation, 1);
        net.apply(
            &phone,
            identity::PROGRAM,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone again".into()),
                consent: consent(&key(1), 1, &phone, 1, expires_at),
            },
        )
        .await;
        let created = net
            .sent_by(
                "probe",
                identity::PROGRAM,
                &identity::Op::CreateProgram {
                    name: "Chief".into(),
                    controller: 1,
                },
            )
            .await;
        assert_eq!(
            abi::decode::<AccountNumber>(output_of(&created)).unwrap(),
            2
        );
        let identity::Reply::Account(Some(chief)) = net
            .ask(identity::PROGRAM, &identity::Query::Get { number: 2 })
            .await
        else {
            panic!()
        };
        assert_eq!(
            chief.control,
            identity::Control::Program {
                executor: "probe".into(),
                controller: 1,
                standing: identity::Standing::Active,
            }
        );
        let identity::Reply::Accounts(controlled) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Controlled {
                    by: 1,
                    page: Page::all(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(controlled.len(), 1);
        assert_eq!(controlled[0].number, 2);
        let not_the_executor = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::SetStanding {
                    account: 2,
                    standing: identity::Standing::Suspended,
                },
            )
            .await;
        assert_eq!(not_the_executor, reason::UNAUTHORIZED);
        let suspended = net
            .sent_by(
                "probe",
                identity::PROGRAM,
                &identity::Op::SetStanding {
                    account: 2,
                    standing: identity::Standing::Suspended,
                },
            )
            .await;
        output_of(&suspended);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::SetName {
                account: 1,
                name: "Alice B".into(),
            },
        )
        .await;
        let circular = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::TransferControl { account: 2, to: 2 },
            )
            .await;
        assert_eq!(circular, reason::WRONG_STATE);
        let identity::Reply::Resolved(resolved) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Resolve {
                    references: vec![
                        identity::Reference::Account(2),
                        identity::Reference::Key(phone.clone()),
                        identity::Reference::Account(9),
                    ],
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(resolved, vec![Some(2), Some(1), None]);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::Revoke { account: 2 },
        )
        .await;
        let identity::Reply::Account(Some(revoked)) = net
            .ask(identity::PROGRAM, &identity::Query::Get { number: 2 })
            .await
        else {
            panic!()
        };
        assert_eq!(
            revoked.control,
            identity::Control::Revoked { controller: 1 }
        );
    });
}
