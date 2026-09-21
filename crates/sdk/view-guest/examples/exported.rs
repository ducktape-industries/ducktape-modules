//! The smallest view that invokes `export_app!` at a crate root. Built as a
//! wasm32 cdylib by `make view-wasm-check`, so the exported surface
//! (`alloc`/`init`/`tick`/`snapshot`/`restore`, the manifest section) is
//! compiled in this tree, not only in a view's.

use view_guest::{Subscription, Task, wire};

#[derive(Clone)]
pub enum Message {
    Noop,
}

pub struct Exported {
    ticks: u32,
}

impl Exported {
    pub(crate) const PREFERRED_WINDOW_SIZE: &'static str = "none";

    fn boot() -> (Self, Task<Message>) {
        (Self { ticks: 0 }, Task::none())
    }

    fn view(&self) -> wire::Node {
        wire::Node::empty()
    }

    fn update(&mut self, _: Message) -> Task<Message> {
        self.ticks += 1;
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        Ok(self.ticks.to_le_bytes().to_vec())
    }

    fn restore(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self {
            ticks: u32::from_le_bytes(bytes.try_into().map_err(|_| "invalid snapshot")?),
        })
    }
}

view_guest::export_app!(Exported, "Exported", "export_app! wasm32 probe", []);
