//! Driver-owned host requests and cancellable streams.
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::methods::{self, Method};
use futures::{Stream, StreamExt};
use std::rc::Rc;

use crate::wire::Request;

/// What one answer carries; the host's refusal is the `Err`.
///
/// A refusal arrives already split into a stable `code` token and the
/// refusing module's own `message` ([`Error`]), so no view parses a
/// transport envelope to find out what happened. A screen shows the
/// `message`.
pub type Answer = Result<Vec<u8>, Error>;

pub use error::Error;

/// The host answered and the bytes are not what this view expected — a decode
/// failure on OUR side, not a refusal anyone authored. One token in one place,
/// so `.map_err(host::malformed)` reads the same in every view.
pub fn malformed(error: String) -> Error {
    Error::new(::error::code::UNEXPECTED_REPLY, error)
}

/// A reply of another variant than the question asks for: the program
/// answered something else. Every typed ask's fallback arm.
pub fn wrong_reply() -> Error {
    malformed("the program answered another question".into())
}

/// One page of a cursored listing: its rows and the cursor of the page after.
pub type Page<T> = (Vec<T>, Option<Vec<u8>>);

/// Follows a cursored listing from `after`: asks page after page, feeding
/// each `next` back, until the listing ends or `max_pages` pages are read.
/// Returns every row read and the cursor still to follow (`None`: all of it).
pub async fn pages<T, F: Future<Output = Result<Page<T>, Error>>>(
    mut after: Option<Vec<u8>>,
    max_pages: usize,
    mut ask: impl FnMut(Option<Vec<u8>>) -> F,
) -> Result<Page<T>, Error> {
    let mut all = Vec::new();
    for _ in 0..max_pages {
        let (rows, next) = ask(after).await?;
        all.extend(rows);
        after = next;
        if after.is_none() {
            break;
        }
    }
    Ok((all, after))
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
    /// One request, answered once. The method names the kind and both codecs;
    /// there is no other way to ask, so there is no other codec.
    pub fn ask<D: Method>(
        &self,
        request: D::Request,
    ) -> impl Future<Output = Result<D::Reply, Error>> + 'static {
        let response = self.request(D::KIND, &D::encode_request(&request));
        self.remember(response.id, &request);
        async move { D::decode_reply(&response.await?).map_err(malformed) }
    }
    /// A subscription: an item per answer until the stream is dropped.
    pub fn subscribe<D: Method>(
        &self,
        request: D::Request,
    ) -> impl Stream<Item = Result<D::Reply, Error>> + Unpin + 'static {
        let subscription = self.raw_subscribe(D::KIND, &D::encode_request(&request));
        self.remember(subscription.id, &request);
        subscription
            .map(|answer| answer.and_then(|bytes| D::decode_reply(&bytes).map_err(malformed)))
    }
    /// A request whose answer nobody waits for.
    pub fn notify<D: Method>(&self, request: D::Request) {
        let id = self
            .0
            .borrow_mut()
            .ask(D::KIND, &D::encode_request(&request));
        self.remember(id, &request);
    }
    /// The request as text, for a test's "unhandled request" panic. Views
    /// run as wasm, where nothing reads it, so they skip the formatting.
    fn remember(&self, id: u64, request: &impl std::fmt::Debug) {
        if cfg!(not(target_arch = "wasm32")) {
            self.0
                .borrow_mut()
                .diagnostics
                .insert(id, format!("{request:?}"));
        }
    }
    pub fn log(&self, message: impl AsRef<str>) {
        self.notify::<methods::HostLog>(message.as_ref().to_owned());
    }
    /// A refusal nothing on screen waits for, kept in the host's log:
    /// `<view>: <what> refused: <refusal>`.
    pub fn log_refused(&self, view: &str, what: &str, refusal: &Error) {
        self.log(format!("{view}: {what} refused: {refusal}"));
    }
    pub fn open_link(&self, link: &str) {
        self.notify::<methods::LinkOpen>(link.to_owned());
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
            None if slot.closed => Poll::Ready(Err(Error::new(
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

#[cfg(test)]
mod pages_tests {
    use super::{pages, Page};

    /// A listing of 0..10 served three rows a page.
    fn listing(after: Option<Vec<u8>>) -> std::future::Ready<Result<Page<u8>, super::Error>> {
        let start = after.map_or(0, |cursor| cursor[0]);
        let end = (start + 3).min(10);
        let next = (end < 10).then(|| vec![end]);
        std::future::ready(Ok(((start..end).collect(), next)))
    }

    #[test]
    fn pages_follow_the_cursor_to_the_end_or_the_cap() {
        let all = futures::executor::block_on(pages(None, 16, listing)).unwrap();
        assert_eq!(all, ((0..10).collect(), None));
        let capped = futures::executor::block_on(pages(None, 2, listing)).unwrap();
        assert_eq!(capped, ((0..6).collect(), Some(vec![6])));
        let resumed = futures::executor::block_on(pages(Some(vec![6]), 16, listing)).unwrap();
        assert_eq!(resumed, ((6..10).collect(), None));
    }
}
