use crate::{wire, AnyElement, App, Element, IntoElement, Lowering, Window};
use gpui::{px, Pixels, StyleRefinement, Styled};
use std::{
    cell::RefCell,
    ops::Range,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

pub use gpui::{FollowMode, ListAlignment, ListOffset, ListScrollEvent, ListSizingBehavior};
type ScrollHandler = dyn FnMut(&ListScrollEvent, &mut Window, &mut App) + 'static;
type ItemRenderer = Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement>;

#[derive(Clone)]
pub struct ListState(Rc<State>);
struct State {
    id: u64,
    inner: RefCell<Inner>,
    scroll_handler: RefCell<Option<Box<ScrollHandler>>>,
}
struct Inner {
    item_count: usize,
    alignment: ListAlignment,
    overdraw: Pixels,
    requested: Range<usize>,
    logical_scroll_top: ListOffset,
    tail_mode: bool,
    following_tail: bool,
    revision: u64,
    commands: Vec<wire::ListCommand>,
}
static NEXT_LIST_STATE: AtomicU64 = AtomicU64::new(1);

impl std::fmt::Debug for ListState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ListState")
    }
}
impl Default for ListState {
    fn default() -> Self {
        Self::new(0, ListAlignment::Top, px(0.))
    }
}

impl ListState {
    pub fn new(item_count: usize, alignment: ListAlignment, overdraw: Pixels) -> Self {
        let item_count = item_count.min(wire::MAX_LIST_ITEMS);
        Self(Rc::new(State {
            id: NEXT_LIST_STATE.fetch_add(1, Ordering::Relaxed),
            inner: RefCell::new(Inner {
                item_count,
                alignment,
                overdraw,
                requested: initial_range(item_count, alignment),
                logical_scroll_top: ListOffset {
                    item_ix: if alignment == ListAlignment::Bottom {
                        item_count
                    } else {
                        0
                    },
                    offset_in_item: px(0.),
                },
                tail_mode: false,
                following_tail: false,
                revision: 0,
                commands: Vec::new(),
            }),
            scroll_handler: RefCell::new(None),
        }))
    }
    pub fn reset(&self, element_count: usize) {
        let count = element_count.min(wire::MAX_LIST_ITEMS);
        let mut inner = self.0.inner.borrow_mut();
        inner.item_count = count;
        inner.requested = initial_range(count, inner.alignment);
        inner.logical_scroll_top = ListOffset {
            item_ix: if inner.alignment == ListAlignment::Bottom {
                count
            } else {
                0
            },
            offset_in_item: px(0.),
        };
        inner.push(wire::ListCommand::Reset { count });
    }
    pub fn remeasure(&self) {
        self.remeasure_items(0..self.item_count());
    }
    pub fn remeasure_items(&self, range: Range<usize>) {
        let mut inner = self.0.inner.borrow_mut();
        let range = bounded_range(range, inner.item_count);
        if !range.is_empty() {
            inner.push(wire::ListCommand::Remeasure {
                start: range.start,
                end: range.end,
            });
        }
    }
    pub fn item_count(&self) -> usize {
        self.0.inner.borrow().item_count
    }
    pub fn splice(&self, old_range: Range<usize>, count: usize) {
        let mut inner = self.0.inner.borrow_mut();
        let old_range = bounded_range(old_range, inner.item_count);
        let count = count.min(wire::MAX_LIST_ITEMS);
        inner.item_count = inner
            .item_count
            .saturating_sub(old_range.len())
            .saturating_add(count)
            .min(wire::MAX_LIST_ITEMS);
        inner.requested = initial_range(inner.item_count, inner.alignment);
        inner.push(wire::ListCommand::Splice {
            start: old_range.start,
            end: old_range.end,
            count,
        });
    }
    pub fn set_scroll_handler(
        &self,
        handler: impl FnMut(&ListScrollEvent, &mut Window, &mut App) + 'static,
    ) {
        *self.0.scroll_handler.borrow_mut() = Some(Box::new(handler));
    }
    pub fn logical_scroll_top(&self) -> ListOffset {
        self.0.inner.borrow().logical_scroll_top
    }
    pub fn scroll_to(&self, scroll_top: ListOffset) {
        let mut inner = self.0.inner.borrow_mut();
        let offset = ListOffset {
            item_ix: scroll_top.item_ix.min(inner.item_count),
            offset_in_item: px(f32::from(scroll_top.offset_in_item).max(0.)),
        };
        inner.logical_scroll_top = offset;
        inner.push(wire::ListCommand::ScrollTo(to_wire_offset(offset)));
    }
    pub fn scroll_to_end(&self) {
        let mut inner = self.0.inner.borrow_mut();
        inner.logical_scroll_top = ListOffset {
            item_ix: inner.item_count,
            offset_in_item: px(0.),
        };
        inner.push(wire::ListCommand::ScrollToEnd);
    }
    pub fn scroll_to_reveal_item(&self, ix: usize) {
        let mut inner = self.0.inner.borrow_mut();
        let ix = ix.min(inner.item_count.saturating_sub(1));
        inner.requested = bounded_window(ix, ix.saturating_add(1), inner.item_count);
        inner.push(wire::ListCommand::ScrollToRevealItem(ix));
    }
    pub fn set_follow_mode(&self, mode: FollowMode) {
        let tail = mode == FollowMode::Tail;
        let mut inner = self.0.inner.borrow_mut();
        if inner.tail_mode != tail {
            inner.tail_mode = tail;
            inner.following_tail = tail;
            inner.push(wire::ListCommand::SetFollowMode { tail });
        }
    }
    pub fn pause_following_tail(&self) {
        let mut inner = self.0.inner.borrow_mut();
        if inner.following_tail {
            inner.following_tail = false;
            inner.push(wire::ListCommand::PauseFollowingTail);
        }
    }
    pub fn is_following_tail(&self) -> bool {
        self.0.inner.borrow().following_tail
    }
    fn request(&self, request: &wire::ListRequest) {
        let mut inner = self.0.inner.borrow_mut();
        inner.requested = bounded_window(request.start, request.end, inner.item_count);
    }
    fn observe(&self, event: &wire::ListScroll, window: &mut Window, app: &mut App) {
        {
            let mut inner = self.0.inner.borrow_mut();
            inner.logical_scroll_top = from_wire_offset(event.offset);
            inner.following_tail = event.is_following_tail;
        }
        let mut handler = self.0.scroll_handler.borrow_mut().take();
        if let Some(callback) = handler.as_mut() {
            callback(
                &ListScrollEvent {
                    visible_range: event.visible_start..event.visible_end,
                    count: event.count,
                    is_scrolled: event.is_scrolled,
                    is_following_tail: event.is_following_tail,
                },
                window,
                app,
            );
        }
        *self.0.scroll_handler.borrow_mut() = handler;
    }
}
impl Inner {
    fn push(&mut self, command: wire::ListCommand) {
        self.revision = self.revision.wrapping_add(1);
        if self.commands.len() < wire::MAX_LIST_COMMANDS {
            self.commands.push(command);
        }
    }
}

pub struct List {
    state: ListState,
    render_item: ItemRenderer,
    style: StyleRefinement,
    sizing_behavior: ListSizingBehavior,
}
pub fn list(
    state: ListState,
    render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> List {
    List {
        state,
        render_item: Box::new(render_item),
        style: StyleRefinement::default(),
        sizing_behavior: ListSizingBehavior::default(),
    }
}
impl List {
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }
}
impl Styled for List {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl Element for List {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            state,
            mut render_item,
            style,
            sizing_behavior,
        } = *self;
        let request_state = state.clone();
        let request_handler =
            lowering.route(move |request: &wire::ListRequest, _, _| request_state.request(request));
        let scroll_handler = state.0.scroll_handler.borrow().is_some().then(|| {
            let scroll_state = state.clone();
            lowering.route(move |event: &wire::ListScroll, window, app| {
                scroll_state.observe(event, window, app)
            })
        });
        let (item_count, alignment, overdraw, following_tail, revision, commands, range) = {
            let mut inner = state.0.inner.borrow_mut();
            (
                inner.item_count,
                inner.alignment,
                inner.overdraw,
                inner.tail_mode,
                inner.revision,
                std::mem::take(&mut inner.commands),
                inner.requested.clone(),
            )
        };
        let mut children = Vec::with_capacity(range.len().min(wire::MAX_LIST_ROWS));
        for index in range.clone().take(wire::MAX_LIST_ROWS) {
            let (window, app) = lowering.parts();
            let row = render_item(index, window, app);
            children.push(lowering.lower_element(row));
        }
        wire::Node::List {
            state: state.0.id,
            path: lowering.current_path().to_vec(),
            item_count,
            alignment: wire_alignment(alignment),
            overdraw: f32::from(overdraw),
            sizing: wire_sizing(sizing_behavior),
            following_tail,
            revision,
            commands,
            request_handler,
            scroll_handler,
            range_start: range.start,
            style,
            children,
        }
    }
}
impl IntoElement for List {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl gpui::prelude::FluentBuilder for List {}

fn initial_range(count: usize, alignment: ListAlignment) -> Range<usize> {
    match alignment {
        ListAlignment::Top => 0..count.min(wire::MAX_LIST_ROWS),
        ListAlignment::Bottom => count.saturating_sub(wire::MAX_LIST_ROWS)..count,
    }
}
fn bounded_range(range: Range<usize>, count: usize) -> Range<usize> {
    let start = range.start.min(count);
    start..range.end.clamp(start, count)
}
fn bounded_window(start: usize, end: usize, count: usize) -> Range<usize> {
    let start = start.min(count);
    start
        ..end
            .clamp(start, count)
            .min(start.saturating_add(wire::MAX_LIST_ROWS))
}
fn wire_alignment(v: ListAlignment) -> wire::ListAlignment {
    match v {
        ListAlignment::Top => wire::ListAlignment::Top,
        ListAlignment::Bottom => wire::ListAlignment::Bottom,
    }
}
fn wire_sizing(v: ListSizingBehavior) -> wire::ListSizingBehavior {
    match v {
        ListSizingBehavior::Infer => wire::ListSizingBehavior::Infer,
        ListSizingBehavior::Auto => wire::ListSizingBehavior::Auto,
    }
}
fn to_wire_offset(v: ListOffset) -> wire::ListOffset {
    wire::ListOffset {
        item_ix: v.item_ix,
        offset_in_item: f32::from(v.offset_in_item),
    }
}
fn from_wire_offset(v: wire::ListOffset) -> ListOffset {
    ListOffset {
        item_ix: v.item_ix,
        offset_in_item: px(v.offset_in_item.max(0.)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{div, Context, Driver, InteractiveElement, ParentElement, Render, View};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct ListView {
        #[serde(skip)]
        state: ListState,
        #[serde(skip)]
        rendered: Vec<usize>,
        scrolls: usize,
    }

    impl View for ListView {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                state: ListState::new(2_000, ListAlignment::Bottom, px(160.)),
                rendered: Vec::new(),
                scrolls: 0,
            }
        }
    }

    impl Render for ListView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.state.set_scroll_handler(cx.listener(|view, _, _, cx| {
                view.scrolls += 1;
                cx.notify();
            }));
            list(
                self.state.clone(),
                cx.processor(|view, index, _, _| {
                    view.rendered.push(index);
                    div()
                        .id(format!("row-{index}"))
                        .child(index.to_string())
                        .into_any_element()
                }),
            )
        }
    }

    fn list_node(frame: &wire::Frame) -> &wire::Node {
        frame.root.as_ref().expect("list root")
    }

    #[test]
    fn two_thousand_items_only_evaluate_a_bounded_requested_window() {
        let mut driver = Driver::<ListView>::new();
        let first = driver.tick(vec![]);
        let (handler, scroll_handler) = match list_node(&first) {
            wire::Node::List {
                item_count,
                range_start,
                children,
                request_handler,
                scroll_handler,
                ..
            } => {
                assert_eq!(*item_count, 2_000);
                assert_eq!(*range_start, 2_000 - wire::MAX_LIST_ROWS);
                assert_eq!(children.len(), wire::MAX_LIST_ROWS);
                (
                    *request_handler,
                    scroll_handler.expect("settled scroll route"),
                )
            }
            other => panic!("expected list, got {other:?}"),
        };
        driver
            .entity()
            .read(|view| assert_eq!(view.rendered.len(), wire::MAX_LIST_ROWS));

        let second = driver.tick(vec![wire::Event::ListRequest {
            handler,
            request: wire::ListRequest {
                start: 0,
                end: usize::MAX,
            },
        }]);
        match list_node(&second) {
            wire::Node::List {
                range_start,
                children,
                ..
            } => {
                assert_eq!(*range_start, 0);
                assert_eq!(children.len(), wire::MAX_LIST_ROWS);
            }
            other => panic!("expected list, got {other:?}"),
        }
        driver
            .entity()
            .read(|view| assert_eq!(view.rendered.len(), 2 * wire::MAX_LIST_ROWS));

        driver.tick(vec![wire::Event::ListScroll {
            handler: scroll_handler,
            event: wire::ListScroll {
                visible_start: 1_990,
                visible_end: 2_000,
                count: 2_000,
                is_scrolled: true,
                is_following_tail: false,
                offset: wire::ListOffset {
                    item_ix: 1_990,
                    offset_in_item: 7.,
                },
            },
        }]);
        driver.entity().read(|view| {
            assert_eq!(view.scrolls, 1);
            assert_eq!(view.state.logical_scroll_top().item_ix, 1_990);
            assert!(!view.state.is_following_tail());
        });
    }

    #[test]
    fn state_operations_cross_once_as_native_commands_and_patch_props() {
        let mut driver = Driver::<ListView>::new();
        driver.tick(vec![]);
        let state = driver.entity().read(|view| view.state.clone());
        state.splice(0..0, 20);
        state.remeasure_items(100..102);
        state.scroll_to_reveal_item(1_999);
        driver.app.notify();
        let frame = driver.tick(vec![]);
        match list_node(&frame) {
            wire::Node::List {
                item_count,
                commands,
                children,
                ..
            } => {
                assert_eq!(*item_count, 2_020);
                assert_eq!(commands.len(), 3);
                assert!(children.len() <= wire::MAX_LIST_ROWS);
            }
            other => panic!("expected list, got {other:?}"),
        }
        state.scroll_to(ListOffset {
            item_ix: 1_999,
            offset_in_item: px(3.),
        });
        driver.app.notify();
        let patched = driver.tick(vec![]);
        assert!(
            patched
                .patches
                .iter()
                .any(|patch| matches!(patch, wire::Patch::Props { .. })),
            "same-shape state operations update by props patches"
        );
        driver.app.notify();
        let next = driver.tick(vec![]);
        assert!(
            matches!(list_node(&next), wire::Node::List { commands, .. } if commands.is_empty())
        );
    }

    #[test]
    fn list_handles_and_requested_windows_are_driver_isolated() {
        let mut first = Driver::<ListView>::new();
        let mut second = Driver::<ListView>::new();
        let first_frame = first.tick(vec![]);
        let second_frame = second.tick(vec![]);
        let (first_state, first_handler) = match list_node(&first_frame) {
            wire::Node::List {
                state,
                request_handler,
                ..
            } => (*state, *request_handler),
            _ => unreachable!(),
        };
        let second_state = match list_node(&second_frame) {
            wire::Node::List { state, .. } => *state,
            _ => unreachable!(),
        };
        assert_ne!(first_state, second_state);
        first.tick(vec![wire::Event::ListRequest {
            handler: first_handler,
            request: wire::ListRequest { start: 7, end: 9 },
        }]);
        first
            .entity()
            .read(|view| assert_eq!(view.rendered.last().copied(), Some(8)));
        second
            .entity()
            .read(|view| assert_eq!(view.rendered.last().copied(), Some(1_999)));
    }
}
