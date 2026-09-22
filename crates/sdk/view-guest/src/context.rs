//! Contexts and handles for the single root entity.
use crate::{executor, slots, FocusHandle, Host, Task, View, Window};
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};

pub(crate) type Callback<V> = Rc<dyn Fn(&mut V, &mut Window, &mut Context<V>)>;

pub(crate) type Globals = std::collections::HashMap<TypeId, Rc<dyn Any>>;

const MAX_UNIFORM_LISTS: usize = 128;

struct UniformListRoute {
    route: u32,
    count: usize,
    measure_index: usize,
    ranges: Vec<Range<usize>>,
    scroll: Option<std::rc::Weak<RefCell<crate::element::UniformListScrollState>>>,
}

pub struct App {
    pub(crate) inner: Rc<AppState>,
    globals: Rc<Globals>,
}
pub(crate) struct AppState {
    pub host: Host,
    pub slots: slots::Context,
    pub tasks: RefCell<Vec<executor::Running>>,
    pub generation: Cell<u64>,
    pub next_focus_id: Cell<u64>,
    pub dirty: Cell<bool>,
    pub macos: bool,
    pub alive: Cell<bool>,
    pub globals: RefCell<Rc<Globals>>,
    uniform_lists: RefCell<HashMap<Vec<crate::wire::ElementIdWire>, UniformListRoute>>,
    next_uniform_route: Cell<u32>,
}
impl App {
    pub(crate) fn for_driver(macos: bool) -> Self {
        let host = Host::default();
        let slots = slots::Context::with_host(macos, host.clone());
        let mut globals = std::collections::HashMap::new();
        globals.insert(
            TypeId::of::<crate::Theme>(),
            Rc::new(crate::Theme::default()) as Rc<dyn Any>,
        );
        let globals = Rc::new(globals);
        Self {
            globals: globals.clone(),
            inner: Rc::new(AppState {
                host,
                slots,
                tasks: RefCell::default(),
                generation: Cell::new(0),
                next_focus_id: Cell::new(0),
                dirty: Cell::new(true),
                macos,
                alive: Cell::new(true),
                globals: RefCell::new(globals),
                uniform_lists: RefCell::default(),
                next_uniform_route: Cell::new(1),
            }),
        }
    }
    pub fn host(&self) -> Host {
        self.inner.host.clone()
    }
    pub fn spawn<R: 'static>(&self, f: impl AsyncFnOnce(&mut AsyncApp) -> R + 'static) -> Task<R> {
        let mut cx = AsyncApp {
            inner: Rc::downgrade(&self.inner),
        };
        let (task, running) = executor::task(async move { f(&mut cx).await });
        self.inner.tasks.borrow_mut().push(running);
        task
    }
    pub(crate) fn window(&self) -> Window {
        Window::new(self.inner.macos, self.inner.slots.clone())
    }
    pub(crate) fn notify(&self) {
        self.inner.dirty.set(true);
        self.inner
            .generation
            .set(self.inner.generation.get().wrapping_add(1));
    }
    pub(crate) fn from_state(inner: Rc<AppState>) -> Self {
        let globals = inner.globals.borrow().clone();
        Self { inner, globals }
    }
    pub(crate) fn refresh_globals(&mut self) {
        self.globals = self.inner.globals.borrow().clone();
    }
    pub fn set_global<G: gpui::Global>(&mut self, global: G) {
        self.set_shared_global(TypeId::of::<G>(), Rc::new(global));
    }
    pub(crate) fn set_shared_global(&mut self, kind: TypeId, global: Rc<dyn Any>) {
        self.refresh_globals();
        Rc::make_mut(&mut self.globals).insert(kind, global);
        *self.inner.globals.borrow_mut() = self.globals.clone();
    }
    pub fn global<G: gpui::Global>(&self) -> &G {
        self.globals
            .get(&TypeId::of::<G>())
            .and_then(|global| global.downcast_ref::<G>())
            .expect("global is not initialized")
    }

    pub(crate) fn uniform_list_route(
        &self,
        path: &[crate::wire::ElementIdWire],
        count: usize,
        measure_index: usize,
        scroll: Option<&Rc<RefCell<crate::element::UniformListScrollState>>>,
    ) -> (u32, Vec<Range<usize>>) {
        let count = count.min(crate::wire::MAX_UNIFORM_LIST_COUNT);
        let mut lists = self.inner.uniform_lists.borrow_mut();
        if !lists.contains_key(path) {
            if lists.len() == MAX_UNIFORM_LISTS {
                if let Some(old) = lists.keys().next().cloned() {
                    lists.remove(&old);
                }
            }
            let route = self.inner.next_uniform_route.get();
            self.inner
                .next_uniform_route
                .set(route.wrapping_add(1).max(1));
            let measure_index = measure_index.min(count.saturating_sub(1));
            lists.insert(
                path.to_vec(),
                UniformListRoute {
                    route,
                    count,
                    measure_index,
                    ranges: (count > 0)
                        .then_some(measure_index..measure_index + 1)
                        .into_iter()
                        .collect(),
                    scroll: scroll.map(Rc::downgrade),
                },
            );
        }
        let state = lists.get_mut(path).expect("uniform-list route inserted");
        state.count = count;
        let previous_measure_index = state.measure_index;
        state.measure_index = measure_index.min(count.saturating_sub(1));
        state.scroll = scroll.map(Rc::downgrade);
        state.ranges.retain_mut(|range| {
            range.start = range.start.min(count);
            range.end = range.end.min(count);
            range.start < range.end
        });
        if previous_measure_index != state.measure_index {
            let previous = previous_measure_index..previous_measure_index.saturating_add(1);
            state.ranges.retain(|range| range != &previous);
        }
        let measurement = state.measure_index..state.measure_index.saturating_add(1).min(count);
        if count > 0 && !state.ranges.iter().any(|range| range == &measurement) {
            state.ranges.insert(0, measurement);
        }
        (state.route, state.ranges.clone())
    }

    pub(crate) fn request_uniform_list_range(
        &self,
        path: Vec<crate::wire::ElementIdWire>,
        route: u32,
        start: usize,
        end: usize,
    ) -> bool {
        let mut lists = self.inner.uniform_lists.borrow_mut();
        let Some(state) = lists.get_mut(&path) else {
            return false;
        };
        if state.route != route || start >= end || start >= state.count {
            return false;
        }
        let end = end
            .min(state.count)
            .min(start.saturating_add(crate::wire::MAX_UNIFORM_LIST_ROWS));
        if end <= start {
            return false;
        }
        let next = if start == 0 { 0..end } else { start..end };
        let changed = !state.ranges.iter().any(|range| range == &next);
        if changed {
            let measurement = state.measure_index..state.measure_index + 1;
            state.ranges.retain(|range| range == &measurement);
            state.ranges.push(next);
        }
        changed
    }

    pub(crate) fn update_uniform_list_state(
        &self,
        path: &[crate::wire::ElementIdWire],
        route: u32,
        top_index: usize,
        scrollable: bool,
        scrolled_to_end: Option<bool>,
    ) {
        let lists = self.inner.uniform_lists.borrow();
        let Some(state) = lists.get(path).filter(|state| state.route == route) else {
            return;
        };
        if let Some(scroll) = state.scroll.as_ref().and_then(std::rc::Weak::upgrade) {
            let mut scroll = scroll.borrow_mut();
            scroll.top_index = top_index;
            scroll.scrollable = scrollable;
            scroll.scrolled_to_end = scrolled_to_end;
        }
    }

    pub fn focus_handle(&mut self) -> FocusHandle {
        let id = self.inner.next_focus_id.get();
        self.inner.next_focus_id.set(id.wrapping_add(1));
        FocusHandle::new(id)
    }

    pub fn new<V: View>(&mut self, build: impl FnOnce(&mut Context<V>) -> V) -> Entity<V> {
        let entity = Entity::reserve(self);
        let value = build(&mut Context {
            app: self,
            entity: entity.clone(),
        });
        *entity.value.borrow_mut() = Some(value);
        entity
    }
}
#[derive(Clone)]
pub struct AsyncApp {
    inner: Weak<AppState>,
}
impl AsyncApp {
    pub fn host(&self) -> Host {
        self.inner.upgrade().expect("app released").host.clone()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Released;
impl std::fmt::Display for Released {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("entity released")
    }
}
impl std::error::Error for Released {}

pub struct Entity<V> {
    pub(crate) value: Rc<RefCell<Option<V>>>,
    app: Weak<AppState>,
}
pub struct WeakEntity<V> {
    value: Weak<RefCell<Option<V>>>,
    app: Weak<AppState>,
}
impl<V> Clone for Entity<V> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            app: self.app.clone(),
        }
    }
}
impl<V> Clone for WeakEntity<V> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            app: self.app.clone(),
        }
    }
}
impl<V> Entity<V> {
    pub(crate) fn reserve(app: &App) -> Self {
        Self {
            value: Rc::default(),
            app: Rc::downgrade(&app.inner),
        }
    }
    pub fn downgrade(&self) -> WeakEntity<V> {
        WeakEntity {
            value: Rc::downgrade(&self.value),
            app: self.app.clone(),
        }
    }
    pub fn read<R>(&self, f: impl FnOnce(&V) -> R) -> R {
        f(self.value.borrow().as_ref().expect("entity initialized"))
    }
}
impl<V: View> Entity<V> {
    pub fn update<R>(
        &self,
        cx: &mut crate::testing::TestAppContext,
        f: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> R {
        self.update_app(cx.app_mut(), f)
    }
    pub(crate) fn update_app<R>(
        &self,
        app: &mut App,
        f: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> R {
        let mut window = app.window();
        self.update_in_window(app, &mut window, f)
    }
    pub(crate) fn update_in_window<R>(
        &self,
        app: &mut App,
        window: &mut Window,
        f: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> R {
        app.refresh_globals();
        assert!(
            self.app.ptr_eq(&Rc::downgrade(&app.inner)),
            "entity belongs to another app"
        );
        let mut value = self.value.borrow_mut();
        let view = value.as_mut().expect("entity initialized");
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        let before = serde_json::to_vec(view).ok();
        let notified = app.inner.generation.get();
        let mut cx = Context {
            app,
            entity: self.clone(),
        };
        let result = f(view, window, &mut cx);
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        if let (Some(before), Ok(after)) = (before, serde_json::to_vec(view)) {
            assert!(
                before == after || notified != cx.app.inner.generation.get(),
                "state changed without cx.notify()"
            );
        }
        #[cfg(not(all(debug_assertions, not(target_arch = "wasm32"))))]
        let _ = notified;
        result
    }
}
impl<V: View> WeakEntity<V> {
    pub fn upgrade(&self) -> Option<Entity<V>> {
        if !self.app.upgrade()?.alive.get() {
            return None;
        }
        Some(Entity {
            value: self.value.upgrade()?,
            app: self.app.clone(),
        })
    }
    pub fn update<R>(
        &self,
        cx: &mut AsyncApp,
        f: impl FnOnce(&mut V, &mut Context<V>) -> R,
    ) -> Result<R, Released> {
        self.update_in(cx, |view, _, cx| f(view, cx))
    }
    pub fn update_in<R>(
        &self,
        cx: &mut AsyncApp,
        f: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Result<R, Released> {
        let entity = self.upgrade().ok_or(Released)?;
        let mut app = App::from_state(cx.inner.upgrade().ok_or(Released)?);
        Ok(entity.update_app(&mut app, f))
    }
}
pub struct Context<'a, V> {
    pub(crate) app: &'a mut App,
    pub(crate) entity: Entity<V>,
}
impl<V> Deref for Context<'_, V> {
    type Target = App;
    fn deref(&self) -> &App {
        self.app
    }
}
impl<V> DerefMut for Context<'_, V> {
    fn deref_mut(&mut self) -> &mut App {
        self.app
    }
}
impl<V> Context<'_, V> {
    pub fn notify(&mut self) {
        self.app.notify();
    }
    pub fn entity(&self) -> Entity<V> {
        self.entity.clone()
    }
    pub fn weak_entity(&self) -> WeakEntity<V> {
        self.entity.downgrade()
    }
}
impl<V: View + 'static> Context<'_, V> {
    pub fn listener<E: ?Sized>(
        &self,
        f: impl Fn(&mut V, &E, &mut Window, &mut Context<V>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let entity = self.weak_entity();
        move |event, window, app| {
            if let Some(entity) = entity.upgrade() {
                entity.update_in_window(app, window, |view, window, cx| f(view, event, window, cx));
            }
        }
    }
    pub fn processor<E, R>(
        &self,
        f: impl Fn(&mut V, E, &mut Window, &mut Context<V>) -> R + 'static,
    ) -> impl Fn(E, &mut Window, &mut App) -> R + 'static {
        let entity = self.entity();
        move |event, window, app| {
            entity.update_in_window(app, window, |view, window, cx| f(view, event, window, cx))
        }
    }
    pub fn spawn<R: 'static>(
        &self,
        f: impl AsyncFnOnce(WeakEntity<V>, &mut AsyncApp) -> R + 'static,
    ) -> Task<R> {
        let entity = self.weak_entity();
        self.app.spawn(async move |cx| f(entity, cx).await)
    }
}

#[cfg(test)]
mod global_tests {
    use super::*;

    struct Counter(usize);
    impl gpui::Global for Counter {}

    #[test]
    fn globals_need_not_clone_and_app_snapshots_refresh_safely() {
        let mut driver = App::for_driver(false);
        driver.set_global(Counter(1));
        let mut task = App::from_state(driver.inner.clone());
        let old = driver.global::<Counter>();
        task.set_global(Counter(2));
        assert_eq!(old.0, 1);
        driver.refresh_globals();
        assert_eq!(driver.global::<Counter>().0, 2);
    }

    #[test]
    fn updating_a_global_preserves_other_tasks_updates() {
        struct Other(usize);
        impl gpui::Global for Other {}
        let mut driver = App::for_driver(false);
        let mut task = App::from_state(driver.inner.clone());
        task.set_global(Other(7));
        driver.set_global(Counter(3));
        assert_eq!(driver.global::<Other>().0, 7);
        assert_eq!(driver.global::<Counter>().0, 3);
    }
}
