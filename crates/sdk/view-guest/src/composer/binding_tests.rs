use super::super::editing;
use super::key_tag;
use super::*;
use crate::{App, Context, Entity, IntoElement, Lowering, Render, View, Window};
use serde::{Deserialize, Serialize};
use wire::keyboard::{Key, Modifiers, Named};

#[derive(Default, Serialize, Deserialize)]
struct ComposerView;

impl View for ComposerView {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for ComposerView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::div().child("unused")
    }
}

fn walk(node: &wire::Node, seen: &mut impl FnMut(&wire::Node)) {
    seen(node);
    for child in node.children() {
        walk(child, seen);
    }
}

fn drawn(draft: &Draft) -> wire::Node {
    drawn_with_key(draft, "c")
}

fn drawn_with_key(draft: &Draft, key: &str) -> wire::Node {
    let mut app = App::new(false);
    let entity = Entity::reserve(&app);
    let mut window = app.window();
    let mut cx = Context {
        app: &mut app,
        entity,
    };
    let element = view(
        draft,
        key,
        "Message #general",
        true,
        &[],
        &mut cx,
        |_: &mut ComposerView, _, _, _| {},
    );
    drop(cx);
    element.into_node(&mut Lowering::new(&mut window, &mut app))
}

fn find_editor(root: &wire::Node) -> Option<&wire::Node> {
    if matches!(root, wire::Node::Editor { .. }) {
        return Some(root);
    }
    root.children().iter().find_map(|child| find_editor(child))
}

fn editor_node(root: &wire::Node) -> &wire::Node {
    find_editor(root).expect("composer includes one editor")
}

fn clickable(root: &wire::Node, key: &str) -> Option<u32> {
    let mut result = None;
    walk(root, &mut |node| {
        if node.key() == Some(key) {
            if let wire::Node::Container { interactivity, .. } = node {
                result = interactivity.on_click;
            }
        }
    });
    result
}

#[test]
fn two_draft_keys_are_two_documents_the_host_can_tell_apart() {
    let a_root = drawn_with_key(&Draft::from_body("room a draft", &[]), "c/room-a");
    let b_root = drawn_with_key(&Draft::default(), "c/room-b");
    let a = editor_node(&a_root);
    let b = editor_node(&b_root);
    let (
        wire::Node::Editor {
            key: a_key,
            document: a_document,
            ..
        },
        wire::Node::Editor {
            key: b_key,
            document: b_document,
            ..
        },
    ) = (a, b)
    else {
        unreachable!()
    };
    assert_ne!(a_key, b_key, "each draft key owns its document identity");
    assert_ne!(
        a_document.document, b_document.document,
        "two drafts the host must not share text between"
    );
    assert_eq!(a_document.document, "c/room-a/editor");
    assert_eq!(b_document.document, "c/room-b/editor");
}

#[test]
fn send_is_the_only_clickable_action_and_is_dead_on_an_empty_draft() {
    let empty = drawn(&Draft::default());
    assert!(clickable(&empty, "c/send").is_none());
    let typed = drawn(&Draft::from_body("hello", &[]));
    assert!(clickable(&typed, "c/send").is_some());
}

#[test]
fn composer_uses_a_real_style_refinement_and_host_editor_binding() {
    let root = drawn(&Draft::default());
    assert!(matches!(root, wire::Node::Container { .. }));
    let wire::Node::Editor { options, .. } = editor_node(&root) else {
        unreachable!()
    };
    let binding = options.binding.as_ref().expect("editor binding");
    assert!(!binding.claims.is_empty());
    assert!(binding.authored);
}

fn roster() -> Vec<MentionChoice> {
    vec![MentionChoice {
        token: "<@1>".into(),
        label: "Ada".into(),
    }]
}

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

fn claimed(draft: &Draft) -> Vec<wire::EditorKeyClaim> {
    let root = drawn(draft);
    let wire::Node::Editor { options, .. } = editor_node(&root) else {
        unreachable!()
    };
    options
        .binding
        .as_ref()
        .expect("editor binding")
        .claims
        .clone()
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
