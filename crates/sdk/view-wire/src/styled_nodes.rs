//! GPUI-styled wire payloads; the host assigns these refinements unchanged
//! after the frame sanitizer has checked them.
use crate::{ElementIdWire, Interactivity, Live, Node, decode_children};
use gpui::{StyleRefinement, Styled};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerNode {
    pub id: Option<ElementIdWire>,
    pub style: StyleRefinement,
    pub interactivity: Interactivity,
    #[serde(deserialize_with = "decode_children")]
    pub children: Vec<Node>,
}
impl Styled for ContainerNode {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextNode {
    pub id: Option<ElementIdWire>,
    pub style: StyleRefinement,
    pub content: String,
    pub heading: Option<u8>,
    pub live: Option<Live>,
}
impl Styled for TextNode {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
