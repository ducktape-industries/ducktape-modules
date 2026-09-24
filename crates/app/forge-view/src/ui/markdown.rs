//! Markdown as a document: headings, paragraphs, lists, fenced code,
//! quotes, rules and pipe tables, each a block of rich text. Inline marks
//! (bold, italic, links) are chat's own tokenizer, so a README reads with
//! the composer's syntax; inline `code` is split out first, since chat's
//! wire has no mark for it.
use ducktape_view_guest::design;
use std::ops::Range;

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{FontStyle, FontWeight, HighlightStyle, UnderlineStyle};

use crate::ui::components::id;
use crate::ui::highlight;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Heading(usize, String),
    Paragraph(String),
    /// nesting depth, marker (`•` or `3.`), text
    Item(usize, String, String),
    Code(String, String),
    Quote(String),
    Rule,
    /// the header row first
    Table(Vec<Vec<String>>),
}

pub(crate) fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        let trimmed = line.trim();
        at += 1;
        if trimmed.is_empty() {
            continue;
        }
        if let Some(fence) = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f)) {
            let lang = trimmed.trim_start_matches(fence).trim().to_owned();
            let mut code = Vec::new();
            while at < lines.len() && !lines[at].trim().starts_with(fence) {
                code.push(lines[at]);
                at += 1;
            }
            at += 1;
            blocks.push(Block::Code(lang, code.join("\n")));
        } else if let Some(level) = heading(trimmed) {
            let text = trimmed[level..].trim().trim_end_matches('#').trim();
            blocks.push(Block::Heading(level, text.to_owned()));
        } else if rule(trimmed) {
            blocks.push(Block::Rule);
        } else if trimmed.starts_with('>') {
            let mut quote = vec![trimmed.trim_start_matches('>').trim()];
            while at < lines.len() && lines[at].trim().starts_with('>') {
                quote.push(lines[at].trim().trim_start_matches('>').trim());
                at += 1;
            }
            blocks.push(Block::Quote(join(&quote)));
        } else if let Some((marker, text)) = item(trimmed) {
            let depth = (line.len() - line.trim_start().len()) / 2;
            let mut body = vec![text];
            while at < lines.len() && continues(lines[at]) && lines[at].starts_with("  ") {
                body.push(lines[at].trim());
                at += 1;
            }
            blocks.push(Block::Item(depth, marker, join(&body)));
        } else if trimmed.starts_with('|') && lines.get(at).is_some_and(|next| separator(next)) {
            let mut rows = vec![cells(trimmed)];
            at += 1;
            while at < lines.len() && lines[at].trim().starts_with('|') {
                rows.push(cells(lines[at].trim()));
                at += 1;
            }
            blocks.push(Block::Table(rows));
        } else {
            let mut body = vec![trimmed];
            while at < lines.len() && continues(lines[at]) {
                body.push(lines[at].trim());
                at += 1;
            }
            blocks.push(Block::Paragraph(join(&body)));
        }
    }
    blocks
}

/// A soft line break is a space, as CommonMark reads it.
fn join(lines: &[&str]) -> String {
    lines.join(" ")
}

fn heading(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    ((1..=6).contains(&level) && line[level..].starts_with(' ')).then_some(level)
}

fn rule(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|mark| compact.chars().all(|c| c.to_string() == *mark))
}

fn item(line: &str) -> Option<(String, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(bullet) {
            let text = text.trim_start();
            return Some(if let Some(rest) = text.strip_prefix("[ ] ") {
                ("☐".to_owned(), rest)
            } else if let Some(rest) = text.strip_prefix("[x] ") {
                ("☑".to_owned(), rest)
            } else {
                ("•".to_owned(), text)
            });
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &line[digits..];
    (digits > 0 && (rest.starts_with(". ") || rest.starts_with(") ")))
        .then(|| (format!("{}.", &line[..digits]), rest[2..].trim_start()))
}

/// A line that carries on the paragraph above it rather than opening a block.
fn continues(line: &str) -> bool {
    let trimmed = line.trim();
    !(trimmed.is_empty()
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || trimmed.starts_with('>')
        || trimmed.starts_with('|')
        || heading(trimmed).is_some()
        || rule(trimmed)
        || item(trimmed).is_some())
}

fn separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.contains('-')
        && trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn cells(line: &str) -> Vec<String> {
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// A run's marks, as one inline tokenizer pass sees them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// One paragraph's text with its marked runs: `` `code` `` first, then
/// chat's `**bold**`, `*italic*`, `[label](url)` and bare links.
pub(crate) fn inline(text: &str) -> (String, Vec<(Range<usize>, Marks)>) {
    let mut out = String::new();
    let mut runs = Vec::new();
    let parts = text.split('`').count();
    for (index, part) in text.split('`').enumerate() {
        // an odd count of backticks leaves the last one literal
        let closed = index % 2 == 1 && parts > index + 1;
        if closed {
            let start = out.len();
            out.push_str(part);
            runs.push((
                start..out.len(),
                Marks {
                    code: true,
                    ..Marks::default()
                },
            ));
            continue;
        }
        if index % 2 == 1 {
            out.push('`');
        }
        for span in chat::message::inline_spans(part) {
            let start = out.len();
            out.push_str(&span.text);
            let mut marks = Marks::default();
            for mark in span.marks {
                match mark {
                    chat::Mark::Bold => marks.bold = true,
                    chat::Mark::Italic => marks.italic = true,
                    chat::Mark::Link(target) => marks.link = Some(target),
                    chat::Mark::Mention(_) => {}
                }
            }
            if marks != Marks::default() {
                runs.push((start..out.len(), marks));
            }
        }
    }
    (out, runs)
}

/// Rich text for one paragraph: styled runs, links pressed through the host.
fn rich(element_id: String, text: &str, theme: &Theme) -> AnyElement {
    let (text, runs) = inline(text);
    let mut links = Vec::new();
    let mut targets = Vec::new();
    let mut mono = Vec::new();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = runs
        .into_iter()
        .map(|(range, marks)| {
            let mut style = HighlightStyle::default();
            if marks.bold {
                style.font_weight = Some(FontWeight::SEMIBOLD);
            }
            if marks.italic {
                style.font_style = Some(FontStyle::Italic);
            }
            if marks.code {
                style.background_color = Some(theme.surface_raised);
                mono.push((range.clone(), design::fonts::FAMILY_MONO.into()));
            }
            if let Some(target) = marks.link {
                style.color = Some(theme.link);
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.link),
                    wavy: false,
                });
                links.push(range.clone());
                targets.push(target);
            }
            (range, style)
        })
        .collect();
    let styled = StyledText::new(text)
        .with_highlights(highlights)
        .with_font_family_overrides(mono);
    InteractiveText::new(id(element_id), styled)
        .on_click(links, move |index, _, cx| {
            // `duck://` and web links go to the app; a relative link names
            // a path this pane cannot resolve, so it stays text
            if let Some(target) = targets.get(index).filter(|t| t.contains("://")) {
                cx.host().open_link(target);
            }
        })
        .into_any_element()
}

/// A markdown document, block by block, under the element `name`.
pub(crate) fn render(name: &str, text: &str, theme: &Theme) -> AnyElement {
    render_blocks(name, &parse(text), theme)
}

/// Blocks [`parse`] already made, drawn as [`render`] draws them.
pub(crate) fn render_blocks(name: &str, blocks: &[Block], theme: &Theme) -> AnyElement {
    let mut column = div()
        .id(id(name.to_owned()))
        .flex()
        .flex_col()
        .gap_2()
        .text_size(design::text::BODY)
        .text_color(theme.foreground);
    for (at, block) in blocks.iter().cloned().enumerate() {
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
                    .aria_level(level)
                    .pt_2()
                    .child(rich(key, &text, theme));
                if level <= 2 {
                    heading = heading.pb_1().border_b_1().border_color(theme.border);
                }
                heading.into_any_element()
            }
            Block::Paragraph(text) => rich(key, &text, theme),
            Block::Item(depth, marker, text) => div()
                .id(id(format!("{key}-li")))
                .flex()
                .gap_2()
                .pl(px(depth as f32 * 18.))
                .child(
                    div()
                        .min_w(px(18.))
                        .text_color(theme.muted)
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::SECONDARY)
                        .child(marker),
                )
                .child(div().flex_1().min_w(px(0.)).child(rich(key, &text, theme)))
                .into_any_element(),
            Block::Code(lang, text) => code(&key, &lang, &text, theme),
            Block::Quote(text) => div()
                .id(id(format!("{key}-quote")))
                .pl_3()
                .border_l_1()
                .border_color(theme.border_strong)
                .text_color(theme.muted)
                .child(rich(key, &text, theme))
                .into_any_element(),
            Block::Rule => div().h(px(1.)).bg(theme.border).into_any_element(),
            Block::Table(rows) => table(&key, rows, theme),
        };
        column = column.child(element);
    }
    column.into_any_element()
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

fn table(key: &str, rows: Vec<Vec<String>>, theme: &Theme) -> AnyElement {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut grid = div()
        .id(id(format!("{key}-table")))
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme.border);
    for (r, row) in rows.into_iter().enumerate() {
        let mut line = div()
            .flex()
            .when(r > 0, |line| line.border_t_1().border_color(theme.border));
        if r == 0 {
            line = line.bg(theme.surface).font_weight(FontWeight::SEMIBOLD);
        }
        for c in 0..width {
            let cell = row.get(c).map(String::as_str).unwrap_or("");
            line = line.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .px_2()
                    .py_1()
                    .when(c > 0, |cell| cell.border_l_1().border_color(theme.border))
                    .child(rich(format!("{key}-{r}-{c}"), cell, theme)),
            );
        }
        grid = grid.child(line);
    }
    grid.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_constructs_parse_to_their_blocks() {
        let doc = "# Title\n\nOne line\ncarried on.\n\n- a\n  more of a\n  - nested\n1. first\n\n\
                   > quoted\n> still\n\n---\n\n```rust\nfn main() {}\n```\n\n\
                   | a | b |\n|---|:-:|\n| 1 | 2 |\n## Next ##";
        assert_eq!(
            parse(doc),
            vec![
                Block::Heading(1, "Title".into()),
                Block::Paragraph("One line carried on.".into()),
                Block::Item(0, "•".into(), "a more of a".into()),
                Block::Item(1, "•".into(), "nested".into()),
                Block::Item(0, "1.".into(), "first".into()),
                Block::Quote("quoted still".into()),
                Block::Rule,
                Block::Code("rust".into(), "fn main() {}".into()),
                Block::Table(vec![
                    vec!["a".into(), "b".into()],
                    vec!["1".into(), "2".into()],
                ]),
                Block::Heading(2, "Next".into()),
            ]
        );
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end_and_a_hash_without_space_is_text() {
        assert_eq!(
            parse("#tag\n```\nx"),
            vec![
                Block::Paragraph("#tag".into()),
                Block::Code(String::new(), "x".into()),
            ]
        );
    }

    #[test]
    fn inline_code_bold_italic_and_links_are_runs() {
        let (text, runs) =
            inline("run `make dev` **now**, see [the docs](duck://net/forge/rfcs) or *not*");
        assert_eq!(text, "run make dev now, see the docs or not");
        let at = |needle: &str| text.find(needle).unwrap();
        let code = at("make dev");
        assert_eq!(
            runs,
            vec![
                (
                    code..code + 8,
                    Marks {
                        code: true,
                        ..Marks::default()
                    }
                ),
                (
                    at("now")..at("now") + 3,
                    Marks {
                        bold: true,
                        ..Marks::default()
                    }
                ),
                (
                    at("the docs")..at("the docs") + 8,
                    Marks {
                        link: Some("duck://net/forge/rfcs".into()),
                        ..Marks::default()
                    }
                ),
                (
                    at("not")..at("not") + 3,
                    Marks {
                        italic: true,
                        ..Marks::default()
                    }
                ),
            ]
        );
        // a lone backtick stays literal
        assert_eq!(inline("a ` b").0, "a ` b");
    }
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

    impl Render for Doc {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = *cx.global::<ducktape_view_guest::Theme>();
            super::render(
                "doc",
                "# Title\n\nSee [rfcs](duck://net-1/forge/rfcs), [local](../a.md) or https://x.example",
                &theme,
            )
        }
    }

    #[test]
    fn a_pressed_link_goes_to_the_host() {
        let mut cx = TestAppContext::new();
        cx.open::<Doc>();
        cx.run_until_parked();
        assert!(cx.has_text("Title"));
        // chat's tokenizer takes `[label](url)` with a scheme only: the
        // relative reference stays literal text, and two links are pressable
        assert!(
            cx.texts()
                .iter()
                .any(|text| text.contains("[local](../a.md)"))
        );
        for index in 0..2 {
            cx.simulate_rich_click("doc-1", index);
        }
        assert_eq!(
            cx.host().opened_links(),
            vec!["duck://net-1/forge/rfcs", "https://x.example"]
        );
    }
}
