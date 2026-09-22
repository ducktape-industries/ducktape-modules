//! GPUI-shaped rich text recipes lowered into one native host paragraph.
use crate::{wire, App, Element, ElementId, IntoElement, Lowering, Window};
use gpui::{
    HighlightStyle, MouseMoveEvent, SharedString, StyleRefinement, Styled, TextRun, TextStyle,
};
use std::ops::Range;

type ClickListener = Box<dyn Fn(usize, &mut Window, &mut App)>;
type HoverListener = Box<dyn Fn(Option<usize>, MouseMoveEvent, &mut Window, &mut App)>;
type TooltipBuilder = Box<dyn Fn(usize, &mut Window, &mut App) -> Option<crate::AnyView>>;

pub struct StyledText {
    text: SharedString,
    runs: Option<Vec<TextRun>>,
    highlights: Option<Vec<(Range<usize>, HighlightStyle)>>,
    font_family_overrides: Vec<(Range<usize>, SharedString)>,
    style: StyleRefinement,
}

impl StyledText {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            runs: None,
            highlights: None,
            font_family_overrides: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn with_default_highlights(
        self,
        default_style: &TextStyle,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        debug_assert!(self.highlights.is_none());
        let mut runs = Vec::new();
        let mut offset = 0;
        for (range, highlight) in highlights {
            if offset < range.start {
                runs.push(default_style.clone().to_run(range.start - offset));
            }
            runs.push(
                default_style
                    .clone()
                    .highlight(highlight)
                    .to_run(range.len()),
            );
            offset = range.end;
        }
        if offset < self.text.len() {
            runs.push(default_style.to_run(self.text.len() - offset));
        }
        self.with_runs(runs)
    }

    pub fn with_highlights(
        mut self,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        debug_assert!(self.runs.is_none());
        self.highlights = Some(highlights.into_iter().collect());
        self
    }

    pub fn with_font_family_overrides(
        mut self,
        overrides: impl IntoIterator<Item = (Range<usize>, SharedString)>,
    ) -> Self {
        self.font_family_overrides = overrides.into_iter().collect();
        self
    }

    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        let mut text = &*self.text;
        for run in &runs {
            text = text.get(run.len..).unwrap_or_else(|| {
                #[cfg(debug_assertions)]
                panic!("invalid text run. Text: '{text}', run: {run:?}");
                #[cfg(not(debug_assertions))]
                panic!("invalid text run");
            });
        }
        assert!(text.is_empty(), "invalid text run");
        self.runs = Some(runs);
        self
    }

    fn lower(
        self,
        id: Option<wire::ElementIdWire>,
        clickable_ranges: Vec<Range<usize>>,
        on_click: Option<u32>,
        on_hover: Option<u32>,
        tooltip: Option<wire::RichTextTooltip>,
    ) -> wire::Node {
        let runs = match self.runs {
            Some(runs) => wire::RichTextRuns::Runs(runs.into_iter().map(Into::into).collect()),
            None => wire::RichTextRuns::Highlights(
                self.highlights
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(range, style)| (range, style.into()))
                    .collect(),
            ),
        };
        wire::Node::RichText {
            id,
            style: self.style,
            text: self.text.to_string(),
            runs,
            font_family_overrides: self.font_family_overrides,
            clickable_ranges,
            on_click,
            on_hover,
            tooltip,
        }
    }
}

impl Styled for StyledText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for StyledText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for StyledText {
    fn lower(self: Box<Self>, _lowering: &mut Lowering<'_>) -> wire::Node {
        (*self).lower(None, Vec::new(), None, None, None)
    }
}

pub struct InteractiveText {
    id: ElementId,
    text: StyledText,
    clickable_ranges: Vec<Range<usize>>,
    on_click: Option<ClickListener>,
    on_hover: Option<HoverListener>,
    tooltip: Option<TooltipBuilder>,
}

impl InteractiveText {
    pub fn new(id: impl Into<ElementId>, text: StyledText) -> Self {
        Self {
            id: id.into(),
            text,
            clickable_ranges: Vec::new(),
            on_click: None,
            on_hover: None,
            tooltip: None,
        }
    }

    pub fn on_click(
        mut self,
        ranges: Vec<Range<usize>>,
        listener: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.clickable_ranges = ranges;
        self.on_click = Some(Box::new(listener));
        self
    }

    pub fn on_hover(
        mut self,
        listener: impl Fn(Option<usize>, MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_hover = Some(Box::new(listener));
        self
    }

    pub fn tooltip(
        mut self,
        builder: impl Fn(usize, &mut Window, &mut App) -> Option<crate::AnyView> + 'static,
    ) -> Self {
        self.tooltip = Some(Box::new(builder));
        self
    }
}

impl Styled for InteractiveText {
    fn style(&mut self) -> &mut StyleRefinement {
        self.text.style()
    }
}

impl IntoElement for InteractiveText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for InteractiveText {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            text,
            clickable_ranges,
            on_click,
            on_hover,
            tooltip,
            ..
        } = *self;
        let id = lowering.current_path().last().cloned();
        let on_click = on_click.map(|listener| {
            lowering.route(move |index: &u32, window, cx| listener(*index as usize, window, cx))
        });
        let on_hover = on_hover.map(|listener| {
            lowering.route(move |event: &wire::RichTextHover, window, cx| {
                listener(
                    event.index.map(|index| index as usize),
                    MouseMoveEvent {
                        position: event.position,
                        pressed_button: event.pressed_button.map(Into::into),
                        modifiers: event.modifiers,
                    },
                    window,
                    cx,
                )
            })
        });
        let tooltip = tooltip.map(|builder| wire::RichTextTooltip {
            request: lowering.rich_text_tooltip(builder),
            character_index: None,
            content: None,
        });
        text.lower(id, clickable_ranges, on_click, on_hover, tooltip)
    }
}

impl gpui::prelude::FluentBuilder for StyledText {}
impl gpui::prelude::FluentBuilder for InteractiveText {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn callbacks_are_allocated_only_while_lowering_each_frame() {
        let make = || {
            InteractiveText::new("rich", StyledText::new("one two"))
                .on_click(vec![0..3, 4..7], |_, _, _| {})
        };
        let mut app = App::for_driver(false);
        let mut window = app.window();
        let first = Lowering::new(&mut window, &mut app).lower(make());
        let mut window = app.window();
        let second = Lowering::new(&mut window, &mut app).lower(make());
        assert!(matches!(
            first,
            wire::Node::RichText {
                on_click: Some(0),
                ..
            }
        ));
        assert!(matches!(
            second,
            wire::Node::RichText {
                on_click: Some(1),
                ..
            }
        ));
    }

    #[test]
    fn a_new_frame_replaces_the_old_rich_text_callback() {
        let hits = Rc::new(RefCell::new(Vec::new()));
        let mut app = App::for_driver(false);
        let first_hits = hits.clone();
        let mut window = app.window();
        let first = Lowering::new(&mut window, &mut app).lower(
            InteractiveText::new("rich", StyledText::new("one"))
                .on_click(std::iter::once(0..3).collect(), move |_, _, _| {
                    first_hits.borrow_mut().push(1)
                }),
        );
        let wire::Node::RichText {
            on_click: Some(first_handler),
            ..
        } = first
        else {
            panic!("first frame route");
        };
        crate::slots::reset(&app.inner.slots);
        let second_hits = hits.clone();
        let mut window = app.window();
        let second = Lowering::new(&mut window, &mut app).lower(
            InteractiveText::new("rich", StyledText::new("two"))
                .on_click(std::iter::once(0..3).collect(), move |_, _, _| {
                    second_hits.borrow_mut().push(2)
                }),
        );
        let wire::Node::RichText {
            on_click: Some(second_handler),
            ..
        } = second
        else {
            panic!("second frame route");
        };
        assert_eq!(first_handler, second_handler);
        let slots = app.inner.slots.clone();
        let mut window = app.window();
        assert!(crate::slots::run_route(
            &slots,
            first_handler,
            &0u32,
            &mut window,
            &mut app,
        ));
        assert_eq!(&*hits.borrow(), &[2]);
    }

    #[test]
    fn two_ranges_dispatch_distinct_value_indices() {
        let hits = Rc::new(RefCell::new(Vec::new()));
        let routed = hits.clone();
        let mut app = App::for_driver(false);
        let mut window = app.window();
        let node = Lowering::new(&mut window, &mut app).lower(
            InteractiveText::new("rich", StyledText::new("one two"))
                .on_click(vec![0..3, 4..7], move |index, _, _| {
                    routed.borrow_mut().push(index)
                }),
        );
        let wire::Node::RichText {
            on_click: Some(handler),
            ..
        } = node
        else {
            panic!("rich text route");
        };
        let slots = app.inner.slots.clone();
        for index in [0u32, 1] {
            let mut window = app.window();
            assert!(crate::slots::run_route(
                &slots,
                handler,
                &index,
                &mut window,
                &mut app,
            ));
        }
        assert_eq!(&*hits.borrow(), &[0, 1]);
    }
}
