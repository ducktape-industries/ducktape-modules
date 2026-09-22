//! Copied text layout and font metadata. Font bytes remain host-owned.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LineHeight {
    Relative(f32),
    Absolute(f32),
}
/// Where a line's glyph run sits in its column. `Start` follows the text
/// direction, as an unaligned line does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shaping {
    Auto,
    Basic,
    Advanced,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Wrapping {
    None,
    Word,
    Glyph,
    WordOrGlyph,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FontFamily {
    Named(String),
    Serif,
    SansSerif,
    Cursive,
    Fantasy,
    Monospace,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedFont {
    pub family: FontFamily,
    pub weight: Weight,
    pub stretch: FontStretch,
    pub style: FontStyle,
}
impl LineHeight {
    pub(super) fn sanitize(&mut self) {
        match self {
            Self::Relative(value) => {
                *value = bounded(*value).clamp(f32::EPSILON, MAX_PIXELS / MAX_TEXT_PIXELS)
            }
            Self::Absolute(value) => *value = bounded(*value).max(f32::EPSILON),
        }
    }
}

impl NamedFont {
    pub(super) fn sanitize(&mut self, budgets: &mut Budgets) {
        if let FontFamily::Named(name) = &mut self.family {
            spend_text(name, budgets);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_graphemes_share_the_frame_node_budget() {
        let mut node = Node::Text {
            id: Some(crate::ElementIdWire::Name("tracked".into())),
            style: gpui::StyleRefinement::default(),
            content: "é".repeat(MAX_TEXT_BYTES_PER_FRAME / 2),
            heading: None,
            live: None,
        };
        sanitize_tree(&mut node).unwrap();
        let Node::Text { content, .. } = node else {
            panic!()
        };
        assert_eq!(content.len(), MAX_TEXT_BYTES_PER_FRAME);
    }
}
