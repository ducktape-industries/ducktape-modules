//! Blocks drawn as elements: styled runs over one `StyledText` per
//! paragraph, links pressed through the caller's [`OnLink`].
use std::ops::Range;
use std::rc::Rc;

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    Div, FontStyle, FontWeight, HighlightStyle, Stateful, StrikethroughStyle, UnderlineStyle,
};

use super::parse::{Block, Text, parse};
use crate::ui::components::id;
use crate::ui::highlight;

/// What a pressed link does, given its raw destination.
pub(crate) type OnLink = Rc<dyn Fn(&String, &mut Window, &mut App)>;

/// Rich text for one paragraph: styled runs, links pressed through `on_link`.
fn rich(element_id: String, text: &Text, theme: &Theme, on_link: &OnLink) -> AnyElement {
    let mut links = Vec::new();
    let mut targets = Vec::new();
    let mut mono = Vec::new();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = text
        .runs
        .iter()
        .map(|(range, marks)| {
            let mut style = HighlightStyle::default();
            if marks.bold {
                style.font_weight = Some(FontWeight::SEMIBOLD);
            }
            if marks.italic {
                style.font_style = Some(FontStyle::Italic);
            }
            if marks.strike {
                style.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                });
            }
            if marks.quiet {
                style.color = Some(theme.muted);
            }
            if marks.code {
                style.background_color = Some(theme.surface_raised);
                mono.push((range.clone(), design::fonts::FAMILY_MONO.into()));
            }
            if let Some(target) = &marks.link {
                style.color = Some(theme.link);
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.link),
                    wavy: false,
                });
                links.push(range.clone());
                targets.push(target.clone());
            }
            (range.clone(), style)
        })
        .collect();
    let styled = StyledText::new(text.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(mono);
    let on_link = on_link.clone();
    InteractiveText::new(id(element_id), styled)
        .on_click(links, move |index, window, cx| {
            if let Some(target) = targets.get(index) {
                on_link(target, window, cx);
            }
        })
        .into_any_element()
}

/// A markdown body that is not a file (a change's), its links pressed
/// through `on_link`.
pub(crate) fn render(name: &str, text: &str, theme: &Theme, on_link: &OnLink) -> AnyElement {
    render_blocks(name, &parse(text), theme, on_link)
}

/// Blocks [`parse`] already made, drawn under the element `name`.
pub(crate) fn render_blocks(
    name: &str,
    blocks: &[Block],
    theme: &Theme,
    on_link: &OnLink,
) -> AnyElement {
    column(name, blocks, theme, on_link)
        .gap_2()
        .text_size(design::text::BODY)
        .text_color(theme.foreground)
        .into_any_element()
}

fn column(name: &str, blocks: &[Block], theme: &Theme, on_link: &OnLink) -> Stateful<Div> {
    let mut out = div().id(id(name.to_owned())).flex().flex_col().gap_2();
    for (at, block) in blocks.iter().enumerate() {
        let key = format!("{name}-{at}");
        let element: AnyElement = match block {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => design::text::TITLE,
                    2 => design::text::SECTION,
                    _ => design::text::BODY,
                };
                let mut heading = div()
                    .id(id(format!("{key}-h")))
                    .text_size(size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .role(Role::Heading)
                    .aria_level(*level)
                    .pt_2()
                    .child(rich(key, text, theme, on_link));
                if *level <= 2 {
                    heading = heading.pb_1().border_b_1().border_color(theme.border);
                }
                heading.into_any_element()
            }
            Block::Paragraph(text) => rich(key, text, theme, on_link),
            Block::Item(marker, body) => div()
                .id(id(format!("{key}-li")))
                .flex()
                .gap_2()
                .child(
                    div()
                        .min_w(px(18.))
                        .text_color(theme.muted)
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::SECONDARY)
                        .child(marker.clone()),
                )
                .child(
                    column(&key, body, theme, on_link)
                        .gap_1()
                        .flex_1()
                        .min_w(px(0.)),
                )
                .into_any_element(),
            Block::Code(lang, text) => code(&key, lang, text, theme),
            Block::Quote(body) => div()
                .id(id(format!("{key}-quote")))
                .pl_3()
                .border_l_1()
                .border_color(theme.border_strong)
                .text_color(theme.muted)
                .child(column(&key, body, theme, on_link))
                .into_any_element(),
            Block::Rule => div().h(px(1.)).bg(theme.border).into_any_element(),
            Block::Table(rows) => table(&key, rows, theme, on_link),
        };
        out = out.child(element);
    }
    out
}

fn code(key: &str, lang: &str, text: &str, theme: &Theme) -> AnyElement {
    let lines: Vec<&str> = text.split('\n').collect();
    let tokens = highlight::tokens(lang, &lines);
    let mut block = div()
        .id(id(format!("{key}-code")))
        .flex()
        .flex_col()
        .p_2()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .overflow_x_scroll();
    for (at, (line, tokens)) in lines.iter().zip(&tokens).enumerate() {
        block = block.child(highlight::line(
            id(format!("{key}-{at}")),
            line,
            tokens,
            theme,
        ));
    }
    block.into_any_element()
}

fn table(key: &str, rows: &[Vec<Text>], theme: &Theme, on_link: &OnLink) -> AnyElement {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut grid = div()
        .id(id(format!("{key}-table")))
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme.border);
    let blank = Text::default();
    for (r, row) in rows.iter().enumerate() {
        let mut line = div()
            .flex()
            .when(r > 0, |line| line.border_t_1().border_color(theme.border));
        if r == 0 {
            line = line.bg(theme.surface).font_weight(FontWeight::SEMIBOLD);
        }
        for c in 0..width {
            line = line.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .px_2()
                    .py_1()
                    .when(c > 0, |cell| cell.border_l_1().border_color(theme.border))
                    .child(rich(
                        format!("{key}-{r}-{c}"),
                        row.get(c).unwrap_or(&blank),
                        theme,
                        on_link,
                    )),
            );
        }
        grid = grid.child(line);
    }
    grid.into_any_element()
}

#[cfg(test)]
mod view_tests {
    use ducktape_view_guest::testing::TestAppContext;
    use ducktape_view_guest::{Context, IntoElement, Render, View, Window};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Doc;

    impl View for Doc {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Doc
        }
    }

    // the forge view's own manifest, so the doc reaches only what it does
    impl ducktape_view_guest::Declared for Doc {
        const CAPABILITIES: &'static [&'static str] =
            <crate::Forge as ducktape_view_guest::Declared>::CAPABILITIES;
    }

    impl Render for Doc {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = *cx.global::<ducktape_view_guest::Theme>();
            let on_link: super::super::OnLink = std::rc::Rc::new(|dest, _, cx| {
                if let Some(super::super::Target::Web(url)) = super::super::target(b"", dest) {
                    cx.host().open_link(&url);
                }
            });
            super::super::render(
                "doc",
                "# Title\n\nSee [rfcs](duck://net-1/forge/rfcs), [local](../a.md) or https://x.example",
                &theme,
                &on_link,
            )
        }
    }

    #[test]
    fn a_pressed_web_link_goes_to_the_host() {
        let mut cx = TestAppContext::new();
        cx.open::<Doc>();
        cx.run_until_parked();
        assert!(cx.has_text("Title"));
        // three links press; the relative one is the forge's to open, not the host's
        for index in 0..3 {
            cx.simulate_rich_click("doc-1", index);
        }
        assert_eq!(
            cx.host().opened_links(),
            vec!["duck://net-1/forge/rfcs", "https://x.example"]
        );
    }
}
