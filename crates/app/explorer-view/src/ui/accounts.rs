//! The Accounts tab and one account.
use super::*;
use crate::decode::scheme;
use identity::Account;

/// What `account` is, its manager named from the list: "Person", "Agent ·
/// managed by Dev · suspended", "Module · forge" (identity's own wording,
/// [`identity::view::kind`], over `Kind::badge` and `Kind::note`).
fn kind(view: &Explorer, account: &Account) -> String {
    identity::view::kind(&account.kind(), |manager| {
        view.account(manager)
            .map(|manager| manager.card.name.clone())
    })
}

pub(super) fn accounts(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let listed = match &view.accounts {
        Loadable::Ready(listed) => listed,
        Loadable::Failed(refusal) => return failed(&refusal.message, cx, theme),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-accounts-loading", "Reading accounts…", theme);
        }
    };
    let rows: Vec<_> = listed
        .list
        .iter()
        .take(LIST_ROWS * 4)
        .map(|account| {
            let name = &account.card.name;
            let sent = activity(view, account).count() as u64;
            row(
                SharedString::from(format!("explorer-account-{}", account.number)).into(),
                name.clone(),
                Route::Account(account.number),
                cx,
                theme,
            )
            .child(design::avatar(name, design::size::AVATAR, theme))
            .child(div().flex_1().truncate().child(name.clone()))
            .child(div().text_color(theme.muted).child(kind(view, account)))
            .child(mono(format!("#{}", account.number)).text_color(theme.faint))
            .child(div().w(DEVICES_W).text_color(theme.muted).child(plural(
                account.keys().len() as u64,
                "device",
                "devices",
            )))
            .child(
                mono(plural(sent, "tx", "tx"))
                    .w(COUNT_W)
                    .flex()
                    .justify_end()
                    .text_color(theme.muted),
            )
            .into_any_element()
        })
        .collect();
    let caption_text = format!(
        "{} · tx in the last {}",
        grouped(listed.list.len() as u64),
        plural(view.chain.blocks.len() as u64, "block", "blocks")
    );
    div()
        .id("explorer-accounts")
        .child(heading(
            "explorer-accounts-heading",
            "Accounts",
            Some(caption(caption_text, theme)),
            theme,
        ))
        .children(rows)
        .children(
            listed
                .more
                .then(|| design::more_not_shown("explorer-accounts-more", theme).px_5()),
        )
        .into_any_element()
}

/// What `account`'s keys signed in the window, newest first.
fn activity<'a>(view: &'a Explorer, account: &'a Account) -> impl Iterator<Item = &'a TxRow> {
    let keys = account.keys();
    view.chain
        .txs
        .iter()
        .filter(move |tx| keys.iter().any(|key| key.key == tx.signer))
}

pub(super) fn account(view: &Explorer, number: u64, cx: Cx, theme: &Theme) -> AnyElement {
    let Some(account) = view.account(number) else {
        return match &view.accounts {
            Loadable::Ready(_) => empty_state(
                "explorer-no-account",
                format!("No account #{number}"),
                "Identity holds no account by this number.",
                theme,
            )
            .into_any_element(),
            Loadable::Failed(refusal) => failed(&refusal.message, cx, theme),
            Loadable::Idle | Loadable::Loading(_) => {
                quiet("explorer-account-loading", "Reading the account…", theme)
            }
        };
    };
    let window = plural(view.chain.blocks.len() as u64, "block", "blocks");
    let sent: Vec<&TxRow> = activity(view, account).collect();
    let rows: Vec<_> = sent
        .iter()
        .take(LIST_ROWS)
        .map(|tx| tx_row(view, tx, true, false, cx, theme).into_any_element())
        .collect();
    let empty = rows.is_empty().then(|| {
        quiet(
            "explorer-no-activity",
            format!("Nothing signed in the last {window}."),
            theme,
        )
    });
    let activity = div()
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
        .children(empty);
    div()
        .id("explorer-account")
        .flex()
        .flex_col()
        .flex_1()
        .child(title(view, account, cx, theme))
        .child(
            div()
                .flex()
                .flex_1()
                .border_t_1()
                .border_color(theme.border)
                .child(activity)
                .child(side(view, account, &sent, theme)),
        )
        .into_any_element()
}

/// The account's name, what it is, and its link.
fn title(view: &Explorer, account: &Account, cx: Cx, theme: &Theme) -> impl IntoElement {
    let name = &account.card.name;
    let about = format!(
        "account {}   {}   {}",
        account.number,
        kind(view, account),
        plural(account.keys().len() as u64, "device", "devices")
    );
    div()
        .flex()
        .items_center()
        .gap_4()
        .px_5()
        .py_4()
        .child(design::avatar(name, PAGE_AVATAR, theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("explorer-title")
                        .text_size(design::text::TITLE)
                        .role(Role::Heading)
                        .aria_level(1)
                        .child(name.clone()),
                )
                .child(
                    mono(about)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted),
                ),
        )
        .child(div().flex_1())
        .children(copy_button(
            view,
            &Route::Account(account.number),
            cx,
            theme,
        ))
}

/// The account's devices, each with when it last signed, and the programs
/// its transactions in the window went to.
fn side(view: &Explorer, account: &Account, sent: &[&TxRow], theme: &Theme) -> impl IntoElement {
    let now = view.chain.now();
    let devices = account.keys().iter().enumerate().map(|(index, key)| {
        let last = sent.iter().find(|tx| tx.signer == key.key);
        let label = key
            .label
            .clone()
            .unwrap_or_else(|| format!("Device {}", index + 1));
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(ROW_H)
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(div().child(label))
            .child(
                mono(format!("{} {}", scheme(key.scheme), short(&key.key)))
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
    let mut used: Vec<(&str, u64)> = Vec::new();
    for tx in sent {
        match used.iter_mut().find(|(program, _)| *program == tx.target) {
            Some((_, count)) => *count += 1,
            None => used.push((&tx.target, 1)),
        }
    }
    used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let programs = used.into_iter().map(|(program, count)| {
        div()
            .flex()
            .items_center()
            .h(ROW_H)
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(mono(program.to_owned()).flex_1())
            .child(mono(format!("{count} tx")).text_color(theme.muted))
    });
    div()
        .id("explorer-account-side")
        .w(SIDE_W)
        .flex_shrink_0()
        .child(heading(
            "explorer-devices-heading",
            "Devices",
            Some(caption(grouped(account.keys().len() as u64), theme)),
            theme,
        ))
        .children(devices)
        .child(heading(
            "explorer-used-heading",
            "Programs used",
            None,
            theme,
        ))
        .children(programs)
}
