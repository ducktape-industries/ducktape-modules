use super::*;

/// A module's account comes from the kernel's `RegisterModule`, which no
/// transaction targets: identity's own live heads, not the window's
/// transactions, re-read the accounts.
#[test]
fn an_identity_head_re_reads_the_accounts() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let heads = cx.host().stream::<Changes<Identity>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert!(!cx.has_text("Module · chess"));
    let asked = cx.host().requests::<Query<Identity>>().len();
    let chess = account(
        8,
        "chess",
        identity::Control::Module {
            module: "chess".into(),
        },
    );
    cx.host().handle::<Query<Identity>>(move |_| {
        Ok(identity::Reply::Accounts(identity::PageResponse {
            height: 13,
            items: vec![ada(), scout(), forge(), chess.clone()],
            next: None,
        }))
    });
    heads.send(Some(13));
    cx.run_until_parked();
    assert!(cx.has_text("Module · chess"), "{:?}", cx.texts());
    assert!(cx.has_text("Agent · managed by Ada · suspended"), "kept");
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), asked + 1);
}

#[test]
fn a_registry_head_re_reads_the_programs() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let heads = cx.host().stream::<Changes<Registry>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("2 programs"));
    cx.host().handle::<Query<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => registry::Reply::Programs(vec![
                entry("chat", 0xab),
                entry("identity", 0xcd),
                entry("chess", 0x11),
            ]),
            registry::Query::Views(0) => registry::Reply::Views(Vec::new()),
            registry::Query::Scheduled { .. } => {
                registry::Reply::Scheduled(registry::PageResponse {
                    height: 13,
                    items: Vec::new(),
                    next: None,
                })
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
    heads.send(Some(13));
    cx.run_until_parked();
    assert!(
        cx.has_text("3 programs") && cx.has_text("chess"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Nothing is scheduled against the registry."));
}

#[test]
fn an_empty_registry_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host().handle::<Query<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => registry::Reply::Programs(Vec::new()),
            registry::Query::Views(0) => registry::Reply::Views(Vec::new()),
            registry::Query::Scheduled { .. } => {
                registry::Reply::Scheduled(registry::PageResponse {
                    height: 1,
                    items: Vec::new(),
                    next: None,
                })
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("No programs"), "{:?}", cx.texts());
    cx.assert_accessible();
}

#[test]
fn refused_accounts_say_why_and_retry_reads_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert!(cx.has_text("identity is not running here"));
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.simulate_click("explorer-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Module · forge"), "{:?}", cx.texts());
}
