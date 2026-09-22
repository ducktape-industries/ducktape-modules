use super::*;
use ducktape_view_guest::{caps::QueryBytes, testing::TestAppContext};

fn status() -> Status {
    Status {
        network: "Workshop".into(),
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
fn respond(cx: &TestAppContext) {
    cx.host().handle::<NodeStatus>(|()| Ok(status()));
    cx.host().handle::<QueryBytes<Identity>>(|q| {
        Ok(match q {
            identity::Query::OfKey { key } => {
                assert_eq!(key, vec![0xab, 0xcd]);
                identity::Reply::OfKey(Some(7))
            }
            identity::Query::Get { number } => {
                assert_eq!(number, 7);
                identity::Reply::Get(Some(identity::Account {
                    number,
                    name: "Maya".into(),
                    control: identity::Control::Keys(vec![identity::Key {
                        scheme: abi::Scheme::Ed25519,
                        key: vec![0xab, 0xcd],
                        label: Some("Laptop key".into()),
                        added_at: 1,
                    }]),
                    avatar: None,
                    bio: None,
                    updated_at: 1,
                }))
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
    cx.host().handle::<QueryBytes<Valset>>(|q| {
        Ok(match q {
            valset::Query::Membership { key } => {
                valset::Reply::Membership(Some(valset::Membership {
                    key,
                    address: "127.0.0.1:19001".into(),
                    standing: valset::Standing::Validator,
                }))
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
}
fn fixture(state: &str, dark: bool) -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Ticks>();
    cx.host().stream::<Live>();
    let props = cx.host().stream::<Props>();
    respond(&cx);
    match state {
        "unregistered" => cx.host().handle::<QueryBytes<Identity>>(|q| {
            assert!(matches!(q, identity::Query::OfKey { .. }));
            Ok(identity::Reply::OfKey(None))
        }),
        "loading" => {
            cx.host().never::<NodeStatus>();
            cx.host().never::<QueryBytes<Identity>>();
        }
        "refused" => {
            cx.host()
                .refuse::<NodeStatus>("unavailable", "The node is unavailable. Try again.");
            cx.host()
                .refuse::<QueryBytes<Identity>>("unavailable", "Account query refused.");
        }
        _ => {}
    }
    cx.set_global(if dark { Theme::dark() } else { Theme::light() });
    cx.open::<Settings>();
    props.push(Session {
        account: if state == "empty" {
            String::new()
        } else {
            "abcd".into()
        },
        dark,
        endpoint: "http://127.0.0.1:19001".into(),
    });
    cx.run_until_parked();
    if state.starts_with("invite") {
        match state {
            "invite-loading" => cx.host().never::<MintInvite>(),
            "invite-refused" => cx
                .host()
                .refuse::<MintInvite>("forbidden", "This node does not allow minting invites."),
            _ => cx.host().handle::<MintInvite>(|request| {
                assert_eq!(request.ttl_days, 7);
                Ok(Invite {
                    invite: "duck-invite:workshop-loopback-example".into(),
                    notes: vec![Note {
                        reason: "expires".into(),
                        sentence: "This invite expires in 7 days.".into(),
                    }],
                })
            }),
        }
        cx.simulate_click("settings/invite/mint");
        cx.run_until_parked();
    }
    cx
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
    cx.host().handle::<MintInvite>(|r| {
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
    let live = cx.host().stream::<Ticks>();
    cx.host().stream::<Live>();
    cx.host().stream::<Props>();
    respond(&cx);
    cx.open::<Settings>();
    cx.host().handle::<NodeStatus>(|()| {
        let mut s = status();
        s.height = 43;
        Ok(s)
    });
    live.push(());
    cx.run_until_parked();
    assert!(cx.has_text("Height / epoch: 43 / 3"));
    let snapshot = cx.snapshot().unwrap();
    cx.restore::<Settings>(&snapshot).unwrap();
    cx.run_until_parked();
    assert!(cx.has_text("Height / epoch: 43 / 3"));
    let mut cx = fixture("refused", false);
    respond(&cx);
    cx.simulate_click("settings/node/retry");
    cx.run_until_parked();
    assert!(cx.has_text("Network: Workshop"));
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
