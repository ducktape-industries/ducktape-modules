use crate::Settings;
use crate::api::*;
use ducktape_view_guest::methods::{Changes, ClipboardWrite, ClockTicks, Query};
use ducktape_view_guest::testing::{StreamSender, TestAppContext};
use ducktape_view_guest::{Theme, wire};

fn status() -> NodeStatus {
    NodeStatus {
        chain_id: "Workshop".into(),
        time: 100,
        block_time_ms: 1000,
        epoch_length: 100,
        height: 42,
        tip: [0xab; 32],
        root: [0xcd; 32],
        epoch: 3,
        identity: vec![0xef; 32],
        contract: 7,
    }
}
fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: name.into(),
            avatar: None,
            bio: None,
            updated_at: 1,
        },
        control,
    }
}
/// Maya's account: a person holding one key.
fn maya(number: u64, label: &str) -> identity::Account {
    let keys = vec![identity::Key {
        scheme: abi::Scheme::Ed25519,
        key: vec![0xab, 0xcd],
        label: Some(label.into()),
        added_at: 1,
    }];
    account(number, "Maya", identity::Control::Person { keys })
}
/// Scout, the agent Maya (7) manages, keyless.
fn scout() -> identity::Account {
    account(
        12,
        "Scout",
        identity::Control::Managed {
            manager: 7,
            category: identity::Category::Agent,
            life: identity::Life::Active { keys: Vec::new() },
            transfers: 0,
        },
    )
}
fn respond(cx: &TestAppContext) {
    cx.host().handle::<ChainStatus>(|()| Ok(status()));
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                assert_eq!(number, 7);
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { by: 7, .. } => {
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![scout()],
                    next: None,
                })
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.host().handle::<Query<Valset>>(|q| {
        Ok(match q {
            valset::Query::Membership { key } => {
                valset::Reply::Membership(Some(valset::Membership {
                    key,
                    address: "127.0.0.1:19001".into(),
                    role: valset::Role::Validator,
                }))
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
}
fn fixture(state: &str, dark: bool) -> TestAppContext {
    seated(state, dark).0
}
/// The fixture, and the session feed the host speaks through.
fn seated(state: &str, dark: bool) -> (TestAppContext, StreamSender<HostSession>) {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ClockTicks>();
    cx.host().stream::<Changes<Valset>>();
    cx.host().stream::<Changes<Identity>>();
    let props = cx.host().stream::<HostSession>();
    respond(&cx);
    match state {
        "unregistered" => cx.host().handle::<Query<Identity>>(|q| match q {
            identity::Query::Resolve { references } => {
                Ok(identity::Reply::Resolved(vec![None; references.len()]))
            }
            q => panic!("an unregistered key asks identity who holds it, nothing more: {q:?}"),
        }),
        "suspended" => cx.host().handle::<Query<Identity>>(|q| {
            Ok(match q {
                identity::Query::Resolve { references } => {
                    assert_eq!(references, vec![identity::Reference::Key(vec![0xab, 0xcd])]);
                    identity::Reply::Resolved(vec![Some(12)])
                }
                identity::Query::Profile { number: 12 } => {
                    identity::Reply::Profile(Some(identity::Profile {
                        number: 12,
                        name: "Scout".into(),
                        kind: identity::Kind::Managed {
                            manager: 7,
                            category: identity::Category::Agent,
                            standing: identity::Standing::Suspended,
                        },
                    }))
                }
                q => panic!("unexpected query: {q:?}"),
            })
        }),
        "loading" => {
            cx.host().never::<ChainStatus>();
            cx.host().never::<Query<Identity>>();
        }
        "refused" => {
            cx.host()
                .refuse::<ChainStatus>("unavailable", "The node is unavailable. Try again.");
            cx.host()
                .refuse::<Query<Identity>>("unavailable", "Account query refused.");
        }
        _ => {}
    }
    cx.set_global(if dark { Theme::dark() } else { Theme::light() });
    cx.open::<Settings>();
    props.send(Session {
        signer: if state == "empty" {
            String::new()
        } else {
            "abcd".into()
        },
        account: (!matches!(state, "empty" | "unregistered" | "suspended")).then_some(7),
        dark,
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    if state.starts_with("invite") {
        match state {
            "invite-loading" => cx.host().never::<InviteCreate>(),
            "invite-refused" => cx
                .host()
                .refuse::<InviteCreate>("forbidden", "This node does not allow minting invites."),
            _ => cx.host().handle::<InviteCreate>(|request| {
                assert_eq!(request.ttl_days, 7);
                Ok(Invite {
                    invite: "duck-invite:workshop-loopback-example".into(),
                    notes: vec![ducktape_view_guest::host::Error {
                        code: "expires".into(),
                        message: "This invite expires in 7 days.".into(),
                    }],
                })
            }),
        }
        cx.simulate_click("settings/invite/mint");
        cx.run_until_parked();
    }
    (cx, props)
}
#[test]
fn four_states_are_honest() {
    assert!(fixture("loading", false).has_text("Reading node status…"));
    assert!(fixture("refused", false).has_text("The node is unavailable. Try again."));
    assert!(fixture("empty", false).has_text("No account"));
    let cx = fixture("ready", false);
    for text in [
        "Network: Workshop",
        "Height / epoch: 42 / 3",
        "Who I am: Maya · account 7",
        "Laptop key: abcd · Validator",
        "Contract version: 7",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.assert_accessible();
}
#[test]
fn invite_ttl_copy_and_refusal() {
    let mut cx = fixture("invite-ready", false);
    cx.host().handle::<ClipboardWrite>(|text| {
        assert_eq!(text, "duck-invite:workshop-loopback-example");
        Ok(())
    });
    cx.simulate_click("settings/invite/copy");
    cx.run_until_parked();
    assert!(cx.has_text("Copied"));
    cx.host().handle::<InviteCreate>(|r| {
        assert_eq!(r.ttl_days, 30);
        Ok(Invite {
            invite: "long-lived".into(),
            notes: vec![],
        })
    });
    cx.simulate_click("settings/ttl/30");
    cx.simulate_click("settings/invite/mint");
    cx.run_until_parked();
    assert!(cx.has_text("long-lived"));
    assert!(fixture("invite-refused", false).has_text("This node does not allow minting invites."));
    assert!(fixture("invite-loading", false).has_text("Minting invite…"));
}
#[test]
fn live_updates_retry_and_restore() {
    let mut cx = TestAppContext::new();
    let live = cx.host().stream::<ClockTicks>();
    cx.host().stream::<Changes<Valset>>();
    cx.host().stream::<Changes<Identity>>();
    cx.host().stream::<HostSession>();
    respond(&cx);
    cx.open::<Settings>();
    cx.host().handle::<ChainStatus>(|()| {
        let mut s = status();
        s.height = 43;
        Ok(s)
    });
    live.send(());
    cx.run_until_parked();
    assert!(cx.has_text("Height / epoch: 43 / 3"));
    let snapshot = cx.snapshot().unwrap();
    cx.restore::<Settings>(&snapshot).unwrap();
    cx.run_until_parked();
    assert!(cx.has_text("Height / epoch: 43 / 3"));
    let mut cx = fixture("refused", false);
    respond(&cx);
    cx.simulate_click("settings/node-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Network: Workshop"));
}
#[test]
fn an_account_changed_elsewhere_is_read_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ClockTicks>();
    cx.host().stream::<Changes<Valset>>();
    let accounts = cx.host().stream::<Changes<Identity>>();
    let props = cx.host().stream::<HostSession>();
    respond(&cx);
    cx.open::<Settings>();
    props.send(Session {
        signer: "abcd".into(),
        account: Some(7),
        ..Session::default()
    });
    cx.run_until_parked();
    assert!(cx.find("settings/agents/12/suspend").is_some());
    // Maya suspends Scout from another device
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                identity::Reply::Account(Some(maya(number, "Laptop key")))
            }
            identity::Query::Managed { .. } => {
                let mut scout = scout();
                scout.control = identity::Control::Managed {
                    manager: 7,
                    category: identity::Category::Agent,
                    life: identity::Life::Suspended { keys: Vec::new() },
                    transfers: 0,
                };
                identity::Reply::Accounts(identity::PageResponse {
                    height: 43,
                    items: vec![scout],
                    next: None,
                })
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
    accounts.send(Some(43));
    cx.run_until_parked();
    assert!(
        cx.find("settings/agents/12/resume").is_some(),
        "{:?}",
        cx.texts()
    );
}
#[test]
fn export_settings_screens() {
    if std::env::var_os("SETTINGS_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/settings/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for (i, state) in [
        "loading",
        "refused",
        "empty",
        "ready",
        "invite-loading",
        "invite-refused",
        "invite-ready",
        "unregistered",
    ]
    .into_iter()
    .enumerate()
    {
        for dark in [false, true] {
            let cx = fixture(state, dark);
            let theme = if dark { "dark" } else { "light" };
            let name = format!("{:02}-{state}-{theme}", i + 1);
            let node = cx.find("settings").expect("settings root");
            std::fs::write(
                out.join(format!("{name}.json")),
                serde_json::to_vec(node).unwrap(),
            )
            .unwrap();
            manifest.push(serde_json::json!({"name":name,"theme":theme,"width":820,"height":1100,"how":"TestAppContext + FakeHost"}));
        }
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn unregistered_key_keeps_its_standing() {
    let cx = fixture("unregistered", false);
    assert!(cx.has_text("Who I am: Unregistered key"));
    assert!(cx.has_text("Host key: abcd · Validator"));
}

#[test]
fn no_key_offers_no_form() {
    let cx = fixture("empty", false);
    assert!(cx.has_text("No host key is selected. Sign in with a key to create an account."));
    assert!(cx.find("settings/account/create/name").is_none());
}

#[test]
fn a_suspended_agents_key_reads_as_such_and_creates_nothing() {
    let cx = fixture("suspended", false);
    assert!(cx.has_text("Who I am: Scout"), "{:?}", cx.texts());
    assert!(cx.has_text("This key belongs to Scout, suspended by its manager."));
    assert!(cx.find("settings/account/create").is_none());
    cx.assert_accessible();
}
#[test]
fn unregistered_key_creates_an_account() {
    let (mut cx, props) = seated("unregistered", false);
    assert!(cx.has_text(
        "Your key isn't linked to an account yet. An account gives you a name others see."
    ));
    cx.assert_accessible();

    // Empty name never reaches the host: identity's own rule (a name is not
    // empty) is mirrored inline.
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Enter an account name."));
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());

    // A refusal from the program lands as a human sentence, name kept.
    cx.host()
        .refuse::<Submit<Identity>>("invalid", "a name is not empty");
    cx.simulate_input("settings/account/create/name", "Maya");
    cx.simulate_submit("settings/account/create/name");
    cx.run_until_parked();
    assert!(cx.has_text("That didn’t go through: a name is not empty"));

    // Success holds the form busy until the host's session names the new
    // account, which is read: "Who I am" now carries the name.
    cx.host().handle::<Submit<Identity>>(|op| {
        assert!(matches!(
            op,
            identity::Op::Create { ref name, scheme: abi::Scheme::Ed25519 } if name == "Maya"
        ));
        Ok(Vec::new())
    });
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                assert_eq!(number, 9);
                identity::Reply::Account(Some(maya(number, "Host key")))
            }
            identity::Query::Managed { .. } => identity::Reply::Accounts(identity::PageResponse {
                height: 42,
                items: vec![],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Creating…"), "{:?}", cx.texts());
    props.send(Session {
        signer: "abcd".into(),
        account: Some(9),
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    assert!(
        cx.has_text("Who I am: Maya · account 9"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("settings/account/create/name").is_none());
}

#[test]
fn create_account_disables_controls_while_busy() {
    let mut cx = fixture("unregistered", false);
    cx.host().never::<Submit<Identity>>();
    cx.simulate_input("settings/account/create/name", "Maya");
    cx.simulate_click("settings/account/create/submit");
    cx.run_until_parked();
    assert!(cx.has_text("Creating…"));
    let Some(wire::Node::Container(wire::ContainerNode { interactivity, .. })) =
        cx.find("settings/account/create/submit")
    else {
        panic!("settings/account/create/submit button")
    };
    assert_eq!(interactivity.aria.disabled, Some(true));
    assert!(interactivity.on_click.is_none());
    let Some(wire::Node::Input { options, .. }) = cx.find("settings/account/create/name") else {
        panic!("settings/account/create/name input")
    };
    assert!(options.disabled);
}

#[test]
fn long_host_key_is_truncated_and_non_validator_standing_is_quiet() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ClockTicks>();
    cx.host().stream::<Changes<Valset>>();
    cx.host().stream::<Changes<Identity>>();
    let props = cx.host().stream::<HostSession>();
    cx.host().handle::<ChainStatus>(|()| Ok(status()));
    let long_key = vec![0x11; 32];
    let long_hex = abi::hex(&long_key);
    cx.host().handle::<Query<Valset>>(|q| {
        Ok(match q {
            valset::Query::Membership { .. } => valset::Reply::Membership(None),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.host().handle::<Query<Identity>>(|q| match q {
        identity::Query::Resolve { .. } => Ok(identity::Reply::Resolved(vec![None])),
        q => panic!("unexpected query: {q:?}"),
    });
    cx.set_global(Theme::light());
    cx.open::<Settings>();
    props.send(Session {
        signer: long_hex.clone(),
        dark: false,
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    let truncated = format!("{}…{}", &long_hex[..8], &long_hex[long_hex.len() - 4..]);
    assert!(
        cx.has_text(&format!("Host key: {truncated}")),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_person_creates_an_agent_and_adds_its_key() {
    let mut cx = fixture("ready", false);
    assert!(
        cx.has_text("Scout: Agent · managed by Maya · account 12 · 0 keys"),
        "{:?}",
        cx.texts()
    );
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(op, identity::Op::CreateAgent { name: "Bot".into() });
        Ok(Vec::new())
    });
    cx.simulate_input("settings/agents/create/name", " Bot ");
    cx.simulate_click("settings/agents/create/submit");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);

    // a request that is not an AddKey for one of Maya's agents never leaves
    cx.simulate_input("settings/agents/key/request", "zz");
    cx.simulate_click("settings/agents/key/submit");
    cx.run_until_parked();
    assert!(cx.has_text("That isn’t a key request for one of your agents."));
    let add = identity::Op::AddKey {
        scheme: abi::Scheme::Ed25519,
        label: Some("sandbox".into()),
        consent: identity::Consent {
            key: vec![0x51; 32],
            account: 12,
            expires_at: 500,
            proof: vec![0x52; 64],
        },
    };
    let expected = add.clone();
    cx.host().handle::<Submit<Identity>>(move |op| {
        assert_eq!(op, expected);
        Ok(Vec::new())
    });
    cx.simulate_input("settings/agents/key/request", &abi::hex(&abi::encode(&add)));
    cx.simulate_click("settings/agents/key/submit");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 2);
    assert!(!cx.has_text("That isn’t a key request for one of your agents."));
    cx.assert_accessible();
}

/// The manager alone renames, suspends, resumes and revokes an agent from
/// its line; each is one op, and the agents are read again after it.
/// Revoking is final, so it takes a second press.
#[test]
fn a_manager_renames_suspends_and_revokes_an_agent() {
    let mut cx = fixture("ready", false);
    let sent = |cx: &TestAppContext| cx.host().requests::<Submit<Identity>>().len();
    cx.simulate_click("settings/agents/12/rename");
    cx.run_until_parked();
    assert!(
        cx.has_text("Enter the agent's new name."),
        "{:?}",
        cx.texts()
    );
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(
            op,
            identity::Op::SetName {
                account: 12,
                name: "Scout II".into()
            }
        );
        Ok(Vec::new())
    });
    cx.simulate_input("settings/agents/12/name", " Scout II ");
    cx.simulate_click("settings/agents/12/rename");
    cx.run_until_parked();
    assert_eq!(sent(&cx), 1);
    assert!(!cx.has_text("Enter the agent's new name."));

    // active, its line offers Suspend; suspended, Resume; revoked, nothing
    assert!(cx.find("settings/agents/12/suspend").is_some());
    assert!(cx.find("settings/agents/12/resume").is_none());
    cx.host().handle::<Submit<Identity>>(|op| {
        assert_eq!(op, identity::Op::Suspend { account: 12 });
        Ok(Vec::new())
    });
    let suspended = |cx: &TestAppContext, life: identity::Life| {
        cx.host().handle::<Query<Identity>>(move |q| {
            let life = life.clone();
            Ok(match q {
                identity::Query::Get { number } => {
                    identity::Reply::Account(Some(maya(number, "Laptop key")))
                }
                identity::Query::Managed { by: 7, .. } => {
                    let identity::Control::Managed {
                        manager,
                        category,
                        transfers,
                        ..
                    } = scout().control
                    else {
                        panic!()
                    };
                    let control = identity::Control::Managed {
                        manager,
                        category,
                        life,
                        transfers,
                    };
                    identity::Reply::Accounts(identity::PageResponse {
                        height: 42,
                        items: vec![identity::Account { control, ..scout() }],
                        next: None,
                    })
                }
                q => panic!("unexpected query: {q:?}"),
            })
        });
    };
    suspended(&cx, identity::Life::Suspended { keys: Vec::new() });
    cx.simulate_click("settings/agents/12/suspend");
    cx.run_until_parked();
    assert_eq!(sent(&cx), 2);
    assert!(
        cx.has_text("Scout: Agent · managed by Maya · account 12 · 0 keys · suspended"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("settings/agents/12/resume").is_some());
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let logged = log.clone();
    cx.host().handle::<Submit<Identity>>(move |op| {
        logged.borrow_mut().push(op);
        Ok(Vec::new())
    });
    cx.simulate_click("settings/agents/12/revoke");
    cx.run_until_parked();
    assert_eq!(sent(&cx), 2, "the first press only asks");
    assert!(cx.has_text("Revoking is final: this agent account never acts again."));
    assert!(cx.has_text("Revoke for good"));
    // another action drops the question
    cx.simulate_click("settings/agents/12/resume");
    cx.run_until_parked();
    assert!(!cx.has_text("Revoke for good"));
    cx.simulate_click("settings/agents/12/revoke");
    cx.run_until_parked();
    assert!(cx.has_text("Revoke for good"));
    suspended(&cx, identity::Life::Revoked);
    cx.simulate_click("settings/agents/12/revoke");
    cx.run_until_parked();
    assert_eq!(
        *log.borrow(),
        [
            identity::Op::Resume { account: 12 },
            identity::Op::Revoke { account: 12 },
        ]
    );
    assert!(cx.has_text("Scout: Agent · managed by Maya · account 12 · 0 keys · revoked"));
    for action in ["rename", "suspend", "resume", "revoke"] {
        assert!(cx.find(&format!("settings/agents/12/{action}")).is_none());
    }
    cx.assert_accessible();
}

/// Identity's live heads re-read the account in place: a rename made
/// elsewhere shows without the account leaving the screen first.
#[test]
fn an_identity_head_re_reads_the_account_in_place() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ClockTicks>();
    cx.host().stream::<Changes<Valset>>();
    let heads = cx.host().stream::<Changes<Identity>>();
    let props = cx.host().stream::<HostSession>();
    respond(&cx);
    cx.open::<Settings>();
    props.send(Session {
        signer: "abcd".into(),
        account: Some(7),
        ..Session::default()
    });
    cx.run_until_parked();
    assert!(cx.has_text("Who I am: Maya · account 7"));
    cx.host().handle::<Query<Identity>>(|q| {
        Ok(match q {
            identity::Query::Get { number } => {
                let mut renamed = maya(number, "Laptop key");
                renamed.card.name = "Maya R".into();
                identity::Reply::Account(Some(renamed))
            }
            identity::Query::Managed { .. } => identity::Reply::Accounts(identity::PageResponse {
                height: 43,
                items: vec![],
                next: None,
            }),
            q => panic!("unexpected query: {q:?}"),
        })
    });
    heads.send(Some(43));
    cx.run_until_parked();
    assert!(
        cx.has_text("Who I am: Maya R · account 7"),
        "{:?}",
        cx.texts()
    );
    assert!(!cx.has_text("Reading your account…"));
}
