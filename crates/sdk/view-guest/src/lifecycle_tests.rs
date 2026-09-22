use crate::{wire, Context, Driver, InteractiveElement, ParentElement, Render, Task, View, Window};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

crate::capability!(Numbers, "test.numbers", (), usize);

#[derive(Default, Serialize, Deserialize)]
struct Streams {
    values: Vec<usize>,
    pending_after_item: bool,
    #[serde(skip)]
    task: Option<Task<()>>,
}
impl View for Streams {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut stream = cx.host().subscribe::<Numbers>(());
        let task = cx.spawn(async move |this, cx| {
            while let Some(value) = stream.next().await {
                let pending = this
                    .update(cx, |view, cx| {
                        view.values.push(value.unwrap());
                        cx.notify();
                        view.pending_after_item
                    })
                    .unwrap();
                if pending {
                    futures::future::pending::<()>().await;
                }
            }
        });
        Self {
            task: Some(task),
            ..Self::default()
        }
    }
}
impl Render for Streams {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
            .id("values")
            .child(self.values.len().to_string())
    }
}

#[test]
fn snapshots_allow_parked_streams_and_reject_ordinary_pending_futures() {
    let mut driver = Driver::<Streams>::new();
    assert!(driver.snapshot().is_err(), "unpolled work is not quiescent");
    driver.tick(vec![]);
    assert!(driver.snapshot().is_ok());
    let task = driver
        .app
        .spawn(async |_| futures::future::pending::<()>().await);
    driver.tick(vec![]);
    assert!(driver.snapshot().is_err());
    drop(task);
    driver.tick(vec![]);
    assert!(driver.snapshot().is_ok());
}

#[test]
fn a_stream_task_awaiting_other_work_is_not_safe_to_snapshot() {
    let mut driver = Driver::<Streams>::new();
    let frame = driver.tick(vec![]);
    driver
        .entity
        .clone()
        .update_app(&mut driver.app, |view, _, cx| {
            view.pending_after_item = true;
            cx.notify();
        });
    driver.tick(vec![wire::Event::Response {
        id: frame.requests[0].id,
        result: Ok(b"1".to_vec()),
        done: false,
    }]);
    driver.entity().read(|view| assert_eq!(view.values, [1]));
    assert!(driver.snapshot().is_err());
}

#[test]
fn a_hot_stream_yields_to_the_tick_budget_and_preserves_item_order() {
    let mut driver = Driver::<Streams>::new();
    let frame = driver.tick(vec![]);
    let id = frame.requests[0].id;
    for value in 0..1000 {
        driver
            .host()
            .fulfill(id, Ok(serde_json::to_vec(&value).unwrap()), false);
    }
    let first = driver.tick(vec![]);
    assert!(first.busy);
    driver
        .entity()
        .read(|view| assert!(view.values.len() < 1000));
    let mut parked = false;
    for _ in 0..20 {
        if !driver.tick(vec![]).busy {
            parked = true;
            break;
        }
    }
    assert!(parked, "hot stream eventually parks");
    driver
        .entity()
        .read(|view| assert_eq!(view.values, (0..1000).collect::<Vec<_>>()));
    assert!(driver.snapshot().is_ok());
    driver.host().close_stream(id);
    driver.tick(vec![]);
    assert!(driver.app.inner.tasks.borrow().is_empty());
}

#[test]
fn dropping_the_stream_task_cancels_its_host_subscription() {
    let mut driver = Driver::<Streams>::new();
    let first = driver.tick(vec![]);
    driver
        .entity
        .clone()
        .update_app(&mut driver.app, |view, _, _| {
            view.task.take();
        });
    let cancelled = driver.tick(vec![]);
    assert_eq!(cancelled.cancels, [first.requests[0].id]);
    assert!(driver.snapshot().is_ok());
}
