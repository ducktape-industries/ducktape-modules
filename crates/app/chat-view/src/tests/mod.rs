//! The view against a fake host: every door it asks is answered here, and
//! each test drives the frame the way a person would.
use super::*;
use chat::{ChannelInfo, MessageHits, MsgRow, Op, Party, PostPolicy, Query, Reply};
use ducktape_view_guest::testing::TestAppContext;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::wire;
use ducktape_view_guest::{Entity, StyleRefinement, Styled};

use crate::api::{Ask, ChatApi, HostId, HostProps, HostVisible, RpcLive, Session, Submit};

mod menus;
mod message;
mod notices;
mod reader;
mod rich;
mod room;
mod writing;

fn page<T>(items: Vec<T>) -> ::chat::PageReply<T> {
    ::chat::PageReply {
        height: 1,
        items,
        next: None,
    }
}

fn channel(id: &str, name: &str, head_seq: u64) -> ChannelInfo {
    ChannelInfo {
        channel: ::chat::ChannelRow {
            id: id.into(),
            name: name.into(),
            created_at: 0,
            post_policy: PostPolicy::Open,
            owner: Party::Account(7),
            archived: false,
            huddle: Vec::new(),
            voice: false,
        },
        head_seq,
    }
}

#[test]
fn preferred_window_keeps_the_original_baseline() {
    assert_eq!(<Chat as View>::PREFERRED_WINDOW_SIZE, "1180,760");
}

#[test]
fn the_root_tracks_the_shared_theme_and_is_accessible() {
    let (mut cx, _) = opened();
    let dark = ducktape_view_guest::Theme::dark();
    cx.set_global(dark);
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode { style, .. })) =
        cx.find("chat-root")
    else {
        panic!("chat root is a styled container");
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

fn row(seq: u64, author: u64, text: &str) -> MsgRow {
    MsgRow {
        channel_id: "general".into(),
        seq,
        message_id: format!("m{seq}"),
        author: Party::Account(author),
        height: 1,
        blocks: vec![chat::Block::paragraph(text)],
        text: text.into(),
        ..MsgRow::default()
    }
}

/// The host doors chat only talks to, never hears back from here.
fn quiet_doors(cx: &mut TestAppContext) {
    cx.host().never::<api::HostRoute>();
    cx.host().never::<ducktape_view_guest::doors::HostBadge>();
    cx.host().never::<ducktape_view_guest::doors::NotifyPost>();
    cx.host().never::<ducktape_view_guest::doors::NotifyRead>();
    cx.host()
        .handle::<ducktape_view_guest::doors::StoreGet>(|_| Ok(None));
    cx.host().never::<ducktape_view_guest::doors::StoreSet>();
}

fn configure(cx: &mut TestAppContext) {
    quiet_doors(cx);
    cx.host()
        .handle::<ducktape_view_guest::doors::HostWidget>(|command| {
            assert!(matches!(command, wire::WidgetCommand::Focus { .. }));
            Ok(())
        });
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(vec![
                chat::AccountRow {
                    number: 7,
                    name: "eddy".into(),
                    program: false,
                    keys: vec!["0102".into()],
                },
                chat::AccountRow {
                    number: 8,
                    name: "reviewer".into(),
                    program: true,
                    keys: Vec::new(),
                },
            ]),
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 3),
                channel("dm-7-8", "dm", 1),
                channel("forge:web:3", "Review web#3", 2),
            ])),
            Query::Roots { channel_id, .. } => Reply::Roots(page(if channel_id == "general" {
                vec![row(1, 7, "hello"), row(2, 8, "**hi** there")]
            } else {
                Vec::new()
            })),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            Query::Thread { .. } => Reply::Thread {
                root: None,
                replies: page(Vec::new()),
            },
            Query::Search { text, .. } => {
                assert_eq!(text, "hello");
                Reply::Hits(MessageHits {
                    hits: vec![row(1, 7, "hello")],
                    capped: false,
                })
            }
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().never::<RpcLive>();
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));
    // The host hands every view the seated key as raw hex, never a handle:
    // resolve it the way identity itself would. "0102" is account 7's own
    // key, matching the roster above; any other key holds no account.
    cx.host().handle::<Ask<identity::view::Identity>>(|query| {
        Ok(match query {
            identity::Query::OfKey { key } if key == [0x01, 0x02] => {
                identity::Reply::Number(Some(7))
            }
            identity::Query::OfKey { .. } => identity::Reply::Number(None),
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
}

/// Boots, seats a reader, lists rooms and opens `general` with two rows.
fn opened() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<HostProps>();
    let visible = cx.host().stream::<HostVisible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    assert!(cx.has_text("Not connected"));
    props.push(Session {
        account: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    assert!(cx.has_text("General"));
    assert!(cx.has_text("No channel open"));
    assert!(cx.has_text("Channels"));
    assert!(
        !cx.has_text("Review web#3"),
        "a program's rooms stay out of the channel list"
    );
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        style,
        interactivity,
        ..
    })) = cx.find("chat-sidebar-channel-general")
    else {
        panic!("channel row is a native container");
    };
    assert!(style.size.width.is_some(), "channel rows fill the sidebar");
    assert_eq!(interactivity.role, Some(ducktape_view_guest::Role::Button));
    assert_eq!(interactivity.aria.selected, Some(true));
    assert!(
        interactivity.on_click.is_some(),
        "channel rows keep their route"
    );
    (cx, view)
}

/// `CHAT_SCREEN_EXPORT=1` writes the opened room's tree for the app's
/// node-less renderer (`ducktape-app --render-tree`), light and dark.
#[test]
fn export_chat_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let (cx, _view) = opened();
    std::fs::write(
        out.join("room-light.json"),
        serde_json::to_vec(cx.root()).unwrap(),
    )
    .unwrap();
}
