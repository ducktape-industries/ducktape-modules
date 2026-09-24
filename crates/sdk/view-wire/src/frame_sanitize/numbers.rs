use super::*;

/// Cuts `text` down to [`MAX_STRING_BYTES`] on a char boundary, in place.
/// Shared by [`sanitize`] (a guest's outbound frame) and the host's inbound
/// edit path (a user's keystroke or paste into an [`Node::Input`]) — one
/// bound on any string either side of the wire sends the other.
pub fn truncate_string(text: &mut String) {
    truncate_to(text, MAX_STRING_BYTES);
}

pub(super) fn truncate_to(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

pub(crate) fn bounded(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(0.0, MAX_PIXELS),
    }
}

/// A number that is not a size: a slider or progress value is the app's,
/// so it is made finite and nothing more. The host clamps it into the range
/// it lays out.
/// A pixel measure that may point either way (a paint-only inset), bounded
/// on both sides; NaN reads as 0.
pub(crate) fn signed_bounded(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(-MAX_PIXELS, MAX_PIXELS),
    }
}

pub(crate) fn finite(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(f32::MIN, f32::MAX),
    }
}

pub(crate) fn bound_optional(value: &mut Option<f32>) {
    if let Some(value) = value {
        *value = bounded(*value);
    }
}
