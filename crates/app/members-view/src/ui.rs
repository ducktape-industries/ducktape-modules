//! The screen: the list on the left, the chosen account on the right.
//! Colour is kept for what it says: the agent tint on an agent's avatar, and
//! the badges of a standing. Everything else is the window, grey captions
//! and hairlines.
use ducktape_view_guest::design::{self, size, space, text};
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Role, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Theme, div, px,
};
use ducktape_view_guest::{Input, prelude::FluentBuilder};

use crate::activity::{self, WINDOW};
use crate::{Group, Members, Row};

/// The list pane's width.
const LIST: Pixels = px(400.);
/// A detail row's first column: a device's label, an agent's name. It
/// gives way first when the pane is narrow.
const KEY_COLUMN: Pixels = px(180.);

pub(crate) fn render(view: &Members, cx: &mut Context<Members>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    div()
        .id("members")
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(text::BODY)
        .child(list_pane(view, cx, &theme))
        .child(
            div()
                .id("members-detail")
                .flex_1()
                .min_w(px(0.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(detail(view, cx, &theme)),
        )
}

fn list_pane(view: &Members, cx: &mut Context<Members>, theme: &Theme) -> impl IntoElement {
    let typed = cx.listener(|view, text: &String, _, cx| {
        view.filter = text.clone();
        cx.notify();
    });
    let count = match view.rows.ready() {
        Some(rows) => design::plural(rows.len() as u64, "account", "accounts"),
        None => String::new(),
    };
    div()
        .id("members-list-pane")
        .w(LIST)
        .flex_shrink_0()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .px(space::BLOCK)
                .pt(px(14.))
                .pb(space::MD)
                .child(design::heading("members-title", "Members", 1, theme).flex_1())
                .child(
                    div()
                        .text_size(text::CAPTION)
                        .text_color(theme.muted)
                        .child(count),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::SM)
                .px(space::BLOCK)
                .pb(space::MD)
                .child(
                    Input::new("members-filter")
                        .h(size::CONTROL)
                        .w_full()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(theme.border_strong)
                        .bg(theme.surface)
                        .text_color(theme.foreground)
                        .value(view.filter.clone())
                        .placeholder("Filter by name or number")
                        .label("Filter members")
                        .on_input(typed),
                )
                .child(chips(view, cx, theme)),
        )
        .child(rows(view, cx, theme))
}

/// All, People, Agents, Modules, each with how many the network holds.
fn chips(view: &Members, cx: &mut Context<Members>, theme: &Theme) -> impl IntoElement {
    let rows = view.rows.ready().map_or(&[][..], Vec::as_slice);
    let count = |group: Option<Group>| {
        rows.iter()
            .filter(|row| group.is_none_or(|group| row.group() == group))
            .count()
    };
    let choices = [
        (None, "All"),
        (Some(Group::People), "People"),
        (Some(Group::Agents), "Agents"),
        (Some(Group::Modules), "Modules"),
    ];
    div()
        .flex()
        .gap(space::XS)
        .children(
            choices
                .into_iter()
                .enumerate()
                .map(|(index, (group, label))| {
                    let on = view.only == group;
                    div()
                        .id(format!("members-chip-{}", index))
                        .flex()
                        .gap(space::XXS)
                        .px(space::SM)
                        .py(space::HAIR)
                        .border_1()
                        .border_color(if on { theme.foreground } else { theme.border })
                        .text_size(text::CAPTION)
                        .text_color(if on { theme.foreground } else { theme.muted })
                        .cursor_pointer()
                        .role(Role::Button)
                        .aria_selected(on)
                        .focusable()
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.only = group;
                            cx.notify();
                        }))
                        .child(label)
                        .child(
                            div()
                                .text_color(theme.faint)
                                .child(count(group).to_string()),
                        )
                }),
        )
}

/// The four states of the roster: loading, refused, empty, ready.
fn rows(view: &Members, cx: &mut Context<Members>, theme: &Theme) -> AnyElement {
    let all = match &view.rows {
        Loadable::Idle | Loadable::Loading(_) => {
            return pad(design::quiet("Reading the roster…", theme))
                .id("members-loading")
                .into_any_element();
        }
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
            return pad(design::refused(
                "members",
                refusal.message.clone(),
                theme,
                retry,
            ))
            .into_any_element();
        }
        Loadable::Ready(rows) => rows,
    };
    if all.is_empty() {
        return design::empty_state(
            "members-empty",
            "No accounts",
            "The identity program of this network holds no accounts yet.",
            theme,
        )
        .into_any_element();
    }
    let shown = view.shown();
    if shown.is_empty() {
        return design::empty_state(
            "members-no-match",
            "Nothing matches",
            format!("No account reads like “{}”.", view.filter.trim()),
            theme,
        )
        .into_any_element();
    }
    let stepped =
        cx.listener(
            |view, event: &KeyDownEvent, _, cx| match event.keystroke.key.as_str() {
                "down" => view.step(true, cx),
                "up" => view.step(false, cx),
                _ => {}
            },
        );
    let mut list = div()
        .id("members-list")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .aria_label("Members")
        .focusable()
        .on_key_down(stepped);
    for (index, group) in Group::ALL.into_iter().enumerate() {
        let members: Vec<&Row> = shown
            .iter()
            .copied()
            .filter(|row| row.group() == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        let label = match group {
            Group::People => "People",
            Group::Agents => "Agents",
            Group::Modules => "Modules",
        };
        list = list.child(
            div()
                .flex()
                .justify_between()
                .px(space::BLOCK)
                .pt(space::MD)
                .pb(space::XXS)
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .when(index > 0, |head| {
                    head.border_t_1().border_color(theme.border)
                })
                .child(label)
                .child(design::mono(members.len().to_string()).text_size(text::CAPTION)),
        );
        for row in members {
            list = list.child(member_row(view, row, all, cx, theme));
        }
    }
    list.into_any_element()
}

fn member_row(
    view: &Members,
    row: &Row,
    all: &[Row],
    cx: &mut Context<Members>,
    theme: &Theme,
) -> impl IntoElement {
    let theme = *theme;
    let number = row.number;
    let selected = view.selected == Some(number);
    let dim = row.kind.note().is_some();
    let name_of = |manager| name(all, manager);
    div()
        .id(format!("members-row-{}", number))
        .flex()
        .items_center()
        .gap(space::MD)
        .h(size::CONTROL)
        .flex_shrink_0()
        .px(space::BLOCK)
        .cursor_pointer()
        .when(selected, |row| row.bg(theme.surface_raised))
        .when(!selected, |row| {
            row.hover(move |style| style.bg(theme.surface))
        })
        .role(Role::Button)
        .aria_selected(selected)
        .focusable()
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(number, cx)))
        .child(avatar(row, size::AVATAR, dim, &theme))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::XS)
                .child(
                    div()
                        .truncate()
                        .when(dim, |name| name.text_color(theme.faint))
                        .child(row.name.clone()),
                )
                .when(view.me == Some(number), |line| {
                    line.child(
                        div()
                            .text_size(text::CAPTION)
                            .text_color(theme.faint)
                            .child("you"),
                    )
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .child(identity::view::kind(&row.kind, name_of)),
        )
}

/// An account's initial: round for a person, tinted for an agent, square
/// and monospaced for a module; faint when it no longer acts.
fn avatar(row: &Row, size: Pixels, dim: bool, theme: &Theme) -> Div {
    let avatar = design::avatar(&row.name, size, theme);
    match row.group() {
        _ if dim => avatar.bg(theme.background).text_color(theme.faint),
        Group::People => avatar,
        Group::Agents => avatar.bg(theme.agent_soft).text_color(theme.agent),
        Group::Modules => avatar
            .rounded_none()
            .font_family(design::fonts::FAMILY_MONO),
    }
}

fn name(rows: &[Row], number: u64) -> Option<String> {
    rows.iter()
        .find(|row| row.number == number)
        .map(|row| row.name.clone())
}

fn pad(inner: impl IntoElement) -> Div {
    div().px(space::BLOCK).py(space::SM).child(inner)
}

fn detail(view: &Members, cx: &mut Context<Members>, theme: &Theme) -> AnyElement {
    let (Some(rows), Some(row)) = (view.rows.ready(), view.selected_row()) else {
        return div()
            .id("members-none")
            .px(px(20.))
            .py(space::BLOCK)
            .child(design::quiet("Choose a member", theme))
            .into_any_element();
    };
    let mut detail = div()
        .flex()
        .flex_col()
        .child(head(view, row, rows, cx, theme))
        .child(devices(row, theme));
    let managed: Vec<&Row> = rows
        .iter()
        .filter(|other| other.manager() == Some(row.number))
        .collect();
    if !managed.is_empty() {
        detail = detail.child(manages(&managed, cx, theme));
    }
    if !row.devices.is_empty() {
        detail = detail.child(activity(view, theme));
    }
    if row.group() == Group::Agents {
        detail = detail.child(
            div()
                .id("members-managed-in-settings")
                .px(px(20.))
                .pt(space::LG)
                .text_size(text::SECONDARY)
                .text_color(theme.muted)
                .child(
                    "Suspend, revoke, rename and keys live in Settings → Account, for the manager.",
                ),
        );
    }
    detail.into_any_element()
}

/// The avatar, the name, one caption line of what the account is, its bio,
/// and the two ways out of this screen.
fn head(
    view: &Members,
    row: &Row,
    rows: &[Row],
    cx: &mut Context<Members>,
    theme: &Theme,
) -> impl IntoElement {
    let text = |text: String| div().child(text).into_any_element();
    // each part keeps its words (and the manager its link) on one line;
    // a narrow pane wraps between parts
    let mut caption: Vec<Vec<AnyElement>> = vec![vec![text(format!("account {}", row.number))]];
    match &row.kind {
        identity::Kind::Person => caption.push(vec![text("Person".into())]),
        identity::Kind::Module(_) => caption.push(vec![text(
            row.kind.badge(|_| String::new()).unwrap_or_default(),
        )]),
        identity::Kind::Managed {
            manager, standing, ..
        } => {
            // the badge with the manager's name left out, and the name as
            // a link that chooses the manager here
            let what = row.kind.badge(|_| String::new()).unwrap_or_default();
            let manager = *manager;
            let label = name(rows, manager).unwrap_or_else(|| format!("#{manager}"));
            let link = div()
                .id("members-manager")
                .text_color(theme.foreground)
                .text_decoration_1()
                .cursor_pointer()
                .role(Role::Link)
                .focusable()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(manager, cx)))
                .child(label)
                .into_any_element();
            caption.push(vec![text(what.trim_end().to_owned()), link]);
            caption.push(vec![
                standing_badge("members-standing", &row.kind, *standing, theme).into_any_element(),
            ]);
        }
    }
    if row.group() != Group::Modules {
        let devices = design::plural(row.devices.len() as u64, "device", "devices");
        caption.push(vec![text(devices)]);
    }
    if let Some(standing) = &row.standing {
        caption.push(vec![
            design::badge(
                "members-validator",
                standing.clone(),
                theme.success,
                theme.success_soft,
            )
            .into_any_element(),
        ]);
    }
    let last = caption.len() - 1;
    let line = caption.into_iter().enumerate().map(|(index, part)| {
        div()
            .flex()
            .items_center()
            .gap(space::XS)
            .whitespace_nowrap()
            .children(part)
            .when(index < last, |part| part.child("·"))
    });
    let number = row.number;
    let explorer = design::explorer::link(&design::explorer::account_path(number));
    let dm = match (view.me, &row.kind) {
        (Some(me), identity::Kind::Person | identity::Kind::Managed { .. }) if me != number => {
            ducklink::mint(
                &view.chain,
                chat::MODULE,
                &[&chat::dm_channel_id(me, number)],
            )
        }
        _ => None,
    };
    div()
        .id("members-detail-head")
        .flex()
        .items_start()
        .gap(space::LG)
        .px(px(20.))
        .pt(space::BLOCK)
        .pb(px(14.))
        .border_b_1()
        .border_color(theme.border)
        .child(avatar(row, px(32.), row.kind.note().is_some(), theme))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("members-detail-title")
                        .text_size(text::TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .id("members-caption")
                        .mt(px(3.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x(space::XS)
                        .gap_y(space::HAIR)
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(text::CAPTION)
                        .text_color(theme.muted)
                        .children(line),
                )
                .when_some(
                    row.bio.clone().filter(|bio| !bio.trim().is_empty()),
                    |who, bio| {
                        who.child(
                            div()
                                .id("members-bio")
                                .mt(space::SM)
                                .max_w(px(460.))
                                .child(bio),
                        )
                    },
                ),
        )
        .child(
            div()
                .flex()
                .gap(space::XS)
                .when_some(dm, |buttons, link| {
                    buttons.child(outline_button("members-open-dm", "Open DM", link, theme))
                })
                .child(outline_button(
                    "members-explorer",
                    "Explorer",
                    explorer,
                    theme,
                )),
        )
}

/// Active, suspended or revoked, in its colour.
fn standing_badge(
    id: &'static str,
    kind: &identity::Kind,
    standing: identity::Standing,
    theme: &Theme,
) -> Stateful<Div> {
    let (foreground, background) = match standing {
        identity::Standing::Active => (theme.success, theme.success_soft),
        identity::Standing::Suspended => (theme.warning, theme.warning_soft),
        identity::Standing::Revoked => (theme.danger, theme.danger_soft),
    };
    design::badge(id, kind.note().unwrap_or("active"), foreground, background)
}

/// A 1px-edged button that opens `link` through the host.
fn outline_button(
    id: &'static str,
    label: &'static str,
    link: String,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id)
        .flex()
        .items_center()
        .h(size::CONTROL)
        .px(space::MD)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(text::SECONDARY)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .focusable()
        .on_click(move |_: &ClickEvent, _, cx| cx.host().open_link(&link))
        .child(label)
}

/// A detail section: its heading and count over its rows, a hairline under.
fn section(
    id: &'static str,
    title: &'static str,
    count: String,
    body: Vec<AnyElement>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .flex_col()
        .pb(space::XS)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .px(px(20.))
                .pt(space::LG)
                .pb(space::XS)
                .child(
                    design::heading(SharedString::from(format!("{id}-heading")), title, 2, theme)
                        .flex_1(),
                )
                .child(
                    design::mono(count)
                        .text_size(text::CAPTION)
                        .text_color(theme.muted),
                ),
        )
        .children(body)
}

/// One line of a section.
fn line() -> Div {
    div()
        .flex()
        .items_center()
        .gap(space::MD)
        .h(size::ROW)
        .px(px(20.))
}

/// A section with nothing in it says so, quietly.
fn empty(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .px(px(20.))
        .pt(space::HAIR)
        .pb(space::SM)
        .text_size(text::SECONDARY)
        .text_color(theme.muted)
        .child(text)
        .into_any_element()
}

fn devices(row: &Row, theme: &Theme) -> impl IntoElement {
    let body = if row.devices.is_empty() {
        vec![empty("No devices", theme)]
    } else {
        row.devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let label = device
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("Device {}", index + 1));
                let hex: String = device
                    .key
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                line()
                    .child(div().w(KEY_COLUMN).min_w(px(0.)).truncate().child(label))
                    .child(
                        design::mono(design::short_hex(&hex))
                            .text_size(text::CAPTION)
                            .text_color(theme.muted),
                    )
                    .child(
                        design::mono(format!("added {}", activity::date(device.added_at)))
                            .ml_auto()
                            .text_size(text::CAPTION)
                            .text_color(theme.faint),
                    )
                    .into_any_element()
            })
            .collect()
    };
    section(
        "members-devices",
        "Devices",
        row.devices.len().to_string(),
        body,
        theme,
    )
}

/// The agents a person manages; pressing one chooses it here.
fn manages(managed: &[&Row], cx: &mut Context<Members>, theme: &Theme) -> impl IntoElement {
    let body = managed
        .iter()
        .filter_map(|agent| {
            let identity::Kind::Managed {
                category: identity::Category::Agent,
                standing,
                ..
            } = agent.kind
            else {
                return None;
            };
            let number = agent.number;
            let theme = *theme;
            let row = line()
                .id(format!("members-manages-{}", number))
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface))
                .role(Role::Button)
                .focusable()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(number, cx)))
                .child(avatar(
                    agent,
                    size::AVATAR_SM,
                    agent.kind.note().is_some(),
                    &theme,
                ))
                .child(
                    div()
                        .w(KEY_COLUMN)
                        .min_w(px(0.))
                        .truncate()
                        .child(agent.name.clone()),
                )
                .child(
                    design::mono(format!("account {number} · Agent"))
                        .text_size(text::CAPTION)
                        .text_color(theme.muted),
                )
                .child(div().ml_auto().child(standing_badge(
                    "members-manages-standing",
                    &agent.kind,
                    standing,
                    &theme,
                )));
            Some(row.into_any_element())
        })
        .collect();
    section(
        "members-manages",
        "Manages",
        managed.len().to_string(),
        body,
        theme,
    )
}

fn activity(view: &Members, theme: &Theme) -> impl IntoElement {
    let window = design::plural(WINDOW, "block", "blocks");
    let body = match &view.activity {
        Loadable::Idle | Loadable::Loading(_) => vec![
            div()
                .id("members-activity-loading")
                .px(px(20.))
                .pb(space::SM)
                .child(design::quiet(format!("Reading the last {window}…"), theme))
                .into_any_element(),
        ],
        Loadable::Failed(refusal) => vec![
            div()
                .px(px(20.))
                .pb(space::SM)
                .child(design::quiet(refusal.message.clone(), theme))
                .into_any_element(),
        ],
        Loadable::Ready(recent) if recent.items.is_empty() => {
            vec![
                div()
                    .id("members-no-activity")
                    .px(px(20.))
                    .pb(space::SM)
                    .child(design::quiet(
                        format!("Nothing signed in the last {window}."),
                        theme,
                    ))
                    .into_any_element(),
            ]
        }
        Loadable::Ready(recent) => recent
            .items
            .iter()
            .enumerate()
            .map(|(index, signed)| {
                line()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .child(signed.title.clone()),
                    )
                    .child(design::block_link(
                        format!("members-block-{}", index),
                        signed.height,
                        theme,
                    ))
                    .child(
                        div()
                            .w(px(52.))
                            .flex()
                            .justify_end()
                            .text_size(text::CAPTION)
                            .text_color(theme.faint)
                            .child(activity::ago(recent.now, signed.time)),
                    )
                    .into_any_element()
            })
            .collect(),
    };
    section(
        "members-activity",
        "Recent activity",
        format!("last {window}"),
        body,
        theme,
    )
}
