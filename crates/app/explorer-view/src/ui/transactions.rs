//! The Transactions tab and one transaction.
use super::*;

pub(super) fn transactions(
    view: &Explorer,
    program: Option<String>,
    cx: Cx,
    theme: &Theme,
) -> AnyElement {
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

pub(super) fn tx(view: &Explorer, hash: &[u8; 32], cx: Cx, theme: &Theme) -> AnyElement {
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
    let op = tx.op();
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
        .children(op.fields.iter().map(|(name, value)| {
            div()
                .flex()
                .gap_4()
                .child(
                    mono(*name)
                        .w(px(90.))
                        .flex_shrink_0()
                        .text_color(theme.muted),
                )
                .child(div().flex_1().child(value.clone()))
        }));
    let copy = copy_button(view, &Route::Tx(*hash), cx, theme);
    div()
        .id("explorer-tx")
        .child(
            div()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .child(titled("Transaction", op.title.clone(), theme)),
                )
                .children(copy.map(|copy| div().px_5().child(copy))),
        )
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
