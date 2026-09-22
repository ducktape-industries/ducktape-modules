use crate::interactivity::{Interactivity, InteractiveElement, Stateful, StatefulInteractiveElement};
use crate::{IntoElement, Lowering, wire};
use gpui::{ImageSource as GpuiImageSource, ObjectFit, RenderImage, Resource, Styled, StyleRefinement};
use std::sync::Arc;

pub use gpui::ImageSource;

/// Image-specific refinements supported by the bounded wire image primitive.
pub trait StyledImage: Sized {
    fn grayscale(self, grayscale: bool) -> Self;
    fn object_fit(self, object_fit: ObjectFit) -> Self;
}

/// A GPUI-shaped image source lowered to an opaque host resource or bounded bytes.
pub struct Img {
    pub(crate) interactivity: Interactivity,
    source: ImageSource,
    style: StyleRefinement,
    grayscale: bool,
    object_fit: ObjectFit,
}

#[track_caller]
pub fn img(source: impl Into<ImageSource>) -> Img {
    Img {
        interactivity: Interactivity::default(),
        source: source.into(),
        style: StyleRefinement::default(),
        grayscale: false,
        object_fit: ObjectFit::Contain,
    }
}

impl Img {
    pub fn extensions() -> &'static [&'static str] {
        &[
            "avif", "jpg", "jpeg", "png", "gif", "webp", "tif", "tiff", "tga", "dds", "bmp",
            "ico", "hdr", "exr", "pbm", "pam", "ppm", "pgm", "ff", "farbfeld", "qoi", "svg",
        ]
    }
}

impl Styled for Img {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl StyledImage for Img {
    fn grayscale(mut self, grayscale: bool) -> Self {
        self.grayscale = grayscale;
        self
    }

    fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

impl StyledImage for Stateful<Img> {
    fn grayscale(mut self, grayscale: bool) -> Self {
        self.element.grayscale = grayscale;
        self
    }

    fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.element.object_fit = object_fit;
        self
    }
}

impl InteractiveElement for Img {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for Img {}

impl IntoElement for Img {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        let key = source_key(&self.source);
        let label = self
            .interactivity
            .aria
            .label
            .as_ref()
            .map(ToString::to_string);
        let data = image_data(self.source);
        let fit = Some(object_fit(self.object_fit));
        let interactivity = self.interactivity.into_wire(lowering);
        wire::Node::Image {
            key,
            hash: stable_hash(&interactivity.id, &data),
            data,
            label,
            fit,
            opacity: None,
            width: None,
            height: None,
            grayscale: self.grayscale,
            style: self.style,
            interactivity,
        }
    }
}

fn object_fit(value: ObjectFit) -> wire::ContentFit {
    match value {
        ObjectFit::Contain => wire::ContentFit::Contain,
        ObjectFit::Cover => wire::ContentFit::Cover,
        ObjectFit::Fill => wire::ContentFit::Fill,
        ObjectFit::None => wire::ContentFit::None,
        ObjectFit::ScaleDown => wire::ContentFit::ScaleDown,
    }
}

fn source_key(source: &ImageSource) -> String {
    match source {
        GpuiImageSource::Resource(resource) => match resource {
            Resource::Uri(uri) => format!("img:uri:{uri}"),
            Resource::Path(path) => format!("img:path:{}", path.display()),
            Resource::Embedded(path) => format!("img:asset:{path}"),
        },
        GpuiImageSource::Render(image) => format!("img:render:{}", image.id.0),
        GpuiImageSource::Image(image) => format!("img:image:{}", image.id()),
        GpuiImageSource::Custom(_) => "img:unsupported-custom".into(),
    }
}

fn image_data(source: ImageSource) -> Option<wire::ImageData> {
    match source {
        GpuiImageSource::Resource(resource) => Some(wire::ImageData::Resource(source_key(
            &GpuiImageSource::Resource(resource),
        ))),
        GpuiImageSource::Render(image) => render_image_data(&image),
        GpuiImageSource::Image(image) => Some(wire::ImageData::Encoded(image.bytes().to_vec())),
        GpuiImageSource::Custom(_) => None,
    }
}

fn render_image_data(image: &Arc<RenderImage>) -> Option<wire::ImageData> {
    let size = image.size(0);
    Some(wire::ImageData::Rgba {
        width: u32::from(size.width),
        height: u32::from(size.height),
        pixels: image.as_bytes(0)?.to_vec(),
    })
}

fn stable_hash(id: &Option<wire::ElementIdWire>, data: &Option<wire::ImageData>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    id.hash(&mut hasher);
    data.hash(&mut hasher);
    hasher.finish()
}

impl gpui::prelude::FluentBuilder for Img {}
