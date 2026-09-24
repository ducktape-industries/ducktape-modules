//! Driver-owned host requests and cancellable streams.
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::doors::{self, Door};
use futures::{Stream, StreamExt};
use std::rc::Rc;

use crate::wire::Request;

/// What one answer carries; the host's refusal is the `Err`.
///
/// A refusal arrives already split into a stable `reason` token and the
/// refusing module's own `sentence` ([`Refusal`]), so no view parses a
/// transport envelope to find out what happened. `Display` writes the
/// sentence, which is what a screen shows.
pub type Answer = Result<Vec<u8>, Refusal>;

pub use crate::wire::Refusal;

/// The host answered and the bytes are not what this view expected — a decode
/// failure on OUR side, not a refusal anyone authored. One token in one place,
/// so `.map_err(host::malformed)` reads the same in every view.
pub fn malformed(error: String) -> Refusal {
    Refusal::new("malformed_reply", error)
}

/// The sentence alone, for a view that only SHOWS a refusal.
///
/// It is a named function and not a `From<Refusal> for String` ON PURPOSE:
/// with a `From`, a plain `?` would flatten a refusal to prose silently, and
/// the next screen that needs to tell "never" from "not yet" would be back to
/// reading the words. `.map_err(host::said)` says out loud that this path shows
/// the refusal and branches on nothing.
pub fn said(refused: Refusal) -> String {
    refused.sentence
}

#[derive(Default)]
struct Slot {
    stream: bool,
    yield_next: bool,
    answers: VecDeque<Answer>,
    closed: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
struct Registry {
    next_id: u64,
    outbox: Vec<Request>,
    pending: HashMap<u64, Arc<Mutex<Slot>>>,
    cancels: Vec<u64>,
    diagnostics: HashMap<u64, String>,
}

impl Registry {
    fn ask(&mut self, kind: &str, payload: &[u8]) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.outbox.push(Request {
            id,
            kind: kind.to_string(),
            payload: payload.to_vec(),
        });
        id
    }
}

/// The request channel owned by one driver; clones share only that driver.
#[derive(Clone, Default)]
pub struct Host(Rc<RefCell<Registry>>);

impl Host {
    fn open(&self, kind: &str, payload: &[u8]) -> (u64, Arc<Mutex<Slot>>) {
        let slot = Arc::new(Mutex::new(Slot::default()));
        let mut registry = self.0.borrow_mut();
        let id = registry.ask(kind, payload);
        registry.pending.insert(id, slot.clone());
        (id, slot)
    }
    fn close(&self, id: u64) {
        let mut registry = self.0.borrow_mut();
        if registry.pending.remove(&id).is_some() {
            registry.cancels.push(id);
        }
        registry.diagnostics.remove(&id);
    }
    pub(crate) fn request(&self, kind: &str, payload: &[u8]) -> Response {
        let (id, slot) = self.open(kind, payload);
        Response {
            id,
            slot,
            host: self.clone(),
        }
    }
    pub(crate) fn raw_subscribe(&self, kind: &str, payload: &[u8]) -> Subscription {
        let (id, slot) = self.open(kind, payload);
        slot.lock().expect("stream slot").stream = true;
        Subscription {
            id,
            slot,
            host: self.clone(),
        }
    }
    /// One request, answered once. The door names the kind and both codecs;
    /// there is no other way to ask, so there is no other codec.
    pub fn ask<D: Door>(
        &self,
        request: D::Request,
    ) -> impl Future<Output = Result<D::Reply, Refusal>> + 'static {
        let response = self.request(D::KIND, &D::encode_request(&request));
        self.0
            .borrow_mut()
            .diagnostics
            .insert(response.id, format!("{request:?}"));
        async move { D::decode_reply(&response.await?).map_err(malformed) }
    }
    /// A subscription: an item per answer until the stream is dropped.
    pub fn subscribe<D: Door>(
        &self,
        request: D::Request,
    ) -> impl Stream<Item = Result<D::Reply, Refusal>> + Unpin + 'static {
        let subscription = self.raw_subscribe(D::KIND, &D::encode_request(&request));
        self.0
            .borrow_mut()
            .diagnostics
            .insert(subscription.id, format!("{request:?}"));
        subscription
            .map(|answer| answer.and_then(|bytes| D::decode_reply(&bytes).map_err(malformed)))
    }
    /// A request whose answer nobody waits for.
    pub fn notify<D: Door>(&self, request: D::Request) {
        let id = self
            .0
            .borrow_mut()
            .ask(D::KIND, &D::encode_request(&request));
        self.0
            .borrow_mut()
            .diagnostics
            .insert(id, format!("{request:?}"));
    }
    pub fn log(&self, message: impl AsRef<str>) {
        self.notify::<doors::HostLog>(message.as_ref().to_owned());
    }
    pub fn open_link(&self, link: &str) {
        self.notify::<doors::HostOpenLink>(link.to_owned());
    }
    pub(crate) fn diagnostic(&self, id: u64) -> Option<String> {
        self.0.borrow().diagnostics.get(&id).cloned()
    }
    pub(crate) fn pending_requests(&self) -> bool {
        self.0
            .borrow()
            .pending
            .values()
            .any(|slot| !slot.lock().expect("request slot").stream)
    }
    pub(crate) fn waiting_stream(&self, waker: &Waker) -> bool {
        self.0.borrow().pending.values().any(|slot| {
            let slot = slot.lock().expect("stream slot");
            slot.stream
                && !slot.closed
                && slot.answers.is_empty()
                && slot
                    .waker
                    .as_ref()
                    .is_some_and(|waiting| waiting.will_wake(waker))
        })
    }
    pub(crate) fn is_stream(&self, id: u64) -> bool {
        self.0
            .borrow()
            .pending
            .get(&id)
            .is_some_and(|slot| slot.lock().expect("request slot").stream)
    }
}

/// The host's eventual answer to a [`Host::ask`].
pub(crate) struct Response {
    id: u64,
    slot: Arc<Mutex<Slot>>,
    host: Host,
}

impl Drop for Response {
    fn drop(&mut self) {
        self.host.close(self.id);
    }
}

impl Future for Response {
    type Output = Answer;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Answer> {
        let mut slot = self.slot.lock().expect("response slot");
        match slot.answers.pop_front() {
            Some(answer) => Poll::Ready(answer),
            None if slot.closed => Poll::Ready(Err(Refusal::new(
                "request_closed",
                "the host closed the request",
            ))),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Every answer the host sends to a [`Host::subscribe`], until it closes.
pub(crate) struct Subscription {
    id: u64,
    slot: Arc<Mutex<Slot>>,
    host: Host,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.host.close(self.id);
    }
}

impl Stream for Subscription {
    type Item = Answer;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Answer>> {
        let mut slot = self.slot.lock().expect("subscription slot");
        if std::mem::take(&mut slot.yield_next) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        match slot.answers.pop_front() {
            Some(answer) => {
                slot.yield_next = true;
                slot.waker = None;
                Poll::Ready(Some(answer))
            }
            None if slot.closed => Poll::Ready(None),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Host {
    /// Everything asked since the last frame, in order.
    pub(crate) fn drain_outbox(&self) -> Vec<Request> {
        let mut registry = self.0.borrow_mut();
        let keep: std::collections::HashSet<_> = registry
            .pending
            .keys()
            .copied()
            .chain(registry.outbox.iter().map(|request| request.id))
            .collect();
        registry.diagnostics.retain(|id, _| keep.contains(id));
        std::mem::take(&mut registry.outbox)
    }

    /// Everything abandoned since the last frame.
    pub(crate) fn drain_cancels(&self) -> Vec<u64> {
        std::mem::take(&mut self.0.borrow_mut().cancels)
    }

    /// Delivers one answer; an id nobody waits for is dropped.
    pub(crate) fn close_stream(&self, id: u64) {
        let slot = self.0.borrow_mut().pending.remove(&id);
        if let Some(slot) = slot {
            let mut slot = slot.lock().expect("answer slot");
            slot.closed = true;
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }

    pub(crate) fn fulfill(&self, id: u64, answer: Answer, done: bool) {
        let slot = {
            let mut registry = self.0.borrow_mut();
            if done {
                registry.pending.remove(&id)
            } else {
                registry.pending.get(&id).cloned()
            }
        };
        if let Some(slot) = slot {
            let mut slot = slot.lock().expect("answer slot");
            slot.answers.push_back(answer);
            slot.closed |= done;
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }
}
