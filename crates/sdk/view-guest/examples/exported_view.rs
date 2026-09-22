//! The smallest `View` that invokes `export_view!` at a crate root; the
//! wasm32 probe for the entity-and-listeners shape.

use serde::{Deserialize, Serialize};
use view_guest::{Context, Render, View, Window, wire};

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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> wire::Node {
        let press = cx.listener(|view, _: &(), _, cx| {
            view.presses += 1;
            cx.notify();
        });
        wire::kit::button(
            "press",
            self.presses.to_string(),
            Some(press),
            Default::default(),
        )
    }
}

view_guest::export_view!(Exported, "Exported", "export_view! wasm32 probe", []);
