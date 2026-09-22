use serde::{Deserialize, Serialize};
use view_guest::{Driver, View, wire, testing::TestAppContext};
use view_guest::prelude::*;

#[derive(Default, Serialize, Deserialize)]
struct Counter {
    clicks: usize,
}
impl View for Counter {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self { Self::default() }
}
impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("button").on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.clicks += 1;
            cx.notify();
        })).child("Click")
    }
}
fn route(frame: &wire::Frame) -> u32 {
    let wire::Node::Container { interactivity, .. } = frame.root.as_ref().unwrap() else {
        panic!("a container")
    };
    interactivity.on_click.unwrap()
}
fn click(handler: u32) -> wire::Event {
    wire::Event::Click { handler, event: (&ClickEvent::default()).into() }
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
        assert_eq!(route(&frame), first_id, "reset must release previous frame routes");
        first.entity().read(|view| assert_eq!(view.clicks, expected));
        second.entity().read(|view| assert_eq!(view.clicks, 0));
    }
    second.tick(vec![click(second_id)]);
    second.entity().read(|view| assert_eq!(view.clicks, 1));
    first.tick(vec![wire::Event::Message(first_id)]);
    first.entity().read(|view| assert_eq!(view.clicks, 20, "message and click routes differ"));
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
    other.update(&mut second, |_, window, cx| listener(&ClickEvent::default(), window, cx));
}

#[derive(Serialize, Deserialize)]
struct GlobalReader { initial: usize }
struct Configuration(usize);
impl Global for Configuration {}
impl View for GlobalReader {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self { initial: cx.global::<Configuration>().0 }
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
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self { Self }
}
impl Render for ThemeReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().bg(cx.global::<Theme>().surface)
    }
}
#[test]
fn host_theme_updates_the_global_and_emits_a_style_patch() {
    let mut driver = Driver::<ThemeReader>::new();
    let first = driver.tick(vec![]);
    let mut root = first.root.unwrap();
    let changed = driver.tick(vec![wire::Event::Theme { dark: true }]);
    assert!(matches!(changed.patches.as_slice(), [wire::Patch::Props { .. }]));
    wire::apply(&mut root, changed.patches).unwrap();
    let wire::Node::Container { style, .. } = root else { panic!("container") };
    assert_eq!(style.background, Some(Theme::dark().surface.into()));
    assert_ne!(Theme::dark().surface, Theme::light().surface);
}
