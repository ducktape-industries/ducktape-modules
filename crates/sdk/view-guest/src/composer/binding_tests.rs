use super::super::editing;
use super::super::{Attachment, Send};
use super::binding_editor::editor;
use super::key_tag;
use super::*;
use crate::{
    wire, App, Context, Driver, Entity, IntoElement, Lowering, Render, Role, Theme, View, Window,
};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use wire::keyboard::{Key, Modifiers, Named};

#[derive(Default, Serialize, Deserialize)]
struct ComposerView {
    draft: Draft,
    choices: Vec<MentionChoice>,
    #[serde(skip)]
    events: Vec<String>,
}

impl View for ComposerView {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            draft: Draft::from_body("hello", &[]),
            ..Self::default()
        }
    }
}

impl Render for ComposerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = self.draft.clone();
        let choices = self.choices.clone();
        view(
            &draft,
            "c",
            "Message #general",
            true,
            true,
            &choices,
            cx,
            |view, event, _, cx| {
                let event = match event {
                    Event::Action(tag) => format!("action:{tag}"),
                    Event::Document(_) => "document".into(),
                    Event::Transaction(_) => "transaction".into(),
                    Event::Committed(change) => format!("commit:{}", change.tag),
                };
                view.events.push(event);
                cx.notify();
            },
        )
    }
}

fn walk(node: &wire::Node, seen: &mut impl FnMut(&wire::Node)) {
    seen(node);
    for child in node.children() {
        walk(child, seen);
    }
}

fn drawn(draft: &Draft) -> wire::Node {
    drawn_with(draft, "c", true, &[])
}

fn drawn_with(draft: &Draft, key: &str, attach: bool, choices: &[MentionChoice]) -> wire::Node {
    let mut app = App::for_driver();
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
        attach,
        choices,
        &mut cx,
        |_: &mut ComposerView, _, _, _| {},
    );
    drop(cx);
    Lowering::new(&mut window, &mut app).lower(element)
}

fn find_editor(root: &wire::Node) -> Option<&wire::Node> {
    if matches!(root, wire::Node::Editor { .. }) {
        return Some(root);
    }
    root.children().iter().find_map(find_editor)
}

fn editor_node(root: &wire::Node) -> &wire::Node {
    find_editor(root).expect("composer includes one editor")
}

fn node<'a>(root: &'a wire::Node, key: &str) -> Option<&'a wire::Node> {
    if root.key() == Some(key) {
        return Some(root);
    }
    root.children().iter().find_map(|child| node(child, key))
}

fn clickable(root: &wire::Node, key: &str) -> Option<u32> {
    let Some(wire::Node::Container(crate::wire::ContainerNode { interactivity, .. })) =
        node(root, key)
    else {
        return None;
    };
    interactivity.on_click
}

#[test]
fn two_drafts_at_one_key_are_two_documents_the_host_can_tell_apart() {
    let field = |draft: &Draft, document: &str| {
        let mut app = App::for_driver();
        let mut window = app.window();
        let handle: Handle<()> = Rc::new(|_, _, _, _| {});
        let field = editor(
            draft,
            "c/editor",
            document,
            "Message",
            true,
            &[],
            handle,
            Theme::default().accent,
        );
        let wire::Node::Editor { id, document, .. } =
            Lowering::new(&mut window, &mut app).lower(field)
        else {
            panic!("the composer's field is an editor node");
        };
        (id, document.document)
    };
    let (a_key, a_document) = field(&Draft::from_body("room a draft", &[]), "chat\u{1f}room-a");
    let (b_key, b_document) = field(&Draft::default(), "chat\u{1f}room-b");
    assert_eq!(a_key, b_key, "the field keeps its placement and its name");
    assert_ne!(
        a_document, b_document,
        "two drafts at one element ID must not share host editor state"
    );
    assert_eq!(a_document, "chat\u{1f}room-a");
    assert_eq!(b_document, "chat\u{1f}room-b");
}

#[test]
fn discarded_composer_editor_does_not_register_routes_before_lowering() {
    let mut app = App::for_driver();
    let handle: Handle<()> = Rc::new(|_, _, _, _| {});
    drop(editor(
        &Draft::default(),
        "c/editor",
        "discarded",
        "Message",
        true,
        &[],
        handle.clone(),
        Theme::default().accent,
    ));
    let field = editor(
        &Draft::default(),
        "c/editor",
        "lowered",
        "Message",
        true,
        &[],
        handle,
        Theme::default().accent,
    );
    let mut window = app.window();
    let wire::Node::Editor { on_document, .. } = Lowering::new(&mut window, &mut app).lower(field)
    else {
        unreachable!()
    };
    assert_eq!(on_document, 0, "discarding a recipe must consume no route");
}

#[test]
fn the_send_is_the_only_primary_and_is_dead_on_an_empty_draft() {
    let empty = drawn(&Draft::default());
    assert!(clickable(&empty, "c/send").is_none());
    let typed = drawn(&Draft::from_body("hello", &[]));
    assert!(clickable(&typed, "c/send").is_some());
}

#[test]
fn every_mark_is_the_same_square_and_the_field_writes_at_body_size() {
    let root = drawn(&Draft::default());
    let mut marks = Vec::new();
    let mut body_size = None;
    let mut editor_bounds = None;
    walk(&root, &mut |node| match node {
        wire::Node::Container(crate::wire::ContainerNode {
            style,
            interactivity,
            ..
        }) if interactivity.role == Some(Role::Button) => {
            if style.size.width == Some(gpui::px(24.).into())
                && style.size.height == Some(gpui::px(24.).into())
            {
                marks.push(interactivity.aria.label.clone());
            }
        }
        wire::Node::Editor { style, .. } => {
            body_size = style.text.font_size;
            editor_bounds = Some((style.min_size.height, style.max_size.height));
        }
        _ => {}
    });
    assert_eq!(
        marks.len(),
        5,
        "attach, bold, italic, code and quote are square marks"
    );
    assert_eq!(
        body_size,
        Some(gpui::px(design::type_scale::BODY as f32).into())
    );
    assert_eq!(
        editor_bounds,
        Some((Some(gpui::px(40.).into()), Some(gpui::px(200.).into())))
    );
}

#[test]
fn restored_editor_presentation_keeps_mention_highlights_and_document_routes() {
    let choices = vec![MentionChoice {
        token: "<@1>".into(),
        label: "Ada".into(),
    }];
    let root = drawn_with(&Draft::from_body("Hi <@1>", &choices), "c", true, &choices);
    let wire::Node::Editor {
        document,
        on_document: _,
        options,
        ..
    } = editor_node(&root)
    else {
        unreachable!()
    };
    assert_eq!(document.document, "c");
    assert!(options
        .binding
        .as_ref()
        .is_some_and(|binding| binding.authored));
    let presentation = options.presentation.as_ref().expect("editor presentation");
    assert_eq!(presentation.formats.len(), 1);
    assert_eq!(presentation.spans.len(), 1);
}

#[test]
fn toolbar_attachment_mention_and_restore_actions_have_reachable_aria_routes() {
    let choices = vec![MentionChoice {
        token: "<@1>".into(),
        label: "Ada".into(),
    }];
    let mut draft = Draft::from_body("@A", &choices);
    draft.editor.move_to(wire::EditorCursor {
        position: wire::EditorPosition { line: 0, column: 2 },
        selection: None,
    });
    draft.attachments.push(Attachment {
        token: "file-1".into(),
        name: "report.txt".into(),
        bytes: 4,
        state: AttachmentState::Failed {
            reason: "upload failed".into(),
        },
    });
    draft.failed_send = Some(Send {
        body: "older".into(),
        attachments: Vec::new(),
    });
    let root = drawn_with(&draft, "c", true, &choices);
    for (key, label) in [
        ("c/attach", "Attach a file"),
        ("c/bold", "Bold"),
        ("c/italic", "Italic"),
        ("c/code", "Code"),
        ("c/quote", "Quote"),
        ("c/attachment/file-1/remove", "Remove attachment"),
        ("c/attachment/file-1/retry", "Retry"),
        ("c/restore", "Restore"),
    ] {
        let Some(wire::Node::Container(crate::wire::ContainerNode { interactivity, .. })) =
            node(&root, key)
        else {
            panic!("missing composer action {key}");
        };
        assert!(interactivity.on_click.is_some(), "{key} has no route");
        assert_eq!(interactivity.aria.label.as_deref(), Some(label));
    }
    let Some(wire::Node::Container(crate::wire::ContainerNode { interactivity, .. })) =
        node(&root, "c/mention/<@1>")
    else {
        panic!("missing mention action");
    };
    assert!(interactivity.on_click.is_some());
    assert_eq!(interactivity.role, Some(Role::MenuItem));
    assert_eq!(interactivity.aria.label.as_deref(), Some("@Ada"));
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
    let key = key_state(&bare(Named::Enter));
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
    let root = drawn_with(draft, "c", true, &roster());
    let wire::Node::Editor { options, .. } = editor_node(&root) else {
        unreachable!()
    };
    options
        .binding
        .as_ref()
        .expect("the field carries its binding")
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
    assert_eq!(removed(&caret("héllo", 1)).as_deref(), Some("hllo"));
    assert_eq!(removed(&caret("a👨‍👩‍👧b", 1)).as_deref(), Some("ab"));
    assert_eq!(removed(&caret("hello", 5)), None);
    let mut mention = Draft::from_body("Hi <@1> there", &roster());
    let at = mention.mentions[0].range.start;
    let text = mention.editor.text();
    mention.editor.move_to(wire::EditorCursor {
        position: editing::position(&text, at),
        selection: None,
    });
    assert_eq!(removed(&mention).as_deref(), Some("Hi  there"));
}

#[test]
fn the_arrows_are_claimed_only_while_the_menu_is_open() {
    let closed = claimed(&caret("hello", 5));
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(!closed.contains(&bare(key)), "{key:?} belongs to the host");
    }
    let open = claimed(&caret("@A", 2));
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(open.contains(&bare(key)), "{key:?} belongs to the menu");
    }
}

#[test]
fn no_claimed_key_but_tab_and_backspace_asks_the_app_for_its_default() {
    let drafts = [
        Draft::default(),
        caret("hello", 2),
        caret("hello", 5),
        caret("@A", 2),
    ];
    for draft in drafts {
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
                "{:?} asks for an unsafe native default",
                claim.key
            );
        }
    }
}

#[test]
fn click_binding_and_document_routes_dispatch_through_the_driver() {
    let mut driver = Driver::<ComposerView>::new();
    let frame = driver.tick(Vec::new());
    driver.tick(crate::testing::press(&frame, "c/bold"));
    driver
        .entity()
        .read(|view| assert!(view.events.iter().any(|event| event == "action:bold")));

    let frame = driver.tick(Vec::new());
    let wire::Node::Editor {
        document,
        on_document,
        options,
        ..
    } = editor_node(frame.root.as_ref().expect("composer frame"))
    else {
        unreachable!()
    };
    let binding = options.binding.as_ref().expect("composer binding");
    driver.tick(vec![wire::Event::EditorRequest {
        handler: binding.on_request,
        request: wire::EditorRequest {
            id: wire::EditorTransactionId {
                instance: 1,
                document: document.document.clone(),
                reset: document.reset,
                sequence: 1,
                attempt: 0,
                text_revision: document.text_revision,
                revision: document.revision,
            },
            state: document.clone(),
            input: wire::EditorRequestInput::Key {
                key: key_state(&wire::EditorKeyClaim {
                    key: Key::Character("b".into()),
                    modifiers: Modifiers::default(),
                    command: true,
                }),
                repeat: false,
            },
            input_time_ms: 1,
        },
    }]);
    driver
        .entity()
        .read(|view| assert!(view.events.iter().any(|event| event == "transaction")));
    let id = wire::editor_document::EditorTransferId {
        instance: 1,
        document: document.document.clone(),
        reset: document.reset,
        serial: 1,
        attempt: 0,
    };
    driver.tick(vec![wire::Event::EditorDocument {
        handler: *on_document,
        message: wire::editor_document::EditorDocumentMessage::Request {
            id,
            target: document.clone(),
        },
    }]);
    driver
        .entity()
        .read(|view| assert!(view.events.iter().any(|event| event == "document")));
}
