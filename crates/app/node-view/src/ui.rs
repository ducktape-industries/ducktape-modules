//! The Nodes screen: a header with the counts, then the set in one of its
//! four states. `render` reads the state and changes nothing.
use abi::hex;
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{Div, FontWeight};
use valset::{Membership, Role as Standing};

use crate::Nodes;

/// The validator's place in the set, before its key.
const PLACE_W: Pixels = px(28.);
/// The widest a member's address runs before it is clipped.
const ADDRESS_W: Pixels = px(220.);

pub(crate) fn render(view: &Nodes, cx: &mut Context<Nodes>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    div()
        .id("nodes")
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(
            div()
                .id("nodes-head")
                .flex()
                .items_center()
                .gap_2()
                .child(design::heading("nodes-title", "Nodes", 1, &theme).flex_1())
                .child(
                    div()
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted)
                        .child(count(view)),
                ),
        )
        .child(body(view, cx, &theme))
}

fn count(view: &Nodes) -> String {
    match view.set.ready() {
        Some(set) => format!(
            "{} · {}",
            design::plural(set.validators.len() as u64, "validator", "validators"),
            design::plural(set.members.len() as u64, "member", "members"),
        ),
        None => String::new(),
    }
}

/// The four states of the set: loading, refused, empty, ready.
fn body(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme) -> AnyElement {
    match &view.set {
        Loadable::Idle | Loadable::Loading(_) => design::quiet("Reading the validator set…", theme)
            .id("nodes-loading")
            .into_any_element(),
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.read(cx));
            design::refused("nodes", refusal.message.clone(), theme, retry).into_any_element()
        }
        Loadable::Ready(set) if set.members.is_empty() && set.validators.is_empty() => {
            design::empty_state(
                "nodes-empty",
                "No members",
                "The validator set of this network is empty.",
                theme,
            )
            .into_any_element()
        }
        Loadable::Ready(set) => div()
            .id("nodes-list")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("nodes-set-header", "Validator set", theme))
            .child(validators(&set.validators, theme))
            .child(section("nodes-members-header", "Memberships", theme))
            .child(members(&set.members, theme))
            .children(
                set.more
                    .then(|| design::more_not_shown("nodes-more", theme)),
            )
            .into_any_element(),
    }
}

/// A section's heading, quieter than the title.
fn section(id: &'static str, label: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Heading)
        .aria_level(2)
        .h(design::size::CONTROL)
        .flex()
        .items_center()
        .pl_2()
        .pr_1()
        .text_size(design::text::SECONDARY)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted)
        .child(label)
}

fn validators(validators: &[Vec<u8>], theme: &Theme) -> AnyElement {
    if validators.is_empty() {
        return design::quiet("No key validates on this network.", theme)
            .id("nodes-no-validators")
            .into_any_element();
    }
    div()
        .id("nodes-validators")
        .flex()
        .flex_col()
        .gap_2()
        .children(validators.iter().enumerate().map(|(index, key)| {
            div()
                .id(ElementId::named_usize("nodes-validator", index))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(PLACE_W)
                        .text_size(design::text::SECONDARY)
                        .text_color(theme.muted)
                        .child((index + 1).to_string()),
                )
                .child(key_text(key).flex_1())
        }))
        .into_any_element()
}

fn members(members: &[Membership], theme: &Theme) -> impl IntoElement {
    div()
        .id("nodes-members")
        .flex()
        .flex_col()
        .gap_2()
        .children(members.iter().enumerate().map(|(index, member)| {
            div()
                .id(ElementId::named_usize("nodes-member", index))
                .flex()
                .items_center()
                .gap_2()
                .child(key_text(&member.key).flex_1())
                .child(
                    div()
                        .max_w(ADDRESS_W)
                        .truncate()
                        .text_size(design::text::SECONDARY)
                        .child(member.address.clone()),
                )
                .child(standing(index, member.role, theme))
        }))
}

/// A key, shortened: never raw.
fn key_text(key: &[u8]) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .text_size(design::text::SECONDARY)
        .child(design::short_hex(&hex(key)))
}

/// Whether a member validates or only resides, as a tag.
fn standing(index: usize, role: Standing, theme: &Theme) -> impl IntoElement {
    let (label, foreground, background) = match role {
        Standing::Validator => ("Validator", theme.success, theme.success_soft),
        Standing::Resident => ("Resident", theme.muted, theme.surface_raised),
    };
    design::badge(
        ElementId::named_usize("nodes-standing", index),
        label,
        foreground,
        background,
    )
}
