//! The smallest `View` that invokes `export_view!` at a crate root; the
//! wasm32 probe for the entity-and-listeners shape.

use serde::{Deserialize, Serialize};
use view_guest::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, View, Window, div,
};

#[derive(Serialize, Deserialize, Default)]
pub struct Exported {
    presses: u32,
}

impl View for Exported {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for Exported {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let press = cx.listener(|view, _: &ClickEvent, _, cx| {
            view.presses += 1;
            cx.notify();
        });
        div()
            .id("press")
            .on_click(press)
            .child(self.presses.to_string())
    }
}

view_guest::export_view!(Exported, "Exported", "export_view! wasm32 probe", []);
