use crate::{AnyElement, IntoElement, Lowering, wire};

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

impl IntoElement for Deferred {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Deferred {
            key: "deferred".into(),
            priority: self.priority.min(16),
            content: Box::new(self.child.into_node(lowering)),
        }
    }
}

impl gpui::prelude::FluentBuilder for Deferred {}

