use super::*;

pub(super) fn plain_line(id: ElementId, text: &str, mono: bool) -> InteractiveText {
    let styled = StyledText::new(text.to_owned());
    let mut text = InteractiveText::new(id, styled).w_full();
    if mono {
        text = text.font_family("JetBrains Mono").text_size(px(12.));
    }
    text
}

pub(super) fn rich_line(
    id: ElementId,
    block: &ChatBlock,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> InteractiveText {
    if block.spans.is_empty() {
        return plain_line(id, &block.text, false);
    }
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut clickable = Vec::new();
    let mut targets = Vec::new();
    for span in &block.spans {
        let start = text.len();
        text.push_str(&span.text);
        let range = start..text.len();
        let mut style = HighlightStyle::default();
        match &span.style {
            SpanStyle::Plain => {}
            SpanStyle::Bold => style.font_weight = Some(FontWeight::BOLD),
            SpanStyle::Italic => style.font_style = Some(FontStyle::Italic),
            SpanStyle::BoldItalic => {
                style.font_weight = Some(FontWeight::BOLD);
                style.font_style = Some(FontStyle::Italic);
            }
            SpanStyle::Link(target) => {
                style.color = Some(theme.link);
                style.font_weight = Some(FontWeight::MEDIUM);
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.link),
                    wavy: false,
                });
                if !target.is_empty() {
                    clickable.push(range.clone());
                    targets.push(target.clone());
                }
            }
            SpanStyle::Mention(account) => {
                style.color = Some(theme.link);
                style.font_weight = Some(FontWeight::MEDIUM);
                if !account.is_empty() {
                    clickable.push(range.clone());
                    targets.push(account.clone());
                }
            }
        }
        highlights.push((range, style));
    }
    let styled = StyledText::new(text).with_highlights(highlights);
    let open = cx.processor(move |chat, index: usize, _window, cx| {
        if let Some(target) = targets.get(index) {
            cx.notify();
            chat.open_link(target.clone(), cx);
        }
    });
    InteractiveText::new(id, styled)
        .w_full()
        .on_click(clickable, open)
}
