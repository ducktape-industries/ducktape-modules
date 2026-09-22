//! Native tooltip placement; visual presentation is the node's GPUI style.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum TooltipPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    FollowCursor,
}
