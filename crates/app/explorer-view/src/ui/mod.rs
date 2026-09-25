//! The explorer's pages. Square corners, rows split by one-pixel rules,
//! hashes and numbers in the data face.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{Div, FontWeight, Stateful};

use crate::decode::{ago, clip, date, grouped, plural, short};
use crate::{Account, BlockRow, Explorer, Route, TxRow};
use design::{empty_state, mono};

/// The most rows one list draws; the rest is reached by search.
const LIST_ROWS: usize = 50;
/// The rows each Overview panel draws.
const LATEST: usize = 12;

type Cx<'a, 'b> = &'a mut Context<'b, Explorer>;

mod accounts;
mod blocks;
mod overview;
mod programs;
mod transactions;
use accounts::{account, accounts};
use blocks::{block, blocks};
use overview::overview;
use programs::programs;
use transactions::{transactions, tx};

pub fn render(view: &Explorer, cx: Cx) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let mut root = div()
        .id("explorer")
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(bar(view, cx, &theme));
    if let Some(note) = &view.note {
        root = root.child(
            div()
                .id("explorer-note")
                .px_5()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(note.clone()),
        );
    }
    root.child(
        div()
            .id("explorer-page")
            .flex_1()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(page(view, cx, &theme)),
    )
    .into_any_element()
}

fn bar(view: &Explorer, cx: Cx, theme: &Theme) -> impl IntoElement {
    let tabs = [
        ("Overview", Route::Overview),
        ("Blocks", Route::Blocks),
        ("Transactions", Route::Transactions(None)),
        ("Accounts", Route::Accounts),
        ("Programs", Route::Programs),
    ];
    let typed = cx.listener(|view: &mut Explorer, text: &String, _, cx| {
        view.search = text.clone();
        cx.notify();
    });
    let submit = cx.listener(|view: &mut Explorer, _: &(), _, cx| view.search(cx));
    let tabs = tabs.into_iter().map(|(label, route)| {
        let active = view.route.tab() == route.tab();
        let go = cx
            .listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
        design::tab(
            format!("explorer-tab-{}", label.to_lowercase()),
            label,
            active,
            theme,
            go,
        )
        .h_full()
        .mx_1()
    });
    div()
        .id("explorer-bar")
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .h(px(44.))
        .px_3()
        .border_b_1()
        .border_color(theme.border)
        .child(div().h_full().flex().items_center().children(tabs))
        .child(
            div().w(px(360.)).flex_shrink_0().child(
                Input::new("explorer-search")
                    .w_full()
                    .h(px(28.))
                    .px_2()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .text_size(design::text::SECONDARY)
                    .value(view.search.clone())
                    .placeholder("Search by height, hash, account or program")
                    .label("Search the chain")
                    .on_input(typed)
                    .on_submit(submit),
            ),
        )
}

fn page(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    if view.chain.blocks.is_empty() && !matches!(view.route, Route::Programs | Route::Accounts) {
        return match &view.chain.failed {
            Some(sentence) => failed(sentence, cx, theme),
            None => quiet("explorer-loading", "Reading the chain…", theme),
        };
    }
    match view.route.clone() {
        Route::Overview => overview(view, cx, theme),
        Route::Blocks => blocks(view, cx, theme),
        Route::Block(height) => block(view, height, cx, theme),
        Route::Transactions(program) => transactions(view, program, cx, theme),
        Route::Tx(hash) => tx(view, &hash, cx, theme),
        Route::Accounts => accounts(view, cx, theme),
        Route::Account(number) => account(view, number, cx, theme),
        Route::Programs => programs(view, cx, theme),
    }
}

// ---------- pieces ----------

fn quiet(id: &'static str, text: &'static str, theme: &Theme) -> AnyElement {
    design::quiet(text, theme)
        .id(id)
        .px_5()
        .py_4()
        .into_any_element()
}

fn failed(sentence: &str, cx: Cx, theme: &Theme) -> AnyElement {
    let retry = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.read_all(cx));
    design::refused("explorer", sentence.to_owned(), theme, retry)
        .m_5()
        .into_any_element()
}

/// A section's title row: its name, then what sits on its right.
fn heading(id: &str, title: &str, right: Option<AnyElement>, theme: &Theme) -> impl IntoElement {
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .h(px(44.))
        .px_5()
        .border_b_1()
        .border_color(theme.border)
        .child(design::heading(format!("{id}-title"), title.to_owned(), 2, theme).flex_1())
        .children(right)
}

fn caption(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    mono(text)
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .into_any_element()
}

/// A link that goes somewhere inside the explorer.
fn link(id: String, text: String, route: Route, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    div()
        .id(SharedString::from(id))
        .text_color(theme.link)
        .hover(|s| s.underline())
        .role(Role::Link)
        .focusable()
        .on_click(go)
        .child(text)
}

/// A clickable row of a list.
fn row(id: ElementId, label: String, route: Route, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_4()
        .h(px(40.))
        .px_5()
        .border_b_1()
        .border_color(theme.border)
        .hover(|s| s.bg(theme.hover))
        .role(Role::Button)
        .aria_label(label)
        .focusable()
        .on_click(go)
}

/// Who signed: the account holding the key, or the key itself.
fn signer(view: &Explorer, key: &[u8], theme: &Theme) -> impl IntoElement {
    let (name, number) = match view.holder(key) {
        Some((account, _)) => (account.name.clone(), Some(account.number)),
        None => (short(key), None),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .w(px(180.))
        .flex_shrink_0()
        .child(design::avatar(&name, px(20.), theme))
        .child(div().truncate().child(name))
        .children(number.map(|number| {
            mono(format!("#{number}"))
                .text_size(design::text::CAPTION)
                .text_color(theme.faint)
        }))
}

fn block_row(block: &BlockRow, now: u64, cx: Cx, theme: &Theme) -> impl IntoElement {
    let id = SharedString::from(format!("explorer-block-{}", block.height)).into();
    row(
        id,
        format!("Block {}", block.height),
        Route::Block(block.height),
        cx,
        theme,
    )
    .when(block.txs == 0, |row| row.text_color(theme.faint))
    .child(
        mono(grouped(block.height))
            .w(px(72.))
            .when(block.txs > 0, |height| {
                height.font_weight(FontWeight::SEMIBOLD)
            }),
    )
    .child(mono(short(&block.id)).flex_1().text_color(theme.muted))
    .child(
        div()
            .text_color(theme.muted)
            .child(format!("{} tx", block.txs)),
    )
    .child(
        mono(ago(now, block.time))
            .w(px(36.))
            .flex()
            .justify_end()
            .text_color(theme.faint),
    )
}

/// One line of a block list: a block, or a run of empty ones.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    Block(&'a BlockRow),
    /// consecutive blocks with no transaction, `oldest..=newest`
    Empty {
        newest: u64,
        oldest: u64,
    },
}

/// `blocks` (newest first) as at most `rows` lines, each run of two or more
/// empty blocks folded into one, so the list reaches back past quiet
/// stretches as far as the window holds.
pub(crate) fn lines(blocks: &[BlockRow], rows: usize) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut at = 0;
    while at < blocks.len() && lines.len() < rows {
        let run = blocks[at..]
            .iter()
            .take_while(|block| block.txs == 0)
            .count();
        if run >= 2 {
            let (newest, oldest) = (blocks[at].height, blocks[at + run - 1].height);
            lines.push(Line::Empty { newest, oldest });
            at += run;
        } else {
            lines.push(Line::Block(&blocks[at]));
            at += 1;
        }
    }
    lines
}

fn block_lines(
    blocks: &[BlockRow],
    rows: usize,
    now: u64,
    cx: Cx,
    theme: &Theme,
) -> Vec<AnyElement> {
    lines(blocks, rows)
        .into_iter()
        .map(|line| match line {
            Line::Block(block) => block_row(block, now, cx, theme).into_any_element(),
            Line::Empty { newest, oldest } => div()
                .id(SharedString::from(format!("explorer-empty-{newest}")))
                .flex()
                .items_center()
                .h(px(40.))
                .px_5()
                .border_b_1()
                .border_color(theme.border)
                .text_color(theme.faint)
                .child(mono(format!(
                    "{}–{} · {} empty blocks",
                    grouped(oldest),
                    grouped(newest),
                    grouped(newest - oldest + 1)
                )))
                .into_any_element(),
        })
        .collect()
}

/// `height` adds the block column; `who` the signer column, which an
/// account's own activity leaves out.
fn tx_row(
    view: &Explorer,
    tx: &TxRow,
    height: bool,
    who: bool,
    cx: Cx,
    theme: &Theme,
) -> impl IntoElement {
    let id = SharedString::from(format!("explorer-tx-{}", abi::hex(&tx.hash)));
    let now = view.chain.now();
    view.describe(tx, cx);
    // empty until the host answers: the program column already says whose
    let title = tx.op().map(|op| clip(&op.title)).unwrap_or_default();
    row(
        ElementId::Name(id),
        title.clone(),
        Route::Tx(tx.hash),
        cx,
        theme,
    )
    .child(mono(short(&tx.hash)).w(px(100.)).text_color(theme.muted))
    .child(
        div()
            .flex_1()
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .child(
                mono(tx.target.clone())
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted),
            )
            .child(div().truncate().child(title)),
    )
    .children(who.then(|| signer(view, &tx.signer, theme)))
    .children(height.then(|| mono(grouped(tx.height)).text_color(theme.muted)))
    .child(
        mono(ago(now, tx.time))
            .w(px(36.))
            .flex()
            .justify_end()
            .text_color(theme.faint),
    )
}

/// A label and its value, one row of a detail page.
fn field(label: &str, value: impl IntoElement, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .min_h(px(40.))
        .px_5()
        .gap_4()
        .border_b_1()
        .border_color(theme.border)
        .child(
            mono(label.to_string())
                .w(px(110.))
                .flex_shrink_0()
                .text_color(theme.muted),
        )
        .child(value)
}

fn titled(kind: &str, title: String, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px_5()
        .py_4()
        .child(
            mono(kind.to_string())
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        )
        .child(
            div()
                .id("explorer-title")
                .text_size(px(22.))
                .role(Role::Heading)
                .aria_level(1)
                .child(title),
        )
}

fn quiet_owned(id: &'static str, text: String, theme: &Theme) -> AnyElement {
    div()
        .id(id)
        .px_5()
        .py_4()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(text)
        .into_any_element()
}

/// "Copy link": this page's `duck://<chain>/explorer/…` link, onto the
/// clipboard. Absent while the session names no chain.
fn copy_button(view: &Explorer, route: &Route, cx: Cx, theme: &Theme) -> Option<AnyElement> {
    let link = view.link(route)?;
    let copy = cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
        view.copy_link(link.clone(), cx)
    });
    Some(
        div()
            .id("explorer-copy-link")
            .px_3()
            .h(px(28.))
            .flex()
            .items_center()
            .border_1()
            .border_color(theme.border)
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .hover(|s| s.bg(theme.hover).text_color(theme.foreground))
            .role(Role::Button)
            .focusable()
            .on_click(copy)
            .child("Copy link")
            .into_any_element(),
    )
}
