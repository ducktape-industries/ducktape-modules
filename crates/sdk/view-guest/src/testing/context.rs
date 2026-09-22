use super::{assert_accessible, find, texts, FakeHost};
use crate::{
    host::Host,
    wire::{Event, Frame, Node},
    App, Driver, Entity, View,
};

trait TestDriver {
    fn tick(&mut self, events: Vec<Event>) -> Frame;
    fn app_mut(&mut self) -> &mut App;
    fn host(&self) -> Host;
    fn snapshot(&self) -> Result<Vec<u8>, String>;
}
impl<V: View> TestDriver for Driver<V> {
    fn tick(&mut self, events: Vec<Event>) -> Frame {
        self.tick(events)
    }
    fn app_mut(&mut self) -> &mut App {
        self.app_mut()
    }
    fn host(&self) -> Host {
        self.host()
    }
    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot()
    }
}

/// A single view and its typed host, driven until no immediate work remains.
#[derive(Default)]
pub struct TestAppContext {
    host: FakeHost,
    driver: Option<Box<dyn TestDriver>>,
    frame: Frame,
    globals: crate::context::Globals,
}

impl TestAppContext {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn host(&self) -> FakeHost {
        self.host.clone()
    }
    pub fn open<V: View>(&mut self) -> Entity<V> {
        let driver = Driver::<V>::initialize_in(self.fresh_app(), None).expect("view initializes");
        let entity = driver.entity();
        self.host.reset_connection();
        self.driver = Some(Box::new(driver));
        self.frame = Frame::default();
        self.run_until_parked();
        entity
    }
    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.driver.as_ref().expect("open a view first").snapshot()
    }
    pub fn restore<V: View>(&mut self, bytes: &[u8]) -> Result<Entity<V>, String> {
        let value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        let driver = Driver::<V>::initialize_in(self.fresh_app(), Some(value))?;
        let entity = driver.entity();
        self.host.reset_connection();
        self.driver = Some(Box::new(driver));
        self.frame = Frame::default();
        self.run_until_parked();
        Ok(entity)
    }
    pub(crate) fn app_mut(&mut self) -> &mut App {
        self.driver.as_mut().expect("open a view first").app_mut()
    }
    fn fresh_app(&self) -> App {
        let mut app = App::for_driver();
        for (kind, value) in &self.globals {
            app.set_shared_global(*kind, value.clone());
        }
        app
    }
    /// Set a global before opening a view, or rerender the current view with it.
    pub fn set_global<G: gpui::Global>(&mut self, global: G) {
        let kind = std::any::TypeId::of::<G>();
        let global: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(global);
        self.globals.insert(kind, global.clone());
        if let Some(driver) = &mut self.driver {
            let app = driver.app_mut();
            app.set_shared_global(kind, global);
            app.notify();
            self.run_until_parked();
        }
    }
    pub fn run_until_parked(&mut self) {
        self.dispatch(Vec::new());
    }
    fn dispatch(&mut self, mut events: Vec<Event>) {
        for _ in 0..10_000 {
            events.extend(self.host.take_events());
            let driver = self.driver.as_mut().expect("open a view first");
            let mut frame = driver.tick(std::mem::take(&mut events));
            self.host.accept(&frame, &driver.host());
            if frame.root.is_none() {
                frame.root = self.frame.root.take();
                if !frame.patches.is_empty() {
                    crate::wire::apply(
                        frame.root.as_mut().expect("patch needs previous tree"),
                        std::mem::take(&mut frame.patches),
                    )
                    .expect("valid view patches");
                }
            }
            let busy = frame.busy;
            self.frame = frame;
            events = self.host.take_events();
            if events.is_empty() && !busy {
                return;
            }
        }
        panic!("view did not park after 10000 ticks");
    }
    pub fn texts(&self) -> Vec<String> {
        texts(&self.frame)
    }
    pub fn has_text(&self, text: &str) -> bool {
        super::has_text(&self.frame, text)
    }
    pub fn find(&self, key: &str) -> Option<&Node> {
        find(&self.frame, key)
    }
    /// The whole tree of the last frame, for node-less host renders.
    pub fn root(&self) -> &Node {
        self.frame.root.as_ref().expect("view has a tree")
    }
    pub fn assert_accessible(&self) {
        assert_accessible(self.frame.root.as_ref().expect("view has a tree"));
    }
    pub fn simulate_click(&mut self, key: &str) {
        self.dispatch(super::press(&self.frame, key));
    }
    pub fn simulate_input(&mut self, key: &str, text: &str) {
        self.dispatch(super::type_into(&self.frame, key, text));
    }
    pub fn simulate_rich_click(&mut self, key: &str, index: usize) {
        self.dispatch(vec![super::rich_click(&self.frame, key, index)]);
    }
    pub fn simulate_submit(&mut self, key: &str) {
        self.dispatch(super::submit(&self.frame, key));
    }
    pub fn simulate_measure(&mut self, key: &str, width: f32, height: f32) {
        self.dispatch(super::measure(&self.frame, key, width, height));
    }
    pub fn simulate_drag(&mut self, key: &str, dx: f64, dy: f64) {
        self.dispatch(super::drag(&self.frame, key, dx, dy));
    }
    pub fn simulate_dismiss(&mut self, key: &str) {
        self.dispatch(super::dismiss(&self.frame, key));
    }
    pub fn simulate_surface(&mut self, key: &str, value: crate::wire::SurfaceValue) {
        self.dispatch(super::surface(&self.frame, key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{doors::Live, Context, InteractiveElement, ParentElement, Render, Task, Window};
    use futures::StreamExt;
    use serde::{Deserialize, Serialize};

    #[derive(Default, Serialize, Deserialize)]
    struct LiveView {
        items: usize,
        #[serde(skip)]
        task: Option<Task<()>>,
    }
    impl View for LiveView {
        fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
            let mut view = Self::default();
            view.restored(window, cx);
            view
        }
        fn restored(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            let mut stream = cx.host().subscribe::<Live>("live".into());
            self.task = Some(cx.spawn(async move |this, cx| {
                while let Some(item) = stream.next().await {
                    item.unwrap();
                    this.update(cx, |view, cx| {
                        view.items += 1;
                        cx.notify();
                    })
                    .unwrap();
                }
            }));
        }
    }
    impl Render for LiveView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
            crate::div().id("items").child(self.items.to_string())
        }
    }

    #[test]
    fn restoring_resubscribes_without_replaying_old_events_or_duplicate_ids() {
        let mut cx = TestAppContext::new();
        let feed = cx.host().stream::<Live>();
        cx.open::<LiveView>();
        feed.push(None);
        cx.run_until_parked();
        assert!(cx.has_text("1"));
        let snapshot = cx.snapshot().unwrap();
        feed.push(None);
        let restored = cx.restore::<LiveView>(&snapshot).unwrap();
        restored.read(|view| assert_eq!(view.items, 1));
        feed.push(None);
        cx.run_until_parked();
        restored.read(|view| assert_eq!(view.items, 2));
        assert_eq!(cx.host().asked::<Live>().len(), 2);
    }
}
