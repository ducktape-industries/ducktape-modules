//! Every screen of this view, replayed from the forge program's own bytes.
//!
//! `crates/app/forge/fixtures/replies.bin` holds the program's real `Respond`
//! bytes; nothing here builds a reply by hand. The fake host decodes one and
//! hands it back, so an unhandled ask is a panic and a screen that reads a
//! field the program does not send cannot compile.
use super::*;
use crate::api::{Ask, Props, Session, SubmitForge};
use crate::state::{ChangeTab, Filter, RepoTab};
use ducktape_view_guest::doors::Id;
use ducktape_view_guest::doors::{Query as Door, Submit};
use ducktape_view_guest::testing::TestAppContext;
use ducktape_view_guest::{Entity, Theme, wire};
use forge::{ChangeFilter, ChangeState, Op, Query, Reply};

use crate::api::ChatApi;

#[path = "../../forge/fixtures/loader.rs"]
mod loader;

/// One committed fixture, exactly as the program answered it.
pub(crate) fn bytes(name: &str) -> Vec<u8> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../forge/fixtures");
    loader::bytes(&dir, name)
}

fn reply(name: &str) -> Reply {
    borsh::from_slice(&bytes(name)).unwrap_or_else(|error| panic!("decode {name}: {error}"))
}

/// Which change record the program is holding in a given scenario.
fn change_fixture(mode: &str) -> &'static str {
    match mode {
        "reviewed" | "review" => "change-reviewed",
        "merged" => "change-merged",
        "closed" => "change-closed",
        "outdated" => "change-outdated",
        _ => "change",
    }
}

/// The program's answers, chosen by query shape — never by hand.
fn answer(query: &Query, mode: &str) -> Reply {
    match query {
        Query::Repos { .. } if mode == "empty" => reply("repos-empty"),
        Query::Repos { .. } => reply("repos"),
        Query::Repo { .. } => reply("repo"),
        Query::Refs { cursor: None, .. } if mode == "unborn" => reply("refs-empty"),
        Query::Refs { cursor: None, .. } => reply("refs"),
        Query::Refs { .. } => reply("refs-empty"),
        Query::Activity { .. } => reply("activity"),
        Query::Tree {
            cursor: None, path, ..
        } if path.is_empty() => reply("tree"),
        Query::Tree { .. } => reply("tree-directory"),
        Query::Blob { oid, .. } => match oid.as_str() {
            "95d586e774a04676a07a142f0e2f97a4f32562cb" => reply("blob-binary"),
            "b90a09e55e43808905fe881245853c1b35b3fb82" => reply("blob-oversize"),
            _ => reply("blob"),
        },
        Query::Log { cursor: None, .. } => reply("log"),
        Query::Log { .. } => reply("log-next"),
        Query::Diff { .. } if mode == "binary" => reply("diff-binary"),
        Query::Diff { .. } => reply("diff-text"),
        Query::Compare { .. } if mode == "diverged" => reply("compare-diverged"),
        Query::Compare { .. } => reply("compare"),
        Query::Changes {
            filter:
                ChangeFilter {
                    state: Some(ChangeState::Closed),
                    ..
                },
            ..
        } => reply("changes-filtered"),
        Query::Changes { .. } if mode == "empty" => reply("changes-empty"),
        Query::Changes { .. } => reply("changes"),
        Query::Change { cursor: None, .. } => reply(change_fixture(mode)),
        Query::Change { .. } => reply("change-reviews-next"),
        Query::Judgment { .. } if mode == "judgment-empty" => reply("judgment-empty"),
        Query::Judgment { .. } => reply("judgment"),
        other => panic!("no fixture for {other:?}"),
    }
}

fn accounts() -> Vec<chat::AccountRow> {
    vec![
        chat::AccountRow {
            number: 7,
            name: "Ada".into(),
            program: false,
            keys: vec![abi::hex(b"tester")],
        },
        chat::AccountRow {
            number: 8,
            name: "Rae".into(),
            program: false,
            keys: vec![abi::hex(b"reviewer")],
        },
        chat::AccountRow {
            number: 9,
            name: "Wren".into(),
            program: false,
            keys: vec![abi::hex(b"writer")],
        },
    ]
}

fn message(seq: u64, author: &str, text: &str) -> chat::MsgRow {
    chat::MsgRow {
        channel_id: "forge:project:1".into(),
        seq,
        message_id: format!("m{seq}"),
        author: author.into(),
        blocks: vec![chat::Block::paragraph(text)],
        text: text.into(),
        ..chat::MsgRow::default()
    }
}

pub(crate) fn configure(cx: &mut TestAppContext, mode: &'static str) {
    cx.host().handle::<Ask>(move |query| {
        if mode == "refused" && !matches!(query, Query::Repos { .. }) {
            return Ok(reply("refused-not-found"));
        }
        Ok(answer(&query, mode))
    });
    cx.host().handle::<Door<ChatApi>>(|query| {
        Ok(match query {
            chat::ChatViewQuery::Accounts { .. } => chat::ChatViewReply::Accounts(accounts()),
            chat::ChatViewQuery::Roots { channel_id, .. } => chat::ChatViewReply::Roots {
                roots: if channel_id == "forge:project:1" {
                    vec![
                        message(1, "system", "Ada opened this change"),
                        message(2, "acct:8", "Reading it now"),
                    ]
                } else {
                    Vec::new()
                },
                has_more: false,
                next_before_seq: None,
            },
            other => panic!("unexpected chat query: {other:?}"),
        })
    });
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));
    cx.host().handle::<SubmitForge>(|_| Ok(Vec::new()));
    cx.host().handle::<Id>(|kind| Ok(format!("{kind}-1")));
    cx.host().never::<Live>();
    cx.host().never::<Visible>();
    // The host hands every view the seated key as raw hex, never a handle:
    // resolve it the way identity itself would. `reviewer`'s hex is account
    // 8's own key, matching the roster above; any other key holds none.
    cx.host().handle::<Door<identity::view::Identity>>(|query| {
        Ok(match query {
            identity::Query::OfKey { key } if key == b"reviewer" => {
                identity::Reply::Number(Some(8))
            }
            identity::Query::OfKey { .. } => identity::Reply::Number(None),
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
}

/// Boots the view, seats a reader and waits for the first reads to land.
pub(crate) fn booted(mode: &'static str) -> (TestAppContext, Entity<Forge>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx, mode);
    let props = cx.host().stream::<Props>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.push(Session {
        account: abi::hex(b"reviewer"),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    (cx, view)
}

/// Boots and opens `project`.
pub(crate) fn opened(mode: &'static str) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = booted(mode);
    cx.simulate_click("forge-repo-project");
    cx.run_until_parked();
    (cx, view)
}

fn disabled(cx: &TestAppContext, id: &str) -> bool {
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        interactivity, ..
    })) = cx.find(id)
    else {
        panic!("{id} button")
    };
    interactivity.aria.disabled == Some(true) && interactivity.on_click.is_none()
}

#[test]
fn session_key_resolves_to_its_account() {
    let (_cx, view) = booted("default");
    view.read(|forge| {
        assert_eq!(forge.my_account(), Some(8));
        assert_eq!(forge.me_key(), Some(b"reviewer".to_vec()));
    });
}

#[test]
fn an_unregistered_key_stays_read_only() {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    let props = cx.host().stream::<Props>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.push(Session {
        account: abi::hex(b"stranger"),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.my_account(), None);
        assert!(forge.me_key().is_none());
    });
    cx.simulate_click("forge-repo-project");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    assert!(disabled(&cx, "forge-filter-judgment"));
}

/// The reader creates the account in Settings, then switches to Forge: the
/// seated key never changes, only identity's own state does, so this has to
/// arrive over identity's live stream, folded into the same reconcile every
/// forge/chat block already runs through.
#[test]
fn an_account_gained_later_re_enables_writes() {
    let registered = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    let reply = registered.clone();
    cx.host()
        .handle::<Door<identity::view::Identity>>(move |query| {
            Ok(match query {
                identity::Query::OfKey { key } if key == b"reviewer" => {
                    identity::Reply::Number(reply.get().then_some(8))
                }
                identity::Query::OfKey { .. } => identity::Reply::Number(None),
                query => panic!("unexpected identity query: {query:?}"),
            })
        });
    let props = cx.host().stream::<Props>();
    let live = cx.host().stream::<Live>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.push(Session {
        account: abi::hex(b"reviewer"),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    assert!(disabled(&cx, "forge-filter-judgment"));

    registered.set(true);
    live.push(Some(1));
    cx.run_until_parked();

    assert!(!disabled(&cx, "forge-filter-judgment"));
    view.read(|forge| assert_eq!(forge.my_account(), Some(8)));
}

#[test]
fn preferred_window_is_the_one_the_plan_asks_for() {
    assert_eq!(<Forge as View>::PREFERRED_WINDOW_SIZE, "1180,760");
}

#[test]
fn the_root_wears_the_shared_theme_and_is_accessible() {
    let (mut cx, _) = booted("default");
    let dark = Theme::dark();
    cx.set_global(dark);
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode { style, .. })) =
        cx.find("forge")
    else {
        panic!("the forge root is a styled container");
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
    cx.assert_accessible();
}

#[test]
fn the_repositories_list_shows_every_column_of_the_plan() {
    let (cx, _) = booted("default");
    assert!(cx.has_text("Repositories"));
    assert!(cx.has_text("project"), "{:?}", cx.texts());
    assert!(cx.has_text("main"), "the default head is a chip");
    assert!(cx.has_text("Ada"), "the owner key resolves to a name");
    assert!(cx.has_text("6 refs"));
    assert!(cx.has_text("height 2"));
}

#[test]
fn an_empty_program_explains_how_a_repository_begins() {
    let (cx, _) = booted("empty");
    assert!(cx.has_text("No repositories yet"));
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("git push duck://"))
    );
}

#[test]
fn an_unborn_repo_says_so_instead_of_resolving_forever() {
    let (cx, _) = opened("unborn");
    assert!(cx.has_text("No commits yet"), "{:?}", cx.texts());
    assert!(!cx.has_text("Resolving the ref…"));
}

#[test]
fn a_refused_read_keeps_its_reason_and_offers_one_retry() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<Ask>(|_| Ok(reply("refused-object-not-held")));
    cx.host()
        .handle::<Door<ChatApi>>(|_| Ok(chat::ChatViewReply::Accounts(accounts())));
    cx.host().never::<Live>();
    cx.host().never::<Visible>();
    cx.host().never::<Props>();
    cx.open::<Forge>();
    cx.run_until_parked();
    let sentence = "object ffffffffffffffffffffffffffffffffffffffff is not held by this node";
    assert!(cx.has_text(sentence), "{:?}", cx.texts());
    assert!(cx.find("forge-repos-list-retry").is_some());
    cx.simulate_click("forge-repos-list-retry");
    cx.run_until_parked();
    assert!(cx.has_text(sentence));
}

#[test]
fn creating_a_repository_validates_its_name_then_shows_the_submission() {
    let (mut cx, view) = booted("default");
    cx.simulate_click("forge-new-repo");
    assert!(cx.has_text("New repository"));
    cx.simulate_input("forge-new-repo-name", "not a name");
    cx.simulate_click("forge-new-repo-submit");
    cx.run_until_parked();
    assert!(
        cx.has_text("A repository name is 1–37 bytes of letters, digits, dot, dash or underscore"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().asked::<SubmitForge>().is_empty());
    cx.simulate_input("forge-new-repo-name", "ledger");
    cx.simulate_click("forge-new-repo-sha256");
    cx.simulate_click("forge-new-repo-submit");
    cx.run_until_parked();
    assert!(cx.host().asked::<SubmitForge>().iter().any(|op| matches!(
        op,
        Op::Create { repo, hash } if repo == "ledger" && *hash == abi::HashKind::Sha256
    )));
    view.read(|forge| assert!(forge.new_repo.is_none()));
}

#[test]
fn a_repository_opens_on_code_with_its_header_ref_picker_and_tabs() {
    let (cx, view) = opened("default");
    view.read(|forge| assert_eq!(forge.nav().repo.as_deref(), Some("project")));
    assert!(cx.has_text("owner Ada"));
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("duck://testnet#0a1b2c3d/forge/project")),
        "{:?}",
        cx.texts()
    );
    for tab in RepoTab::ALL {
        assert!(
            cx.find(&format!("forge-tab-{}", tab.slug())).is_some(),
            "{} tab",
            tab.label()
        );
    }
    // The ref picker carries every ref the paged read followed.
    assert!(cx.find("forge-ref-refs/heads/clean").is_some());
    assert!(cx.find("forge-ref-refs/heads/conflict").is_some());
}

#[test]
fn the_about_panel_docks_what_the_repo_record_carries() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-dock-about");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().dock, Some(crate::state::Dock::About)));
    assert!(cx.has_text("sha1"), "{:?}", cx.texts());
    assert!(cx.has_text("64 bytes"), "the founded inline blob bound");
    assert!(cx.has_text("Wren"), "the granted writer");
    cx.simulate_click("forge-dock-close");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().dock.is_none()));
}

#[test]
fn code_reads_the_tree_then_one_file_and_says_what_it_cannot_show() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.head_name(), b"refs/heads/clean".to_vec()));
    assert!(cx.has_text("README.md"), "{:?}", cx.texts());
    assert!(cx.has_text("empty.txt"));
    // The root tree carries a README, so it is what the pane shows.
    assert!(cx.find("forge-readme-body").is_some());
    cx.simulate_input("forge-tree-search", "empty");
    cx.run_until_parked();
    assert!(
        cx.find("forge-tree-README.md").is_none(),
        "the filter drops the row"
    );
    cx.simulate_input("forge-tree-search", "");
    cx.simulate_click("forge-tree-README.md");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().blob.is_some()));
    assert!(cx.find("forge-blob-lines").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("one"), "the first source line is drawn");
    cx.simulate_click("forge-blob-close");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().blob.is_none()));
}

#[test]
fn an_oversize_blob_is_a_header_not_a_body() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.run_until_parked();
    view.update(&mut cx, |forge, _, cx| {
        forge.open_file(
            b"large.txt".to_vec(),
            "b90a09e55e43808905fe881245853c1b35b3fb82".into(),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.has_text("Too large to show"), "{:?}", cx.texts());
    assert!(cx.find("forge-blob-lines").is_none());
    view.update(&mut cx, |forge, _, cx| {
        forge.open_file(
            b"image.bin".to_vec(),
            "95d586e774a04676a07a142f0e2f97a4f32562cb".into(),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.has_text("Binary file"));
}

#[test]
fn commits_follows_the_cursor_and_opens_one_commit_with_its_diff() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    // `log` carries a next cursor; the second page is the root commit.
    let asked = cx.host().asked::<Ask>();
    assert!(
        asked.iter().any(|query| matches!(
            query,
            Query::Log {
                cursor: Some(_),
                ..
            }
        )),
        "the log follows its cursor"
    );
    view.read(|forge| {
        let Some(Reply::Log { page, .. }) = forge.ready(&Query::Log {
            repo: "project".into(),
            from: forge.revision(),
            cursor: None,
            limit: crate::queries::PAGE,
        }) else {
            panic!("the log landed");
        };
        assert_eq!(page.items.len(), 2, "both pages are one list");
        assert!(page.next.is_none());
    });
    assert!(cx.has_text("Feature"), "{:?}", cx.texts());
    cx.simulate_click("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
    cx.run_until_parked();
    assert!(
        cx.has_text("Feature\n\nReview these bytes.\n"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("ebfb8b62"), "the parent is named");
    assert!(
        cx.find("forge-commit-diff-file-src/lib.rs").is_some(),
        "a commit's diff names its files above the rows"
    );
    assert!(cx.find("forge-commit-diff").is_some());
    cx.simulate_click("forge-commit-close");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().commit.is_none()));
}

#[test]
fn refs_carry_their_distance_from_the_default_head_and_open_a_draft() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    assert!(cx.has_text("clean"), "{:?}", cx.texts());
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("1 ahead") && text.contains("fast-forward")),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("forbids force pushes and ref deletions"))
    );
    cx.simulate_click("forge-compare-clean");
    cx.run_until_parked();
    view.read(|forge| {
        let form = forge.form.as_ref().expect("a change draft");
        assert_eq!(form.from, b"refs/heads/clean".to_vec());
        assert_eq!(form.into, b"refs/heads/main".to_vec());
    });
    assert!(cx.has_text("New change"));
}

#[test]
fn settings_shows_only_what_the_contract_exposes_and_grants_by_account() {
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-tab-settings");
    cx.run_until_parked();
    assert!(cx.has_text("Allow force pushes"));
    assert!(cx.has_text("Allow ref deletion"));
    assert!(cx.has_text("Wren"), "the granted writer resolves to a name");
    cx.simulate_click("forge-settings-force");
    cx.simulate_click("forge-settings-head-clean");
    cx.simulate_click("forge-settings-save");
    cx.run_until_parked();
    assert!(cx.host().asked::<SubmitForge>().iter().any(|op| matches!(
        op,
        Op::Configure { repo, settings }
            if repo == "project" && settings.allow_force && settings.head == b"refs/heads/clean"
    )));
    cx.simulate_input("forge-settings-grant-input", "acct:7");
    cx.simulate_click("forge-settings-grant");
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<SubmitForge>()
            .iter()
            .any(|op| matches!(op, Op::Grant { key, .. } if key == b"tester"))
    );
    cx.simulate_click(&format!("forge-settings-revoke-{}", abi::hex(b"writer")));
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<SubmitForge>()
            .iter()
            .any(|op| matches!(op, Op::Revoke { key, .. } if key == b"writer"))
    );
}

#[test]
fn the_narrow_window_folds_the_rail_and_the_dock_into_toggles() {
    let (mut cx, view) = opened("default");
    cx.simulate_measure("forge-viewport", 720., 600.);
    cx.run_until_parked();
    view.read(|forge| assert!(forge.layout.narrow()));
    assert!(cx.find("forge-toggle-rail").is_some());
    assert!(cx.find("forge-rail").is_none(), "the rail folds away");
    cx.simulate_click("forge-toggle-rail");
    cx.run_until_parked();
    assert!(cx.find("forge-rail").is_some());
}

#[test]
fn a_snapshot_restores_the_same_screen_without_replaying_events() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    view.update(&mut cx, |forge, _, cx| {
        forge.open_change_tab(ChangeTab::Files, cx);
        forge.toggle_viewed(b"src/lib.rs", cx);
    });
    cx.run_until_parked();
    let snapshot = cx.snapshot().unwrap();

    let mut restored = TestAppContext::new();
    configure(&mut restored, "default");
    restored.host().never::<Props>();
    let view = restored.restore::<Forge>(&snapshot).unwrap();
    restored.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.nav().repo.as_deref(), Some("project"));
        assert_eq!(forge.nav().change, Some(1));
        assert_eq!(forge.nav().change_tab, ChangeTab::Files);
        assert!(forge.viewed.contains("project#1:src/lib.rs"));
    });
    assert!(
        !restored.host().asked::<Ask>().is_empty(),
        "a restored view reads again"
    );
}

#[test]
fn judgment_is_its_own_query_keyed_by_the_readers_own_key() {
    let (mut cx, view) = opened("judgment");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.filter, Filter::Judgment));
    assert!(
        cx.host()
            .asked::<Ask>()
            .iter()
            .any(|query| matches!(query, Query::Judgment { key, .. } if key == b"reviewer")),
        "the reader's signing key comes from the roster"
    );
    assert!(cx.has_text("review requested"), "{:?}", cx.texts());
}

#[test]
fn an_empty_judgment_says_nothing_waits_on_you() {
    let (mut cx, _) = opened("judgment-empty");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    assert!(cx.has_text("Nothing waits on you"), "{:?}", cx.texts());
}

/// Opens change #1 of `project` on one of its tabs.
pub(crate) fn change_screen(mode: &'static str, tab: ChangeTab) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = opened(mode);
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    cx.simulate_click(&format!("forge-change-tab-{}", tab.slug()));
    cx.run_until_parked();
    (cx, view)
}

#[path = "change_tests.rs"]
mod change_tests;

#[path = "screen_tests.rs"]
mod screen_tests;
