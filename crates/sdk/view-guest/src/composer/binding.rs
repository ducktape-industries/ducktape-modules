//! Guest-owned composer projection and its GPUI presentation.

use super::{Draft, MentionChoice};
use crate::context::Callback;
use crate::prelude::*;
use crate::{
    wire, App, EditorDocumentUpdate, EditorElement, EditorElementEvent, EditorTransaction, View,
};
use std::rc::Rc;

#[path = "binding_editor.rs"]
mod binding_editor;
#[cfg(test)]
pub(crate) use binding_editor::key_tag;
use binding_editor::{editor, matching_choices};

#[derive(Clone, Debug)]
pub struct Change {
    pub before: String,
    pub after: String,
    pub cursor: wire::EditorCursor,
    pub tag: String,
}

pub enum Event<V> {
    Document(EditorDocumentUpdate),
    Transaction(EditorTransaction<Callback<V>>),
    Committed(Change),
    Action(String),
}

impl<V> Clone for Event<V> {
    fn clone(&self) -> Self {
        match self {
            Self::Document(update) => Self::Document(update.clone()),
            Self::Transaction(transaction) => Self::Transaction(transaction.clone()),
            Self::Committed(change) => Self::Committed(change.clone()),
            Self::Action(action) => Self::Action(action.clone()),
        }
    }
}

pub enum Outcome<V> {
    Updated,
    Run(Callback<V>),
    Action(String),
    Enqueue(String),
}

pub type Handle<V> = Rc<dyn Fn(&mut V, Event<V>, &mut Window, &mut Context<V>)>;
type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

impl Draft {
    pub fn handle<V: 'static>(
        &mut self,
        event: Event<V>,
        choices: &[MentionChoice],
        cx: &mut App,
    ) -> Outcome<V> {
        match event {
            Event::Document(update) => {
                update.apply(&mut self.editor, cx);
                Outcome::Updated
            }
            Event::Transaction(transaction) => transaction
                .apply(&mut self.editor, cx)
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
                    "send" | "paste" | "copy" | "cut" | "restore" => Outcome::Action(change.tag),
                    _ => Outcome::Updated,
                }
            }
            Event::Action(tag) => Outcome::Enqueue(tag),
        }
    }
}

const TEXT_INSET: f32 = design::spacing::MD as f32;
const CONTROL_INSET: f32 = design::spacing::XXS as f32;

/// Dresses a mark's sign as what it does: bold, italic, code, quote, list.
type Face = fn(crate::Div) -> crate::Div;

#[derive(IntoElement)]
struct Mark {
    id: ElementId,
    sign: SharedString,
    label: SharedString,
    face: Face,
    on_click: Option<Click>,
}

impl RenderOnce for Mark {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let sign = (self.face)(div()).child(self.sign);
        let mut mark = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(self.label)
            .aria_disabled(self.on_click.is_none())
            .flex()
            .items_center()
            .justify_center()
            .size(px(design::height::CONTROL as f32))
            .text_size(px(design::type_scale::BODY as f32))
            .text_color(theme.muted)
            .child(sign);
        if let Some(on_click) = self.on_click {
            mark = mark
                .hover(move |style| style.bg(theme.surface_raised).text_color(theme.foreground))
                .on_click(on_click);
        }
        mark
    }
}

#[derive(IntoElement)]
struct ActionButton {
    id: ElementId,
    label: SharedString,
    primary: bool,
    on_click: Option<Click>,
}

impl RenderOnce for ActionButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        // A primary button that can't act yet reads as resting, not as ink.
        let (background, foreground, border) = match (self.primary, self.on_click.is_some()) {
            (true, true) => (theme.primary, theme.primary_foreground, theme.primary),
            (true, false) => (theme.surface_raised, theme.muted, theme.surface_raised),
            (false, _) => (theme.surface, theme.foreground, theme.border),
        };
        let mut button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .aria_disabled(self.on_click.is_none())
            .flex()
            .items_center()
            .justify_center()
            .h(px(design::height::CONTROL as f32))
            .px_2()
            .border_1()
            .border_color(border)
            .bg(background)
            .text_color(foreground)
            .text_size(px(design::type_scale::SECONDARY as f32))
            .child(self.label);
        if let Some(on_click) = self.on_click {
            button = button.on_click(on_click);
        }
        button
    }
}

#[derive(IntoElement)]
struct MentionItem {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_click: Option<Click>,
}

impl RenderOnce for MentionItem {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let mut row = div()
            .id(self.id)
            .role(Role::MenuItem)
            .aria_label(self.label.clone())
            .aria_selected(self.selected)
            .w_full()
            .flex()
            .items_center()
            .min_h(px(design::height::ROW as f32))
            .px_2()
            .bg(if self.selected {
                theme.accent_soft
            } else {
                theme.background
            })
            .text_color(if self.selected {
                theme.accent_foreground
            } else {
                theme.foreground
            })
            .text_size(px(design::type_scale::BODY as f32))
            .child(self.label);
        if let Some(on_click) = self.on_click {
            row = row.on_click(on_click);
        }
        row
    }
}

fn press<V: View + 'static>(
    editable: bool,
    tag: String,
    handle: &Handle<V>,
    cx: &Context<V>,
) -> Option<Click> {
    if !editable {
        return None;
    }
    let handle = handle.clone();
    Some(Box::new(cx.listener(
        move |view, _: &ClickEvent, window, cx| {
            handle(view, Event::Action(tag.clone()), window, cx);
            cx.notify();
        },
    )))
}

#[allow(clippy::too_many_arguments)]
pub fn view<V: View + 'static>(
    draft: &Draft,
    key: &str,
    hint: &str,
    // what the commit button says: "Send" for a new message, "Save" for an
    // edit; the composer does not guess from the draft
    commit: &str,
    editable: bool,
    choices: &[MentionChoice],
    cx: &mut Context<V>,
    handle: impl Fn(&mut V, Event<V>, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let handle: Handle<V> = Rc::new(handle);
    let editor = editor(
        draft,
        &format!("{key}/editor"),
        key,
        hint,
        editable,
        choices,
        handle.clone(),
        cx.global::<Theme>().accent,
    );
    let mut rows: Vec<AnyElement> = Vec::new();

    if let Some((_, partial)) = draft.query(draft.editor.state_view()) {
        let matches = matching_choices(choices, &partial);
        let selected = draft.menu_index.min(matches.len().saturating_sub(1));
        let menu = matches
            .into_iter()
            .enumerate()
            .map(|(index, choice)| MentionItem {
                id: ElementId::Name(format!("{key}/mention/{}", choice.token).into()),
                label: format!("@{}", choice.label).into(),
                selected: index == selected,
                on_click: press(editable, format!("mention:{}", choice.token), &handle, cx),
            })
            .collect::<Vec<_>>();
        if !menu.is_empty() {
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(menu)
                    .into_any_element(),
            );
        }
    }
    rows.push(editor.into_any_element());

    if !draft.note.is_empty() {
        rows.push(
            div()
                .mx(px(TEXT_INSET))
                .text_sm()
                .text_color(cx.global::<Theme>().danger)
                .child(draft.note.clone())
                .into_any_element(),
        );
    }
    if draft.failed_send.is_some() {
        rows.push(
            div()
                .mx(px(TEXT_INSET))
                .flex()
                .items_center()
                .gap_2()
                .p_2()
                .bg(cx.global::<Theme>().danger_soft)
                .text_color(cx.global::<Theme>().danger)
                .child(div().flex_1().child("An earlier message wasn’t sent"))
                .child(ActionButton {
                    id: ElementId::Name(format!("{key}/restore").into()),
                    label: "Restore".into(),
                    primary: false,
                    on_click: press(editable, "restore".into(), &handle, cx),
                })
                .into_any_element(),
        );
    }

    let mut toolbar = div()
        .mx(px(CONTROL_INSET))
        .flex()
        .items_center()
        .gap(px(2.));
    let faces: [(&str, &str, &str, Face); 5] = [
        ("B", "Bold", "bold", |sign| {
            sign.font_weight(crate::FontWeight::BOLD)
        }),
        ("I", "Italic", "italic", |sign| sign.italic()),
        ("</>", "Code", "code", |sign| {
            sign.font_family(design::fonts::FAMILY_MONO)
                .text_size(crate::design::text::CAPTION)
        }),
        ("“", "Quote", "quote", |sign| sign.text_size(px(16.))),
        ("•", "List", "list", |sign| {
            sign.text_size(px(22.)).font_weight(crate::FontWeight::BOLD)
        }),
    ];
    for (sign, label, tag, face) in faces {
        toolbar = toolbar.child(Mark {
            id: ElementId::Name(format!("{key}/{tag}").into()),
            sign: sign.into(),
            label: label.into(),
            face,
            on_click: press(editable, tag.into(), &handle, cx),
        });
    }
    let sendable = editable && draft.can_send(draft.editor.state_view().text);
    toolbar = toolbar.child(div().flex_1()).child(ActionButton {
        id: ElementId::Name(format!("{key}/send").into()),
        label: commit.to_owned().into(),
        primary: true,
        on_click: press(sendable, "send".into(), &handle, cx),
    });
    rows.push(toolbar.into_any_element());

    div()
        .id(ElementId::Name(key.into()))
        .flex()
        .flex_col()
        .gap_1()
        .border_1()
        .border_color(cx.global::<Theme>().border)
        .bg(cx.global::<Theme>().background)
        .pb(px(CONTROL_INSET))
        .children(rows)
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;
