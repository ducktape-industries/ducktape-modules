//! The message menus: the floating "…" list, the reaction picker and the
//! delete confirmation at the pointer; the edit composer under its stream.
use ducktape_view_guest::view::Cx;
use ducktape_view_guest::wire::{self, AlignX, ButtonPreset, Length, Node, kit};

use super::el::{El, subtle, tall_glyph};
use crate::client::reaction_palette;
use crate::{Chat, Menu, Mode, Pane};

const MENU_ITEM_HEIGHT: f32 = kit::height::CONTROL as f32;
const MENU_ITEM_GAP: f32 = 2.;
const MENU_INSET: f32 = kit::spacing::XS as f32;
const PICKER_COLUMNS: u32 = 8;
pub const PICKER_CELL: f32 = 32.;
const PICKER_GAP: f32 = 2.;
const PICKER_INSET: f32 = kit::spacing::SM as f32;

fn prefix(pane: Pane) -> &'static str {
    match pane {
        Pane::Timeline => "chat/room/message-",
        Pane::Thread => "chat/thread/thread-",
    }
}

/// The frame the host focuses when a menu opens.
pub fn focus_key(pane: Pane, mode: Mode) -> String {
    let prefix = prefix(pane);
    let focus = match mode {
        Mode::Reactions => "reaction-focus",
        Mode::Delete => "delete-focus",
        _ => "action-focus",
    };
    format!("{prefix}{focus}")
}

/// The menu that floats at the pointer. An edit is not one — it sits under
/// its stream (see `editing`).
pub fn floating(chat: &Chat, cx: &mut Cx<Chat>) -> Option<Node> {
    let menu = chat.menu.as_ref()?;
    let size = match menu.mode {
        Mode::More => menu_size(if menu.pane == Pane::Thread { 4 } else { 5 }),
        Mode::Reactions => picker_size(reaction_palette().len()),
        Mode::Delete => (280., 96.),
        Mode::Toolbar | Mode::Editing => return None,
    };
    let (x, y) = origin(menu.at, size, chat.layout.viewport);
    let shade = if kit::is_dark() { 0.5 } else { 0.16 };
    Some(Node::Float {
        key: "chat/floating-menu".into(),
        x,
        y,
        scale: 1.,
        shadow: wire::Shadow {
            color: Some(wire::Rgba([0., 0., 0., shade])),
            x: Some(0.),
            y: Some(4.),
            blur: Some(16.),
        },
        radius: Some([kit::radius::CARD as f32; 4]),
        content: Box::new(
            El(message_menu(chat, menu, cx))
                .w(Length::Fixed(size.0))
                .node(),
        ),
    })
}

/// The edit composer, when the open menu is an edit in `pane`.
pub fn editing(chat: &Chat, pane: Pane, cx: &mut Cx<Chat>) -> Option<Node> {
    let menu = chat.menu.as_ref()?;
    (menu.mode == Mode::Editing && menu.pane == pane).then(|| message_menu(chat, menu, cx))
}

fn message_menu(chat: &Chat, menu: &Menu, cx: &mut Cx<Chat>) -> Node {
    let (pane, seq, rev) = (menu.pane, menu.seq, menu.rev);
    let key = focus_key(pane, menu.mode);
    let prefix = prefix(pane);
    let writable = chat.may_write();
    let mut children = Vec::new();
    match menu.mode {
        Mode::Toolbar | Mode::More => {
            let mut items = Vec::new();
            if pane == Pane::Timeline {
                let open = cx.on(move |chat, cx| chat.open_thread(seq, cx));
                items.push(item(
                    format!("{prefix}reply"),
                    "↩",
                    "Reply in thread",
                    Some(open),
                ));
            }
            let link = chat.message_link(seq);
            let copy = (!link.is_empty()).then(|| {
                cx.on(move |chat, cx| {
                    chat.close_menu();
                    chat.copy_text(link.clone(), "Message link copied", cx);
                })
            });
            let react = writable.then(|| {
                cx.on(move |chat, cx| chat.open_menu(pane, seq, rev, Mode::Reactions, cx))
            });
            let edit = writable
                .then(|| cx.on(move |chat, cx| chat.open_menu(pane, seq, rev, Mode::Editing, cx)));
            let delete = writable
                .then(|| cx.on(move |chat, cx| chat.open_menu(pane, seq, rev, Mode::Delete, cx)));
            items.extend([
                item(format!("{prefix}add-reaction"), "😀", "Add reaction", react),
                item(format!("{prefix}copy-link"), "🔗", "Copy link", copy),
                item(format!("{prefix}edit"), "✎", "Edit message", edit),
                item(format!("{prefix}delete"), "🗑", "Delete message", delete),
            ]);
            children.push(
                El::column(format!("{prefix}menu-actions"), items)
                    .gap(MENU_ITEM_GAP)
                    .node(),
            );
        }
        Mode::Reactions => {
            let cells = reaction_palette()
                .into_iter()
                .map(|emoji| {
                    let press = writable
                        .then(|| cx.on(move |chat, cx| chat.react(seq, emoji.into(), true, cx)));
                    cell(format!("{prefix}reaction/{emoji}"), emoji, press)
                })
                .collect();
            let columns = PICKER_COLUMNS as f32;
            children.push(Node::Grid {
                key: format!("{prefix}reaction-grid"),
                columns: Some(PICKER_COLUMNS),
                fluid: None,
                spacing: Some(PICKER_GAP),
                padding: None,
                width: Some(Length::Fixed(
                    columns * PICKER_CELL + (columns - 1.) * PICKER_GAP,
                )),
                height: None,
                aspect: None,
                background: None,
                border: None,
                children: cells,
            });
        }
        Mode::Editing => {
            let target = crate::composer::send::Target::Edit {
                channel: chat.room_id(),
                seq,
                base_rev: rev,
            };
            children.push(super::room::composer(
                chat,
                target,
                "Edit message",
                !chat.session.busy,
                cx,
            ));
            let close = cx.on(|chat, _| chat.close_menu());
            children.push(
                El::column(
                    format!("{prefix}close-row"),
                    [subtle(
                        format!("{prefix}close"),
                        "Cancel message edit",
                        Some(close),
                    )],
                )
                .align_x(AlignX::Right)
                .node(),
            );
        }
        Mode::Delete => {
            let close = cx.on(|chat, _| chat.close_menu());
            let confirm = (!chat.session.busy).then(|| cx.on(|chat, cx| chat.delete_armed(cx)));
            children.push(kit::strong(
                format!("{prefix}confirm"),
                "Delete this message?",
            ));
            children.push(kit::wrapping(kit::secondary(
                format!("{prefix}confirm-detail"),
                "It leaves the room for everyone.",
            )));
            children.push(
                El::row(
                    format!("{prefix}confirm-row"),
                    [
                        kit::spacer(),
                        subtle(format!("{prefix}close"), "Cancel", Some(close)),
                        kit::button(
                            format!("{prefix}confirm-delete"),
                            "Delete",
                            confirm,
                            ButtonPreset::Danger,
                        ),
                    ],
                )
                .gap(kit::spacing::XS as f32)
                .align_x(AlignX::Right)
                .node(),
            );
        }
    }
    let inset = match menu.mode {
        Mode::Toolbar | Mode::More => MENU_INSET,
        Mode::Reactions => PICKER_INSET,
        Mode::Editing | Mode::Delete => kit::spacing::LG as f32,
    };
    let mut frame = kit::card(
        key,
        El::column(format!("{prefix}menu"), children)
            .gap(kit::spacing::SM as f32)
            .node(),
    );
    if let Node::Container { padding, .. } = &mut frame {
        *padding = Some(wire::Edges::all(inset));
    }
    match menu.mode {
        Mode::Editing => El(frame).pad_xy(16., kit::spacing::XXS as f32).node(),
        _ => frame,
    }
}

fn menu_size(items: usize) -> (f32, f32) {
    let rows = items as f32;
    (
        220.,
        MENU_INSET * 2. + rows * MENU_ITEM_HEIGHT + (rows - 1.).max(0.) * MENU_ITEM_GAP,
    )
}

fn picker_size(count: usize) -> (f32, f32) {
    let columns = PICKER_COLUMNS as f32;
    let rows = (count as f32 / columns).ceil();
    (
        PICKER_INSET * 2. + columns * PICKER_CELL + (columns - 1.) * PICKER_GAP,
        PICKER_INSET * 2. + rows * PICKER_CELL + (rows - 1.).max(0.) * PICKER_GAP,
    )
}

/// Where a menu of `size` sits for a press inside `viewport`: its top-left at
/// the pointer, flipped left or up at an edge, never past the corner.
pub fn origin(press: (f32, f32), size: (f32, f32), viewport: (f32, f32)) -> (f32, f32) {
    const GUTTER: f32 = 8.;
    let x = if press.0 + size.0 + GUTTER <= viewport.0 {
        press.0
    } else {
        press.0 - size.0
    };
    let y = if press.1 + size.1 + GUTTER <= viewport.1 {
        press.1 + 4.
    } else {
        press.1 - size.1 - 4.
    };
    (x.max(GUTTER), y.max(GUTTER))
}

/// One row of a dropdown: a glyph, then the words, left-aligned.
fn item(key: String, glyph: &str, label: &str, on_press: Option<u32>) -> Node {
    let content = El::centered_row(
        format!("{key}/row"),
        [
            El(kit::nowrap(tall_glyph(
                format!("{key}/glyph"),
                glyph,
                kit::type_scale::BODY as f32,
                MENU_ITEM_HEIGHT,
            )))
            .w(Length::Fixed(20.))
            .node(),
            kit::nowrap(kit::text(format!("{key}/label"), label)),
        ],
    )
    .gap(kit::spacing::SM as f32)
    .node();
    let mut button = kit::button_child(key, content, on_press, ButtonPreset::Subtle);
    if let Node::Button {
        label: accessible,
        width,
        height,
        padding,
        ..
    } = &mut button
    {
        *accessible = Some(label.into());
        *width = Some(Length::Fill);
        *height = Some(Length::Fixed(MENU_ITEM_HEIGHT));
        *padding = Some(wire::Edges {
            top: 0.,
            right: kit::spacing::SM as f32,
            bottom: 0.,
            left: kit::spacing::SM as f32,
        });
    }
    button
}

/// One cell of the picker: a fixed square with the emoji's full glyph.
fn cell(key: String, emoji: &str, on_press: Option<u32>) -> Node {
    let glyph = tall_glyph(
        format!("{key}/glyph"),
        emoji,
        kit::type_scale::TITLE as f32,
        PICKER_CELL,
    );
    let mut button = kit::button_child(key, glyph, on_press, ButtonPreset::Subtle);
    if let Node::Button {
        label,
        description,
        width,
        height,
        padding,
        ..
    } = &mut button
    {
        *label = Some("Add reaction".into());
        *description = Some(emoji.into());
        *width = Some(Length::Fixed(PICKER_CELL));
        *height = Some(Length::Fixed(PICKER_CELL));
        *padding = Some(wire::Edges::all(0.));
    }
    button
}
