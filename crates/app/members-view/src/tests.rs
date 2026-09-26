use super::*;
use abi::Scheme;
use ducktape_view_guest::methods::{
    Block, BlockPage, ChainBlocks, Description, ModuleDescribe, Session, Tx,
};
use ducktape_view_guest::testing::{StreamSender, TestAppContext};
use ducktape_view_guest::wire::{ContainerNode, Node, TextNode};
use ducktape_view_guest::{Hsla, Theme};

#[test]
fn the_window_opens_wide_enough_for_the_list_and_the_detail() {
    assert_eq!(<Members as View>::PREFERRED_WINDOW_SIZE, "960,640");
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let (mut cx, _) = ready();
    let dark = Theme::dark();
    cx.set_global(dark);
    let Some(Node::Container(ContainerNode { style, .. })) = cx.find("members") else {
        panic!("members root is a styled container");
    };
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|background| background.as_solid()),
        Some(dark.background)
    );
    assert_eq!(style.text.color, Some(dark.foreground));
}

fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: name.into(),
            avatar: None,
            bio: Some(format!("{name}'s bio")),
            updated_at: 0,
        },
        control,
    }
}

fn key(key: &[u8], label: &str) -> identity::Key {
    identity::Key {
        scheme: Scheme::Ed25519,
        key: key.to_vec(),
        label: Some(label.into()),
        added_at: 0,
    }
}

fn person(number: u64, name: &str, keys: Vec<identity::Key>) -> identity::Account {
    account(number, name, identity::Control::Person { keys })
}

fn module(number: u64, name: &str) -> identity::Account {
    account(
        number,
        name,
        identity::Control::Module {
            module: name.into(),
        },
    )
}

fn agent(number: u64, name: &str, manager: u64, life: identity::Life) -> identity::Account {
    account(
        number,
        name,
        identity::Control::Managed {
            manager,
            category: identity::Category::Agent,
            life,
            transfers: 0,
        },
    )
}

fn page<T>(items: Vec<T>) -> module_registry::PageResponse<T> {
    module_registry::PageResponse {
        height: 1,
        items,
        next: None,
    }
}

const EDDY: &[u8] = b"\x01\x02";
const SCOUT: &[u8] = b"\x09\x09";

/// eddy (#7, the reader, a validator) manages scout (#9, active) and relay
/// (#10, revoked); chat (#8) is a module; ada (#11) is another person.
fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Identity>>(|query| {
        assert!(matches!(query, identity::Query::List { .. }));
        Ok(identity::Reply::Accounts(page(vec![
            module(8, "chat"),
            person(7, "eddy", vec![key(EDDY, "laptop")]),
            agent(
                9,
                "scout",
                7,
                identity::Life::Active {
                    keys: vec![key(SCOUT, "sandbox")],
                },
            ),
            agent(10, "relay", 7, identity::Life::Revoked),
            person(11, "ada", vec![key(b"\x0b", "phone")]),
        ])))
    });
    cx.host().handle::<Query<Valset>>(|query| {
        assert!(matches!(query, valset::Query::Memberships { .. }));
        Ok(valset::Reply::Memberships(page(vec![valset::Membership {
            key: EDDY.to_vec(),
            address: "10.0.0.1:4000".into(),
            role: valset::Role::Validator,
        }])))
    });
    // two blocks: scout posted at 12, eddy at 11 and 12
    cx.host().handle::<ChainBlocks>(|page: BlockPage| {
        let tx = |signer: &[u8], seq| Tx {
            signer: signer.to_vec(),
            seq,
            target: "chat".into(),
            payload: vec![seq as u8],
            ..Tx::default()
        };
        let block = |height, txs| Block {
            height,
            time: height * 60_000,
            txs,
            ..Block::default()
        };
        Ok(match page.before {
            None => vec![
                block(12, vec![tx(EDDY, 2), tx(SCOUT, 5)]),
                block(11, vec![tx(EDDY, 1)]),
            ],
            Some(_) => Vec::new(),
        })
    });
    cx.host().handle::<ModuleDescribe>(|(target, payload)| {
        Ok(Some(Description {
            title: format!("Post {} in {target}", payload[0]),
            fields: Vec::new(),
        }))
    });
}

fn ready() -> (TestAppContext, StreamSender<Changes<Identity>>) {
    let mut cx = TestAppContext::new();
    let session = cx.host().stream::<HostSession>();
    let feed = cx.host().stream::<Changes<Identity>>();
    respond(&mut cx);
    cx.open::<Members>();
    session.send(Session {
        account: Some(7),
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    (cx, feed)
}

/// The colour `text` is drawn in under `key`, inherited down the tree.
fn color_of(cx: &TestAppContext, key: &str, text: &str) -> Option<Hsla> {
    fn walk(node: &Node, text: &str, color: Option<Hsla>) -> Option<Option<Hsla>> {
        match node {
            Node::Text(TextNode { content, style, .. }) if content == text => {
                Some(style.text.color.or(color))
            }
            Node::Container(ContainerNode {
                style, children, ..
            }) => {
                let color = style.text.color.or(color);
                children.iter().find_map(|child| walk(child, text, color))
            }
            _ => None,
        }
    }
    walk(cx.find(key)?, text, None).flatten()
}

#[test]
fn the_list_groups_people_then_agents_then_modules_and_chooses_no_one() {
    let (cx, _) = ready();
    let texts = cx.texts();
    // the last of each, past the chips of the same names
    let at = |text: &str| texts.iter().rposition(|t| t == text).unwrap();
    assert!(at("People") < at("eddy") && at("eddy") < at("ada"));
    assert!(at("ada") < at("Agents") && at("Agents") < at("scout"));
    assert!(at("relay") < at("Modules") && at("Modules") < at("chat"));
    for text in [
        "5 accounts",
        "you",
        "Person",
        "Module · chat",
        "Agent · managed by eddy",
        "Agent · managed by eddy · revoked",
        "Choose a member",
    ] {
        assert!(cx.has_text(text), "{text}: {texts:?}");
    }
    // nothing chosen, nothing read of the chain
    assert!(cx.host().requests::<ChainBlocks>().is_empty());
    assert_eq!(cx.host().requests::<Changes<Identity>>().len(), 1);
}

#[test]
fn choosing_a_person_shows_their_devices_the_agents_they_manage_and_their_activity() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    for text in [
        "account 7",
        "1 device",
        "validator",
        "eddy's bio",
        "Devices",
        "laptop",
        "0102",
        "Manages",
        "account 9 · Agent",
        "active",
        "revoked",
        "Recent activity",
        "last 1,000 blocks",
        "Post 2 in chat",
        "Post 1 in chat",
        "block 12",
        "block 11",
        "1m",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    // scout's post is scout's, not eddy's
    assert!(!cx.has_text("Post 5 in chat"));
    // the reader's own account has no one to DM
    assert!(cx.find("members-open-dm").is_none());
    cx.simulate_click("members-explorer");
    assert_eq!(
        cx.host().opened_links(),
        vec!["duck://explorer/account/7".to_owned()]
    );
    // a managed agent is chosen in place
    cx.simulate_click("members-manages-9");
    cx.run_until_parked();
    assert!(cx.has_text("scout's bio"));
}

#[test]
fn an_agent_names_its_manager_its_standing_and_its_devices() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    for text in [
        "account 9",
        "Agent · managed by",
        "eddy",
        "active",
        "1 device",
        "sandbox",
        "Post 5 in chat",
        "Suspend, revoke, rename and keys live in Settings → Account, for the manager.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(!cx.has_text("Post 1 in chat") && !cx.has_text("Post 2 in chat"));
    cx.simulate_click("members-open-dm");
    assert_eq!(
        cx.host().opened_links(),
        vec![ducklink::mint("testnet#0a1b2c3d", "chat", &["dm-7-9"]).unwrap()]
    );
    // the manager's name chooses the manager
    cx.simulate_click("members-manager");
    cx.run_until_parked();
    assert!(cx.has_text("eddy's bio"));
}

#[test]
fn a_module_has_no_devices_and_no_one_to_dm() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-8");
    cx.run_until_parked();
    assert!(cx.has_text("Module · chat"));
    assert!(cx.has_text("No devices"));
    assert!(!cx.has_text("Recent activity") && !cx.has_text("1 device"));
    assert!(cx.find("members-open-dm").is_none());
    assert!(cx.find("members-explorer").is_some());
    assert!(cx.host().requests::<ChainBlocks>().is_empty());
}

#[test]
fn a_revoked_agent_stays_listed_dimmed() {
    let (cx, _) = ready();
    let theme = Theme::light();
    assert_eq!(color_of(&cx, "members-row-10", "relay"), Some(theme.faint));
    assert_ne!(color_of(&cx, "members-row-9", "scout"), Some(theme.faint));
}

#[test]
fn the_filter_and_the_chips_narrow_together_and_keep_the_choice() {
    let (mut cx, _) = ready();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    let reads = cx.host().requests::<Query<Identity>>().len();
    // Modules: only chat
    cx.simulate_click("members-chip-3");
    assert!(cx.find("members-row-8").is_some() && cx.find("members-row-7").is_none());
    // and a filter on top: nothing is both a module and "ed"
    cx.simulate_input("members-filter", "ed");
    assert!(cx.has_text("Nothing matches"));
    // People and "ed": eddy alone
    cx.simulate_click("members-chip-1");
    assert!(cx.find("members-row-7").is_some() && cx.find("members-row-11").is_none());
    // the chosen agent is hidden from the list, not from the detail
    assert!(cx.has_text("scout's bio"));
    cx.simulate_click("members-chip-0");
    cx.simulate_input("members-filter", "");
    assert!(cx.find("members-row-11").is_some());
    assert!(cx.has_text("scout's bio"));
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), reads);
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<HostSession>();
    cx.host().stream::<Changes<Identity>>();
    cx.host().never::<Query<Identity>>();
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the roster…"));
}

#[test]
fn a_roster_with_nobody_in_it_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<HostSession>();
    cx.host().stream::<Changes<Identity>>();
    cx.host()
        .handle::<Query<Identity>>(|_| Ok(identity::Reply::Accounts(page(vec![]))));
    cx.host()
        .handle::<Query<Valset>>(|_| Ok(valset::Reply::Memberships(page(vec![]))));
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("No accounts"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<HostSession>();
    cx.host().stream::<Changes<Identity>>();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("identity is not running here"));
    respond(&mut cx);
    cx.simulate_click("members-retry");
    cx.run_until_parked();
    assert!(cx.has_text("eddy"));
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), 2);
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_choice() {
    let (mut cx, feed) = ready();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "refresh temporarily unavailable");
    feed.send(None);
    cx.run_until_parked();
    assert!(cx.has_text("scout's bio"));
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), 2);
    cx.host().handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(page(vec![
            person(7, "eddy", vec![key(EDDY, "laptop")]),
            agent(
                9,
                "scout",
                7,
                identity::Life::Suspended {
                    keys: vec![key(SCOUT, "sandbox")],
                },
            ),
        ])))
    });
    feed.send(None);
    cx.run_until_parked();
    assert!(cx.has_text("suspended") && !cx.has_text("ada"));
    cx.simulate_input("members-filter", "sc");
    cx.simulate_click("members-chip-2");
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<HostSession>();
    restored.host().stream::<Changes<Identity>>();
    restored.host().never::<Query<Identity>>();
    restored.host().never::<ChainBlocks>();
    let view = restored.restore::<Members>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("scout's bio"));
    view.read(|view| {
        assert_eq!(view.filter, "sc");
        assert_eq!(view.only, Some(Group::Agents));
        assert_eq!(view.selected, Some(9));
    });
    assert_eq!(restored.host().requests::<Query<Identity>>().len(), 1);
    // the activity is read again for the restored choice
    assert_eq!(restored.host().requests::<ChainBlocks>().len(), 1);
}

#[test]
fn the_list_and_the_detail_are_accessible() {
    let (mut cx, _) = ready();
    cx.assert_accessible();
    cx.simulate_click("members-row-9");
    cx.run_until_parked();
    cx.assert_accessible();
}
