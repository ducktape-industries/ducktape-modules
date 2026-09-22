//! The guest's single window delegates platform work to its host.
use crate::{slots, wire};
pub struct Window {
    macos: bool,
    slots: slots::Context,
}
impl Window {
    pub(crate) fn new(macos: bool, slots: slots::Context) -> Self {
        Self { macos, slots }
    }
    pub fn is_macos(&self) -> bool {
        self.macos
    }
    pub fn focus(&mut self, target: impl Into<String>) {
        self.dispatch(wire::WidgetCommand::Focus {
            target: target.into(),
        });
    }
    pub fn dispatch(&mut self, command: wire::WidgetCommand) {
        slots::host(&self.slots).notify::<crate::caps::Widget>(command);
    }
}
