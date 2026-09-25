use crate::{
    doors::{self, Door},
    host::{malformed, Refusal},
    wire::{Event, Frame, Request},
};
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

type Key = (&'static str, Option<&'static str>, bool);
type Handler = Box<dyn FnMut(&Request) -> Option<Event>>;

#[derive(Default)]
struct State {
    handlers: HashMap<Key, Handler>,
    requests: Vec<Request>,
    events: Vec<Event>,
    logs: Vec<String>,
    links: Vec<String>,
    streams: Vec<Rc<RefCell<StreamState>>>,
    declared: Option<&'static [&'static str]>,
}

/// A typed host whose requests must be explicitly handled by a test.
#[derive(Clone, Default)]
pub struct FakeHost(Rc<RefCell<State>>);

impl FakeHost {
    pub fn handle<C: Door>(
        &self,
        handler: impl FnMut(C::Request) -> Result<C::Reply, Refusal> + 'static,
    ) {
        self.register::<C>(handler, false);
    }
    fn register<C: Door>(
        &self,
        mut handler: impl FnMut(C::Request) -> Result<C::Reply, Refusal> + 'static,
        stream: bool,
    ) {
        self.0.borrow_mut().handlers.insert(
            (C::KIND, C::TARGET, stream),
            Box::new(move |request| {
                let result = C::decode_request(&request.payload)
                    .map_err(malformed)
                    .and_then(&mut handler)
                    .map(|reply| C::encode_reply(&reply));
                Some(Event::Response {
                    id: request.id,
                    result,
                    done: true,
                })
            }),
        );
    }

    pub fn never<C: Door>(&self) {
        for stream in [false, true] {
            self.0.borrow_mut().handlers.insert(
                (C::KIND, C::TARGET, stream),
                Box::new(|request| {
                    C::decode_request(&request.payload).expect("valid capability request");
                    None
                }),
            );
        }
    }

    pub fn refuse<C: Door>(&self, reason: &str, sentence: &str) {
        let refusal = Refusal::new(reason, sentence);
        self.handle::<C>({
            let refusal = refusal.clone();
            move |_| Err(refusal.clone())
        });
        self.register::<C>(move |_| Err(refusal.clone()), true);
    }

    pub fn stream<C: Door>(&self) -> Feed<C> {
        let state = Rc::new(RefCell::new(StreamState::default()));
        let subscription = state.clone();
        self.0.borrow_mut().streams.push(state.clone());
        self.0.borrow_mut().handlers.insert(
            (C::KIND, C::TARGET, true),
            Box::new(move |request| {
                C::decode_request(&request.payload).expect("valid capability request");
                let mut stream = subscription.borrow_mut();
                stream.ids.push(request.id);
                if stream.closed {
                    stream
                        .host
                        .as_ref()
                        .expect("active stream host")
                        .close_stream(request.id);
                }
                None
            }),
        );
        Feed {
            state,
            host: self.clone(),
            marker: PhantomData,
        }
    }

    pub fn asked<C: Door>(&self) -> Vec<C::Request> {
        self.0
            .borrow()
            .requests
            .iter()
            .filter(|r| matches::<C>(r))
            .map(|r| C::decode_request(&r.payload).expect("valid capability request"))
            .collect()
    }
    pub fn logs(&self) -> Vec<String> {
        self.0.borrow().logs.clone()
    }
    pub fn opened_links(&self) -> Vec<String> {
        self.0.borrow().links.clone()
    }
    pub(super) fn reset_connection(&self) {
        let mut state = self.0.borrow_mut();
        state.events.clear();
        for stream in &state.streams {
            let mut stream = stream.borrow_mut();
            if let Some(host) = stream.host.take() {
                for id in stream.ids.drain(..) {
                    host.close_stream(id);
                }
            }
        }
    }
    pub(super) fn declare(&self, capabilities: &'static [&'static str]) {
        self.0.borrow_mut().declared = Some(capabilities);
    }
    pub(super) fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }

    pub(super) fn accept(&self, frame: &Frame, host: &crate::host::Host) {
        let mut state = self.0.borrow_mut();
        for stream in &state.streams {
            let mut stream = stream.borrow_mut();
            stream.host = Some(host.clone());
            stream.ids.retain(|id| !frame.cancels.contains(id));
        }
        for request in &frame.requests {
            state.requests.push(request.clone());
            // The app refuses a door the manifest leaves out
            // (`undeclared_capability`); a test fails on it instead.
            let capability = request
                .kind
                .split_once('.')
                .map_or(&*request.kind, |(c, _)| c);
            if let Some(declared) = state.declared {
                assert!(
                    !doors::is_capability(capability) || declared.contains(&capability),
                    "undeclared_capability: `{}` needs the `{capability}` capability, \
                     which this view's export_view! does not declare",
                    request.kind
                );
            }
            match request.kind.as_str() {
                doors::HostLog::KIND => {
                    state
                        .logs
                        .push(doors::HostLog::decode_request(&request.payload).expect("log line"));
                    continue;
                }
                doors::HostOpenLink::KIND => {
                    state
                        .links
                        .push(doors::HostOpenLink::decode_request(&request.payload).expect("link"));
                    continue;
                }
                _ => {}
            }
            let target = target_of(request);
            let stream = host.is_stream(request.id);
            let key = state
                .handlers
                .keys()
                .find(|(kind, addressed, subscribed)| {
                    *kind == request.kind
                        && *addressed == target.as_deref()
                        && *subscribed == stream
                })
                .or_else(|| {
                    state.handlers.keys().find(|(kind, addressed, subscribed)| {
                        *kind == request.kind && addressed.is_none() && *subscribed == stream
                    })
                })
                .copied();
            let Some(handler) = key.and_then(|key| state.handlers.get_mut(&key)) else {
                panic!(
                    "unhandled {} request {}",
                    request.kind,
                    host.diagnostic(request.id)
                        .unwrap_or_else(|| String::from_utf8_lossy(&request.payload).into_owned())
                );
            };
            if let Some(event) = handler(request) {
                state.events.push(event);
            }
        }
    }
}

/// The program a node door addresses, read off its [`doors::Call`] envelope.
fn target_of(request: &Request) -> Option<String> {
    doors::decode::<doors::Call>(&request.payload)
        .ok()
        .map(|call| call.target)
}

fn matches<C: Door>(request: &Request) -> bool {
    request.kind == C::KIND
        && C::TARGET.is_none_or(|target| target_of(request).as_deref() == Some(target))
}

#[derive(Default)]
struct StreamState {
    ids: Vec<u64>,
    closed: bool,
    host: Option<crate::host::Host>,
}

pub struct Feed<C: Door> {
    state: Rc<RefCell<StreamState>>,
    host: FakeHost,
    marker: PhantomData<C>,
}
impl<C: Door> Feed<C> {
    pub fn push(&self, item: C::Reply) {
        let state = self.state.borrow();
        assert!(!state.closed, "cannot push to a closed stream");
        let payload = C::encode_reply(&item);
        self.host
            .0
            .borrow_mut()
            .events
            .extend(state.ids.iter().map(|id| Event::Response {
                id: *id,
                result: Ok(payload.clone()),
                done: false,
            }));
    }
    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        state.closed = true;
        let host = state.host.clone();
        for id in state.ids.drain(..) {
            host.as_ref().expect("active stream host").close_stream(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doors::{Program, Query, RpcLive};

    struct First;
    struct Second;
    macro_rules! module {
        ($name:ident, $target:literal) => {
            impl Program for $name {
                const NAME: &'static str = $target;
                type Op = ();
                type Query = String;
                type Reply = String;
            }
        };
    }
    module!(First, "first");
    module!(Second, "second");

    fn request<C: Door>(id: u64, value: C::Request) -> Request {
        Request {
            id,
            kind: C::KIND.into(),
            payload: C::encode_request(&value),
        }
    }

    #[test]
    fn handlers_and_request_history_distinguish_envelope_targets() {
        let host = FakeHost::default();
        host.handle::<Query<First>>(|query| Ok(format!("first:{query}")));
        host.handle::<Query<Second>>(|query| Ok(format!("second:{query}")));
        host.accept(
            &Frame {
                requests: vec![
                    request::<Query<First>>(1, "one".into()),
                    request::<Query<Second>>(2, "two".into()),
                ],
                ..Frame::default()
            },
            &crate::host::Host::default(),
        );
        assert_eq!(host.asked::<Query<First>>(), ["one"]);
        assert_eq!(host.asked::<Query<Second>>(), ["two"]);
        let events = host.take_events();
        assert!(
            matches!(&events[0], Event::Response { id: 1, result: Ok(bytes), done: true } if Query::<First>::decode_reply(bytes).unwrap() == "first:one")
        );
        assert!(
            matches!(&events[1], Event::Response { id: 2, result: Ok(bytes), done: true } if Query::<Second>::decode_reply(bytes).unwrap() == "second:two")
        );
    }

    #[test]
    fn streams_stop_delivering_to_cancelled_subscriptions() {
        let host = FakeHost::default();
        let feed = host.stream::<RpcLive>();
        let channel = crate::host::Host::default();
        let stream = channel.subscribe::<RpcLive>("first".into());
        host.accept(
            &Frame {
                requests: channel.drain_outbox(),
                ..Frame::default()
            },
            &channel,
        );
        feed.push(None);
        assert_eq!(host.take_events().len(), 1);
        drop(stream);
        host.accept(
            &Frame {
                cancels: channel.drain_cancels(),
                ..Frame::default()
            },
            &channel,
        );
        feed.push(None);
        assert!(host.take_events().is_empty());
    }

    #[test]
    fn one_capability_can_handle_asks_and_subscriptions_independently() {
        let host = FakeHost::default();
        host.handle::<Query<First>>(|query| Ok(format!("answer:{query}")));
        let feed = host.stream::<Query<First>>();
        let channel = crate::host::Host::default();
        let _ask = channel.ask::<Query<First>>("ask".into());
        let _stream = channel.subscribe::<Query<First>>("subscribe".into());
        host.accept(
            &Frame {
                requests: channel.drain_outbox(),
                ..Frame::default()
            },
            &channel,
        );
        let replies = host.take_events();
        assert!(matches!(
            &replies[..],
            [Event::Response {
                id: 0,
                done: true,
                ..
            }]
        ));
        feed.push("item".into());
        assert!(matches!(
            &host.take_events()[..],
            [Event::Response {
                id: 1,
                done: false,
                ..
            }]
        ));
    }

    #[test]
    fn closing_a_feed_finishes_without_fabricating_an_item_or_refusal() {
        use futures::StreamExt;
        let host = FakeHost::default();
        let feed = host.stream::<RpcLive>();
        let channel = crate::host::Host::default();
        let mut stream = channel.subscribe::<RpcLive>("first".into());
        host.accept(
            &Frame {
                requests: channel.drain_outbox(),
                ..Frame::default()
            },
            &channel,
        );
        feed.close();
        assert!(futures::executor::block_on(stream.next()).is_none());
    }

    #[test]
    #[should_panic(expected = "unhandled rpc.query request")]
    fn unexpected_requests_fail_at_the_host_boundary() {
        FakeHost::default().accept(
            &Frame {
                requests: vec![request::<Query<First>>(1, "unexpected".into())],
                ..Frame::default()
            },
            &crate::host::Host::default(),
        );
    }
}
