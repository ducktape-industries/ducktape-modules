//! Copied container backgrounds; gradients retain the native eight-stop limit.
use super::Rgba;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorStop {
    pub offset: f32,
    pub color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Background {
    Color(Rgba),
    Linear {
        angle: f32,
        stops: [Option<ColorStop>; 8],
    },
}
