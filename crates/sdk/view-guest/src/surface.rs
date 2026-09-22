//! A typed guest recipe for a host-painted surface.
use crate::Element;

use crate::{wire, App, ElementId, IntoElement, Lowering, Window};

type SurfaceListener = Box<dyn Fn(&wire::SurfaceValue, &mut Window, &mut App)>;

pub struct Surface {
    id: ElementId,
    name: String,
    args: Vec<wire::SurfaceValue>,
    on_event: Option<SurfaceListener>,
}

pub fn surface(
    id: impl Into<ElementId>,
    name: impl Into<String>,
    args: impl Into<Vec<wire::SurfaceValue>>,
) -> Surface {
    Surface {
        id: id.into(),
        name: name.into(),
        args: args.into(),
        on_event: None,
    }
}

impl Surface {
    pub fn on_event(
        mut self,
        listener: impl Fn(&wire::SurfaceValue, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_event = Some(Box::new(listener));
        self
    }
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Surface {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let key = wire::ElementIdWire::from_gpui(self.id)
            .expect("element ID must be portable across the view boundary")
            .name()
            .expect("surface IDs require names until retained nodes carry ElementIdWire")
            .to_owned();
        wire::Node::Surface {
            key,
            name: self.name,
            args: self.args,
            on_event: self.on_event.map(|listener| lowering.route(listener)),
        }
    }
}

impl gpui::prelude::FluentBuilder for Surface {}
