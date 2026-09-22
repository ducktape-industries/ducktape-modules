use super::{wire, App, Window};

/// A guest-app-local focus allocation. Native GPUI focus handles cannot cross
/// the wasm boundary, so the host maps this opaque token to one native handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusHandle {
    pub(super) id: u64,
}

impl FocusHandle {
    pub(crate) fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn focus(&self, window: &mut Window, _cx: &mut App) {
        window.dispatch(wire::WidgetCommand::FocusHandle { handle: self.id });
    }
}
