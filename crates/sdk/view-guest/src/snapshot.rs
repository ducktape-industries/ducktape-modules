//! State transfer into a fresh root entity without replaying construction.
use crate::{Driver, View, slots};
impl<V: View> Driver<V> {
    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        let _guard = self.app.inner.slots.enter();
        if slots::editor_pending()
            || slots::editor_transferring()
            || self.busy
            || self.host().pending_requests()
            || !crate::executor::snapshot_ready(&self.app.inner.tasks.borrow(), &self.host())
        {
            return Err("guest has pending work; snapshot after it settles".into());
        }
        self.entity
            .read(|view| serde_json::to_vec(view).map_err(|error| error.to_string()))
    }
    pub fn from_snapshot(bytes: &[u8], macos: bool) -> Result<Self, String> {
        let view = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        Self::initialize(macos, Some(view))
    }
}
