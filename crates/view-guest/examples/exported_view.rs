//! The smallest `View` that invokes `export_view!` at a crate root; the
//! wasm32 twin of `exported.rs` for the state-and-handlers shape.

use serde::{Deserialize, Serialize};
use view_guest::{Cx, View, wire};

#[derive(Serialize, Deserialize, Default)]
pub struct Exported {
    presses: u32,
}

impl View for Exported {
    fn boot(_: &mut Cx<Self>) -> Self {
        Self::default()
    }

    fn render(&mut self, cx: &mut Cx<Self>) -> wire::Node {
        let press = cx.on(|view, _| view.presses += 1);
        wire::kit::button(
            "press",
            self.presses.to_string(),
            Some(press),
            Default::default(),
        )
    }
}

view_guest::export_view!(Exported, "Exported", "export_view! wasm32 probe", []);
