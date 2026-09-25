//! The Transactions tab and one transaction.
use super::*;
use crate::decode::{amount, clip, preview};
use ducktape_view_guest::design;
use ducktape_view_guest::methods::Value;

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
        return empty_state(
            "explorer-no-tx",
            "Transaction not found",
            format!(
                "It is not in the last {} this explorer reads.",
                plural(view.chain.blocks.len() as u64, "block", "blocks")
            ),
            theme,
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
                .child(design::avatar(&account.name, px(20.), theme))
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
            mono(format!("code {}", design::short_hex(&code))).text_color(theme.faint)
        }));
    view.describe(tx, cx);
    let op = tx.op();
    let fields: Vec<AnyElement> = op
        .map(|op| op.fields.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    mono(field.label.clone())
                        .w(px(90.))
                        .flex_shrink_0()
                        .text_color(theme.muted),
                )
                .child(div().flex_1().child(value(
                    view,
                    &field.value,
                    &format!("{index}"),
                    cx,
                    theme,
                )))
                .into_any_element()
        })
        .collect();
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
        .children(fields);
    let copy = copy_button(view, &Route::Tx(*hash), cx, theme);
    div()
        .id("explorer-tx")
        .child(
            div()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().child(titled(
                    "Transaction",
                    op.map(|op| clip(&op.title)).unwrap_or_default(),
                    theme,
                )))
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
                .text_size(design::text::SECTION)
                .font_weight(FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(2)
                .child("Operation"),
        )
        .child(operation)
        .into_any_element()
}

/// One described value, shown as what it is: an account by its name, a
/// key by the account holding it, a program as a link to its
/// transactions, a hash short. `at` keeps each link's id its own.
fn value(view: &Explorer, shown: &Value, at: &str, cx: Cx, theme: &Theme) -> AnyElement {
    match shown {
        Value::Text(text) => div().child(clip(text)).into_any_element(),
        Value::Account(number) => account(view, *number, at, cx, theme),
        Value::Key(key) => match view.holder(key) {
            Some((holder, _)) => account(view, holder.number, at, cx, theme),
            None => mono(short(key)).into_any_element(),
        },
        Value::Program(name) => {
            let runs = view
                .network
                .ready()
                .is_some_and(|network| network.programs.iter().any(|entry| &entry.program == name));
            match runs {
                true => link(
                    format!("explorer-value-{at}"),
                    name.clone(),
                    Route::Transactions(Some(name.clone())),
                    cx,
                    theme,
                )
                .into_any_element(),
                false => mono(name.clone()).into_any_element(),
            }
        }
        Value::Hash(bytes) => mono(short(bytes)).into_any_element(),
        Value::Amount { value, decimals } => mono(amount(*value, *decimals)).into_any_element(),
        Value::Time(millis) => mono(date(*millis)).into_any_element(),
        Value::Bytes { len, preview: head } => mono(preview(*len, head)).into_any_element(),
        Value::List(items) => div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| value(view, item, &format!("{at}-{index}"), cx, theme)),
            )
            .into_any_element(),
    }
}

/// An account as its avatar and name, a link to its page; its number
/// where the window has no such account.
fn account(view: &Explorer, number: u64, at: &str, cx: Cx, theme: &Theme) -> AnyElement {
    let name = view
        .accounts
        .ready()
        .and_then(|accounts| accounts.iter().find(|account| account.number == number))
        .map(|account| account.name.clone());
    let shown = name.clone().unwrap_or_else(|| format!("account {number}"));
    div()
        .flex()
        .items_center()
        .gap_2()
        .children(name.map(|name| design::avatar(&name, px(18.), theme)))
        .child(link(
            format!("explorer-value-{at}"),
            shown,
            Route::Account(number),
            cx,
            theme,
        ))
        .child(mono(format!("#{number}")).text_color(theme.faint))
        .into_any_element()
}
