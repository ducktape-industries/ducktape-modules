//! The state-and-handlers shape of a view: a [`View`] owns its state, renders
//! it, and reacts through closures handed to [`Cx`] — the way a GPUI view is
//! written. No message enum, no `update` match: a handler IS the state
//! change, and a load IS the future plus the slot it fills.
//!
//! Runs on the ordinary [`Driver`](crate::Driver) through [`Shell`], whose
//! message type is an [`Effect`]: a closure over the view. The host sees the
//! same frames, snapshots and requests it sees from an Elm `App`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::host::{self, Answer, Refusal};
use crate::wire::{self, Node};
use crate::{App, Subscription, Task, slots};

/// What a view is: serialisable state, a render, and the two moments it
/// starts work — boot and restore.
pub trait View: Serialize + DeserializeOwned + 'static {
    /// `"none"` or `"WxH"`, read by the host from the manifest.
    const PREFERRED_WINDOW_SIZE: &'static str = "none";
    fn boot(cx: &mut Cx<Self>) -> Self;
    fn render(&mut self, cx: &mut Cx<Self>) -> Node;
    /// Right after a snapshot came back: every `Loaded::Idle` that should be
    /// loading and every watch that should be open starts again here.
    fn restored(&mut self, _cx: &mut Cx<Self>) {}
}

/// One deferred state change: what a handler, a finished load or a watch
/// item does to the view when the driver runs it.
pub struct Effect<V>(Rc<Run<V>>);

type Run<V> = dyn Fn(&mut V, &mut Cx<V>);
type RunAnswer<V> = dyn Fn(&mut V, Answer, &mut Cx<V>);

impl<V> Clone for Effect<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<V: 'static> Effect<V> {
    /// A state change that runs once: what a child component hands back
    /// for the view to run.
    pub fn once(run: impl FnOnce(&mut V, &mut Cx<V>) + 'static) -> Self {
        let run = RefCell::new(Some(run));
        Self(Rc::new(move |view, cx| {
            if let Some(run) = run.borrow_mut().take() {
                run(view, cx)
            }
        }))
    }
    fn noop() -> Self {
        Self(Rc::new(|_, _| {}))
    }
    /// Runs the change on the view now.
    pub fn run(&self, view: &mut V, cx: &mut Cx<V>) {
        (self.0)(view, cx)
    }
}

/// One `kind` a view may ask the host for, with the request and reply it
/// carries. `KIND` is `<capability>.<operation>`; the host refuses one the
/// manifest did not declare. JSON both ways unless overridden.
pub trait Capability {
    const KIND: &'static str;
    type Request: Serialize;
    type Reply: DeserializeOwned;
    fn encode(request: &Self::Request) -> Vec<u8> {
        serde_json::to_vec(request).expect("request encodes")
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
        serde_json::from_slice(bytes).map_err(|error| host::malformed(error.to_string()))
    }
}

/// A module a view talks to: its name on the node and the types it speaks.
/// Implemented next to the view (a marker type), never by the module crate,
/// which must not link this runtime.
pub trait Module {
    const NAME: &'static str;
    /// `op.submit` payload.
    type Op: Serialize;
    /// `rpc.query` request / reply — the module's own query surface.
    type Query: Serialize;
    type Reply: DeserializeOwned;
    /// `rpc.view` request / reply — the module's index-tier view.
    type ViewQuery: Serialize;
    type ViewReply: DeserializeOwned;
}

/// A [`Capability`] whose request and reply are both JSON.
///
/// `capability!(Pick, "fs.pick", Value, Vec<SelectedFile>);`
#[macro_export]
macro_rules! capability {
    ($name:ident, $kind:literal, $request:ty, $reply:ty) => {
        pub struct $name;
        impl $crate::view::Capability for $name {
            const KIND: &'static str = $kind;
            type Request = $request;
            type Reply = $reply;
        }
    };
}

/// `rpc.view` against `M`.
pub struct ViewOf<M>(std::marker::PhantomData<M>);
impl<M: Module> Capability for ViewOf<M> {
    const KIND: &'static str = "rpc.view";
    type Request = M::ViewQuery;
    type Reply = M::ViewReply;
    fn encode(request: &Self::Request) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "target": M::NAME, "query": request }))
            .expect("request encodes")
    }
}

/// `rpc.query` against `M`.
pub struct Query<M>(std::marker::PhantomData<M>);
impl<M: Module> Capability for Query<M> {
    const KIND: &'static str = "rpc.query";
    type Request = M::Query;
    type Reply = M::Reply;
    fn encode(request: &Self::Request) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "target": M::NAME, "query": request }))
            .expect("request encodes")
    }
}

/// `op.submit` to `M`; the reply is whatever the node echoes.
pub struct Submit<M>(std::marker::PhantomData<M>);
impl<M: Module> Capability for Submit<M> {
    const KIND: &'static str = "op.submit";
    type Request = M::Op;
    type Reply = serde_json::Value;
    fn encode(request: &Self::Request) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "target": M::NAME, "payload": request }))
            .expect("request encodes")
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(bytes).map_err(|error| host::malformed(error.to_string()))
    }
}

/// `rpc.live`: one item per state change of the named module.
pub struct Live;
impl Capability for Live {
    const KIND: &'static str = "rpc.live";
    type Request = String;
    type Reply = ();
    fn encode(module: &String) -> Vec<u8> {
        module.as_bytes().to_vec()
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
}

/// `host.visible`: whether the view is on screen.
pub struct Visible;
impl Capability for Visible {
    const KIND: &'static str = "host.visible";
    type Request = ();
    type Reply = bool;
    fn encode(_: &()) -> Vec<u8> {
        Vec::new()
    }
    fn decode(bytes: &[u8]) -> Result<bool, Refusal> {
        Ok(bytes == b"true" || bytes == b"1" || bytes == b"visible")
    }
}

/// One request, typed by its capability, from inside a future that has no
/// `Cx`: a load that pages through several replies asks here.
pub fn ask<C: Capability>(
    request: C::Request,
) -> impl Future<Output = Result<C::Reply, Refusal>> + 'static {
    let response = host::request(C::KIND, &C::encode(&request));
    async move { C::decode(&response.await?) }
}

/// Data the view asked for, in the four states it can be in. `Loading`
/// carries the abort handle: dropping the slot cancels the load.
/// Serialises `Loading` as `Idle`, so a restored view reloads it.
#[derive(Default)]
pub enum Loaded<T> {
    #[default]
    Idle,
    Loading(wire::task::Handle),
    Ready(T),
    Failed(Refusal),
}

impl<T> Loaded<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }
    pub fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading(_))
    }
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
    pub fn failed(&self) -> Option<&Refusal> {
        match self {
            Self::Failed(refusal) => Some(refusal),
            _ => None,
        }
    }
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LoadedSnapshot<T> {
    Idle,
    Ready(T),
    Failed(Refusal),
}

impl<T: Serialize> Serialize for Loaded<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Idle | Self::Loading(_) => LoadedSnapshot::<&T>::Idle,
            Self::Ready(value) => LoadedSnapshot::Ready(value),
            Self::Failed(refusal) => LoadedSnapshot::Failed(refusal.clone()),
        }
        .serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Loaded<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match LoadedSnapshot::deserialize(deserializer)? {
            LoadedSnapshot::Idle => Self::Idle,
            LoadedSnapshot::Ready(value) => Self::Ready(value),
            LoadedSnapshot::Failed(refusal) => Self::Failed(refusal),
        })
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Loaded<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => f.write_str("Idle"),
            Self::Loading(_) => f.write_str("Loading"),
            Self::Ready(value) => f.debug_tuple("Ready").field(value).finish(),
            Self::Failed(refusal) => f.debug_tuple("Failed").field(refusal).finish(),
        }
    }
}

/// One open host stream and the handler its items run.
struct Watch<V> {
    id: u64,
    kind: &'static str,
    payload: Vec<u8>,
    run: Rc<RunAnswer<V>>,
}

impl<V> Hash for Watch<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.id, self.kind, &self.payload).hash(state);
    }
}

fn watch_stream<V: 'static>(watch: &Rc<Watch<V>>) -> wire::task::BoxStream<Effect<V>> {
    let run = watch.run.clone();
    host::subscribe(watch.kind, &watch.payload)
        .map(move |answer| {
            let run = run.clone();
            Effect::once(move |view, cx| run(view, answer, cx))
        })
        .boxed_local()
}

type Watches<V> = Rc<RefCell<BTreeMap<u64, Rc<Watch<V>>>>>;

/// A watch stays open while this is alive. `#[serde(skip)]` it; `restored`
/// opens it again.
pub struct Watching {
    id: u64,
    watches: Rc<dyn Fn(u64)>,
}

impl Drop for Watching {
    fn drop(&mut self) {
        (self.watches)(self.id);
    }
}

impl std::fmt::Debug for Watching {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Watching({})", self.id)
    }
}

/// What a view reaches the runtime through: handlers into the frame, work
/// onto the executor, requests and streams to the host.
pub struct Cx<V> {
    tasks: Vec<Task<Effect<V>>>,
    watches: Watches<V>,
    next_watch: Rc<Cell<u64>>,
}

impl<V: 'static> Default for Cx<V> {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            watches: Rc::default(),
            next_watch: Rc::default(),
        }
    }
}

impl<V: 'static> Cx<V> {
    /// A press handler: the id a `Node`'s `on_press` takes.
    pub fn on(&mut self, run: impl FnMut(&mut V, &mut Cx<V>) + 'static) -> u32 {
        let run = RefCell::new(run);
        slots::message(Effect(Rc::new(move |view, cx| {
            (run.borrow_mut())(view, cx)
        })))
    }

    /// A handler with the value the host echoes: `String` for an input,
    /// `bool` for a toggle, `f32` for a slider, `u32` for a pick, `(f32,
    /// f32)` for a size or pointer, `(f64, f64)` for a drag.
    pub fn on_value<A: 'static>(
        &mut self,
        run: impl FnMut(&mut V, A, &mut Cx<V>) + 'static,
    ) -> u32 {
        let run = Rc::new(RefCell::new(run));
        slots::handler::<A, Effect<V>>(Box::new(move |value| {
            let run = run.clone();
            Some(Effect::once(move |view, cx| {
                (run.borrow_mut())(view, value, cx)
            }))
        }))
    }

    /// Runs `work` on the executor; what it resolves to runs on the view.
    /// The handle aborts it; dropped plain, the work runs to completion.
    pub fn spawn<R>(&mut self, work: impl Future<Output = R> + 'static) -> wire::task::Handle
    where
        R: FnOnce(&mut V, &mut Cx<V>) + 'static,
    {
        let (task, handle) = Task::perform(work, Effect::once).abortable();
        self.tasks.push(task);
        handle
    }

    /// A re-read that leaves what is on screen in place until fresh data
    /// lands; a refusal changes nothing.
    pub fn refresh<T: 'static>(
        &mut self,
        work: impl Future<Output = Result<T, Refusal>> + 'static,
        land: impl FnOnce(&mut V, T, &mut Cx<V>) + 'static,
    ) {
        self.spawn(async move {
            let result = work.await;
            move |view: &mut V, cx: &mut Cx<V>| {
                if let Ok(value) = result {
                    land(view, value, cx);
                }
            }
        });
    }

    /// Starts a load and hands back the `Loading` to put in the slot `at`
    /// names; the result lands in the same slot. Replacing the slot aborts
    /// the load.
    pub fn load<T: 'static>(
        &mut self,
        work: impl Future<Output = Result<T, Refusal>> + 'static,
        at: impl Fn(&mut V) -> &mut Loaded<T> + 'static,
    ) -> Loaded<T> {
        let handle = self.spawn(async move {
            let result = work.await;
            move |view: &mut V, _: &mut Cx<V>| {
                *at(view) = match result {
                    Ok(value) => Loaded::Ready(value),
                    Err(refusal) => Loaded::Failed(refusal),
                }
            }
        });
        Loaded::Loading(handle.abort_on_drop())
    }

    /// One request, typed by its capability.
    pub fn ask<C: Capability>(
        &self,
        request: C::Request,
    ) -> impl Future<Output = Result<C::Reply, Refusal>> + 'static {
        ask::<C>(request)
    }

    /// A host stream, typed by its capability; every item runs `run`. Open
    /// while the returned guard lives.
    pub fn watch<C: Capability>(
        &mut self,
        request: C::Request,
        run: impl FnMut(&mut V, Result<C::Reply, Refusal>, &mut Cx<V>) + 'static,
    ) -> Watching {
        let id = self.next_watch.get();
        self.next_watch.set(id + 1);
        let run = RefCell::new(run);
        let watch = Watch {
            id,
            kind: C::KIND,
            payload: C::encode(&request),
            run: Rc::new(move |view: &mut V, answer: Answer, cx: &mut Cx<V>| {
                (run.borrow_mut())(view, answer.and_then(|bytes| C::decode(&bytes)), cx)
            }),
        };
        self.watches.borrow_mut().insert(id, Rc::new(watch));
        let watches = self.watches.clone();
        Watching {
            id,
            watches: Rc::new(move |id| {
                watches.borrow_mut().remove(&id);
            }),
        }
    }

    /// Tells the host something nobody waits on.
    pub fn notify<C: Capability>(&self, request: C::Request) {
        host::notify(C::KIND, &C::encode(&request));
    }

    /// A mutation of this view's mounted widget tree (focus, an editor
    /// action); nothing comes back.
    pub fn widget(&mut self, command: wire::WidgetCommand) {
        self.tasks.push(crate::widget::perform(command));
    }

    fn take_tasks(&mut self) -> Task<Effect<V>> {
        Task::batch(std::mem::take(&mut self.tasks))
    }

    fn subscription(&self) -> Subscription<Effect<V>> {
        Subscription::batch(
            self.watches
                .borrow()
                .values()
                .map(|watch| Subscription::run_with(watch.clone(), watch_stream)),
        )
    }
}

/// The `App` a [`View`] runs as. `export_view!` names it; a test drives
/// `Driver::<Shell<MyView>>::new()`.
pub struct Shell<V: View> {
    view: RefCell<V>,
    cx: RefCell<Cx<V>>,
}

impl<V: View> Shell<V> {
    pub const PREFERRED_WINDOW_SIZE: &'static str = V::PREFERRED_WINDOW_SIZE;

    pub fn state(&self) -> std::cell::Ref<'_, V> {
        self.view.borrow()
    }

    /// The state, writable: for a test that puts the view in a position the
    /// protocol alone reaches slowly. Take effect on the next tick.
    pub fn state_mut(&self) -> std::cell::RefMut<'_, V> {
        self.view.borrow_mut()
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&*self.view.borrow()).map_err(|error| error.to_string())
    }

    pub fn restore(bytes: &[u8]) -> Result<Self, String> {
        let mut view: V = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        let mut cx = Cx::default();
        view.restored(&mut cx);
        if !cx.tasks.is_empty() {
            // A restore has no update to spawn from: the first tick runs
            // this and drains what `restored` started.
            slots::defer(vec![Effect::<V>::noop()]);
        }
        Ok(Self {
            view: RefCell::new(view),
            cx: RefCell::new(cx),
        })
    }
}

impl<V: View> crate::SnapshotApp for Shell<V> {
    fn snapshot(&self) -> Result<Vec<u8>, String> {
        Shell::snapshot(self)
    }
    fn restore(bytes: &[u8]) -> Result<Self, String> {
        Shell::restore(bytes)
    }
}

impl<V: View> App for Shell<V> {
    type Message = Effect<V>;

    fn boot() -> (Self, Task<Effect<V>>) {
        let mut cx = Cx::default();
        let view = V::boot(&mut cx);
        let boot = cx.take_tasks();
        (
            Self {
                view: RefCell::new(view),
                cx: RefCell::new(cx),
            },
            boot,
        )
    }

    fn view(&self) -> Node {
        let mut cx = self.cx.borrow_mut();
        let node = self.view.borrow_mut().render(&mut cx);
        if !cx.tasks.is_empty() {
            slots::defer(vec![Effect::<V>::noop()]);
        }
        node
    }

    fn update(&mut self, effect: Effect<V>) -> Task<Effect<V>> {
        let mut cx = self.cx.borrow_mut();
        (effect.0)(&mut self.view.borrow_mut(), &mut cx);
        cx.take_tasks()
    }

    fn subscription(&self) -> Subscription<Effect<V>> {
        self.cx.borrow().subscription()
    }
}

/// Exports a [`View`] as the wasm component the host loads. Same arguments
/// as [`export_app!`](crate::export_app): the type, its name, a sentence,
/// and the capabilities it may ask for.
#[macro_export]
macro_rules! export_view {
    ($view:ty, $name:expr, $description:expr, [$($capability:literal),* $(,)?]) => {
        $crate::export_driver!($crate::view::Shell<$view>, $name, $description, [$($capability),*]);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Driver;
    use crate::testing;

    struct Echo;
    impl Capability for Echo {
        const KIND: &'static str = "test.echo";
        type Request = u32;
        type Reply = u32;
    }

    #[derive(Serialize, Deserialize, Default)]
    struct Counter {
        count: u32,
        number: Loaded<u32>,
        items: Vec<u32>,
        #[serde(skip)]
        feed: Option<Watching>,
    }

    impl View for Counter {
        fn boot(cx: &mut Cx<Self>) -> Self {
            Self {
                number: cx.load(cx.ask::<Echo>(7), |v| &mut v.number),
                feed: Some(cx.watch::<Echo>(1, |v, item, _| v.items.push(item.unwrap()))),
                ..Self::default()
            }
        }
        fn render(&mut self, cx: &mut Cx<Self>) -> Node {
            let press = cx.on(|v, _| v.count += 1);
            let label = match &self.number {
                Loaded::Ready(n) => format!("{} {}", self.count, n),
                Loaded::Loading(_) => format!("{} loading", self.count),
                _ => format!("{} idle", self.count),
            };
            wire::kit::button("press", label, Some(press), Default::default())
        }
        fn restored(&mut self, cx: &mut Cx<Self>) {
            if self.number.is_idle() {
                self.number = cx.load(cx.ask::<Echo>(7), |v| &mut v.number);
            }
        }
    }

    #[test]
    fn handlers_loads_and_watches_reach_the_view() {
        let mut driver = Driver::<Shell<Counter>>::new();
        let frame = driver.tick(vec![]);
        assert_eq!(testing::texts(&frame), vec!["0 loading"]);
        let requests = frame.requests;
        assert_eq!(requests.len(), 2);
        let load = requests.iter().find(|r| r.payload == b"7").unwrap().id;
        let feed = requests.iter().find(|r| r.payload == b"1").unwrap().id;

        let frame = driver.tick(vec![testing::answer(load, b"42")]);
        assert_eq!(testing::texts(&frame), vec!["0 42"]);

        let frame = driver.tick(testing::press(&frame, "press"));
        assert_eq!(testing::texts(&frame), vec!["1 42"]);

        driver.tick(vec![testing::item(feed, b"5"), testing::item(feed, b"6")]);
        assert_eq!(driver.app.state().items, vec![5, 6]);

        // Settled: snapshot works, drops Loading to Idle, restore reloads.
        let bytes = driver.snapshot().unwrap();
        let mut restored = Driver::<Shell<Counter>>::from_snapshot(&bytes, false).unwrap();
        let frame = restored.tick(vec![]);
        assert_eq!(testing::texts(&frame), vec!["1 42"]);
        assert!(frame.requests.is_empty());
    }

    #[test]
    fn a_dropped_slot_aborts_its_load_and_a_dropped_guard_its_watch() {
        let mut driver = Driver::<Shell<Counter>>::new();
        let frame = driver.tick(vec![]);
        assert_eq!(frame.requests.len(), 2);
        driver.tick(vec![]);
        {
            let mut view = driver.app.view.borrow_mut();
            view.number = Loaded::Idle;
            view.feed = None;
        }
        let frame = driver.tick(vec![]);
        assert_eq!(frame.cancels.len(), 2);
        assert!(driver.snapshot().is_ok());
    }
}
