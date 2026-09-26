//! The Overview tab: the head, the latest blocks and transactions.
use super::*;

/// A stat keeps this much of the row; the four fold two by two below it.
const STAT_MIN_W: Pixels = px(200.);
/// A latest-rows panel keeps this much; past it, the transactions wrap under.
const PANEL_MIN_W: Pixels = px(320.);

pub(super) fn overview(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    div()
        .id("explorer-overview")
        .flex()
        .flex_col()
        .flex_1()
        .child(stats(view, theme))
        .child(
            // narrow, the transactions wrap under the blocks
            div()
                .flex()
                .flex_wrap()
                .flex_1()
                .child(latest_blocks(view, cx, theme))
                .child(latest_txs(view, cx, theme)),
        )
        .into_any_element()
}

/// The row of figures: height, epoch, validators, accounts.
fn stats(view: &Explorer, theme: &Theme) -> impl IntoElement {
    let status = view.status.ready();
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
        .map_or_else(dash, |accounts| grouped(accounts.list.len() as u64));
    let pair = || {
        div()
            .flex()
            .flex_1()
            .min_w(STAT_MIN_W * 2.)
            .border_b_1()
            .border_color(theme.border)
    };
    // in pairs, so a narrow row folds to two by two, never three and one
    div()
        .id("explorer-stats")
        .flex()
        .flex_wrap()
        .child(
            pair()
                .child(stat(
                    "explorer-stat-height",
                    "Height",
                    height,
                    cadence,
                    theme,
                ))
                .child(stat("explorer-stat-epoch", "Epoch", epoch, next, theme)),
        )
        .child(
            pair()
                .child(stat(
                    "explorer-stat-validators",
                    "Validators",
                    validators,
                    String::new(),
                    theme,
                ))
                .child(stat(
                    "explorer-stat-accounts",
                    "Accounts",
                    accounts,
                    String::new(),
                    theme,
                )),
        )
}

/// One figure: its label, its value, and a note under it.
fn stat(
    id: &'static str,
    label: &'static str,
    value: String,
    note: String,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_w(STAT_MIN_W)
        .flex()
        .flex_col()
        .gap_1()
        .px_5()
        .py_4()
        .border_r_1()
        .border_color(theme.border)
        .child(
            mono(label)
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        )
        .child(div().text_size(design::text::TITLE).child(value))
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(note),
        )
}

fn latest_blocks(view: &Explorer, cx: Cx, theme: &Theme) -> impl IntoElement {
    let all = link(
        "explorer-all-blocks".into(),
        "All blocks →".into(),
        Route::Blocks,
        cx,
        theme,
    )
    .text_size(design::text::SECONDARY)
    .into_any_element();
    let blocks = block_lines(&view.chain.blocks, LATEST, view.chain.now(), cx, theme);
    div()
        .id("explorer-latest-blocks")
        .flex_1()
        .min_w(PANEL_MIN_W)
        .max_w(LATEST_BLOCKS_W)
        .border_r_1()
        .border_color(theme.border)
        .child(heading(
            "explorer-latest-blocks-heading",
            "Latest activity",
            Some(all),
            theme,
        ))
        .children(blocks)
}

fn latest_txs(view: &Explorer, cx: Cx, theme: &Theme) -> impl IntoElement {
    let all = link(
        "explorer-all-txs".into(),
        "All transactions →".into(),
        Route::Transactions(None),
        cx,
        theme,
    )
    .text_size(design::text::SECONDARY)
    .into_any_element();
    let txs: Vec<_> = view
        .chain
        .txs
        .iter()
        .take(LATEST)
        .map(|tx| tx_row(view, tx, false, true, cx, theme).into_any_element())
        .collect();
    let none = txs.is_empty().then(|| {
        quiet(
            "explorer-no-txs",
            format!(
                "No transactions in the last {}.",
                plural(view.chain.blocks.len() as u64, "block", "blocks")
            ),
            theme,
        )
    });
    div()
        .id("explorer-latest-txs")
        .flex_1()
        .min_w(PANEL_MIN_W)
        .child(heading(
            "explorer-latest-txs-heading",
            "Latest transactions",
            Some(all),
            theme,
        ))
        .children(txs)
        .children(none)
}
