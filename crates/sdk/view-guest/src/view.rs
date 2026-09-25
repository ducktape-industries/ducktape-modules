//! A serializable root view and small loading conveniences.
pub use crate::doors::{Door, Program, Query, Submit};
use crate::host::Refusal;
use crate::{Context, IntoElement, Task, Window};
use futures::{Stream, StreamExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::future::Future;

pub trait Render: 'static + Sized {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}
/// The capabilities a view's manifest declares. `export_view!` implements
/// it from its list, so `TestAppContext` refuses what the app would refuse.
pub trait Declared {
    const CAPABILITIES: &'static [&'static str];
}
pub trait View: Render + Serialize + DeserializeOwned {
    const PREFERRED_WINDOW_SIZE: &'static str = "none";
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self;
    fn restored(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
}
/// Data the view asked for, in the four states it can be in. `Loading`
/// carries the abort handle: dropping the slot cancels the load.
/// Serialises `Loading` as `Idle`, so a restored view reloads it.
#[derive(Default)]
pub enum Loaded<T> {
    #[default]
    Idle,
    Loading(Task<()>),
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
    Failed { reason: String, sentence: String },
}

/// A finished load: its value, or the refusal in its place.
impl<T> From<Result<T, Refusal>> for Loaded<T> {
    fn from(result: Result<T, Refusal>) -> Self {
        match result {
            Ok(value) => Self::Ready(value),
            Err(refusal) => Self::Failed(refusal),
        }
    }
}

impl<T: Serialize> Serialize for Loaded<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Idle | Self::Loading(_) => LoadedSnapshot::<&T>::Idle,
            Self::Ready(value) => LoadedSnapshot::Ready(value),
            Self::Failed(refusal) => LoadedSnapshot::Failed {
                reason: refusal.reason.clone(),
                sentence: refusal.sentence.clone(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Loaded<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match LoadedSnapshot::deserialize(deserializer)? {
            LoadedSnapshot::Idle => Self::Idle,
            LoadedSnapshot::Ready(value) => Self::Ready(value),
            LoadedSnapshot::Failed { reason, sentence } => {
                Self::Failed(Refusal::new(reason, sentence))
            }
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

impl<V: View> Context<'_, V> {
    pub fn load<T: 'static>(
        &mut self,
        work: impl Future<Output = Result<T, Refusal>> + 'static,
        at: impl Fn(&mut V) -> &mut Loaded<T> + 'static,
    ) -> Loaded<T> {
        let task = self.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                *at(view) = Loaded::from(result);
                cx.notify();
            });
        });
        Loaded::Loading(task)
    }
    /// Runs `each` on every item `stream` yields, in order, until the
    /// stream ends or the view is gone; the view is re-rendered after each.
    /// Keep the task: dropping it unsubscribes. A refused item is handed to
    /// `each` like any other and does not end the stream, so each follower
    /// says what a refusal means to it.
    pub fn follow<T: 'static>(
        &mut self,
        mut stream: impl Stream<Item = T> + Unpin + 'static,
        mut each: impl FnMut(&mut V, T, &mut Window, &mut Context<V>) + 'static,
    ) -> Task<()> {
        self.spawn(async move |this, cx| {
            while let Some(item) = stream.next().await {
                let landed = this.update_in(cx, |view, window, cx| {
                    each(view, item, window, cx);
                    cx.notify();
                });
                if landed.is_err() {
                    break;
                }
            }
        })
    }

    pub fn refresh<T: 'static>(
        &mut self,
        work: impl Future<Output = Result<T, Refusal>> + 'static,
        land: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
    ) {
        self.spawn(async move |this, cx| {
            if let Ok(value) = work.await {
                let _ = this.update(cx, |view, cx| {
                    land(view, value, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
#[macro_export]
macro_rules! export_view {
    ($view:ty, $name:expr, $description:expr, [$($capability:literal),* $(,)?]) => {
        $crate::export_driver!($view, $name, $description, [$($capability),*]);
    };
}

#[cfg(test)]
mod follow_tests {
    use crate::doors::RpcLive;
    use crate::testing::TestAppContext;
    use crate::{Context, IntoElement, ParentElement, Render, Task, View, Window};
    use serde::{Deserialize, Serialize};

    /// Counts the heads a program's live stream announces.
    #[derive(Default, Serialize, Deserialize)]
    struct Heads {
        seen: usize,
        #[serde(skip)]
        live: Option<Task<()>>,
    }
    impl View for Heads {
        fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
            let live = cx.host().subscribe::<RpcLive>("chat".into());
            Self {
                seen: 0,
                live: Some(cx.follow(live, |view: &mut Heads, _, _, _| view.seen += 1)),
            }
        }
    }
    impl crate::Declared for Heads {
        const CAPABILITIES: &'static [&'static str] = &["rpc"];
    }
    impl Render for Heads {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::div().child(self.seen.to_string())
        }
    }

    #[test]
    fn a_follower_hears_every_item_until_its_task_is_dropped() {
        let mut cx = TestAppContext::new();
        let feed = cx.host().stream::<RpcLive>();
        let view = cx.open::<Heads>();
        cx.run_until_parked();
        feed.push(None);
        feed.push(None);
        cx.run_until_parked();
        view.read(|heads| assert_eq!(heads.seen, 2));
        assert!(cx.has_text("2"), "each item re-renders the view");
        view.update(&mut cx, |heads, _, _| heads.live = None);
        cx.run_until_parked();
        feed.push(None);
        cx.run_until_parked();
        view.read(|heads| assert_eq!(heads.seen, 2));
    }
}
