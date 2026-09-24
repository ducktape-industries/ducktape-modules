//! The Overview tab: the head, the latest blocks and transactions.
use super::*;
use ducktape_view_guest::design;

pub(super) fn overview(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
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
            .child(
                mono(label)
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted),
            )
            .child(div().text_size(px(22.)).child(value))
            .child(
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(note),
            )
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
    .text_size(design::text::SECONDARY)
    .into_any_element();
    let all_txs = link(
        "explorer-all-txs".into(),
        "All transactions →".into(),
        Route::Transactions(None),
        cx,
        theme,
    )
    .text_size(design::text::SECONDARY)
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
