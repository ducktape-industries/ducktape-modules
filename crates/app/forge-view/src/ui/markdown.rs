//! Markdown as a document, read by pulldown-cmark (CommonMark plus GFM
//! tables, task lists, strikethrough and footnotes) into blocks of rich
//! text. Raw HTML is shown as the text it is, never as markup; an image is
//! its alt text, quiet, and nothing is fetched.
use ducktape_view_guest::design;
use std::ops::Range;
use std::rc::Rc;

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    Div, FontStyle, FontWeight, HighlightStyle, Stateful, StrikethroughStyle, UnderlineStyle,
};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::ui::components::id;
use crate::ui::highlight;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Heading(usize, Text),
    Paragraph(Text),
    /// marker (`•`, `3.`, `☐`, `☑` or a footnote's `[1]`), then its body
    Item(String, Vec<Block>),
    Code(String, String),
    Quote(Vec<Block>),
    Rule,
    /// the header row first
    Table(Vec<Vec<Text>>),
}

/// One run of prose and its marked ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Text {
    pub text: String,
    pub runs: Vec<(Range<usize>, Marks)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// an image's alt text or a footnote reference: drawn muted
    pub quiet: bool,
    pub link: Option<String>,
}

enum Frame {
    Root,
    Quote,
    /// the next ordinal of an ordered list
    List(Option<u64>),
    Item(String),
}

#[derive(Default)]
struct Builder {
    frames: Vec<(Frame, Vec<Block>)>,
    text: Text,
    bold: usize,
    italic: usize,
    strike: usize,
    quiet: usize,
    links: Vec<String>,
    code: Option<(String, String)>,
    table: Option<Vec<Vec<Text>>>,
}

impl Builder {
    fn marks(&self) -> Marks {
        Marks {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: false,
            quiet: self.quiet > 0,
            link: self.links.last().cloned(),
        }
    }

    fn push(&mut self, text: &str, marks: Marks) {
        let start = self.text.text.len();
        self.text.text.push_str(text);
        if marks != Marks::default() {
            self.text.runs.push((start..self.text.text.len(), marks));
        }
    }

    /// Prose outside a link: a bare `https://`, `http://` or `duck://` word
    /// is a link too, as it was under chat's tokenizer.
    fn prose(&mut self, text: &str) {
        let marks = self.marks();
        if marks.link.is_some() {
            return self.push(text, marks);
        }
        let mut rest = text;
        while let Some(at) = ["https://", "http://", "duck://"]
            .iter()
            .filter_map(|scheme| rest.find(scheme))
            .min()
        {
            let end = rest[at..]
                .find(char::is_whitespace)
                .map_or(rest.len(), |len| at + len);
            let url =
                rest[at..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'', '"']);
            self.push(&rest[..at], marks.clone());
            self.push(
                url,
                Marks {
                    link: Some(url.to_owned()),
                    ..marks.clone()
                },
            );
            rest = &rest[at + url.len()..];
        }
        self.push(rest, marks);
    }

    fn take(&mut self) -> Text {
        std::mem::take(&mut self.text)
    }

    fn block(&mut self, block: Block) {
        self.frames.last_mut().expect("root frame").1.push(block);
    }

    /// Loose inline text (a tight list item's) becomes a paragraph before
    /// anything else opens.
    fn flush(&mut self) {
        if !self.text.text.trim().is_empty() {
            let text = self.take();
            self.block(Block::Paragraph(text));
        }
        self.text = Text::default();
    }

    fn close(&mut self) -> (Frame, Vec<Block>) {
        self.flush();
        self.frames.pop().expect("an open frame")
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Image { .. } => {
                self.quiet += 1;
                self.italic += 1;
                self.push("[", self.marks());
            }
            Tag::Superscript | Tag::Subscript => {}
            Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell => self.flush(),
            Tag::TableHead | Tag::TableRow => {
                if let Some(rows) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Vec::new());
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.frames.push((Frame::Quote, Vec::new()));
            }
            Tag::List(first) => {
                self.flush();
                self.frames.push((Frame::List(first), Vec::new()));
            }
            Tag::Item => {
                self.flush();
                let marker = match self.frames.last_mut() {
                    Some((Frame::List(Some(next)), _)) => {
                        *next += 1;
                        format!("{}.", *next - 1)
                    }
                    _ => "•".to_owned(),
                };
                self.frames.push((Frame::Item(marker), Vec::new()));
            }
            Tag::FootnoteDefinition(label) => {
                self.flush();
                self.frames
                    .push((Frame::Item(format!("[{label}]")), Vec::new()));
            }
            Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => self.flush(),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis => self.italic -= 1,
            TagEnd::Strong => self.bold -= 1,
            TagEnd::Strikethrough => self.strike -= 1,
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                if self.text.text.ends_with('[') {
                    self.push("image", self.marks());
                }
                self.push("]", self.marks());
                self.quiet -= 1;
                self.italic -= 1;
            }
            TagEnd::Heading(level) => {
                let text = self.take();
                self.block(Block::Heading(level as usize, text));
            }
            TagEnd::TableCell => {
                let text = self.take();
                if let Some(row) = self.table.as_mut().and_then(|rows| rows.last_mut()) {
                    row.push(text);
                }
            }
            TagEnd::Table => {
                let rows = self.table.take().unwrap_or_default();
                self.block(Block::Table(rows));
            }
            TagEnd::CodeBlock => {
                if let Some((lang, mut body)) = self.code.take() {
                    if body.ends_with('\n') {
                        body.pop();
                    }
                    self.block(Block::Code(lang, body));
                }
            }
            TagEnd::BlockQuote(_) => {
                let (_, blocks) = self.close();
                self.block(Block::Quote(blocks));
            }
            TagEnd::List(_) => {
                let (_, items) = self.close();
                self.frames.last_mut().expect("root frame").1.extend(items);
            }
            TagEnd::Item | TagEnd::FootnoteDefinition => {
                if let (Frame::Item(marker), blocks) = self.close() {
                    self.block(Block::Item(marker, blocks));
                }
            }
            TagEnd::HtmlBlock => {
                let len = self.text.text.trim_end().len();
                self.text.text.truncate(len);
                self.flush();
            }
            _ => self.flush(),
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some((_, body)) => body.push_str(&text),
                None => self.prose(&text),
            },
            Event::Code(text) => {
                let marks = Marks {
                    code: true,
                    ..self.marks()
                };
                self.push(&text, marks);
            }
            // raw HTML is text: shown, never interpreted
            Event::Html(text) | Event::InlineHtml(text) => self.push(&text, self.marks()),
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.push(&text, self.marks());
            }
            Event::FootnoteReference(label) => {
                let marks = Marks {
                    quiet: true,
                    ..self.marks()
                };
                self.push(&format!("[{label}]"), marks);
            }
            Event::SoftBreak => self.push(" ", self.marks()),
            Event::HardBreak => self.push("\n", Marks::default()),
            Event::Rule => {
                self.flush();
                self.block(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some((Frame::Item(marker), _)) = self.frames.last_mut() {
                    *marker = if done { "☑" } else { "☐" }.to_owned();
                }
            }
        }
    }
}

pub(crate) fn parse(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let mut builder = Builder {
        frames: vec![(Frame::Root, Vec::new())],
        ..Builder::default()
    };
    for event in Parser::new_ext(text, options) {
        builder.event(event);
    }
    builder.flush();
    builder
        .frames
        .pop()
        .map(|(_, blocks)| blocks)
        .unwrap_or_default()
}

/// Where a link goes: the web (or a `duck://` link) through the host, or a
/// file of this repository by its full path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Web(String),
    Path(Vec<u8>),
}

/// Resolves a link's destination against `dir`, the directory of the file
/// it sits in (the root for a change's body). Each segment is read through
/// ducklink's `%XX` decoding, so `My%20File.md` names `My File.md`. An
/// anchor alone, another scheme or a path above the root goes nowhere.
pub(crate) fn target(dir: &[u8], dest: &str) -> Option<Target> {
    if ["duck://", "http://", "https://"]
        .iter()
        .any(|scheme| dest.starts_with(scheme))
    {
        return Some(Target::Web(dest.to_owned()));
    }
    let path = dest.split(['#', '?']).next().unwrap_or("");
    if path.is_empty()
        || path
            .split('/')
            .next()
            .is_some_and(|head| head.contains(':'))
    {
        return None;
    }
    let mut parts: Vec<Vec<u8>> = if path.starts_with('/') {
        Vec::new()
    } else {
        dir.split(|b| *b == b'/')
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    };
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(decoded(name)),
        }
    }
    (!parts.is_empty()).then(|| Target::Path(parts.join(&b'/')))
}

/// One path segment with its `%XX` escapes read. A segment ducklink will
/// not read as one name (a literal `(`, lowercase hex, an escaped `/` or
/// `..`) stays as written, so it can never climb or split a path.
fn decoded(segment: &str) -> Vec<u8> {
    match ducklink::tail(segment).as_deref() {
        Ok([name]) => name.as_bytes().to_vec(),
        _ => segment.as_bytes().to_vec(),
    }
}

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
mod tests {
    use super::*;

    fn plain(text: &str) -> Text {
        Text {
            text: text.into(),
            runs: Vec::new(),
        }
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(plain(text))
    }

    #[test]
    fn the_block_constructs_parse_to_their_blocks() {
        let doc = "# Title\n\nOne line\ncarried on.\n\n- a\n  - nested\n- [ ] todo\n- [x] done\n\n\
                   3. third\n4. fourth\n\n> quoted\n> still\n>\n> > deeper\n\n---\n\n\
                   ```rust title\nfn main() {}\n```\n\n    indented\n\n\
                   | a | b |\n|---|:-:|\n| 1 | 2 |\n\n## Next ##";
        assert_eq!(
            parse(doc),
            vec![
                Block::Heading(1, plain("Title")),
                para("One line carried on."),
                Block::Item(
                    "•".into(),
                    vec![para("a"), Block::Item("•".into(), vec![para("nested")])]
                ),
                Block::Item("☐".into(), vec![para("todo")]),
                Block::Item("☑".into(), vec![para("done")]),
                Block::Item("3.".into(), vec![para("third")]),
                Block::Item("4.".into(), vec![para("fourth")]),
                Block::Quote(vec![
                    para("quoted still"),
                    Block::Quote(vec![para("deeper")])
                ]),
                Block::Rule,
                Block::Code("rust".into(), "fn main() {}".into()),
                Block::Code(String::new(), "indented".into()),
                Block::Table(vec![
                    vec![plain("a"), plain("b")],
                    vec![plain("1"), plain("2")],
                ]),
                Block::Heading(2, plain("Next")),
            ]
        );
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end_and_a_hash_without_space_is_text() {
        assert_eq!(
            parse("#tag\n```\nx"),
            vec![para("#tag"), Block::Code(String::new(), "x".into())]
        );
    }

    fn only(doc: &str) -> Text {
        match parse(doc).as_slice() {
            [Block::Paragraph(text)] => text.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn inline_marks_are_runs() {
        let text = only(
            "run `make dev` **now**, *or* ~~never~~, see [the docs](duck://net/forge/rfcs) \
             or https://x.example.",
        );
        assert_eq!(
            text.text,
            "run make dev now, or never, see the docs or https://x.example."
        );
        let at = |needle: &str| {
            let start = text.text.find(needle).unwrap();
            start..start + needle.len()
        };
        let marks = |f: fn(&mut Marks)| {
            let mut marks = Marks::default();
            f(&mut marks);
            marks
        };
        assert_eq!(
            text.runs,
            vec![
                (at("make dev"), marks(|m| m.code = true)),
                (at("now"), marks(|m| m.bold = true)),
                (
                    at("or never").start..at("or never").start + 2,
                    marks(|m| m.italic = true)
                ),
                (at("never"), marks(|m| m.strike = true)),
                (
                    at("the docs"),
                    Marks {
                        link: Some("duck://net/forge/rfcs".into()),
                        ..Marks::default()
                    }
                ),
                (
                    at("https://x.example"),
                    Marks {
                        link: Some("https://x.example".into()),
                        ..Marks::default()
                    }
                ),
            ]
        );
    }

    #[test]
    fn an_image_is_its_quiet_alt_text_and_a_footnote_its_label() {
        let blocks = parse("See ![the logo](logo.png) and ![](x.png)[^1].\n\n[^1]: A note.");
        let [Block::Paragraph(text), Block::Item(marker, note)] = blocks.as_slice() else {
            panic!("{blocks:?}")
        };
        assert_eq!(text.text, "See [the logo] and [image][1].");
        assert!(text.runs.iter().all(|(_, m)| m.quiet && m.link.is_none()));
        assert_eq!(marker, "[1]");
        assert_eq!(note, &vec![para("A note.")]);
    }

    #[test]
    fn raw_html_is_shown_as_text_never_as_markup() {
        let blocks = parse("<div align=\"center\"><b>hi</b></div>\n\nan <i>inline</i> tag");
        assert_eq!(
            blocks,
            vec![
                para("<div align=\"center\"><b>hi</b></div>"),
                para("an <i>inline</i> tag"),
            ]
        );
    }

    #[test]
    fn a_relative_link_resolves_against_the_file_directory() {
        let path = |p: &str| Some(Target::Path(p.as_bytes().to_vec()));
        assert_eq!(
            target(b"docs/guide", "../README.md"),
            path("docs/README.md")
        );
        assert_eq!(target(b"docs", "./a/b.md#part"), path("docs/a/b.md"));
        assert_eq!(target(b"docs", "/src/lib.rs"), path("src/lib.rs"));
        assert_eq!(target(b"", "GUIDE.md?plain=1"), path("GUIDE.md"));
        assert_eq!(target(b"docs", "../../x.md"), None, "above the root");
        assert_eq!(target(b"docs", "#anchor"), None);
        assert_eq!(target(b"", "mailto:a@b.c"), None);
        assert_eq!(target(b"docs", "My%20File.md"), path("docs/My File.md"));
        assert_eq!(target(b"", "a%20b/%EB%B3%B4.md#x"), path("a b/\u{bcf4}.md"));
        // not one decodable name: kept as written, never a traversal
        assert_eq!(target(b"docs", "%2E%2E/x.md"), path("docs/%2E%2E/x.md"));
        assert_eq!(target(b"", "a(1).md"), path("a(1).md"));
        assert_eq!(
            target(b"docs", "https://x.example/a"),
            Some(Target::Web("https://x.example/a".into()))
        );
        assert_eq!(
            target(b"", "duck://net/forge/x"),
            Some(Target::Web("duck://net/forge/x".into()))
        );
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
            let on_link: super::OnLink = std::rc::Rc::new(|dest, _, cx| {
                if let Some(super::Target::Web(url)) = super::target(b"", dest) {
                    cx.host().open_link(&url);
                }
            });
            super::render(
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
