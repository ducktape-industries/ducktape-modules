//! The guest's single window delegates platform work to its host.
use crate::{slots, wire};
pub struct Window {
    slots: slots::Context,
}
impl Window {
    pub(crate) fn new(slots: slots::Context) -> Self {
        Self { slots }
    }
    pub fn focus(&mut self, target: impl Into<crate::ElementId>) {
        self.dispatch(wire::WidgetCommand::Focus {
            target: vec![crate::element::wire_id(target.into())],
        });
    }
    pub fn dispatch(&mut self, command: wire::WidgetCommand) {
        slots::host(&self.slots).notify::<crate::caps::Widget>(command);
    }
}
