//! Property tests for the wire's hostile-input contract: a random tree either
//! comes back out of `decode` refused for a reason the door actually names,
//! or `sanitize` pulls it inside every bound `sanitize_node` promises; bytes a
//! hostile guest could have written never make `decode` panic; and a
//! hand-crafted length-prefix bomb is refused before it is walked.
//!
//! No new dependency: the generator is a splitmix64 PRNG seeded by a fixed
//! constant, so a failure prints its seed and the run reproduces exactly.

use std::collections::HashSet;

use view_wire::*;

/// Mirrors the wire's own private ceiling on decoded nodes (`16 *
/// MAX_NODES`, see `decode`'s doc comment): decode refuses a frame that
/// would build more than this many nodes, independent of `MAX_NODES` itself,
/// which only bounds what `sanitize` keeps.
const MAX_DECODED_NODES: usize = 16 * MAX_NODES;
/// Mirrors the wire's private `MAX_PIXELS`: every non-text size sanitize
/// keeps is clamped to this range.
const PIXEL_BOUND: f32 = 8192.0;
mod hostile {
    use super::*;

    mod rng;
    use rng::*;
    mod generation;
    use generation::*;
    mod checks;
    use checks::*;
    mod styles;
    use styles::*;
    mod cases;
}
