use crate::{wire, AnyElement, Element, IntoElement, Lowering};

/// A native deferred draw. It is not the guest subtree cache represented by `Node::Lazy`.
pub struct Deferred {
    child: AnyElement,
    priority: usize,
}

#[track_caller]
pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: child.into_any_element(),
        priority: 0,
    }
}

impl Deferred {
    pub fn with_priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    pub fn priority(self, priority: usize) -> Self {
        self.with_priority(priority)
    }
}

impl Element for Deferred {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let this = *self;
        wire::Node::Deferred {
            priority: this.priority,
            content: Box::new(lowering.lower_element(this.child)),
        }
    }
}

impl IntoElement for Deferred {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Deferred {}
