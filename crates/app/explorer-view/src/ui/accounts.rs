//! The Accounts tab and one account.
use super::*;
use ducktape_view_guest::design;

pub(super) fn accounts(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
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

pub(super) fn account(view: &Explorer, number: u64, cx: Cx, theme: &Theme) -> AnyElement {
    let Some(account) = view
        .accounts
        .ready()
        .and_then(|list| list.iter().find(|account| account.number == number))
    else {
        return match &view.accounts {
            Loaded::Ready(_) => empty_state(
                "explorer-no-account",
                format!("No account #{number}"),
                "Identity holds no account by this number.",
                theme,
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
                    .text_size(design::text::CAPTION)
                    .text_color(theme.faint),
            )
            .child(
                mono(match last {
                    Some(tx) => format!("last used {} ago", ago(now, tx.time)),
                    None => "not used lately".into(),
                })
                .text_size(design::text::CAPTION)
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
    let copy = copy_button(view, &Route::Account(number), cx, theme);
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
                            .text_size(design::text::CAPTION)
                            .text_color(theme.muted),
                        ),
                )
                .child(div().flex_1())
                .children(copy),
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
