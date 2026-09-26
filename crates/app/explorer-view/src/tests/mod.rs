//! A node the explorer reads, played by the fake host: a short chain, three
//! accounts, one validator and a registry, each answered as the program
//! would. Tests by what they cover: `describe`, `window`, `pages`, `search`.
mod describe;
mod follow;
mod pages;
mod search;
mod window;

pub(crate) use std::cell::RefCell;
pub(crate) use std::rc::Rc;

pub(crate) use crate::*;
pub(crate) use abi::BlobId;
pub(crate) use ducktape_view_guest::methods::{
    Block, BlockPage, BlockRef, ChainBlock, ChainBlocks, ChainHeads, ChainStatus, Changes,
    ClipboardWrite, ClockTicks, Description, Head, HostRoute, HostSession, ModuleDescribe,
    NodeStatus, Query, Session, Tx, Value,
};
pub(crate) use ducktape_view_guest::testing::{StreamSender, TestAppContext};
pub(crate) use identity::view::Identity;
pub(crate) use module_registry as registry;
pub(crate) use module_registry::view::Registry;
pub(crate) use valset::view::Valset;

pub(crate) const ADA: [u8; 32] = [1; 32];
pub(crate) const STRANGER: [u8; 32] = [2; 32];
pub(crate) const VALIDATOR: [u8; 32] = [9; 32];
/// Block times, milliseconds: block `h` lands at `T0 + h` seconds.
pub(crate) const T0: u64 = 1_790_000_000_000;

pub(crate) fn post(channel: &str, text: &str) -> Vec<u8> {
    borsh::to_vec(&chat::Op::PostMessage {
        channel_id: channel.into(),
        message_id: "m1".into(),
        blocks: chat::parse_message(text),
        thread: None,
    })
    .unwrap()
}

pub(crate) fn tx(seed: u8, signer: [u8; 32], target: &str, payload: Vec<u8>) -> Tx {
    Tx {
        hash: [seed; 32],
        signer: signer.to_vec(),
        seq: seed as u64,
        target: target.into(),
        payload,
    }
}

/// Blocks 0..=`tip`: 11 carries Ada's post and their DM to account 7, 12 a
/// stranger's op to a program that describes nothing.
pub(crate) fn chain(tip: u64) -> Vec<Block> {
    (0..=tip)
        .map(|height| Block {
            height,
            id: [(height as u8).wrapping_add(100); 32],
            parent: [(height as u8).wrapping_add(99); 32],
            time: T0 + height * 1000,
            epoch: height / 10,
            proposer: Some(VALIDATOR.to_vec()),
            txs: match height {
                11 => vec![
                    tx(0xa1, ADA, "chat", post("design", "hello there")),
                    tx(0xc3, ADA, "chat", post(&chat::dm_channel_id(7, 3), "ping")),
                ],
                12 => vec![tx(0xb2, STRANGER, "mystery", vec![1, 2, 3, 4])],
                _ => Vec::new(),
            },
        })
        .collect()
}

pub(crate) fn page(chain: &[Block], ask: &BlockPage) -> Vec<Block> {
    let top = chain.len() as u64 - 1;
    let Some(start) = ask.before.map_or(Some(top), |before| before.checked_sub(1)) else {
        return Vec::new();
    };
    (0..=start.min(top))
        .rev()
        .take(ask.limit as usize)
        .map(|height| chain[height as usize].clone())
        .collect()
}

pub(crate) fn status(height: u64) -> NodeStatus {
    NodeStatus {
        chain_id: "test#1".into(),
        block_time_ms: 1000,
        epoch_length: 10,
        height,
        epoch: (height + 1) / 10,
        ..NodeStatus::default()
    }
}

pub(crate) fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: name.into(),
            avatar: None,
            bio: None,
            updated_at: 0,
        },
        control,
    }
}

pub(crate) fn ada() -> identity::Account {
    let keys = vec![identity::Key {
        scheme: abi::Scheme::Ed25519,
        key: ADA.to_vec(),
        label: Some("laptop".into()),
        added_at: 0,
    }];
    account(3, "Ada", identity::Control::Person { keys })
}

/// The agent Ada manages, suspended: no keys yet.
pub(crate) fn scout() -> identity::Account {
    account(
        5,
        "Scout",
        identity::Control::Managed {
            manager: 3,
            category: identity::Category::Agent,
            life: identity::Life::Suspended { keys: Vec::new() },
        },
    )
}

/// forge's own account.
pub(crate) fn forge() -> identity::Account {
    account(
        6,
        "forge",
        identity::Control::Module {
            module: "forge".into(),
        },
    )
}
/// A node at `tip`, whose tip the test may move.
pub(crate) fn node(
    cx: &mut TestAppContext,
    tip: Rc<RefCell<u64>>,
) -> (StreamSender<HostSession>, StreamSender<HostRoute>) {
    let feeds = (
        cx.host().stream::<HostSession>(),
        cx.host().stream::<HostRoute>(),
    );
    follow(cx);
    let host = cx.host();
    let head = tip.clone();
    host.handle::<ChainStatus>(move |()| Ok(status(*head.borrow())));
    let head = tip.clone();
    host.handle::<ChainBlocks>(move |ask| Ok(page(&chain(*head.borrow()), &ask)));
    host.handle::<ChainBlock>(move |by| {
        let chain = chain(*tip.borrow());
        Ok(match by {
            BlockRef::Height(height) => chain.get(height as usize).cloned(),
            BlockRef::Id(id) => chain.into_iter().find(|block| block.id == id),
        })
    });
    host.handle::<Query<Identity>>(|query| match query {
        identity::Query::List { .. } => {
            Ok(identity::Reply::Accounts(module_registry::PageResponse {
                height: 1,
                items: vec![ada(), scout(), forge()],
                next: None,
            }))
        }
        other => panic!("unexpected identity query: {other:?}"),
    });
    host.handle::<Query<Valset>>(|_| Ok(valset::Reply::Validators(vec![VALIDATOR.to_vec()])));
    respond(cx);
    describes(cx);
    feeds
}

pub(crate) fn entry(program: &str, code: u8) -> registry::Entry {
    registry::Entry {
        program: program.into(),
        code: BlobId::Sha256([code; 32]),
        params: vec![1, 2, 3],
    }
}

pub(crate) fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => {
                registry::Reply::Programs(vec![entry("chat", 0xab), entry("identity", 0xcd)])
            }
            registry::Query::Views(0) => registry::Reply::Views(vec![registry::View {
                name: "explorer".into(),
                view: BlobId::Sha256([0xef; 32]),
            }]),
            registry::Query::Scheduled { .. } => {
                registry::Reply::Scheduled(module_registry::PageResponse {
                    height: 1,
                    items: vec![registry::Scheduled {
                        height: 120,
                        change: registry::Change::Remove("forge".into()),
                    }],
                    next: None,
                })
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

pub(crate) fn ready() -> (TestAppContext, Rc<RefCell<u64>>) {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    (cx, tip)
}

/// The host's `module.describe`, as each program's describe module would
/// answer: its own `describe` over the op, `None` for a program without one.
pub(crate) fn describes(cx: &mut TestAppContext) {
    fn with<T: borsh::BorshDeserialize>(
        op: &[u8],
        describe: fn(&T) -> Description,
    ) -> Option<Description> {
        borsh::from_slice(op).ok().map(|op| describe(&op))
    }
    cx.host().handle::<ModuleDescribe>(|(program, op)| {
        Ok(match program.as_str() {
            "chat" => with(&op, chat::describe),
            "forge" => with(&op, forge::describe),
            "identity" => with(&op, identity::describe),
            "valset" => with(&op, valset::describe),
            registry::MODULE => with(&op, registry::describe),
            _ => None,
        })
    });
}

/// A full window: 1,000 blocks, each with a post, and a 1 MB push at the tip.
pub(crate) fn heavy(cx: &mut TestAppContext) {
    let tip = WINDOW as u64;
    let chain: Vec<Block> = (0..=tip)
        .map(|height| {
            let seed = (height % 250) as u8;
            let mut tx = if height == tip {
                let push = forge::Op::Push {
                    repo: "app".into(),
                    request: vec![0x50; 1 << 20],
                };
                tx(0xfe, ADA, forge::MODULE, borsh::to_vec(&push).unwrap())
            } else {
                let text = format!("message {height}");
                tx(seed, ADA, chat::MODULE, post("design", &text))
            };
            tx.hash[..8].copy_from_slice(&height.to_le_bytes());
            Block {
                height,
                id: [seed; 32],
                parent: [seed.wrapping_sub(1); 32],
                time: T0 + height * 1000,
                epoch: height / 10,
                proposer: Some(VALIDATOR.to_vec()),
                txs: vec![tx],
            }
        })
        .collect();
    follow(cx);
    let host = cx.host();
    host.stream::<ChainHeads>();
    host.stream::<HostSession>();
    host.stream::<HostRoute>();
    host.handle::<ChainStatus>(move |()| Ok(status(tip)));
    let blocks = chain.clone();
    host.handle::<ChainBlocks>(move |ask| Ok(page(&blocks, &ask)));
    host.handle::<ChainBlock>(move |by| {
        Ok(match by {
            BlockRef::Height(height) => chain.get(height as usize).cloned(),
            BlockRef::Id(id) => chain.iter().find(|block| block.id == id).cloned(),
        })
    });
    host.handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(module_registry::PageResponse {
            height: 1,
            items: vec![ada()],
            next: None,
        }))
    });
    host.handle::<Query<Valset>>(|_| Ok(valset::Reply::Validators(vec![VALIDATOR.to_vec()])));
    respond(cx);
    describes(cx);
}

/// The live heads of the three programs whose lists the explorer shows.
pub(crate) fn follow(cx: &TestAppContext) {
    cx.host().stream::<Changes<Identity>>();
    cx.host().stream::<Changes<Valset>>();
    cx.host().stream::<Changes<Registry>>();
}

/// A restore that must read nothing: every ask left unanswered.
pub(crate) fn quiet_host(cx: &TestAppContext) {
    follow(cx);
    let host = cx.host();
    host.stream::<ChainHeads>();
    host.never::<ChainStatus>();
    host.never::<HostSession>();
    host.never::<HostRoute>();
    host.never::<ChainBlocks>();
    host.never::<Query<Identity>>();
    host.never::<Query<Valset>>();
    host.never::<Query<Registry>>();
    host.never::<ModuleDescribe>();
}
