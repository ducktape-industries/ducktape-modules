//! One of every `Event`, and the `Frame` around the tree.
use super::*;

pub fn every_event() -> Vec<Event> {
    let modifiers = keyboard::Modifiers::default();
    vec![
        Event::Observation {
            event: events::Event::Window(events::Window::FileHovered("a.txt".into())),
            captured: false,
        },
        Event::Mouse {
            event: mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            },
            captured: true,
        },
        Event::Keyboard {
            event: keyboard::Event::Press {
                state: key_state(),
                text: Some("\n".into()),
                repeat: false,
            },
            captured: false,
        },
        Event::Message(3),
        Event::Click {
            handler: 1,
            event: click::Click::Keyboard {
                button: click::KeyboardButton::Enter,
                bounds: Bounds {
                    origin: at(0.0, 0.0),
                    size: size(px(10.0), px(10.0)),
                },
            },
        },
        Event::MouseDown {
            handler: 2,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseDown {
                button: click::MouseButton::Left,
                position: at(1.0, 2.0),
                modifiers,
                click_count: 1,
                first_mouse: false,
            },
        },
        Event::MouseUp {
            handler: 3,
            phase: DispatchPhase::Capture,
            event: interactivity::MouseUp {
                button: click::MouseButton::Right,
                position: at(1.0, 2.0),
                modifiers,
                click_count: 1,
            },
        },
        Event::MouseDownOut {
            handler: 4,
            event: interactivity::MouseDown {
                button: click::MouseButton::Middle,
                position: at(3.0, 4.0),
                modifiers,
                click_count: 2,
                first_mouse: true,
            },
        },
        Event::MouseUpOut {
            handler: 5,
            event: interactivity::MouseUp {
                button: click::MouseButton::Back,
                position: at(3.0, 4.0),
                modifiers,
                click_count: 1,
            },
        },
        Event::MousePressure {
            handler: 6,
            phase: DispatchPhase::Bubble,
            event: interactivity::MousePressure {
                pressure: 0.5,
                stage: interactivity::PressureStage::Normal,
                position: at(1.0, 1.0),
                modifiers,
            },
        },
        Event::MouseMove {
            handler: 7,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseMove {
                position: at(5.0, 6.0),
                pressed_button: Some(click::MouseButton::Left),
                modifiers,
            },
        },
        Event::MouseExit {
            handler: 8,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseExit {
                position: at(5.0, 6.0),
                pressed_button: None,
                modifiers,
            },
        },
        Event::ScrollWheel {
            handler: 9,
            phase: DispatchPhase::Bubble,
            event: interactivity::ScrollWheel {
                position: at(5.0, 6.0),
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
                modifiers,
                touch_phase: interactivity::TouchPhase::Moved,
            },
        },
        Event::Pinch {
            handler: 10,
            phase: DispatchPhase::Bubble,
            event: interactivity::Pinch {
                position: at(5.0, 6.0),
                delta: 1.1,
                modifiers,
                phase: interactivity::TouchPhase::Started,
            },
        },
        Event::KeyDown {
            handler: 11,
            phase: DispatchPhase::Bubble,
            event: interactivity::KeyDown {
                state: key_state(),
                repeat: false,
                prefer_character_input: false,
            },
        },
        Event::KeyUp {
            handler: 12,
            phase: DispatchPhase::Capture,
            event: interactivity::KeyUp { state: key_state() },
        },
        Event::ModifiersChanged {
            handler: 13,
            event: interactivity::ModifiersChanged {
                modifiers,
                capslock: true,
            },
        },
        Event::Hover {
            handler: 14,
            hovered: true,
        },
        Event::FileDropExit { handler: 15 },
        Event::AuxClick {
            handler: 16,
            event: click::Click::Touch {
                position: at(7.0, 8.0),
                tap_count: 1,
                long_press: false,
            },
        },
        Event::TooltipRequest {
            request: 17,
            character_index: Some(3),
        },
        Event::Surface {
            handler: 18,
            value: SurfaceValue::List(vec![SurfaceValue::Bool(true), SurfaceValue::Unit]),
        },
        Event::Input {
            handler: 19,
            text: "xy".into(),
        },
        Event::EditorDocument {
            handler: 20,
            message: EditorDocumentMessage::Transfer(EditorTransfer::Chunk {
                id: transfer(),
                index: 0,
                bytes: b"draft".to_vec(),
            }),
        },
        Event::EditorRequest {
            handler: 21,
            request: EditorRequest {
                id: transaction(),
                state: document(9),
                input: EditorRequestInput::Interaction {
                    action: EditorInteraction::Action { tag: "send".into() },
                },
                input_time_ms: 40,
            },
        },
        Event::EditorTransaction {
            handler: 22,
            event: EditorTransactionEvent::Commit {
                id: transaction(),
                origin: Some(EditorRequestInput::Key {
                    key: key_state(),
                    repeat: false,
                }),
                before: document(9),
                after: document(10),
                patches: vec![EditorPatch {
                    start_byte: 9,
                    end_byte: 9,
                    replacement: "z".into(),
                }],
                kind: EditorEditKind::Insert,
                history: EditorHistoryEffect::ExtendPrevious,
                input_time_ms: 42,
            },
        },
        Event::Theme { dark: true },
        Event::Toggle {
            handler: 23,
            on: true,
        },
        Event::Slide {
            handler: 24,
            value: 0.5,
        },
        Event::Select {
            handler: 25,
            index: 1,
        },
        Event::RichTextHover {
            handler: 26,
            event: RichTextHover {
                index: Some(2),
                position: at(9.0, 9.0),
                pressed_button: None,
                modifiers: gpui::Modifiers::default(),
            },
        },
        Event::Size {
            handler: 27,
            width: 100.0,
            height: 50.0,
        },
        Event::Pointer {
            handler: 28,
            x: 12.5,
            y: 3.0,
        },
        Event::Drag {
            handler: 29,
            dx: 4.0,
            dy: -2.0,
        },
        Event::Scroll {
            handler: 30,
            dx: 0.0,
            dy: -1.0,
            pixels: false,
        },
        Event::ScrollOffset {
            handler: 31,
            x: 24.0,
            y: 50.0,
            relative_x: 0.2,
            relative_y: 0.1,
        },
        Event::UniformListRange {
            path: vec![id("root"), id("uniform")],
            route: 3,
            start: 0,
            end: 2,
        },
        Event::UniformListState {
            path: vec![id("root"), id("uniform")],
            route: 3,
            top_index: 0,
            scrollable: true,
            scrolled_to_end: Some(false),
        },
        Event::ListRequest {
            handler: 32,
            request: ListRequest { start: 0, end: 3 },
        },
        Event::ListScroll {
            handler: 33,
            event: ListScroll {
                visible_start: 0,
                visible_end: 3,
                count: 3,
                is_scrolled: false,
                is_following_tail: true,
                offset: ListOffset::default(),
            },
        },
        Event::Response {
            id: 1,
            result: Err(Refusal::new("not_found", "no such room")),
            done: true,
        },
        Event::Resync,
    ]
}

pub fn every_frame() -> Frame {
    Frame {
        upstream_sanitization: Default::default(),
        editor_decisions: vec![EditorResponse {
            id: transaction(),
            decision: EditorDecision::Apply {
                patches: vec![],
                cursor: EditorCursor::default(),
                history: EditorHistoryEffect::NewGroup,
            },
        }],
        editor_documents: vec![EditorDocumentMessage::Failed {
            id: transfer(),
            reason: EditorTransferError::Order,
        }],
        tooltip_responses: vec![TooltipResponse {
            request: 17,
            character_index: Some(3),
            content: Some(boxed("the tip")),
        }],
        mouse_interest: true,
        event_interest: events::Interest {
            focus: true,
            close: false,
            files: true,
            input_method: false,
        },
        root: Some(every_node()),
        patches: vec![
            Patch::Replace {
                path: vec![0],
                node: text("replaced"),
            },
            Patch::Props {
                path: vec![1],
                node: text("re-propped"),
            },
            Patch::Insert {
                path: vec![],
                index: 2,
                node: text("inserted"),
            },
            Patch::Remove {
                path: vec![],
                index: 3,
            },
            Patch::Move {
                path: vec![],
                from: 4,
                to: 5,
            },
        ],
        requests: vec![Request {
            id: 1,
            kind: doors::HostLog::KIND.into(),
            payload: doors::HostLog::encode_request(&"hello".into()),
        }],
        cancels: vec![2],
        unchanged: false,
        busy: true,
    }
}

pub fn event_variant(event: &Event) -> &'static str {
    match event {
        Event::Observation { .. } => "Observation",
        Event::Mouse { .. } => "Mouse",
        Event::Keyboard { .. } => "Keyboard",
        Event::Message(_) => "Message",
        Event::Click { .. } => "Click",
        Event::MouseDown { .. } => "MouseDown",
        Event::MouseUp { .. } => "MouseUp",
        Event::MouseDownOut { .. } => "MouseDownOut",
        Event::MouseUpOut { .. } => "MouseUpOut",
        Event::MousePressure { .. } => "MousePressure",
        Event::MouseMove { .. } => "MouseMove",
        Event::MouseExit { .. } => "MouseExit",
        Event::ScrollWheel { .. } => "ScrollWheel",
        Event::Pinch { .. } => "Pinch",
        Event::KeyDown { .. } => "KeyDown",
        Event::KeyUp { .. } => "KeyUp",
        Event::ModifiersChanged { .. } => "ModifiersChanged",
        Event::Hover { .. } => "Hover",
        Event::FileDropExit { .. } => "FileDropExit",
        Event::AuxClick { .. } => "AuxClick",
        Event::TooltipRequest { .. } => "TooltipRequest",
        Event::Surface { .. } => "Surface",
        Event::Input { .. } => "Input",
        Event::EditorDocument { .. } => "EditorDocument",
        Event::EditorRequest { .. } => "EditorRequest",
        Event::EditorTransaction { .. } => "EditorTransaction",
        Event::Theme { .. } => "Theme",
        Event::Toggle { .. } => "Toggle",
        Event::Slide { .. } => "Slide",
        Event::Select { .. } => "Select",
        Event::RichTextHover { .. } => "RichTextHover",
        Event::Size { .. } => "Size",
        Event::Pointer { .. } => "Pointer",
        Event::Drag { .. } => "Drag",
        Event::Scroll { .. } => "Scroll",
        Event::ScrollOffset { .. } => "ScrollOffset",
        Event::UniformListRange { .. } => "UniformListRange",
        Event::UniformListState { .. } => "UniformListState",
        Event::ListRequest { .. } => "ListRequest",
        Event::ListScroll { .. } => "ListScroll",
        Event::Response { .. } => "Response",
        Event::Resync => "Resync",
    }
}
