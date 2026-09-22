use super::*;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Default, Serialize, Deserialize)]
struct Probe {
    received: Vec<String>,
    #[serde(skip)]
    streams: Vec<Task<()>>,
    #[serde(skip)]
    renders: usize,
    #[serde(skip)]
    listener: u32,
}
impl Probe {
    fn watch(&mut self, cx: &mut Context<Self>, kind: &'static str) {
        let mut stream = cx.host().raw_subscribe(kind, &[]);
        self.streams.push(cx.spawn(async move |this, cx| {
            while let Some(reply) = stream.next().await {
                reply.unwrap();
                if this
                    .update(cx, |view, cx| {
                        view.received.push(kind.into());
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}
impl View for Probe {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.watch(cx, "visible");
        view.watch(cx, "data");
        view
    }
    fn restored(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.streams = Self::new(window, cx).streams;
    }
}
impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> wire::Node {
        self.renders += 1;
        self.listener = cx.listener(|view, _: &(), _, cx| {
            view.received.push("press".into());
            cx.notify();
        });
        wire::kit::button(
            "press",
            self.received.len().to_string(),
            Some(self.listener),
            Default::default(),
        )
    }
}
fn response(id: u64, done: bool) -> wire::Event {
    wire::Event::Response {
        id,
        result: Ok(Vec::new()),
        done,
    }
}

#[test]
fn held_streams_remain_open_and_dropping_tasks_cancels_them() {
    let mut driver = Driver::<Probe>::new();
    let first = driver.tick(vec![]);
    assert_eq!(first.requests.len(), 2);
    let id = first.requests[0].id;
    assert!(driver.tick(vec![]).requests.is_empty());
    driver.tick(vec![response(id, false)]);
    driver
        .entity()
        .read(|view| assert_eq!(view.received, ["visible"]));
    assert!(
        driver.snapshot().is_ok(),
        "held streams can be restored independently"
    );
    driver.entity().update_app(driver.app_mut(), |view, _, cx| {
        view.streams.clear();
        view.watch(cx, "replacement");
    });
    let frame = driver.tick(vec![]);
    assert_eq!(frame.cancels.len(), 2);
    assert!(frame.cancels.contains(&id));
    assert_eq!(frame.requests.len(), 1);
    assert_eq!(frame.requests[0].kind, "replacement");
}

#[test]
fn snapshot_refuses_unsettled_writes_and_failed_restore_keeps_old_driver() {
    let mut driver = Driver::<Probe>::new();
    driver.tick(vec![]);
    driver
        .app_mut()
        .spawn(async move |cx| {
            cx.host().request("write", &[]).await.unwrap();
        })
        .detach();
    let frame = driver.tick(vec![]);
    assert!(driver.snapshot().is_err());
    let write = frame
        .requests
        .iter()
        .find(|request| request.kind == "write")
        .unwrap();
    driver.tick(vec![response(write.id, true)]);
    let snapshot = driver.snapshot().unwrap();
    assert!(Driver::<Probe>::from_snapshot(&[], false).is_err());
    assert_eq!(driver.snapshot().unwrap(), snapshot);
    let mut restored = Driver::<Probe>::from_snapshot(&snapshot, false).unwrap();
    assert_eq!(restored.tick(vec![]).requests.len(), 2);
}

#[test]
fn stream_updates_follow_response_order_instead_of_spawn_order() {
    let mut driver = Driver::<Probe>::new();
    let requests = driver.tick(vec![]).requests;
    for order in [["data", "visible"], ["visible", "data"]] {
        driver.entity().update_app(driver.app_mut(), |view, _, cx| {
            view.received.clear();
            cx.notify();
        });
        let events = order
            .iter()
            .map(|kind| {
                response(
                    requests
                        .iter()
                        .find(|request| request.kind == *kind)
                        .unwrap()
                        .id,
                    false,
                )
            })
            .collect();
        driver.tick(events);
        driver
            .entity()
            .read(|view| assert_eq!(view.received, order));
    }
}

#[test]
fn unchanged_frames_preserve_listener_tables_and_resync_renders() {
    let mut driver = Driver::<Probe>::new();
    assert!(driver.tick(vec![]).root.is_some());
    let (listener, renders) = driver.entity().read(|view| (view.listener, view.renders));
    for _ in 0..3 {
        assert!(driver.tick(vec![]).unchanged);
    }
    driver
        .entity()
        .read(|view| assert_eq!(view.renders, renders));
    let frame = driver.tick(vec![wire::Event::Message(listener)]);
    assert!(!frame.unchanged);
    driver
        .entity()
        .read(|view| assert_eq!(view.received, ["press"]));
    let renders = driver.entity().read(|view| view.renders);
    assert!(driver.tick(vec![wire::Event::Resync]).root.is_some());
    driver
        .entity()
        .read(|view| assert_eq!(view.renders, renders + 1));
}

#[test]
fn two_drivers_on_one_thread_have_independent_hosts() {
    let mut first = Driver::<Probe>::new();
    let mut second = Driver::<Probe>::new();
    let first_requests = first.tick(vec![]).requests;
    let second_requests = second.tick(vec![]).requests;
    assert_eq!(first_requests.len(), 2);
    assert_eq!(second_requests.len(), 2);
    assert_eq!(
        first_requests[0].id, second_requests[0].id,
        "each host starts its own ID sequence"
    );
    first.tick(vec![response(first_requests[0].id, false)]);
    second.tick(vec![]);
    first
        .entity()
        .read(|view| assert_eq!(view.received, ["visible"]));
    second
        .entity()
        .read(|view| assert!(view.received.is_empty()));
    first.app_mut().host().log("only first");
    assert!(second.tick(vec![]).requests.is_empty());
    let frame = first.tick(vec![]);
    assert_eq!(frame.requests.len(), 1);
    assert_eq!(frame.requests[0].payload, b"only first");
}

#[test]
fn tasks_are_awaitable_drop_cancels_and_detach_runs() {
    let mut driver = Driver::<Probe>::new();
    let order = Rc::new(RefCell::new(Vec::new()));
    let child_order = order.clone();
    let child = driver.app_mut().spawn(async move |_| {
        child_order.borrow_mut().push(1);
        3
    });
    let parent_order = order.clone();
    let parent = driver.app_mut().spawn(async move |_| {
        let value = child.await;
        parent_order.borrow_mut().push(value);
        value + 1
    });
    let canceled_order = order.clone();
    drop(driver.app_mut().spawn(async move |_| {
        canceled_order.borrow_mut().push(99);
    }));
    let detached_order = order.clone();
    driver
        .app_mut()
        .spawn(async move |_| {
            detached_order.borrow_mut().push(5);
        })
        .detach();
    driver.tick(vec![]);
    assert_eq!(futures::executor::block_on(parent), 4);
    assert_eq!(*order.borrow(), vec![1, 3, 5]);
}

#[test]
fn spawning_from_an_entity_update_settles_without_borrowing_the_view() {
    let mut driver = Driver::<Probe>::new();
    driver.entity().update_app(driver.app_mut(), |_, _, cx| {
        cx.spawn(async move |this, cx| {
            this.update(cx, |view, cx| {
                view.received.push("first".into());
                cx.notify();
                cx.spawn(async move |this, cx| {
                    this.update(cx, |view, cx| {
                        view.received.push("second".into());
                        cx.notify();
                    })
                    .unwrap();
                })
                .detach();
            })
            .unwrap();
        })
        .detach();
    });
    let frame = driver.tick(vec![]);
    assert!(!frame.busy);
    driver
        .entity()
        .read(|view| assert_eq!(view.received, ["first", "second"]));
}

#[test]
fn weak_entity_updates_fail_after_the_driver_is_released() {
    let mut driver = Driver::<Probe>::new();
    let weak = driver.entity().downgrade();
    let saved = Rc::new(RefCell::new(None));
    let captured = saved.clone();
    driver
        .app_mut()
        .spawn(async move |cx| {
            *captured.borrow_mut() = Some(cx.clone());
        })
        .detach();
    driver.tick(vec![]);
    let mut cx = saved.borrow_mut().take().unwrap();
    drop(driver);
    assert!(weak.upgrade().is_none());
    assert_eq!(weak.update(&mut cx, |_, _| ()), Err(Released));
}

#[test]
fn a_self_waking_future_is_budgeted_and_keeps_the_frame_busy() {
    let mut driver = Driver::<Probe>::new();
    let polls = Rc::new(Cell::new(0));
    let counted = polls.clone();
    let task = driver.app_mut().spawn(async move |_| {
        std::future::poll_fn(move |cx| {
            counted.set(counted.get() + 1);
            cx.waker().wake_by_ref();
            std::task::Poll::<()>::Pending
        })
        .await;
    });
    assert!(driver.tick(vec![]).busy);
    assert!(
        polls.get() > 0 && polls.get() <= 4096,
        "poll budget must bound a frame: {}",
        polls.get()
    );
    assert!(driver.snapshot().is_err());
    drop(task);
    assert!(!driver.tick(vec![]).busy);
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
#[test]
#[should_panic(expected = "state changed without cx.notify()")]
fn update_guard_detects_missing_notify() {
    let mut driver = Driver::<Probe>::new();
    driver.entity().update_app(driver.app_mut(), |view, _, _| {
        view.received.push("forgot".into())
    });
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
#[test]
#[should_panic(expected = "state changed without cx.notify()")]
fn listener_guard_detects_missing_notify() {
    #[derive(Serialize, Deserialize)]
    struct Silent(bool);
    impl View for Silent {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(false)
        }
    }
    impl Render for Silent {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> wire::Node {
            let press = cx.listener(|view, _: &(), _, _| view.0 = true);
            wire::kit::button("silent", "Silent", Some(press), Default::default())
        }
    }
    let mut driver = Driver::<Silent>::new();
    let frame = driver.tick(vec![]);
    driver.tick(crate::testing::press(&frame, "Silent"));
}

#[test]
fn messages_and_responses_settle_in_input_order() {
    let mut driver = Driver::<Probe>::new();
    let requests = driver.tick(vec![]).requests;
    let listener = driver.entity().read(|view| view.listener);
    driver.tick(vec![
        wire::Event::Message(listener),
        response(requests[1].id, false),
        wire::Event::Message(listener),
    ]);
    driver
        .entity()
        .read(|view| assert_eq!(view.received, ["press", "data", "press"]));
}

#[test]
fn repeated_spawns_exhaust_the_round_budget_and_resume_next_frame() {
    fn enqueue(cx: &mut Context<Probe>, remaining: usize) {
        cx.spawn(async move |this, cx| {
            this.update(cx, |view, cx| {
                view.received.push("round".into());
                cx.notify();
                if remaining > 1 {
                    enqueue(cx, remaining - 1);
                }
            })
            .unwrap();
        })
        .detach();
    }
    let mut driver = Driver::<Probe>::new();
    driver
        .entity()
        .update_app(driver.app_mut(), |_, _, cx| enqueue(cx, 40));
    assert!(driver.tick(vec![]).busy);
    driver
        .entity()
        .read(|view| assert!(view.received.len() < 40));
    let mut busy = true;
    for _ in 0..10 {
        busy = driver.tick(vec![]).busy;
        if !busy {
            break;
        }
    }
    assert!(!busy);
    driver
        .entity()
        .read(|view| assert_eq!(view.received.len(), 40));
}

#[test]
fn patches_reconstruct_the_rendered_tree_and_picture_bytes_are_not_retained() {
    #[derive(Serialize, Deserialize)]
    struct Picture(u32);
    impl View for Picture {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(0)
        }
    }
    impl Render for Picture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> wire::Node {
            let mut children = vec![wire::kit::image_resource("picture", "shared-image")];
            children.extend((0..20).map(|i| {
                wire::kit::text(
                    format!("row/{i}"),
                    format!("Row {i}: {}", if i == 0 { self.0 } else { 0 }),
                )
            }));
            wire::kit::column("picture-view", children)
        }
    }
    let mut driver = Driver::<Picture>::new();
    let first = driver.tick(vec![]);
    let mut mounted = first.root.unwrap();
    let mut pictures = 0;
    mounted.for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            assert!(data.is_some());
            *data = None;
            pictures += 1;
        }
    });
    assert_eq!(pictures, 1);
    assert_eq!(driver.last_root.as_ref(), Some(&mounted));
    assert!(driver.tick(vec![]).unchanged);
    driver.entity().update_app(driver.app_mut(), |view, _, cx| {
        view.0 = 1;
        cx.notify();
    });
    let frame = driver.tick(vec![]);
    assert!(!frame.unchanged);
    assert!(!frame.patches.is_empty());
    wire::apply(&mut mounted, frame.patches).unwrap();
    // The host stores picture data separately after applying a patch.
    mounted.for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            *data = None;
        }
    });
    assert_eq!(driver.last_root.as_ref(), Some(&mounted));
}

#[test]
fn notifying_during_render_requests_another_frame() {
    #[derive(Serialize, Deserialize)]
    struct Again(bool);
    impl View for Again {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(false)
        }
    }
    impl Render for Again {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> wire::Node {
            if !self.0 {
                self.0 = true;
                cx.notify();
            }
            wire::Node::empty()
        }
    }
    let mut driver = Driver::<Again>::new();
    assert!(driver.tick(vec![]).busy);
    assert!(!driver.tick(vec![]).busy);
}
