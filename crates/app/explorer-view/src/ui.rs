//! The explorer's pages. Square corners, rows split by one-pixel rules,
//! hashes and numbers in the data face.
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{Div, FontWeight, Stateful};

use crate::components::EmptyState;
use crate::decode::{ago, date, grouped, plural, short};
use crate::{Account, BlockRow, Explorer, Route, TxRow};

const MONO: &str = "JetBrains Mono";
/// The most rows one list draws; the rest is reached by search.
const LIST_ROWS: usize = 50;
/// The rows each Overview panel draws.
const LATEST: usize = 12;

type Cx<'a, 'b> = &'a mut Context<'b, Explorer>;

pub fn render(view: &Explorer, cx: Cx) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let mut root = div()
        .id("explorer")
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(px(13.))
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
                .text_size(px(12.))
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
        div()
            .id(format!("explorer-tab-{}", label.to_lowercase()))
            .h_full()
            .flex()
            .items_center()
            .px_2()
            .mx_1()
            .text_color(if active {
                theme.foreground
            } else {
                theme.muted
            })
            .when(active, |tab| {
                tab.border_b_2()
                    .border_color(theme.foreground)
                    .font_weight(FontWeight::MEDIUM)
            })
            .hover(|tab| tab.text_color(theme.foreground))
            .role(Role::Tab)
            .aria_selected(active)
            .focusable()
            .on_click(go)
            .child(label)
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
                    .text_size(px(12.))
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

fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(MONO)
        .text_size(px(12.))
        .whitespace_nowrap()
        .child(text.into())
}

fn quiet(id: &'static str, text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .id(id)
        .px_5()
        .py_4()
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(text)
        .into_any_element()
}

fn failed(sentence: &str, cx: Cx, theme: &Theme) -> AnyElement {
    let retry = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.read_all(cx));
    div()
        .id("explorer-refused")
        .m_5()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(theme.danger)
        .bg(theme.danger_soft)
        .child(sentence.to_string())
        .child(
            div()
                .id("explorer-retry")
                .px_2()
                .py_1()
                .bg(theme.surface)
                .hover(|s| s.bg(theme.surface_raised))
                .role(Role::Button)
                .focusable()
                .on_click(retry)
                .child("Retry"),
        )
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
        .child(
            div()
                .id(SharedString::from(format!("{id}-title")))
                .flex_1()
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(2)
                .child(title.to_string()),
        )
        .children(right)
}

fn caption(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    mono(text)
        .text_size(px(11.))
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

fn avatar(name: &str, size: f32, theme: &Theme) -> impl IntoElement {
    let initial: String = name
        .chars()
        .next()
        .into_iter()
        .flat_map(char::to_uppercase)
        .collect();
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.surface_raised)
        .text_color(theme.muted)
        .text_size(px(size * 0.45))
        .child(initial)
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
        .child(avatar(&name, 20., theme))
        .child(div().truncate().child(name))
        .children(number.map(|number| {
            mono(format!("#{number}"))
                .text_size(px(11.))
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
    row(
        ElementId::Name(id),
        tx.op.title.clone(),
        Route::Tx(tx.hash),
        cx,
        theme,
    )
    .child(mono(short(&tx.hash)).w(px(84.)).text_color(theme.muted))
    .child(
        div()
            .flex_1()
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .child(
                mono(tx.target.clone())
                    .text_size(px(11.))
                    .text_color(theme.muted),
            )
            .child(div().truncate().child(tx.op.title.clone())),
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
                .text_size(px(11.))
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

// ---------- pages ----------

fn overview(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let status = view.status.ready();
    let stat = |id: &'static str, label: &'static str, value: String, note: String| {
        div()
            .id(id)
            .flex_1()
            .flex()
            .flex_col()
            .gap_1()
            .px_5()
            .py_4()
            .border_r_1()
            .border_color(theme.border)
            .child(mono(label).text_size(px(11.)).text_color(theme.muted))
            .child(div().text_size(px(22.)).child(value))
            .child(div().text_size(px(12.)).text_color(theme.muted).child(note))
    };
    let dash = || "—".to_string();
    let (height, cadence) = match status {
        Some(status) => (
            grouped(status.height),
            format!("Block every {:.1} s", status.block_time_ms as f64 / 1000.),
        ),
        None => (dash(), String::new()),
    };
    let (epoch, next) = match status {
        Some(status) if status.epoch_length > 0 => {
            let closes = (status.epoch + 1) * status.epoch_length - 1;
            let left = closes.saturating_sub(status.height);
            (
                grouped(status.epoch),
                format!("Next in {}", plural(left, "block", "blocks")),
            )
        }
        _ => (dash(), String::new()),
    };
    let validators = view
        .validators
        .ready()
        .map_or_else(dash, |keys| grouped(keys.len() as u64));
    let accounts = view
        .accounts
        .ready()
        .map_or_else(dash, |accounts| grouped(accounts.len() as u64));
    let now = view.chain.now();
    let all_blocks = link(
        "explorer-all-blocks".into(),
        "All blocks →".into(),
        Route::Blocks,
        cx,
        theme,
    )
    .text_size(px(12.))
    .into_any_element();
    let all_txs = link(
        "explorer-all-txs".into(),
        "All transactions →".into(),
        Route::Transactions(None),
        cx,
        theme,
    )
    .text_size(px(12.))
    .into_any_element();
    let blocks = block_lines(&view.chain.blocks, LATEST, now, cx, theme);
    let txs: Vec<_> = view
        .chain
        .txs
        .iter()
        .take(LATEST)
        .map(|tx| tx_row(view, tx, false, true, cx, theme).into_any_element())
        .collect();
    let no_txs = txs.is_empty().then(|| {
        quiet_owned(
            "explorer-no-txs",
            format!(
                "No transactions in the last {}.",
                plural(view.chain.blocks.len() as u64, "block", "blocks")
            ),
            theme,
        )
    });
    div()
        .id("explorer-overview")
        .flex()
        .flex_col()
        .flex_1()
        .child(
            div()
                .id("explorer-stats")
                .flex()
                .border_b_1()
                .border_color(theme.border)
                .child(stat("explorer-stat-height", "Height", height, cadence))
                .child(stat("explorer-stat-epoch", "Epoch", epoch, next))
                .child(stat(
                    "explorer-stat-validators",
                    "Validators",
                    validators,
                    String::new(),
                ))
                .child(stat(
                    "explorer-stat-accounts",
                    "Accounts",
                    accounts,
                    String::new(),
                )),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .child(
                    div()
                        .id("explorer-latest-blocks")
                        .w(px(420.))
                        .flex_shrink_0()
                        .border_r_1()
                        .border_color(theme.border)
                        .child(heading(
                            "explorer-latest-blocks-heading",
                            "Latest blocks",
                            Some(all_blocks),
                            theme,
                        ))
                        .children(blocks),
                )
                .child(
                    div()
                        .id("explorer-latest-txs")
                        .flex_1()
                        .child(heading(
                            "explorer-latest-txs-heading",
                            "Latest transactions",
                            Some(all_txs),
                            theme,
                        ))
                        .children(txs)
                        .children(no_txs),
                ),
        )
        .into_any_element()
}

fn quiet_owned(id: &'static str, text: String, theme: &Theme) -> AnyElement {
    div()
        .id(id)
        .px_5()
        .py_4()
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(text)
        .into_any_element()
}

fn blocks(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let now = view.chain.now();
    let held = view.chain.blocks.len() as u64;
    let rows = block_lines(&view.chain.blocks, LIST_ROWS, now, cx, theme);
    div()
        .id("explorer-blocks")
        .child(heading(
            "explorer-blocks-heading",
            "Blocks",
            Some(caption(
                format!("the last {}", plural(held, "block", "blocks")),
                theme,
            )),
            theme,
        ))
        .children(rows)
        .into_any_element()
}

fn transactions(view: &Explorer, program: Option<String>, cx: Cx, theme: &Theme) -> AnyElement {
    let window = plural(view.chain.blocks.len() as u64, "block", "blocks");
    let matching: Vec<&TxRow> = view
        .chain
        .txs
        .iter()
        .filter(|tx| program.as_ref().is_none_or(|program| &tx.target == program))
        .collect();
    let rows: Vec<_> = matching
        .iter()
        .take(LIST_ROWS)
        .map(|tx| tx_row(view, tx, true, true, cx, theme).into_any_element())
        .collect();
    let title = match &program {
        Some(program) => format!("Transactions · {program}"),
        None => "Transactions".into(),
    };
    let empty = rows.is_empty().then(|| {
        quiet_owned(
            "explorer-no-txs",
            format!("No transactions in the last {window}."),
            theme,
        )
    });
    div()
        .id("explorer-transactions")
        .child(heading(
            "explorer-transactions-heading",
            &title,
            Some(caption(
                format!(
                    "{} in the last {window}",
                    plural(matching.len() as u64, "transaction", "transactions")
                ),
                theme,
            )),
            theme,
        ))
        .children(rows)
        .children(empty)
        .into_any_element()
}

fn block(view: &Explorer, height: u64, cx: Cx, theme: &Theme) -> AnyElement {
    let Some((block, txs)) = view.block(height) else {
        return match &view.opened {
            Loaded::Ready(None) => EmptyState::new(
                "explorer-no-block",
                format!("No block {}", grouped(height)),
                "This node keeps no finalized block at this height.",
            )
            .into_any_element(),
            Loaded::Failed(refusal) => failed(&refusal.sentence, cx, theme),
            _ => quiet("explorer-block-loading", "Reading the block…", theme),
        };
    };
    let head = view.status.ready().map_or(0, |status| status.height);
    let step = |id: &'static str, text: String, to: Option<u64>, cx: Cx| {
        let enabled = to.is_some();
        let button = div()
            .id(id)
            .px_3()
            .h(px(28.))
            .flex()
            .items_center()
            .border_1()
            .border_color(theme.border)
            .text_size(px(12.))
            .text_color(if enabled {
                theme.foreground
            } else {
                theme.faint
            })
            .role(Role::Button)
            .child(text);
        match to {
            Some(to) => {
                let go = cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
                    view.go(Route::Block(to), cx)
                });
                button.focusable().hover(|s| s.bg(theme.hover)).on_click(go)
            }
            None => button.aria_disabled(true),
        }
    };
    let previous = height.checked_sub(1);
    let next = (height < head).then_some(height + 1);
    let now = view.chain.now();
    let proposer = block.proposer.as_ref().map(|key| {
        let place = view
            .validators
            .ready()
            .and_then(|keys| keys.iter().position(|seated| seated == key));
        let name = match place {
            Some(place) => format!("validator {}", place + 1),
            None => "validator".into(),
        };
        field(
            "Proposer",
            div()
                .flex()
                .gap_2()
                .child(mono(name))
                .child(mono(format!("ed25519 {}", short(key))).text_color(theme.faint)),
            theme,
        )
    });
    let parent = match previous {
        Some(previous) => div()
            .flex()
            .gap_2()
            .child(
                link(
                    "explorer-parent".into(),
                    grouped(previous),
                    Route::Block(previous),
                    cx,
                    theme,
                )
                .font_family(MONO)
                .text_size(px(12.)),
            )
            .child(mono(short(&block.parent)).text_color(theme.muted)),
        None => div().child(mono(short(&block.parent)).text_color(theme.muted)),
    };
    let count = txs.len() as u64;
    let rows: Vec<_> = txs
        .into_iter()
        .map(|tx| tx_row(view, tx, false, true, cx, theme).into_any_element())
        .collect();
    let empty = rows.is_empty().then(|| {
        quiet(
            "explorer-block-empty",
            "No transactions in this block.",
            theme,
        )
    });
    div()
        .id("explorer-block")
        .child(
            div()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .child(titled("Block", grouped(height), theme)),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .px_5()
                        .child(step(
                            "explorer-previous",
                            format!("← {}", previous.map_or(String::new(), grouped)),
                            previous,
                            cx,
                        ))
                        .child(step(
                            "explorer-next",
                            format!("{} →", grouped(height + 1)),
                            next,
                            cx,
                        )),
                ),
        )
        .child(field("Hash", mono(abi::hex(&block.id)), theme))
        .child(field("Parent", parent, theme))
        .child(field(
            "Time",
            div()
                .flex()
                .gap_2()
                .child(date(block.time))
                .child(mono(format!("{} ago", ago(now, block.time))).text_color(theme.faint)),
            theme,
        ))
        .children(proposer)
        .child(field("Epoch", mono(grouped(block.epoch)), theme))
        .child(heading(
            "explorer-block-txs-heading",
            "Transactions",
            Some(caption(grouped(count), theme)),
            theme,
        ))
        .children(rows)
        .children(empty)
        .into_any_element()
}

fn tx(view: &Explorer, hash: &[u8; 32], cx: Cx, theme: &Theme) -> AnyElement {
    let Some(tx) = view.tx(hash) else {
        return EmptyState::new(
            "explorer-no-tx",
            "Transaction not found",
            format!(
                "It is not in the last {} this explorer reads.",
                plural(view.chain.blocks.len() as u64, "block", "blocks")
            ),
        )
        .into_any_element();
    };
    let now = view.chain.now();
    let from = match view.holder(&tx.signer) {
        Some((account, device)) => {
            let key = &account.devices[device];
            let label = key
                .label
                .clone()
                .unwrap_or_else(|| format!("device {}", device + 1));
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(avatar(&account.name, 20., theme))
                .child(link(
                    "explorer-from".into(),
                    account.name.clone(),
                    Route::Account(account.number),
                    cx,
                    theme,
                ))
                .child(
                    mono(format!(
                        "#{} {label} · {} {}",
                        account.number,
                        key.scheme,
                        short(&tx.signer)
                    ))
                    .text_color(theme.faint),
                )
        }
        None => div().child(mono(abi::hex(&tx.signer))),
    };
    let code = view.network.ready().and_then(|network| {
        network
            .programs
            .iter()
            .find(|entry| entry.program == tx.target)
            .map(|entry| entry.code.clone())
    });
    let program = div()
        .flex()
        .gap_2()
        .child(mono(tx.target.clone()))
        .children(code.map(|code| {
            let tail = &code[code.len().saturating_sub(4)..];
            mono(format!("code {}…{tail}", &code[..4.min(code.len())])).text_color(theme.faint)
        }));
    let (variant, path) = match tx.op.kind.split_once("::") {
        Some((_, variant)) => (variant.to_string(), tx.op.kind.clone()),
        None => (tx.op.title.clone(), String::new()),
    };
    let operation = div()
        .id("explorer-operation")
        .mx_5()
        .mb_4()
        .p_4()
        .flex()
        .flex_col()
        .gap_1()
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .mb_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(variant))
                .child(mono(path).text_size(px(11.)).text_color(theme.faint)),
        )
        .children(tx.op.fields.iter().map(|(name, value)| {
            div()
                .flex()
                .gap_4()
                .child(
                    mono(name.clone())
                        .w(px(90.))
                        .flex_shrink_0()
                        .text_color(theme.muted),
                )
                .child(div().flex_1().child(value.clone()))
        }));
    div()
        .id("explorer-tx")
        .child(div().border_b_1().border_color(theme.border).child(titled(
            "Transaction",
            tx.op.title.clone(),
            theme,
        )))
        .child(field(
            "Block",
            link(
                "explorer-tx-block".into(),
                format!("In block {}", grouped(tx.height)),
                Route::Block(tx.height),
                cx,
                theme,
            ),
            theme,
        ))
        .child(field("Hash", mono(abi::hex(&tx.hash)), theme))
        .child(field("From", from, theme))
        .child(field("Program", program, theme))
        .child(field("Sequence", mono(grouped(tx.seq)), theme))
        .child(field(
            "Time",
            div()
                .flex()
                .gap_2()
                .child(date(tx.time))
                .child(mono(format!("{} ago", ago(now, tx.time))).text_color(theme.faint)),
            theme,
        ))
        .child(div().h(px(12.)))
        .child(
            div()
                .id("explorer-operation-heading")
                .px_5()
                .py_2()
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(2)
                .child("Operation"),
        )
        .child(operation)
        .into_any_element()
}

fn accounts(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let list = match &view.accounts {
        Loaded::Ready(list) => list,
        Loaded::Failed(refusal) => return failed(&refusal.sentence, cx, theme),
        _ => return quiet("explorer-accounts-loading", "Reading accounts…", theme),
    };
    let rows: Vec<_> = list
        .iter()
        .take(LIST_ROWS * 4)
        .map(|account| {
            let sent = activity(view, account).count() as u64;
            row(
                SharedString::from(format!("explorer-account-{}", account.number)).into(),
                account.name.clone(),
                Route::Account(account.number),
                cx,
                theme,
            )
            .child(avatar(&account.name, 20., theme))
            .child(div().flex_1().truncate().child(account.name.clone()))
            .child(mono(format!("#{}", account.number)).text_color(theme.faint))
            .child(div().w(px(90.)).text_color(theme.muted).child(plural(
                account.devices.len() as u64,
                "device",
                "devices",
            )))
            .child(
                mono(plural(sent, "tx", "tx"))
                    .w(px(60.))
                    .flex()
                    .justify_end()
                    .text_color(theme.muted),
            )
            .into_any_element()
        })
        .collect();
    div()
        .id("explorer-accounts")
        .child(heading(
            "explorer-accounts-heading",
            "Accounts",
            Some(caption(
                format!(
                    "{} · tx in the last {}",
                    grouped(list.len() as u64),
                    plural(view.chain.blocks.len() as u64, "block", "blocks")
                ),
                theme,
            )),
            theme,
        ))
        .children(rows)
        .into_any_element()
}

/// What `account`'s devices signed in the window, newest first.
fn activity<'a>(view: &'a Explorer, account: &'a Account) -> impl Iterator<Item = &'a TxRow> {
    view.chain
        .txs
        .iter()
        .filter(|tx| account.devices.iter().any(|device| device.key == tx.signer))
}

fn account(view: &Explorer, number: u64, cx: Cx, theme: &Theme) -> AnyElement {
    let Some(account) = view
        .accounts
        .ready()
        .and_then(|list| list.iter().find(|account| account.number == number))
    else {
        return match &view.accounts {
            Loaded::Ready(_) => EmptyState::new(
                "explorer-no-account",
                format!("No account #{number}"),
                "Identity holds no account by this number.",
            )
            .into_any_element(),
            Loaded::Failed(refusal) => failed(&refusal.sentence, cx, theme),
            _ => quiet("explorer-account-loading", "Reading the account…", theme),
        };
    };
    let window = plural(view.chain.blocks.len() as u64, "block", "blocks");
    let now = view.chain.now();
    let sent: Vec<&TxRow> = activity(view, account).collect();
    let mut used: Vec<(String, u64)> = Vec::new();
    for tx in &sent {
        match used.iter_mut().find(|(program, _)| *program == tx.target) {
            Some((_, count)) => *count += 1,
            None => used.push((tx.target.clone(), 1)),
        }
    }
    used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let rows: Vec<_> = sent
        .iter()
        .take(LIST_ROWS)
        .map(|tx| tx_row(view, tx, true, false, cx, theme).into_any_element())
        .collect();
    let empty = rows.is_empty().then(|| {
        quiet_owned(
            "explorer-no-activity",
            format!("Nothing signed in the last {window}."),
            theme,
        )
    });
    let devices = account.devices.iter().enumerate().map(|(index, device)| {
        let last = sent.iter().find(|tx| tx.signer == device.key);
        let label = device
            .label
            .clone()
            .unwrap_or_else(|| format!("Device {}", index + 1));
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(40.))
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(div().child(label))
            .child(
                mono(format!("{} {}", device.scheme, short(&device.key)))
                    .flex_1()
                    .text_size(px(11.))
                    .text_color(theme.faint),
            )
            .child(
                mono(match last {
                    Some(tx) => format!("last used {} ago", ago(now, tx.time)),
                    None => "not used lately".into(),
                })
                .text_size(px(11.))
                .text_color(theme.muted),
            )
    });
    let programs = used.into_iter().map(|(program, count)| {
        div()
            .flex()
            .items_center()
            .h(px(40.))
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(mono(program).flex_1())
            .child(mono(format!("{count} tx")).text_color(theme.muted))
    });
    div()
        .id("explorer-account")
        .flex()
        .flex_col()
        .flex_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .px_5()
                .py_4()
                .child(avatar(&account.name, 40., theme))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .id("explorer-title")
                                .text_size(px(22.))
                                .role(Role::Heading)
                                .aria_level(1)
                                .child(account.name.clone()),
                        )
                        .child(
                            mono(format!(
                                "account {}   {}",
                                account.number,
                                plural(account.devices.len() as u64, "device", "devices")
                            ))
                            .text_size(px(11.))
                            .text_color(theme.muted),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div()
                        .id("explorer-activity")
                        .flex_1()
                        .border_r_1()
                        .border_color(theme.border)
                        .child(heading(
                            "explorer-activity-heading",
                            "Activity",
                            Some(caption(
                                format!(
                                    "{} in the last {window}",
                                    plural(sent.len() as u64, "transaction", "transactions")
                                ),
                                theme,
                            )),
                            theme,
                        ))
                        .children(rows)
                        .children(empty),
                )
                .child(
                    div()
                        .id("explorer-account-side")
                        .w(px(380.))
                        .flex_shrink_0()
                        .child(heading(
                            "explorer-devices-heading",
                            "Devices",
                            Some(caption(grouped(account.devices.len() as u64), theme)),
                            theme,
                        ))
                        .children(devices)
                        .child(heading(
                            "explorer-used-heading",
                            "Programs used",
                            None,
                            theme,
                        ))
                        .children(programs),
                ),
        )
        .into_any_element()
}

fn programs(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let network = match &view.network {
        Loaded::Ready(network) => network,
        Loaded::Failed(refusal) => return failed(&refusal.sentence, cx, theme),
        _ => return quiet("explorer-programs-loading", "Reading the registry…", theme),
    };
    if network.programs.is_empty() && network.changes.is_empty() {
        return EmptyState::new(
            "explorer-empty",
            "No programs",
            "The registry of this network runs nothing yet.",
        )
        .into_any_element();
    }
    let running = network.programs.iter().map(|entry| {
        let go = Route::Transactions(Some(entry.program.clone()));
        row(
            SharedString::from(format!("explorer-program-{}", entry.program)).into(),
            entry.program.clone(),
            go,
            cx,
            theme,
        )
        .child(mono(entry.program.clone()).flex_1())
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child(plural(entry.params as u64, "param byte", "param bytes")),
        )
        .child(mono(short_code(&entry.code)).text_color(theme.muted))
        .into_any_element()
    });
    let running: Vec<_> = running.collect();
    let scheduled: Vec<_> = network
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| {
            div()
                .id(ElementId::named_usize("explorer-change", index))
                .flex()
                .items_center()
                .gap_4()
                .h(px(40.))
                .px_5()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .px_1()
                        .text_size(px(11.))
                        .text_color(if change.verb.starts_with("Remove") {
                            theme.danger
                        } else {
                            theme.accent_foreground
                        })
                        .bg(if change.verb.starts_with("Remove") {
                            theme.danger_soft
                        } else {
                            theme.accent_soft
                        })
                        .child(change.verb.clone()),
                )
                .child(mono(change.program.clone()).flex_1())
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted)
                        .child(format!("at {}", change.height)),
                )
                .children(
                    change
                        .code
                        .as_ref()
                        .map(|code| mono(short_code(code)).text_color(theme.muted)),
                )
                .into_any_element()
        })
        .collect();
    let nothing_scheduled = scheduled.is_empty().then(|| {
        quiet(
            "explorer-no-changes",
            "Nothing is scheduled against the registry.",
            theme,
        )
    });
    div()
        .id("explorer-list")
        .child(heading(
            "explorer-running-header",
            "Running",
            Some(caption(
                plural(network.programs.len() as u64, "program", "programs"),
                theme,
            )),
            theme,
        ))
        .children(running)
        .child(heading(
            "explorer-scheduled-header",
            "Scheduled",
            None,
            theme,
        ))
        .children(scheduled)
        .children(nothing_scheduled)
        .into_any_element()
}

fn short_code(code: &str) -> String {
    let mut head: String = code.chars().take(12).collect();
    if code.chars().count() > 12 {
        head.push('…');
    }
    head
}
