//! Nodes: the validator set the `valset` program answers with, and every
//! membership it holds — the key, the address it is reached at, and whether
//! it validates or only resides.
//!
//! The rows are valset's own types (`queries.rs`), kept as they land and
//! worded only when drawn (`ui.rs`); valset's live heads re-read them.
use ducktape_view_guest::host::Error;
use ducktape_view_guest::methods::Changes;
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{Context, IntoElement, Render, Task, View, Window, export_view};
use serde::{Deserialize, Serialize};
use valset::view::Valset;

mod queries;
mod ui;

pub use queries::Set;

#[derive(Serialize, Deserialize, Default)]
pub struct Nodes {
    pub(crate) set: Loadable<Set>,
    /// What the view follows; dropping them unsubscribes.
    #[serde(skip)]
    pub(crate) followers: Vec<Task<()>>,
}

impl View for Nodes {
    const PREFERRED_WINDOW_SIZE: &'static str = "680,620";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let heads = cx.host().subscribe::<Changes<Valset>>(());
        self.followers = vec![cx.for_each(heads, |view, head, _, cx| match head {
            Ok(_) => view.read(cx),
            Err(refusal) => log(cx, "valset's live heads", &refusal),
        })];
        self.read(cx);
    }
}

impl Render for Nodes {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

impl Nodes {
    /// One read of the set — the boot, a retry, a restore, a live head.
    /// What is already on screen stays there while it runs.
    pub(crate) fn read(&mut self, cx: &mut Context<Self>) {
        let work = queries::set(cx.host());
        match self.set.ready() {
            Some(_) => cx.refresh(work, |view, set, _| view.set = Loadable::Ready(set)),
            None => self.set = cx.load(work, |view| &mut view.set),
        }
        cx.notify();
    }
}

/// A refusal nothing on screen waits for, kept in the host's log.
fn log(cx: &mut Context<Nodes>, what: &str, refusal: &Error) {
    cx.host().log(format!("nodes: {what} refused: {refusal}"));
}

export_view!(
    Nodes,
    "Nodes",
    "The validator set of this network and every membership behind it.",
    ["module", "host"]
);

#[cfg(test)]
mod tests;
