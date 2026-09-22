//! A serializable root view and small loading conveniences.
pub use crate::doors::{Door, Program, Query, Submit};
use crate::host::Refusal;
use crate::{Context, IntoElement, Task, Window};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::future::Future;

pub trait Render: 'static + Sized {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
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

impl<V: View> Context<'_, V> {
    pub fn load<T: 'static>(
        &mut self,
        work: impl Future<Output = Result<T, Refusal>> + 'static,
        at: impl Fn(&mut V) -> &mut Loaded<T> + 'static,
    ) -> Loaded<T> {
        let task = self.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                *at(view) = match result {
                    Ok(value) => Loaded::Ready(value),
                    Err(error) => Loaded::Failed(error),
                };
                cx.notify();
            });
        });
        Loaded::Loading(task)
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
