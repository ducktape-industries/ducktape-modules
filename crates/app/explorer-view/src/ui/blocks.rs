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
            Loadable::Idle | Loadable::Loading(_) | Loadable::Ready(Some(_)) => {
                quiet("explorer-block-loading", "Reading the block…", theme)
            }
        };
    };
    let now = view.chain.now();
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
        .child(title(view, height, cx, theme))
        .child(field("Hash", mono(abi::hex(&block.id)), theme))
        .child(field("Parent", parent(&block, cx, theme), theme))
        .child(field(
            "Time",
            div()
                .flex()
                .gap_2()
                .child(date(block.time))
                .child(mono(format!("{} ago", ago(now, block.time))).text_color(theme.faint)),
            theme,
        ))
        .children(proposer(view, &block, theme))
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

/// The block's height, its link, and the steps to the blocks either side.
fn title(view: &Explorer, height: u64, cx: Cx, theme: &Theme) -> impl IntoElement {
    let head = view.status.ready().map_or(0, |status| status.height);
    let previous = height.checked_sub(1);
    let next = (height < head).then_some(height + 1);
    let back = format!("← {}", previous.map_or(String::new(), grouped));
    let on = format!("{} →", grouped(height + 1));
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
                .children(copy_button(view, &Route::Block(height), cx, theme))
                .child(step("explorer-previous", back, previous, cx, theme))
                .child(step("explorer-next", on, next, cx, theme)),
        )
}

/// A step to the block at `to`; none there, it stands disabled.
fn step(
    id: &'static str,
    text: String,
    to: Option<u64>,
    cx: Cx,
    theme: &Theme,
) -> impl IntoElement {
    let button = div()
        .id(id)
        .px_3()
        .h(design::size::CONTROL)
        .flex()
        .items_center()
        .border_1()
        .border_color(theme.border)
        .text_size(design::text::SECONDARY)
        .text_color(if to.is_some() {
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
            let hover = theme.hover;
            button.focusable().hover(move |s| s.bg(hover)).on_click(go)
        }
        None => button.aria_disabled(true),
    }
}

/// The parent's height, a link, then its hash.
fn parent(block: &BlockRow, cx: Cx, theme: &Theme) -> impl IntoElement {
    let hash = mono(short(&block.parent)).text_color(theme.muted);
    let Some(previous) = block.height.checked_sub(1) else {
        return div().child(hash);
    };
    div()
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
        .child(hash)
}

/// Who proposed the block: its place in the validator set, then its key.
fn proposer(view: &Explorer, block: &BlockRow, theme: &Theme) -> Option<impl IntoElement> {
    let key = block.proposer.as_ref()?;
    let place = view
        .validators
        .ready()
        .and_then(|keys| keys.iter().position(|seated| seated == key));
    let name = match place {
        Some(place) => format!("validator {}", place + 1),
        None => "validator".into(),
    };
    Some(field(
        "Proposer",
        div()
            .flex()
            .gap_2()
            .child(mono(name))
            .child(mono(format!("ed25519 {}", short(key))).text_color(theme.faint)),
        theme,
    ))
}
