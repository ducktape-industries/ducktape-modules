use super::*;

type UniformProcessor = Box<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

/// A handle for controlling a guest uniform list across frames.
#[derive(Clone, Default)]
pub struct UniformListScrollHandle(Rc<RefCell<UniformListScrollState>>);

#[derive(Default)]
pub(crate) struct UniformListScrollState {
    pub(crate) request: Option<wire::list::UniformListScrollRequest>,
    pub(crate) y_flipped: bool,
    pub(crate) top_index: usize,
    pub(crate) scrollable: bool,
    pub(crate) scrolled_to_end: Option<bool>,
}

impl UniformListScrollHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn request(&self, index: usize, strategy: ScrollStrategy, offset: usize, strict: bool) {
        let strategy = match strategy {
            ScrollStrategy::Top => wire::list::UniformListScrollStrategy::Top,
            ScrollStrategy::Center => wire::list::UniformListScrollStrategy::Center,
            ScrollStrategy::Bottom => wire::list::UniformListScrollStrategy::Bottom,
            ScrollStrategy::Nearest => wire::list::UniformListScrollStrategy::Nearest,
        };
        self.0.borrow_mut().request = Some(wire::list::UniformListScrollRequest {
            index,
            strategy,
            offset,
            strict,
        });
    }

    pub fn scroll_to_item(&self, index: usize, strategy: ScrollStrategy) {
        self.request(index, strategy, 0, false);
    }

    pub fn scroll_to_item_strict(&self, index: usize, strategy: ScrollStrategy) {
        self.request(index, strategy, 0, true);
    }

    pub fn scroll_to_item_with_offset(
        &self,
        index: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.request(index, strategy, offset, false);
    }

    pub fn scroll_to_item_strict_with_offset(
        &self,
        index: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.request(index, strategy, offset, true);
    }

    pub fn y_flipped(&self) -> bool {
        self.0.borrow().y_flipped
    }

    pub fn logical_scroll_top_index(&self) -> usize {
        self.0.borrow().top_index
    }

    pub fn is_scrollable(&self) -> bool {
        self.0.borrow().scrollable
    }

    pub fn is_scrolled_to_end(&self) -> Option<bool> {
        self.0.borrow().scrolled_to_end
    }

    pub fn scroll_to_bottom(&self) {
        self.scroll_to_item(usize::MAX, ScrollStrategy::Bottom);
    }
}

/// A GPUI-shaped uniform list recipe. The host owns layout and virtualization.
pub struct UniformList {
    count: usize,
    processor: UniformProcessor,
    pub(crate) interactivity: Interactivity,
    measure_index: usize,
    sizing: wire::list::UniformListSizing,
    horizontal_sizing: wire::list::UniformListHorizontalSizing,
    scroll: Option<UniformListScrollHandle>,
    y_flipped: bool,
}

pub fn uniform_list<R: IntoElement>(
    id: impl Into<ElementId>,
    count: usize,
    processor: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
) -> UniformList {
    let mut style = StyleRefinement::default();
    style.overflow.y = Some(Overflow::Scroll);
    let mut interactivity = Interactivity::default();
    interactivity.id = Some(id.into());
    interactivity.base_style = style;
    UniformList {
        count,
        processor: Box::new(move |range, window, app| {
            processor(range, window, app)
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect()
        }),
        interactivity,
        measure_index: 0,
        sizing: wire::list::UniformListSizing::Auto,
        horizontal_sizing: wire::list::UniformListHorizontalSizing::FitList,
        scroll: None,
        y_flipped: false,
    }
}

impl UniformList {
    pub fn with_width_from_item(mut self, item_index: Option<usize>) -> Self {
        self.measure_index = item_index.unwrap_or(0);
        self
    }

    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing = match behavior {
            ListSizingBehavior::Infer => wire::list::UniformListSizing::Infer,
            ListSizingBehavior::Auto => wire::list::UniformListSizing::Auto,
        };
        self
    }

    pub fn with_horizontal_sizing_behavior(
        mut self,
        behavior: ListHorizontalSizingBehavior,
    ) -> Self {
        self.horizontal_sizing = match behavior {
            ListHorizontalSizingBehavior::FitList => {
                self.interactivity.base_style.overflow.x = None;
                wire::list::UniformListHorizontalSizing::FitList
            }
            ListHorizontalSizingBehavior::Unconstrained => {
                self.interactivity.base_style.overflow.x = Some(Overflow::Scroll);
                wire::list::UniformListHorizontalSizing::Unconstrained
            }
        };
        self
    }

    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }

    pub fn y_flipped(mut self, y_flipped: bool) -> Self {
        self.y_flipped = y_flipped;
        self
    }
}

impl Styled for UniformList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl crate::InteractiveElement for UniformList {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl Element for UniformList {
    fn id(&self) -> Option<ElementId> {
        self.interactivity.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let count = self.count.min(wire::MAX_UNIFORM_LIST_COUNT);
        let path = lowering.current_path().to_vec();
        let id = path
            .last()
            .cloned()
            .expect("uniform list lowers inside its authored scope");
        let measure_index = self.measure_index.min(count.saturating_sub(1));
        let scroll = self.scroll.as_ref().map(|handle| &handle.0);
        let (route, ranges) = lowering
            .app
            .uniform_list_route(&path, count, measure_index, scroll);
        let mut indices = Vec::new();
        let mut children = Vec::new();
        'ranges: for range in ranges {
            let range = range.start.min(count)..range.end.min(count);
            if range.is_empty() {
                continue;
            }
            let rendered = (self.processor)(range.clone(), lowering.window, lowering.app);
            for (index, child) in range.zip(rendered) {
                let index = index as u32;
                if indices.contains(&index) {
                    continue;
                }
                if indices.len() == wire::MAX_UNIFORM_LIST_ROWS {
                    break 'ranges;
                }
                indices.push(index);
                children.push(lowering.lower_element(child));
            }
        }
        let scroll_request = self
            .scroll
            .as_ref()
            .and_then(|handle| handle.0.borrow_mut().request.take());
        if let Some(handle) = &self.scroll {
            handle.0.borrow_mut().y_flipped = self.y_flipped;
        }
        let style = self.interactivity.base_style.clone();
        let (_, interactivity) = self.interactivity.into_wire(lowering);
        wire::Node::UniformList {
            id,
            path,
            route,
            style,
            interactivity,
            count,
            measure_index,
            sizing: self.sizing,
            horizontal_sizing: self.horizontal_sizing,
            y_flipped: self.y_flipped,
            scroll_request,
            indices,
            children,
        }
    }
}

impl IntoElement for UniformList {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Div {}
impl gpui::prelude::FluentBuilder for Input {}
impl gpui::prelude::FluentBuilder for AnyElement {}
impl gpui::prelude::FluentBuilder for UniformList {}
