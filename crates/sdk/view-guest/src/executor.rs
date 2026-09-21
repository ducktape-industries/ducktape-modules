//! A local executor: futures are polled only after their waker fires.
use futures::channel::oneshot;
use futures::future::{AbortHandle, Abortable, LocalBoxFuture};
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};

/// A spawned future. Keep it to await its result; drop cancels, detach continues.
#[must_use]
pub struct Task<R> {
    result: oneshot::Receiver<R>,
    abort: AbortHandle,
    detached: bool,
}
impl<R> Task<R> {
    pub fn detach(mut self) {
        self.detached = true;
    }
}
impl<R> Future for Task<R> {
    type Output = R;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<R> {
        Pin::new(&mut self.result)
            .poll(cx)
            .map(|result| result.expect("task executor released"))
    }
}
impl<R> Drop for Task<R> {
    fn drop(&mut self) {
        if !self.detached {
            self.abort.abort();
        }
    }
}
impl<R> std::fmt::Debug for Task<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Task")
    }
}
pub(crate) struct Running {
    future: LocalBoxFuture<'static, ()>,
    woken: Arc<Woken>,
}
struct Woken(AtomicBool);
impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}
pub(crate) fn task<R: 'static>(future: impl Future<Output = R> + 'static) -> (Task<R>, Running) {
    let (sender, result) = oneshot::channel();
    let (abort, registration) = AbortHandle::new_pair();
    let future = async move {
        let _ = Abortable::new(
            async move {
                let _ = sender.send(future.await);
            },
            registration,
        )
        .await;
    };
    (
        Task {
            result,
            abort,
            detached: false,
        },
        Running {
            future: Box::pin(future),
            woken: Arc::new(Woken(AtomicBool::new(true))),
        },
    )
}
pub(crate) fn ready(tasks: &[Running]) -> bool {
    tasks.iter().any(|task| task.woken.0.load(Ordering::SeqCst))
}
pub(crate) fn snapshot_ready(tasks: &[Running], host: &crate::Host) -> bool {
    tasks.iter().all(|task| {
        !task.woken.0.load(Ordering::SeqCst)
            && host.waiting_stream(&Waker::from(task.woken.clone()))
    })
}
const MAX_POLLS: usize = 64;

pub(crate) fn poll(tasks: &mut Vec<Running>) -> bool {
    for _ in 0..MAX_POLLS {
        let mut polled = false;
        tasks.retain_mut(|task| {
            if !task.woken.0.swap(false, Ordering::SeqCst) {
                return true;
            }
            polled = true;
            let waker = Waker::from(task.woken.clone());
            task.future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        });
        if !polled {
            return false;
        }
    }
    ready(tasks)
}
