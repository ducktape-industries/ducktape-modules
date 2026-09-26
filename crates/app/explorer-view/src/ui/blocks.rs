//! The Blocks tab and one block.
use super::*;
use ducktape_view_guest::design;

pub(super) fn blocks(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
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

pub(super) fn block(view: &Explorer, height: u64, cx: Cx, theme: &Theme) -> AnyElement {
    let Some((block, txs)) = view.block(height) else {
        return match &view.opened {
            Loadable::Ready(None) => empty_state(
                "explorer-no-block",
                format!("No block {}", grouped(height)),
                "This node keeps no finalized block at this height.",
                theme,
            )
            .into_any_element(),
            Loadable::Failed(refusal) => failed(&refusal.message, cx, theme),
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
            .text_size(design::text::SECONDARY)
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
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::SECONDARY),
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
    let copy = copy_button(view, &Route::Block(height), cx, theme);
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
                        .children(copy)
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
