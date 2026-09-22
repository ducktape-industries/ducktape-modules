use serde::{Deserialize, Serialize};
use view_guest::prelude::*;
use view_guest::{Driver, View, testing::TestAppContext, wire};

#[derive(Default, Serialize, Deserialize)]
struct Counter {
    clicks: usize,
}
impl View for Counter {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("button")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.clicks += 1;
                cx.notify();
            }))
            .child("Click")
    }
}
fn route(frame: &wire::Frame) -> u32 {
    let wire::Node::Container { interactivity, .. } = frame.root.as_ref().unwrap() else {
        panic!("a container")
    };
    interactivity.on_click.unwrap()
}
fn click(handler: u32) -> wire::Event {
    wire::Event::Click {
        handler,
        event: (&ClickEvent::default()).into(),
    }
}

#[derive(Default, Serialize, Deserialize)]
struct PointerSurface {
    seen: Vec<(usize, bool, f32)>,
}
impl View for PointerSurface {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for PointerSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("surface").on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.seen.push((
                    event.click_count,
                    event.modifiers.shift,
                    event.position.x.as_f32(),
                ));
                cx.notify();
            }),
        )
    }
}

#[test]
fn pointer_listener_preserves_payload_and_routes_after_frame_reset() {
    let mut driver = Driver::<PointerSurface>::new();
    let mut other = Driver::<PointerSurface>::new();
    let first = driver.tick(vec![]);
    let other_frame = other.tick(vec![]);
    let wire::Node::Container { interactivity, .. } = first.root.as_ref().unwrap() else {
        panic!("a container")
    };
    let handler = interactivity.on_mouse_down.expect("mouse route");
    let wire::Node::Container { interactivity, .. } = other_frame.root.as_ref().unwrap() else {
        panic!("a container")
    };
    assert_eq!(interactivity.on_mouse_down, Some(handler));
    let event = wire::interactivity::MouseDown {
        button: wire::click::MouseButton::Right,
        position: wire::interactivity::point(12.5, 7.0),
        modifiers: wire::keyboard::Modifiers {
            shift: true,
            ..Default::default()
        },
        click_count: 2,
        first_mouse: true,
    };
    let next = driver.tick(vec![wire::Event::MouseDown {
        handler,
        phase: wire::DispatchPhase::Bubble,
        event,
    }]);
    driver.entity().read(|view| {
        assert_eq!(view.seen, vec![(2, true, 12.5)]);
    });
    other.tick(vec![wire::Event::MouseDown {
        handler: handler + 1,
        phase: wire::DispatchPhase::Bubble,
        event,
    }]);
    other.entity().read(|view| assert!(view.seen.is_empty()));
    let wire::Node::Container { interactivity, .. } = next.root.as_ref().unwrap() else {
        panic!("a container")
    };
    assert_eq!(interactivity.on_mouse_down, Some(handler));
}

#[derive(Serialize, Deserialize)]
struct TooltipSurface;
impl View for TooltipSurface {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for TooltipSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("target")
            .key_context("Tooltip mode = help")
            .tooltip_show_delay(std::time::Duration::from_millis(250))
            .hoverable_tooltip(|_, cx| cx.new(|_| TooltipContent).into())
    }
}

#[derive(Serialize, Deserialize)]
struct TooltipContent;
impl View for TooltipContent {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for TooltipContent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("tip").child("Help")
    }
}

#[test]
fn tooltip_delay_is_order_independent_and_builder_runs_only_after_request() {
    let mut driver = Driver::<TooltipSurface>::new();
    let frame = driver.tick(vec![]);
    let wire::Node::Container { interactivity, .. } = frame.root.as_ref().unwrap() else {
        panic!("a container")
    };
    let tooltip = interactivity.tooltip.as_ref().expect("tooltip recipe");
    let context = interactivity.key_context.as_ref().expect("key context");
    assert_eq!(context.entries[0].key.as_ref(), "Tooltip");
    assert_eq!(context.entries[1].key.as_ref(), "mode");
    assert_eq!(context.entries[1].value.as_deref(), Some("help"));
    assert!(tooltip.hoverable);
    assert_eq!(tooltip.delay_ms, 250);
    assert!(
        tooltip.content.is_none(),
        "ordinary render must not build the tooltip"
    );
    let response = driver.tick(vec![wire::Event::TooltipRequest {
        request: tooltip.request,
        character_index: None,
    }]);
    let [response] = response.tooltip_responses.as_slice() else {
        panic!("one tooltip response")
    };
    assert_eq!(response.request, tooltip.request);
    assert_eq!(response.character_index, None);
    let Some(content) = response.content.as_deref() else {
        panic!("ordinary tooltip has content")
    };
    let wire::Node::Container { id, children, .. } = content else {
        panic!("tooltip content is a lowered container")
    };
    assert_eq!(id, &Some(wire::ElementIdWire::Name("tip".into())));
    assert_eq!(children.len(), 1);
}

#[derive(Serialize, Deserialize)]
struct RichTooltipSurface;
impl View for RichTooltipSurface {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for RichTooltipSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        InteractiveText::new("rich-tip", StyledText::new("alpha beta"))
            .tooltip(|index, _, cx| (index == 6).then(|| cx.new(|_| TooltipContent).into()))
    }
}

#[test]
fn rich_text_tooltip_routes_character_index_and_explicit_none() {
    let mut driver = Driver::<RichTooltipSurface>::new();
    let frame = driver.tick(vec![]);
    let wire::Node::RichText {
        tooltip: Some(tooltip),
        ..
    } = frame.root.as_ref().unwrap()
    else {
        panic!("a rich text tooltip recipe")
    };

    let some = driver.tick(vec![wire::Event::TooltipRequest {
        request: tooltip.request,
        character_index: Some(6),
    }]);
    let [some] = some.tooltip_responses.as_slice() else {
        panic!("one tooltip response")
    };
    assert_eq!(some.character_index, Some(6));
    assert!(some.content.is_some());

    let none = driver.tick(vec![wire::Event::TooltipRequest {
        request: tooltip.request,
        character_index: Some(0),
    }]);
    let [none] = none.tooltip_responses.as_slice() else {
        panic!("one explicit empty tooltip response")
    };
    assert_eq!(none.character_index, Some(0));
    assert!(none.content.is_none());
}

#[derive(Serialize, Deserialize)]
struct FocusSurface;
impl View for FocusSurface {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for FocusSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let first = cx.focus_handle();
        let same = first.clone();
        let unrelated = cx.focus_handle();
        div()
            .child(div().id("first").track_focus(&first))
            .child(div().id("same").track_focus(&same))
            .child(div().id("other").track_focus(&unrelated))
    }
}

#[test]
fn opaque_focus_allocations_share_only_through_clone() {
    let frame = Driver::<FocusSurface>::new().tick(vec![]);
    let wire::Node::Container { children, .. } = frame.root.unwrap() else {
        panic!("root container")
    };
    let ids: Vec<_> = children
        .iter()
        .map(|node| match node {
            wire::Node::Container { interactivity, .. } => interactivity.focus_handle.unwrap(),
            _ => panic!("focus container"),
        })
        .collect();
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn click_routes_are_frame_owned_and_driver_isolated() {
    let mut first = Driver::<Counter>::new();
    let mut second = Driver::<Counter>::new();
    let first_id = route(&first.tick(vec![]));
    let second_id = route(&second.tick(vec![]));
    assert_eq!(first_id, second_id);
    for expected in 1..=20 {
        let frame = first.tick(vec![click(first_id)]);
        assert_eq!(
            route(&frame),
            first_id,
            "reset must release previous frame routes"
        );
        first
            .entity()
            .read(|view| assert_eq!(view.clicks, expected));
        second.entity().read(|view| assert_eq!(view.clicks, 0));
    }
    second.tick(vec![click(second_id)]);
    second.entity().read(|view| assert_eq!(view.clicks, 1));
    first.tick(vec![wire::Event::Message(first_id)]);
    first
        .entity()
        .read(|view| assert_eq!(view.clicks, 20, "message and click routes differ"));
}

#[test]
fn globals_return_borrowed_non_clone_values() {
    struct NonClone(usize);
    impl Global for NonClone {}
    let mut cx = TestAppContext::new();
    let entity = cx.open::<Counter>();
    cx.set_global(NonClone(7));
    entity.update(&mut cx, |_, _, cx| {
        let global: &NonClone = cx.global::<NonClone>();
        assert_eq!(global.0, 7);
    });
}

#[test]
fn listeners_use_weak_entities() {
    let mut first = TestAppContext::new();
    let entity = first.open::<Counter>();
    let listener = entity.update(&mut first, |_, _, cx| {
        cx.listener(|_: &mut Counter, _: &ClickEvent, _, _| panic!("released listener ran"))
    });
    drop(entity);
    drop(first);
    let mut second = TestAppContext::new();
    let other = second.open::<Counter>();
    other.update(&mut second, |_, window, cx| {
        listener(&ClickEvent::default(), window, cx)
    });
}

#[derive(Serialize, Deserialize)]
struct GlobalReader {
    initial: usize,
}
struct Configuration(usize);
impl Global for Configuration {}
impl View for GlobalReader {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            initial: cx.global::<Configuration>().0,
        }
    }
    fn restored(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.initial = cx.global::<Configuration>().0;
    }
}
impl Render for GlobalReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(cx.global::<Configuration>().0.to_string())
    }
}
#[test]
fn test_globals_are_available_during_creation_and_restore_without_clone() {
    let mut cx = TestAppContext::new();
    cx.set_global(Configuration(13));
    let entity = cx.open::<GlobalReader>();
    entity.read(|view| assert_eq!(view.initial, 13));
    let snapshot = cx.snapshot().unwrap();
    cx.set_global(Configuration(21));
    let restored = cx.restore::<GlobalReader>(&snapshot).unwrap();
    restored.read(|view| assert_eq!(view.initial, 21));
    let second = cx.open::<GlobalReader>();
    second.read(|view| assert_eq!(view.initial, 21));
}

#[derive(Default, Serialize, Deserialize)]
struct ThemeReader;
impl View for ThemeReader {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for ThemeReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().bg(cx.global::<Theme>().surface)
    }
}
#[test]
fn host_theme_events_update_the_global_and_emit_style_patches() {
    let mut driver = Driver::<ThemeReader>::new();
    let first = driver.tick(vec![]);
    let mut root = first.root.unwrap();
    let changed = driver.tick(vec![wire::Event::Theme { dark: true }]);
    assert!(matches!(
        changed.patches.as_slice(),
        [wire::Patch::Props { .. }]
    ));
    wire::apply(&mut root, changed.patches).unwrap();
    let wire::Node::Container { ref style, .. } = root else {
        panic!("container")
    };
    assert_eq!(style.background, Some(Theme::dark().surface.into()));
    assert_ne!(Theme::dark().surface, Theme::light().surface);

    let changed = driver.tick(vec![wire::Event::Theme { dark: false }]);
    assert!(matches!(
        changed.patches.as_slice(),
        [wire::Patch::Props { .. }]
    ));
    wire::apply(&mut root, changed.patches).unwrap();
    let wire::Node::Container { ref style, .. } = root else {
        panic!("container")
    };
    assert_eq!(style.background, Some(Theme::light().surface.into()));
}
