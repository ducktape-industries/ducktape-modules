use super::super::editing;
use super::editor::key_tag;
use super::*;
use wire::keyboard::{Key, Modifiers, Named};

/// Every node under `node`, itself first.
fn walk(node: &wire::Node, seen: &mut impl FnMut(&wire::Node)) {
    seen(node);
    match node {
        wire::Node::Linear { children, .. } => {
            for child in children {
                walk(child, seen);
            }
        }
        wire::Node::Container { content, .. } => walk(content, seen),
        wire::Node::Button {
            content: wire::ButtonContent::Child(child),
            ..
        } => walk(child, seen),
        _ => {}
    }
}

fn drawn(draft: &Draft) -> wire::Node {
    let mut app = crate::App::new(false);
    let entity = crate::Entity::reserve(&app);
    let mut cx = Context {
        app: &mut app,
        entity,
    };
    view(
        draft,
        "c",
        "Message #general",
        true,
        true,
        &[],
        &mut cx,
        |_: &mut (), _, _, _| (),
    )
}

/// ONE PLACE ON SCREEN, ONE DOCUMENT PER DRAFT. The host keys its native
/// editor state by the document id, not by the node key, so two drafts
/// presented at the same key under one id are one document to the host:
/// it hands the second draft the first's text and then drops every
/// transaction, because the guest's `before` never matches. The node key
/// is what the accessibility tree and every test door address, so it must
/// NOT move when the document does.
#[test]
fn two_drafts_at_one_key_are_two_documents_the_host_can_tell_apart() {
    let field = |draft: &Draft, document: &str| {
        let wire::Node::Editor { key, document, .. } = editor(
            draft,
            "c/editor",
            document,
            "Message",
            true,
            &[],
            Rc::new(|_: &mut (), _, _, _| ()),
        ) else {
            panic!("the composer's field is an editor node");
        };
        (key, document.document)
    };
    let (a_key, a_document) = field(&Draft::from_body("room a draft", &[]), "chat\u{1f}room-a");
    let (b_key, b_document) = field(&Draft::default(), "chat\u{1f}room-b");
    assert_eq!(a_key, b_key, "the field keeps its place and its name");
    assert_ne!(
        a_document, b_document,
        "two drafts the host must not share text between"
    );
    assert_eq!(a_document, "chat\u{1f}room-a");
    assert_eq!(b_document, "chat\u{1f}room-b");
}

/// The composer's shape is a claim a reader can see at a glance: ONE
/// action is the action, and it is dead until there is something to
/// send. Six identical buttons in a row is the shape this replaced.
#[test]
fn the_send_is_the_only_primary_and_is_dead_on_an_empty_draft() {
    let primaries = |draft: &Draft| {
        let mut found: Vec<(String, Option<u32>)> = Vec::new();
        walk(&drawn(draft), &mut |node| {
            if let wire::Node::Button {
                key,
                style,
                on_press,
                ..
            } = node
                && style.preset == wire::ButtonPreset::Primary
            {
                found.push((key.clone(), *on_press));
            }
        });
        found
    };
    let empty = primaries(&Draft::default());
    assert_eq!(empty.len(), 1, "one primary action, not six: {empty:?}");
    assert_eq!(empty[0].0, "c/send");
    assert!(empty[0].1.is_none(), "an empty draft cannot be sent");
    let typed = primaries(&Draft::from_body("hello", &[]));
    assert!(typed[0].1.is_some(), "a draft with words can be sent");
}

/// The marks are squares of one size. A mark that takes its size from
/// its glyph gives a toolbar of five different boxes.
#[test]
fn every_mark_is_the_same_square_and_the_field_writes_at_body_size() {
    let mut squares = 0;
    let mut body_size = None;
    walk(&drawn(&Draft::default()), &mut |node| match node {
        wire::Node::Button { width, height, .. }
            if *width == Some(wire::Length::Fixed(MARK))
                && *height == Some(wire::Length::Fixed(MARK)) =>
        {
            squares += 1;
        }
        wire::Node::Editor { options, .. } => body_size = options.size,
        _ => {}
    });
    // attach, bold, italic, code, quote
    assert_eq!(squares, 5, "five marks, all one square");
    assert_eq!(body_size, Some(kit::type_scale::BODY as f32));
}

#[test]
fn menu_navigation_commits_before_enter_chooses_a_stable_identity() {
    let choices = vec![
        MentionChoice {
            token: "<@1>".into(),
            label: "Ada".into(),
        },
        MentionChoice {
            token: "<@2>".into(),
            label: "Alan".into(),
        },
    ];
    let mut draft = Draft::from_body("@A", &choices);
    draft.editor.move_to(wire::EditorCursor {
        position: wire::EditorPosition { line: 0, column: 2 },
        selection: None,
    });
    let cursor = draft.editor.cursor();
    draft.committed("@A", "@A", cursor, "menu-next", &choices);
    let key = wire::keyboard::KeyState {
        key: Key::Named(Named::Enter),
        modifiers: Modifiers::default(),
        modified_key: Key::Named(Named::Enter),
        physical_key: wire::keyboard::Physical::Unidentified(
            wire::keyboard::NativeCode::Unidentified,
        ),
        location: wire::keyboard::Location::Standard,
    };
    assert_eq!(
        key_tag(&draft, &choices, draft.editor.state_view(), &key),
        "mention:<@2>"
    );
    draft.committed("@A", "@A", cursor, "menu-dismiss", &choices);
    assert_eq!(
        key_tag(&draft, &choices, draft.editor.state_view(), &key),
        "send"
    );
    draft.observed("@A", "@Al");
    assert!(!draft.menu_dismissed);
}

fn roster() -> Vec<MentionChoice> {
    vec![MentionChoice {
        token: "<@1>".into(),
        label: "Ada".into(),
    }]
}

/// The draft `body` reads, with the caret at byte `at` and nothing
/// selected — the state a person is in between keystrokes.
fn caret(body: &str, at: usize) -> Draft {
    let choices = roster();
    let mut draft = Draft::from_body(body, &choices);
    let text = draft.editor.text();
    draft.editor.move_to(wire::EditorCursor {
        position: editing::position(&text, at),
        selection: None,
    });
    draft
}

fn key_state(claim: &wire::EditorKeyClaim) -> wire::keyboard::KeyState {
    wire::keyboard::KeyState {
        key: claim.key.clone(),
        modifiers: Modifiers {
            control: claim.command,
            ..claim.modifiers
        },
        modified_key: claim.key.clone(),
        physical_key: wire::keyboard::Physical::Unidentified(
            wire::keyboard::NativeCode::Unidentified,
        ),
        location: wire::keyboard::Location::Standard,
    }
}

/// The keys this frame's field asks the host to route to the guest.
fn claimed(draft: &Draft) -> Vec<wire::EditorKeyClaim> {
    let node = editor(
        draft,
        "c",
        "c",
        "Message",
        true,
        &roster(),
        Rc::new(|_: &mut (), _, _, _| ()),
    );
    let wire::Node::Editor { options, .. } = node else {
        panic!("the composer's field is an editor node");
    };
    options
        .binding
        .expect("the field carries its binding")
        .claims
}

fn decision(draft: &Draft, claim: &wire::EditorKeyClaim) -> wire::EditorDecision {
    let choices = roster();
    let state = draft.editor.state_view();
    let tag = key_tag(draft, &choices, state, &key_state(claim));
    draft.decide(&tag, &choices, state)
}

fn bare(key: Named) -> wire::EditorKeyClaim {
    wire::EditorKeyClaim {
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        command: false,
    }
}

/// Escape with no menu open has nothing to dismiss. The host keeps it
/// (it is not claimed), and if a stale claim routes it here anyway the
/// answer is silence — not the native default, which stops the view.
#[test]
fn escape_with_no_menu_is_the_hosts_and_says_nothing_if_asked() {
    let draft = caret("hello", 5);
    assert!(!claimed(&draft).contains(&bare(Named::Escape)));
    assert_eq!(
        key_tag(
            &draft,
            &roster(),
            draft.editor.state_view(),
            &key_state(&bare(Named::Escape))
        ),
        "ignore"
    );
    assert!(matches!(
        decision(&draft, &bare(Named::Escape)),
        wire::EditorDecision::Noop
    ));
}

/// Cut with nothing selected cuts nothing — and says so itself, because
/// an installed app faults on a cut handed back as its own default.
#[test]
fn cut_with_nothing_selected_says_nothing() {
    let draft = caret("hello", 2);
    let cut = wire::EditorKeyClaim {
        key: Key::Character("x".into()),
        modifiers: Modifiers::default(),
        command: true,
    };
    assert_eq!(
        key_tag(
            &draft,
            &roster(),
            draft.editor.state_view(),
            &key_state(&cut)
        ),
        "cut"
    );
    assert!(matches!(decision(&draft, &cut), wire::EditorDecision::Noop));
}

/// Forward delete is the view's own work: one character ahead of the
/// caret, a whole mention when the caret sits at its edge (the rule
/// `expanded` already keeps for a selection), and nothing at the end.
#[test]
fn forward_delete_removes_what_is_ahead_of_the_caret() {
    let removed = |draft: &Draft| {
        let before = draft.editor.text();
        match decision(draft, &bare(Named::Delete)) {
            wire::EditorDecision::Apply {
                patches, cursor, ..
            } => Some(wire::patched_editor_text(&before, &patches, cursor).unwrap()),
            wire::EditorDecision::Noop => None,
            other => panic!("a delete never hands the key back: {other:?}"),
        }
    };
    assert_eq!(removed(&caret("hello", 2)).as_deref(), Some("helo"));
    // a character is not a byte
    assert_eq!(removed(&caret("héllo", 1)).as_deref(), Some("hllo"));
    // and what a person sees as one character goes as one
    assert_eq!(removed(&caret("a👨‍👩‍👧b", 1)).as_deref(), Some("ab"));
    // at the end there is nothing ahead to remove
    assert_eq!(removed(&caret("hello", 5)), None);
    // the mention goes whole, the same as a selection over it would
    let mut mention = Draft::from_body("Hi <@1> there", &roster());
    let at = mention.mentions[0].range.start;
    let text = mention.editor.text();
    mention.editor.move_to(wire::EditorCursor {
        position: editing::position(&text, at),
        selection: None,
    });
    assert_eq!(removed(&mention).as_deref(), Some("Hi  there"));
}

/// The arrows need a caret move through a layout this view does not
/// have, so they are the menu's keys while the menu is open and the
/// host's own the rest of the time.
#[test]
fn the_arrows_are_claimed_only_while_the_menu_is_open() {
    let closed = claimed(&caret("hello", 5));
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(
            !closed.contains(&bare(key)),
            "{key:?} is the host's while no menu is open"
        );
    }
    let open = claimed(&caret("@A", 2));
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(
            open.contains(&bare(key)),
            "{key:?} moves the open menu, so the menu claims it"
        );
    }
}

/// The whole defect in one assertion: an app installed today knows only
/// Enter, Tab and Backspace as native editor defaults and stops the view
/// on any other key handed back. So no claimed key but Tab and Backspace
/// may ever answer `DefaultEditorAction` — whatever the draft holds.
#[test]
fn no_claimed_key_but_tab_and_backspace_asks_the_app_for_its_default() {
    let drafts = [
        ("an empty draft", Draft::default()),
        ("words, nothing selected", caret("hello", 2)),
        ("the caret at the end", caret("hello", 5)),
        ("an open mention menu", caret("@A", 2)),
    ];
    for (what, draft) in drafts {
        for claim in claimed(&draft) {
            let native = matches!(
                decision(&draft, &claim),
                wire::EditorDecision::DefaultEditorAction
            );
            let allowed = !claim.command
                && matches!(
                    claim.key,
                    Key::Named(Named::Tab) | Key::Named(Named::Backspace)
                );
            assert!(
                !native || allowed,
                "{:?} on {what} stops every app installed today",
                claim.key
            );
        }
    }
}
