//! The component export of [`crate::realtime::Call`] for the
//! `ducktape:lane@0.1.0` world `media`: one instance per module plane, its
//! state in guest memory between steps. `step` before `init` (or after a
//! refused `init`) has no instance to drive and returns no effects.

use std::sync::Mutex;

use ducktape_lane_sdk::types::{Config, Effect, Event};

use crate::realtime::Call;

static CALL: Mutex<Option<Call>> = Mutex::new(None);

struct Guest;

impl ducktape_lane_sdk::Guest for Guest {
    fn init(config: Config) -> Result<(), String> {
        let call = Call::new(config)?;
        *CALL.lock().expect("single-threaded guest") = Some(call);
        Ok(())
    }

    fn step(event: Event, now_ms: u64) -> Vec<Effect> {
        CALL.lock()
            .expect("single-threaded guest")
            .as_mut()
            .map_or_else(Vec::new, |call| call.step(event, now_ms))
    }
}

ducktape_lane_sdk::export_media!(Guest);
