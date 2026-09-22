use super::*;

impl Node {
    /// The node an empty view renders as.
    pub fn empty() -> Self {
        Self::Space {
            style: gpui::StyleRefinement::default(),
        }
    }

    /// Hashes the current copied subtree without allocating an encoded buffer.
    /// A host uses this after sanitization: shared frame budgets may change
    /// content even when a guest memo generation stays the same.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::Hasher;
        struct Sink(std::hash::DefaultHasher);
        impl std::io::Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.write(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink(std::hash::DefaultHasher::new());
        rmp_serde::encode::write_named(&mut sink, self).expect("node fingerprint sink cannot fail");
        sink.0.finish()
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Container { id, .. }
            | Self::Text { id, .. }
            | Self::Image { id, .. }
            | Self::Svg { id, .. }
            | Self::RichText { id, .. } => id.as_ref().and_then(ElementIdWire::name),
            Self::Input { id, .. }
            | Self::Editor { id, .. }
            | Self::UniformList { id, .. }
            | Self::ResizeHandle { id, .. }
            | Self::MouseArea { id, .. }
            | Self::Sensor { id, .. }
            | Self::Responsive { id, .. }
            | Self::Scroll { id, .. }
            | Self::Overlay { id, .. }
            | Self::ImageViewer { id, .. }
            | Self::Slider { id, .. }
            | Self::PickList { id, .. }
            | Self::ComboBox { id, .. }
            | Self::Surface { id, .. }
            | Self::Float { id, .. }
            | Self::Lazy { id, .. }
            | Self::When { id, .. }
            | Self::Qr { id, .. }
            | Self::Button { id, .. }
            | Self::Rule { id, .. }
            | Self::Toggle { id, .. }
            | Self::Radio { id, .. }
            | Self::Progress { id, .. }
            | Self::Tooltip { id, .. } => id.name(),
            Self::List { .. }
            | Self::Space { .. }
            | Self::Anchored { .. }
            | Self::Deferred { .. }
            | Self::Canvas { .. } => None,
        }
    }

    /// The node's identity without reducing a typed GPUI ID to text.
    pub fn identity(&self) -> Option<IdentityKeyRef<'_>> {
        match self {
            Self::Container { id, .. }
            | Self::Text { id, .. }
            | Self::Image { id, .. }
            | Self::Svg { id, .. }
            | Self::RichText { id, .. } => id.as_ref().map(IdentityKeyRef::Element),
            Self::Input { id, .. }
            | Self::Editor { id, .. }
            | Self::UniformList { id, .. }
            | Self::ResizeHandle { id, .. }
            | Self::MouseArea { id, .. }
            | Self::Sensor { id, .. }
            | Self::Responsive { id, .. }
            | Self::Scroll { id, .. }
            | Self::Overlay { id, .. }
            | Self::ImageViewer { id, .. }
            | Self::Slider { id, .. }
            | Self::PickList { id, .. }
            | Self::ComboBox { id, .. }
            | Self::Surface { id, .. }
            | Self::Float { id, .. }
            | Self::Lazy { id, .. }
            | Self::When { id, .. }
            | Self::Qr { id, .. }
            | Self::Button { id, .. }
            | Self::Rule { id, .. }
            | Self::Toggle { id, .. }
            | Self::Radio { id, .. }
            | Self::Progress { id, .. }
            | Self::Tooltip { id, .. } => Some(IdentityKeyRef::Element(id)),
            Self::List { .. }
            | Self::Space { .. }
            | Self::Anchored { .. }
            | Self::Deferred { .. }
            | Self::Canvas { .. } => None,
        }
    }

    /// The node's children in order. One arm per variant, here and in
    /// [`Node::children_mut`] and [`Node::child_list_mut`]: everything that
    /// walks, diffs or patches a tree goes through these three, so a new
    /// variant is a new arm in each and nothing else.
    pub fn children(&self) -> &[Node] {
        match self {
            Self::Container { children, .. }
            | Self::Tooltip { children, .. }
            | Self::Overlay { children, .. }
            | Self::List { children, .. }
            | Self::UniformList { children, .. }
            | Self::When { children, .. }
            | Self::Anchored { children, .. }
            | Self::Image {
                state_children: children,
                ..
            } => children,
            Self::Float { content, .. }
            | Self::Responsive { content, .. }
            | Self::Lazy { content, .. }
            | Self::Deferred { content, .. }
            | Self::Sensor { child: content, .. }
            | Self::ResizeHandle { content, .. }
            | Self::MouseArea { content, .. }
            | Self::Scroll { content, .. } => std::slice::from_ref(content),
            Self::Button {
                content: ButtonContent::Child(child),
                ..
            } => std::slice::from_ref(child),
            Self::Button { .. }
            | Self::Qr { .. }
            | Self::RichText { .. }
            | Self::Text { .. }
            | Self::Svg { .. }
            | Self::ImageViewer { .. }
            | Self::Input { .. }
            | Self::Editor { .. }
            | Self::Space { .. }
            | Self::Rule { .. }
            | Self::Toggle { .. }
            | Self::Radio { .. }
            | Self::Slider { .. }
            | Self::PickList { .. }
            | Self::ComboBox { .. }
            | Self::Progress { .. }
            | Self::Canvas { .. }
            | Self::Surface { .. } => &[],
        }
    }

    /// Runs `visit` on every node in the tree, depth first, this one first.
    pub fn for_each_mut(&mut self, visit: &mut impl FnMut(&mut Node)) {
        visit(self);
        for child in self.children_mut() {
            child.for_each_mut(visit);
        }
    }

    pub fn children_mut(&mut self) -> &mut [Node] {
        match self {
            Self::Container { children, .. }
            | Self::Tooltip { children, .. }
            | Self::Overlay { children, .. }
            | Self::List { children, .. }
            | Self::UniformList { children, .. }
            | Self::When { children, .. }
            | Self::Anchored { children, .. }
            | Self::Image {
                state_children: children,
                ..
            } => children,
            Self::Float { content, .. }
            | Self::Responsive { content, .. }
            | Self::Lazy { content, .. }
            | Self::Deferred { content, .. }
            | Self::Sensor { child: content, .. }
            | Self::ResizeHandle { content, .. }
            | Self::MouseArea { content, .. }
            | Self::Scroll { content, .. } => std::slice::from_mut(content),
            Self::Button {
                content: ButtonContent::Child(child),
                ..
            } => std::slice::from_mut(child),
            Self::Button { .. }
            | Self::Qr { .. }
            | Self::RichText { .. }
            | Self::Text { .. }
            | Self::Input { .. }
            | Self::Editor { .. }
            | Self::Space { .. }
            | Self::Rule { .. }
            | Self::Toggle { .. }
            | Self::Radio { .. }
            | Self::Slider { .. }
            | Self::PickList { .. }
            | Self::ComboBox { .. }
            | Self::Progress { .. }
            | Self::Svg { .. }
            | Self::ImageViewer { .. }
            | Self::Canvas { .. }
            | Self::Surface { .. } => &mut [],
        }
    }

    /// The children as a list that can grow and shrink, for the variants
    /// that hold one; a fixed-arity node (a container's one content) has
    /// none, and no patch may insert into, remove from or move within it.
    pub fn child_list_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Self::Container { children, .. }
            | Self::List { children, .. }
            | Self::UniformList { children, .. }
            | Self::When { children, .. }
            | Self::Tooltip { children, .. }
            | Self::Anchored { children, .. }
            | Self::Image {
                state_children: children,
                ..
            }
            | Self::Overlay { children, .. } => Some(children),
            Self::Float { .. }
            | Self::Responsive { .. }
            | Self::Lazy { .. }
            | Self::Deferred { .. }
            | Self::Sensor { .. }
            | Self::ResizeHandle { .. }
            | Self::MouseArea { .. }
            | Self::Scroll { .. }
            | Self::Button { .. }
            | Self::Qr { .. }
            | Self::RichText { .. }
            | Self::Text { .. }
            | Self::Input { .. }
            | Self::Editor { .. }
            | Self::Space { .. }
            | Self::Rule { .. }
            | Self::Toggle { .. }
            | Self::Radio { .. }
            | Self::Slider { .. }
            | Self::PickList { .. }
            | Self::ComboBox { .. }
            | Self::Progress { .. }
            | Self::Svg { .. }
            | Self::ImageViewer { .. }
            | Self::Canvas { .. }
            | Self::Surface { .. } => None,
        }
    }

    /// Every node in the tree, depth first, this one included.
    pub fn count(&self) -> usize {
        1 + self.children().iter().map(Node::count).sum::<usize>()
    }
}
