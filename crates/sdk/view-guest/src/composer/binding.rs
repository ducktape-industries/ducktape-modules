//! Guest controls enqueue actions into the same native editor transaction stream.
use super::{Draft, MentionChoice};
use crate::context::Callback;
use crate::{Context, Window};
use crate::{EditorDocumentUpdate, EditorTransaction, kit, wire};
use std::rc::Rc;
#[path = "binding_editor.rs"]
mod editor;
use editor::{editor, matching_choices};

#[derive(Clone, Debug)]
pub struct Change {
    pub before: String,
    pub after: String,
    pub cursor: wire::EditorCursor,
    pub tag: String,
}

/// What the field and the controls hand the view; `V` is the view the
/// draft lives in, whose `Context` registered the handlers.
#[derive(Clone, Debug)]
pub enum Event<V> {
    Document(EditorDocumentUpdate),
    Transaction(EditorTransaction<Callback<V>>),
    Committed(Change),
    Action(String),
}

/// What handling an event asks of the view.
pub enum Outcome<V> {
    Updated,
    /// an editor transaction's own follow-up, to run on the view
    Run(Callback<V>),
    Action(String),
    Enqueue(String),
}

/// The handler a composer site gives: the view, the event, the runtime.
pub type Handle<V> = Rc<dyn Fn(&mut V, Event<V>, &mut Window, &mut Context<V>)>;

impl Draft {
    pub fn handle<V: 'static>(&mut self, event: Event<V>, choices: &[MentionChoice]) -> Outcome<V> {
        match event {
            Event::Document(update) => {
                update.apply(&mut self.editor);
                Outcome::Updated
            }
            Event::Transaction(transaction) => transaction
                .apply(&mut self.editor)
                .map_or(Outcome::Updated, Outcome::Run),
            Event::Committed(change) => {
                self.committed(
                    &change.before,
                    &change.after,
                    change.cursor,
                    &change.tag,
                    choices,
                );
                match change.tag.as_str() {
                    "send" | "attach" | "paste" | "copy" | "cut" | "restore" => {
                        Outcome::Action(change.tag)
                    }
                    _ => {
                        let attachment_action =
                            change.tag.starts_with("remove:") || change.tag.starts_with("retry:");
                        if attachment_action {
                            Outcome::Action(change.tag)
                        } else {
                            Outcome::Updated
                        }
                    }
                }
            }
            Event::Action(tag) => Outcome::Enqueue(tag),
        }
    }
}

/// Where the draft's text starts, from the field's left edge: the native
/// field pads its own text this far, and every row under it lines up there.
const TEXT_INSET: f32 = kit::spacing::MD as f32;
/// A row of controls stops short of that line, because a square control
/// centres its sign and so carries the rest of the distance inside its own
/// box. Aligning the BOXES would push every sign a glyph's width to the
/// right of the draft's first letter.
const CONTROL_INSET: f32 = kit::spacing::XXS as f32;
/// A mark button is a square holding one sign, and tall enough that the
/// host's button does not clip the sign to its line box.
const MARK: f32 = 24.;

/// One mark a draft can carry: a quiet square holding a single typographic
/// sign. The sign is what a reader sees; `name` is what a screen reader
/// hears, since a sign is not a word.
fn mark(key: String, sign: &str, name: &str, on_press: Option<u32>) -> wire::Node {
    let mut button = kit::button_child(
        key.clone(),
        kit::nowrap(kit::text_options(
            kit::text_size(
                kit::text(format!("{key}/sign"), sign),
                kit::type_scale::BODY as f32,
            ),
            wire::TextOptions {
                line_height: Some(wire::LineHeight::Absolute(MARK)),
                ..Default::default()
            },
        )),
        on_press,
        wire::ButtonPreset::Subtle,
    );
    let wire::Node::Button {
        label,
        width,
        height,
        padding,
        ..
    } = &mut button
    else {
        unreachable!()
    };
    *label = Some(name.into());
    *width = Some(wire::Length::Fixed(MARK));
    *height = Some(wire::Length::Fixed(MARK));
    *padding = Some(wire::Edges::all(0.));
    button
}

/// A file the draft is carrying: its name over what became of it, and the
/// way to take it back out — one chip, not three loose lines.
fn chip(key: &str, name: &str, note: &str, tone: kit::Tone, remove: Option<u32>) -> wire::Node {
    let p = kit::palette();
    let mut body = kit::spaced(
        kit::column(
            format!("{key}/body"),
            [
                kit::nowrap(kit::weighted(
                    kit::text_size(
                        kit::text(format!("{key}/name"), name),
                        kit::type_scale::SECONDARY as f32,
                    ),
                    wire::Weight::Medium,
                )),
                kit::nowrap(kit::colored(
                    kit::text_size(
                        kit::text(format!("{key}/note"), note),
                        kit::type_scale::CAPTION as f32,
                    ),
                    tone.color(p),
                )),
            ],
        ),
        1.,
    );
    body = kit::sized(body, Some(wire::Length::Shrink), None);
    let mut chip = kit::container(
        format!("{key}/chip"),
        kit::spaced(
            kit::centered_row(
                format!("{key}/row"),
                [body, mark(format!("{key}/remove"), "×", "Remove", remove)],
            ),
            kit::spacing::XXS as f32,
        ),
    );
    let wire::Node::Container {
        border,
        background,
        padding,
        width,
        ..
    } = &mut chip
    else {
        unreachable!()
    };
    *border = Some(wire::Border {
        color: Some(kit::rgba(p.border)),
        width: Some(1.),
        radius: Some([kit::radius::CONTROL as f32; 4]),
    });
    *background = Some(wire::Background::Color(kit::rgba(p.surface)));
    *padding = Some(wire::Edges {
        top: kit::spacing::XXS as f32,
        right: kit::spacing::XXS as f32,
        bottom: kit::spacing::XXS as f32,
        left: kit::spacing::SM as f32,
    });
    *width = Some(wire::Length::Shrink);
    chip
}

/// The draft, everything it carries, and the row that sends it — one
/// plate reading down to a single action on the right.
///
/// The plate is drawn HERE. The host mounts the field as a bare text
/// surface with no border and no fill of its own (verified on a live app,
/// 2026-09-16), so a composer that draws nothing is a placeholder and a
/// row of controls floating loose on the timeline's own background, which
/// is what this replaced.
#[allow(clippy::too_many_arguments)]
pub fn view<V: 'static>(
    draft: &Draft,
    key: &str,
    hint: &str,
    editable: bool,
    attach: bool,
    choices: &[MentionChoice],
    cx: &mut Context<V>,
    handle: impl Fn(&mut V, Event<V>, &mut Window, &mut Context<V>) + 'static,
) -> wire::Node {
    let handle: Handle<V> = Rc::new(handle);
    let editor_key = format!("{key}/editor");
    let press = |tag: String| -> Option<u32> {
        let handle = handle.clone();
        editable.then(|| {
            cx.listener(move |view, _: &(), window, cx| {
                handle(view, Event::Action(tag.clone()), window, cx);
                cx.notify();
            })
        })
    };
    let text = draft.editor.state_view().text;
    let carries_file = draft
        .attachments
        .iter()
        .any(|held| matches!(held.state, super::AttachmentState::Ready { .. }));
    let waits_on_upload = draft
        .attachments
        .iter()
        .any(|held| held.state == super::AttachmentState::Uploading);
    // a send needs something to say, and waits for its files to land
    let sendable = editable && (!text.trim().is_empty() || carries_file) && !waits_on_upload;

    let mut rows = Vec::new();
    // The choices sit ABOVE the draft: picking one must not slide the row
    // of controls out from under the reader's pointer.
    if let Some((_, partial)) = draft.query(draft.editor.state_view()) {
        let matches = matching_choices(choices, &partial);
        let selected = draft.menu_index.min(matches.len().saturating_sub(1));
        let picks: Vec<wire::Node> = matches
            .into_iter()
            .enumerate()
            .map(|(index, choice)| {
                let row = format!("{key}/mention/{}", choice.token);
                // a handle reads from the left; the host centres a button's
                // child, so a spacer after the name pushes it back over
                kit::list_row(
                    row.clone(),
                    kit::row(
                        format!("{row}/row"),
                        [
                            kit::nowrap(kit::text(
                                format!("{row}/name"),
                                format!("@{}", choice.label),
                            )),
                            kit::spacer(),
                        ],
                    ),
                    index == selected,
                    press(format!("mention:{}", choice.token)),
                )
            })
            .collect();
        if !picks.is_empty() {
            rows.push(kit::spaced(
                kit::column(format!("{key}/mentions"), picks),
                1.,
            ));
        }
    }
    rows.push(editor(
        draft,
        &editor_key,
        key,
        hint,
        editable,
        choices,
        handle.clone(),
    ));
    if !draft.attachments.is_empty() {
        let chips: Vec<wire::Node> = draft
            .attachments
            .iter()
            .map(|held| {
                let at = format!("{key}/attachment/{}", held.token);
                let (note, tone) = match &held.state {
                    super::AttachmentState::Uploading => {
                        ("Uploading…".to_owned(), kit::Tone::Neutral)
                    }
                    super::AttachmentState::Ready { uri } => (uri.clone(), kit::Tone::Neutral),
                    super::AttachmentState::Failed { reason } => {
                        (reason.clone(), kit::Tone::Danger)
                    }
                    super::AttachmentState::Unavailable => {
                        ("Select the file again".to_owned(), kit::Tone::Warning)
                    }
                };
                let mut carried = vec![chip(
                    &at,
                    &held.name,
                    &note,
                    tone,
                    press(format!("remove:{}", held.token)),
                )];
                // a failure is the one state with a way out of it
                if matches!(held.state, super::AttachmentState::Failed { .. }) {
                    carried.push(kit::button(
                        format!("{at}/retry"),
                        "Retry",
                        press(format!("retry:{}", held.token)),
                        wire::ButtonPreset::Subtle,
                    ));
                }
                kit::spaced(
                    kit::centered_row(format!("{at}/held"), carried),
                    kit::spacing::XXS as f32,
                )
            })
            .collect();
        rows.push(inset(
            kit::spaced(
                kit::wrapped_row(format!("{key}/attachments"), chips),
                kit::spacing::XS as f32,
            ),
            TEXT_INSET,
        ));
    }
    if !draft.note.is_empty() {
        rows.push(inset(
            kit::row(
                format!("{key}/note-row"),
                [kit::wrapping(kit::text_size(
                    kit::tone_text(format!("{key}/note"), &draft.note, kit::Tone::Danger),
                    kit::type_scale::SECONDARY as f32,
                ))],
            ),
            TEXT_INSET,
        ));
    }
    if draft.failed_send.is_some() {
        rows.push(inset(
            kit::notice(
                format!("{key}/failed"),
                kit::spaced(
                    kit::centered_row(
                        format!("{key}/failed/row"),
                        [
                            kit::text(
                                format!("{key}/failed/note"),
                                "An earlier message wasn’t sent",
                            ),
                            kit::spacer(),
                            kit::button(
                                format!("{key}/restore"),
                                "Restore",
                                press("restore".into()),
                                wire::ButtonPreset::Subtle,
                            ),
                        ],
                    ),
                    kit::spacing::SM as f32,
                ),
                kit::Tone::Danger,
            ),
            0.,
        ));
    }
    // What the draft can carry, then the one action that sends it. Each
    // mark is a sign rather than a word: five words in a row read as a
    // sentence, five signs read as a toolbar.
    let mut controls = Vec::new();
    if attach {
        controls.push(mark(
            format!("{key}/attach"),
            "+",
            "Attach a file",
            press("attach".into()),
        ));
    }
    controls.extend([
        mark(format!("{key}/bold"), "B", "Bold", press("bold".into())),
        mark(
            format!("{key}/italic"),
            "I",
            "Italic",
            press("italic".into()),
        ),
        // Latin punctuation only: the product face carries it. A dingbat
        // quote mark (❞) or an angle-quote pair (‹›) falls out of Inter and
        // lands in whatever the system has, which is a tofu box on a host
        // with no fallback and an emoji on one that has too much.
        mark(format!("{key}/code"), "<>", "Code", press("code".into())),
        mark(format!("{key}/quote"), "”", "Quote", press("quote".into())),
        kit::spacer(),
    ]);
    controls.push(kit::button(
        format!("{key}/send"),
        "Send",
        sendable.then(|| {
            let handle = handle.clone();
            cx.listener(move |view, _: &(), window, cx| {
                handle(view, Event::Action("send".into()), window, cx);
                cx.notify();
            })
        }),
        wire::ButtonPreset::Primary,
    ));
    rows.push(inset(
        kit::spaced(kit::centered_row(format!("{key}/toolbar"), controls), 2.),
        CONTROL_INSET,
    ));
    plate(
        key,
        kit::spaced(
            kit::column(format!("{key}/rows"), rows),
            kit::spacing::XS as f32,
        ),
    )
}

/// The box the whole draft lives in: the window's own colour inside a
/// control's hairline. It pads nothing — the field pads its own text and
/// every other row reaches that line itself, so one inset governs.
fn plate(key: &str, child: wire::Node) -> wire::Node {
    let p = kit::palette();
    let mut node = kit::container(key, child);
    let wire::Node::Container {
        border,
        background,
        padding,
        ..
    } = &mut node
    else {
        unreachable!()
    };
    *border = Some(wire::Border {
        color: Some(kit::rgba(p.border_strong)),
        width: Some(1.),
        radius: Some([kit::radius::CARD as f32; 4]),
    });
    *background = Some(wire::Background::Color(kit::rgba(p.background)));
    *padding = Some(wire::Edges {
        top: 0.,
        right: 0.,
        bottom: CONTROL_INSET,
        left: 0.,
    });
    node
}

/// A row under the field, moved in to the line the draft's own text sits
/// on. The column's spacing owns the air between rows, so nothing here
/// pads its own top or bottom — two sources of vertical rhythm is how a
/// stack ends up with three different gaps in it.
fn inset(node: wire::Node, sides: f32) -> wire::Node {
    kit::padded(
        node,
        wire::Edges {
            top: 0.,
            right: sides,
            bottom: 0.,
            left: sides,
        },
    )
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;
