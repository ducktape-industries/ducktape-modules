use crate::interactivity::{InteractiveElement, Interactivity, StatefulInteractiveElement};
use crate::{wire, Element, IntoElement, Lowering};
use gpui::{
    point, px, radians, size, Pixels, Point, Radians, SharedString, Size, StyleRefinement, Styled,
};

enum Source {
    None,
    Data(Vec<u8>),
    Asset(SharedString),
    External(SharedString),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transformation {
    scale: Size<f32>,
    translate: Point<Pixels>,
    rotate: Radians,
}

impl Default for Transformation {
    fn default() -> Self {
        Self {
            scale: size(1., 1.),
            translate: point(px(0.), px(0.)),
            rotate: radians(0.),
        }
    }
}

impl Transformation {
    pub fn scale(value: Size<f32>) -> Self {
        Self {
            scale: value,
            ..Default::default()
        }
    }
    pub fn translate(value: Point<Pixels>) -> Self {
        Self {
            translate: value,
            ..Default::default()
        }
    }
    pub fn rotate(value: impl Into<Radians>) -> Self {
        Self {
            rotate: value.into(),
            ..Default::default()
        }
    }
    pub fn with_scaling(mut self, value: Size<f32>) -> Self {
        self.scale = value;
        self
    }
    pub fn with_translation(mut self, value: Point<Pixels>) -> Self {
        self.translate = value;
        self
    }
    pub fn with_rotation(mut self, value: impl Into<Radians>) -> Self {
        self.rotate = value.into();
        self
    }
    fn wire(self) -> wire::SvgTransformation {
        wire::SvgTransformation {
            scale: [self.scale.width, self.scale.height],
            translate: [f32::from(self.translate.x), f32::from(self.translate.y)],
            rotate: self.rotate.0,
        }
    }
}

pub struct Svg {
    pub(crate) interactivity: Interactivity,
    source: Source,
    transformation: Transformation,
}

#[track_caller]
pub fn svg() -> Svg {
    Svg {
        interactivity: Interactivity::default(),
        source: Source::None,
        transformation: Transformation::default(),
    }
}

impl Svg {
    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.source = Source::Asset(path.into());
        self
    }
    pub fn external_path(mut self, path: impl Into<SharedString>) -> Self {
        self.source = Source::External(path.into());
        self
    }
    pub fn data(mut self, data: &[u8]) -> Self {
        self.source = Source::Data(data.to_vec());
        self
    }
    pub fn with_transformation(mut self, value: Transformation) -> Self {
        self.transformation = value;
        self
    }
}

impl Styled for Svg {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}
impl InteractiveElement for Svg {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}
impl StatefulInteractiveElement for Svg {}

impl Element for Svg {
    fn id(&self) -> Option<gpui::ElementId> {
        self.interactivity.id.clone()
    }
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            interactivity,
            source,
            transformation,
        } = *self;
        let style = interactivity.base_style.clone();
        let label = interactivity.aria.label.as_ref().map(ToString::to_string);
        let (id, interactivity) = interactivity.into_wire(lowering);
        let source = match source {
            Source::None => wire::SvgSource::None,
            Source::Data(bytes) => {
                let (hash, bytes) = lowering.picture(bytes);
                wire::SvgSource::Data { hash, bytes }
            }
            Source::Asset(path) => wire::SvgSource::Asset(path.to_string()),
            Source::External(path) => wire::SvgSource::External(path.to_string()),
        };
        wire::Node::Svg {
            id,
            source,
            transformation: transformation.wire(),
            label,
            style,
            interactivity,
        }
    }
}

impl IntoElement for Svg {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl gpui::prelude::FluentBuilder for Svg {}
