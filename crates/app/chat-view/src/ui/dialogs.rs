//! What opens over the screen: the channel-creation card and the attachment
//! preview.
use ducktape_view_guest::view::{Cx, Loaded};
use ducktape_view_guest::wire::{Length, Node, SurfaceValue, kit, kit::Tone};

use crate::Chat;
use ducktape_view_guest::wire::kit::*;

pub fn channel_create(chat: &Chat, cx: &mut Cx<Chat>) -> Option<Node> {
    let create = chat.create.as_ref()?;
    let key = "chat/create";
    let busy = create.busy;
    let typed = cx.on_value(|chat, text: String, _| {
        if let Some(c) = &mut chat.create {
            c.name = text;
        }
    });
    let submit = cx.on(|chat, cx| chat.create_channel(cx));
    let voice = cx.on(|chat, _| {
        if let Some(c) = &mut chat.create {
            c.voice = !c.voice;
        }
    });
    let members = cx.on(|chat, _| {
        if let Some(c) = &mut chat.create
            && !c.voice
        {
            c.members_only = !c.members_only;
        }
    });
    let cancel = cx.on(|chat, _| chat.create = None);
    let mut children = vec![
        kit::heading(format!("{key}/title"), "Create a channel"),
        text_field(
            format!("{key}/name"),
            "Channel name",
            &create.name,
            typed,
            Some(submit),
            busy,
        ),
        action(
            format!("{key}/voice"),
            if create.voice {
                "Voice room: On"
            } else {
                "Voice room: Off"
            },
            (!busy).then_some(voice),
        ),
        action(
            format!("{key}/members"),
            if create.members_only {
                "Members only: On"
            } else {
                "Members only: Off"
            },
            (!busy && !create.voice).then_some(members),
        ),
    ];
    if !create.error.is_empty() {
        children.push(kit::tone_text(
            format!("{key}/error"),
            &create.error,
            Tone::Danger,
        ));
    }
    children.push(kit::row(
        format!("{key}/actions"),
        [
            subtle(format!("{key}/cancel"), "Cancel", (!busy).then_some(cancel)),
            gated(
                primary(
                    format!("{key}/submit"),
                    "Create channel",
                    (!busy && chat.session.connected && !chat.session.busy).then_some(submit),
                ),
                chat.session.holds_account(),
                "Create an account to create a channel",
            ),
        ],
    ));
    let card = kit::card(
        format!("{key}/card"),
        kit::spaced(kit::column(key, children), kit::spacing::SM as f32),
    );
    Some(width(padded_all(card, 20.), Length::Fixed(480.)))
}

/// The file pressed, shown where the reader is: a picture at the size the
/// screen allows, a text file's head in a code plate, or the plate that says
/// there is nothing to show.
pub fn preview(chat: &Chat, cx: &mut Cx<Chat>) -> Option<Node> {
    let preview = chat.preview.as_ref()?;
    let key = "chat/preview";
    let link = preview.link.clone();
    let path = crate::files::attachment_file_path(&link);
    let name = path.rsplit('/').next().unwrap_or_default().to_owned();
    let open = cx.on(move |chat, _| chat.open_link(link.clone()));
    let close = cx.on(|chat, _| chat.preview = None);
    let header = kit::spaced(
        kit::centered_row(
            format!("{key}/header"),
            [
                fill_width(kit::nowrap(kit::strong(format!("{key}/name"), &name))),
                subtle(format!("{key}/open-in-files"), "Open in Files", Some(open)),
                glyph(format!("{key}/close"), "✕", "Close preview", Some(close)),
            ],
        ),
        kit::spacing::SM as f32,
    );
    let body = preview_body(chat, key, &path, cx);
    Some(padded_all(
        kit::spaced(kit::column(key, [header, body]), kit::spacing::MD as f32),
        14.,
    ))
}

fn preview_body(chat: &Chat, key: &str, path: &str, cx: &mut Cx<Chat>) -> Node {
    let preview = chat.preview.as_ref().expect("a preview");
    let screen = chat.layout.viewport;
    if let Some(&(w, h)) = chat.pictures.get(&preview.link)
        && w > 0
        && h > 0
    {
        let (bw, bh) = crate::files::preview_box(w, h, screen);
        let surface = Node::Surface {
            key: format!("{key}/picture"),
            name: "picture".into(),
            args: vec![
                SurfaceValue::Str(crate::files::PICTURE_SURFACE.into()),
                SurfaceValue::Str(path.to_owned()),
            ],
            on_event: None,
        };
        return height(
            width(
                kit::container(format!("{key}/frame"), surface),
                Length::Fixed(bw),
            ),
            Length::Fixed(bh),
        );
    }
    let (plate_width, plate_height) = crate::files::preview_room(screen);
    let read = match &preview.read {
        Loaded::Failed(refusal) => {
            return kit::notice(
                format!("{key}/failed"),
                kit::wrapping(kit::text(
                    format!("{key}/reason"),
                    format!("Could not read this file: {}", refusal.sentence),
                )),
                Tone::Danger,
            );
        }
        Loaded::Ready(read) => read,
        _ => return kit::secondary(format!("{key}/reading"), "Reading the file…"),
    };
    if read.binary {
        return kit::empty_state(
            format!("{key}/binary"),
            "No preview",
            crate::files::BINARY_PLATE,
        );
    }
    // binary-or-text is the wire's call; markdown-vs-code is the path's
    let dark = chat.session.dark;
    let document = if crate::files::markdown_path(path) {
        let on_link = cx.on_value(|chat, value: SurfaceValue, _| {
            if let SurfaceValue::Str(link) = value {
                chat.open_link(link);
            }
        });
        Node::Surface {
            key: format!("{key}/markdown"),
            name: "markdown".into(),
            args: vec![
                SurfaceValue::Str(read.text.clone()),
                SurfaceValue::Str(String::new()),
                SurfaceValue::Bool(dark),
            ],
            on_event: Some(on_link),
        }
    } else {
        Node::Surface {
            key: format!("{key}/code"),
            name: "code".into(),
            args: vec![
                SurfaceValue::Str(read.text.clone()),
                SurfaceValue::Str(path.to_owned()),
                SurfaceValue::Bool(dark),
            ],
            on_event: None,
        }
    };
    let mut children = vec![fill(kit::container(format!("{key}/plate"), document))];
    if read.clipped {
        children.push(kit::caption(
            format!("{key}/clipped"),
            "Only the beginning is shown here. Open in Files for the whole file.",
        ));
    }
    height(
        width(
            kit::spaced(
                kit::column(format!("{key}/document"), children),
                kit::spacing::XS as f32,
            ),
            Length::Fixed(plate_width),
        ),
        Length::Fixed(plate_height),
    )
}
