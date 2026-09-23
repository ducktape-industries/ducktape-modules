//! Event routes and sent pictures belong to one running driver.
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Weak};

type ClickRoute = Rc<dyn Fn(&gpui::ClickEvent, &mut crate::Window, &mut crate::App)>;
type TooltipRoute =
    Rc<dyn Fn(Option<usize>, &mut crate::Window, &mut crate::App) -> Option<crate::AnyView>>;
pub(crate) type TooltipBuilder =
    Box<dyn Fn(&mut crate::Window, &mut crate::App) -> crate::AnyView + 'static>;
pub(crate) type RichTextTooltipBuilder =
    Box<dyn Fn(usize, &mut crate::Window, &mut crate::App) -> Option<crate::AnyView> + 'static>;
type EditorReceiver = (
    crate::wire::editor_document::EditorTransferId,
    crate::wire::editor_document::EditorDocumentRef,
    crate::wire::editor_document::EditorTransferReceiver,
);
type EventHandler<A> = Rc<dyn Fn(&A, &mut crate::Window, &mut crate::App)>;

struct EventRoute<A>(EventHandler<A>);

#[derive(Default)]
struct Tables {
    identity: Arc<()>,
    editor_responses: Vec<crate::wire::EditorResponse>,
    editor_documents: Vec<crate::wire::editor_document::EditorDocumentMessage>,
    editor_sender: Option<crate::wire::editor_document::EditorTransferSender>,
    editor_receiver: Option<EditorReceiver>,
    editor_pending: Vec<crate::wire::EditorTransactionId>,
    host: crate::Host,
    mouse_interest: bool,
    event_interest: crate::wire::events::Interest,
    messages: Routes<Rc<dyn Any>>,
    handlers: Routes<Rc<dyn Any>>,
    clicks: Routes<ClickRoute>,
    tooltips: Routes<TooltipRoute>,
    row: Option<Row>,
    tooltip_responses: Vec<crate::wire::TooltipResponse>,
    pictures: HashSet<(bool, u64)>,
}

/// A route table. Routes taken while a list row lowers get ids from the row
/// and their order in it, not from the order of the whole frame: a row
/// scrolled in above the others renumbered every route after it otherwise,
/// and every node carrying one went out again as a props patch.
struct Routes<T> {
    frame: Vec<T>,
    rows: std::collections::HashMap<u32, T>,
}

impl<T> Default for Routes<T> {
    fn default() -> Self {
        Self {
            frame: Vec::new(),
            rows: std::collections::HashMap::new(),
        }
    }
}

/// The list row being lowered: its key, and how many routes it has taken.
#[derive(Clone, Copy)]
pub(crate) struct Row {
    key: u64,
    taken: u32,
}

/// Row ids have the top bit set; frame ids never get that far.
const ROW_IDS: u32 = 1 << 31;

impl<T: Clone> Routes<T> {
    fn push(&mut self, row: &mut Option<Row>, route: T, what: &str) -> u32 {
        if let Some(row) = row {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::hash::DefaultHasher::new();
            (row.key, row.taken).hash(&mut hasher);
            row.taken += 1;
            let id = ROW_IDS | hasher.finish() as u32;
            // A collision only costs the stable id: the route takes a frame id.
            if let std::collections::hash_map::Entry::Vacant(slot) = self.rows.entry(id) {
                slot.insert(route);
                return id;
            }
        }
        let id = u32::try_from(self.frame.len())
            .ok()
            .filter(|id| *id < ROW_IDS)
            .unwrap_or_else(|| panic!("too many {what} routes"));
        self.frame.push(route);
        id
    }

    fn get(&self, id: u32) -> Option<T> {
        match id & ROW_IDS {
            0 => self.frame.get(id as usize).cloned(),
            _ => self.rows.get(&id).cloned(),
        }
    }
}

/// Routes taken from here to [`leave_row`] are keyed by `key`.
pub(crate) fn enter_row(context: &Context, key: u64) -> Option<Row> {
    context.0.borrow_mut().row.replace(Row { key, taken: 0 })
}

pub(crate) fn leave_row(context: &Context, outer: Option<Row>) {
    context.0.borrow_mut().row = outer;
}

#[derive(Clone, Default)]
pub struct Context(Rc<RefCell<Tables>>);

impl Context {
    pub(crate) fn with_host(host: crate::Host) -> Self {
        let context = Self::default();
        context.0.borrow_mut().host = host;
        context
    }

    fn tables(&self) -> Rc<RefCell<Tables>> {
        self.0.clone()
    }

    pub(crate) fn identity(&self) -> Weak<()> {
        Arc::downgrade(&self.0.borrow().identity)
    }
}

/// Returns a picture hash and its bytes the first time this driver sends it.
pub fn picture(context: &Context, bytes: impl AsRef<[u8]>) -> (u64, Option<Vec<u8>>) {
    use std::hash::{Hash, Hasher};
    let bytes = bytes.as_ref();
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let hash = hasher.finish();
    let mut tables = context.0.borrow_mut();
    if tables.pictures.len() >= 4_096 && !tables.pictures.contains(&(false, hash)) {
        tables.pictures.clear();
    }
    let first = tables.pictures.insert((false, hash));
    (hash, first.then(|| bytes.to_vec()))
}

pub(crate) fn clear_pictures(context: &Context) {
    context.0.borrow_mut().pictures.clear();
}

/// A typed handler returns None for a value it cannot route.
pub fn handler<A: 'static, M: 'static>(
    context: &Context,
    handler: Box<dyn Fn(A) -> Option<M>>,
) -> u32 {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let tables = &mut *tables;
    let handler: Rc<dyn Any> = Rc::new(handler);
    tables.handlers.push(&mut tables.row, handler, "handler")
}

pub(crate) fn reset(context: &Context) {
    let tables = context.tables();
    let old = {
        let mut tables = tables.borrow_mut();
        (
            std::mem::take(&mut tables.messages),
            std::mem::take(&mut tables.handlers),
            std::mem::take(&mut tables.clicks),
            std::mem::take(&mut tables.tooltips),
        )
    };
    drop(old);
}

pub(crate) fn tooltip(context: &Context, build: TooltipBuilder) -> u32 {
    let tables = &mut *context.0.borrow_mut();
    let route: TooltipRoute = Rc::new(move |_, window, cx| Some(build(window, cx)));
    tables.tooltips.push(&mut tables.row, route, "tooltip")
}

pub(crate) fn rich_text_tooltip(context: &Context, build: RichTextTooltipBuilder) -> u32 {
    let tables = &mut *context.0.borrow_mut();
    let route: TooltipRoute =
        Rc::new(move |index, window, cx| index.and_then(|index| build(index, window, cx)));
    tables.tooltips.push(&mut tables.row, route, "tooltip")
}

pub(crate) fn tooltip_route(context: &Context, index: u32) -> Option<TooltipRoute> {
    context.0.borrow().tooltips.get(index)
}

pub(crate) fn tooltip_response(context: &Context, response: crate::wire::TooltipResponse) {
    context.0.borrow_mut().tooltip_responses.push(response);
}

pub(crate) fn take_tooltip_responses(context: &Context) -> Vec<crate::wire::TooltipResponse> {
    std::mem::take(&mut context.0.borrow_mut().tooltip_responses)
}

pub(crate) fn take_message<M: Clone + 'static>(context: &Context, index: u32) -> Option<M> {
    let tables = context.tables();
    let message = {
        let tables = tables.borrow();
        tables.messages.get(index)?
    };
    message.downcast_ref::<M>().cloned()
}

pub(crate) fn run_handler<A: 'static, M: 'static>(
    context: &Context,
    index: u32,
    value: A,
) -> Option<M> {
    let tables = context.tables();
    let handler = {
        let tables = tables.borrow();
        tables.handlers.get(index)?
    };
    handler.downcast_ref::<Box<dyn Fn(A) -> Option<M>>>()?(value)
}

pub(crate) fn route<A: 'static>(
    context: &Context,
    listener: impl Fn(&A, &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let tables = context.tables();
    let tables = &mut *tables.borrow_mut();
    let route: Rc<dyn Any> = Rc::new(EventRoute::<A>(Rc::new(listener)));
    tables.handlers.push(&mut tables.row, route, "handler")
}

pub(crate) fn message_route(
    context: &Context,
    listener: impl Fn(&(), &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let tables = context.tables();
    let tables = &mut *tables.borrow_mut();
    let route: Rc<dyn Any> = Rc::new(EventRoute::<()>(Rc::new(listener)));
    tables.messages.push(&mut tables.row, route, "message")
}

pub(crate) fn run_route<A: 'static>(
    context: &Context,
    index: u32,
    event: &A,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let tables = context.tables();
    let handler = {
        let tables = tables.borrow();
        tables.handlers.get(index)
    };
    let Some(route) =
        handler.and_then(|route| route.downcast_ref::<EventRoute<A>>().map(|r| r.0.clone()))
    else {
        return false;
    };
    route(event, window, app);
    true
}

pub(crate) fn run_message_route(
    context: &Context,
    index: u32,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let tables = context.tables();
    let route = {
        let tables = tables.borrow();
        tables.messages.get(index).and_then(|route| {
            route
                .downcast_ref::<EventRoute<()>>()
                .map(|route| route.0.clone())
        })
    };
    let Some(route) = route else { return false };
    route(&(), window, app);
    true
}

pub(crate) fn event_interest(context: &Context) -> crate::wire::events::Interest {
    context.0.borrow().event_interest
}

pub(crate) fn mouse_interest(context: &Context) -> bool {
    context.0.borrow().mouse_interest
}

pub(crate) fn click(
    context: &Context,
    listener: impl Fn(&gpui::ClickEvent, &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let tables = &mut *context.0.borrow_mut();
    tables
        .clicks
        .push(&mut tables.row, Rc::new(listener), "click")
}

pub(crate) fn run_click(
    context: &Context,
    index: u32,
    event: &gpui::ClickEvent,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let listener = context.0.borrow().clicks.get(index);
    if let Some(listener) = listener {
        listener(event, window, app);
        true
    } else {
        false
    }
}

mod editor;
pub(crate) use editor::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_contexts_restore_typed_routes_and_picture_history() {
        let first = Context::default();
        let first_route =
            handler::<String, String>(&first, Box::new(|text| Some(format!("first:{text}"))));
        assert!(picture(&first, b"svg").1.is_some());
        {
            let second = Context::default();
            let route =
                handler::<String, String>(&second, Box::new(|text| Some(format!("second:{text}"))));
            assert_eq!(route, 0, "each driver starts its own typed route table");
            assert_eq!(
                run_handler::<String, String>(&second, route, "x".into()).as_deref(),
                Some("second:x")
            );
            assert!(
                picture(&second, b"svg").1.is_some(),
                "a new host needs its own picture bytes"
            );
        }
        assert_eq!(
            run_handler::<String, String>(&first, first_route, "x".into()).as_deref(),
            Some("first:x")
        );
        assert!(
            picture(&first, b"svg").1.is_none(),
            "returning to the first driver preserves its picture history"
        );
    }

    #[test]
    fn clearing_picture_history_resends_content() {
        let context = Context::default();
        assert!(picture(&context, b"image").1.is_some());
        assert!(picture(&context, b"image").1.is_none());
        clear_pictures(&context);
        assert_eq!(
            picture(&context, b"image").1.as_deref(),
            Some(b"image".as_slice())
        );
    }
}

#[cfg(test)]
mod response_budget_tests {
    use super::*;
    use crate::wire::{
        self, EditorDecision, EditorHistoryEffect, EditorPatch, EditorResponse, EditorTransactionId,
    };

    #[test]
    fn independent_large_responses_cross_decodable_frames_without_losing_identity() {
        let context = Context::default();
        for sequence in 1..=2 {
            editor_response(
                &context,
                EditorResponse {
                    id: EditorTransactionId {
                        instance: 1,
                        document: format!("app:doc{sequence}"),
                        reset: 0,
                        sequence,
                        attempt: 1,
                        text_revision: 0,
                        revision: 0,
                    },
                    decision: EditorDecision::Apply {
                        patches: vec![EditorPatch {
                            start_byte: 0,
                            end_byte: 0,
                            replacement: "x"
                                .repeat(wire::editor_transaction::MAX_EDITOR_PATCH_BYTES),
                        }],
                        cursor: Default::default(),
                        history: EditorHistoryEffect::NewGroup,
                    },
                },
            );
        }
        for sequence in 1..=2 {
            let frame = wire::Frame {
                editor_decisions: take_editor_responses(&context),
                ..Default::default()
            };
            assert_eq!(
                frame.editor_decisions.len(),
                1,
                "one complete response per aggregate byte budget"
            );
            assert_eq!(frame.editor_decisions[0].id.sequence, sequence);
            assert!(wire::decode::<wire::Frame>(&wire::encode(&frame)).is_ok());
            assert_eq!(
                context.0.borrow().editor_responses.len(),
                (2 - sequence) as usize
            );
            assert!(
                editor_pending(&context),
                "sent responses remain outstanding until their commits"
            );
        }
        assert!(take_editor_responses(&context).is_empty());
    }
}

pub(crate) fn host(context: &Context) -> crate::Host {
    context.0.borrow().host.clone()
}
