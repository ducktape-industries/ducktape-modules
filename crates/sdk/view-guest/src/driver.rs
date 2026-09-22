use super::*;

const MAX_ROUNDS: usize = 8;

pub struct Driver<V: View> {
    pub(crate) app: App,
    pub(crate) entity: Entity<V>,
    pub(crate) last_root: Option<wire::Node>,
    pub(crate) busy: bool,
}
impl<V: View> Drop for Driver<V> {
    fn drop(&mut self) {
        self.app.inner.alive.set(false);
        self.app.inner.tasks.borrow_mut().clear();
    }
}
impl<V: View> Default for Driver<V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<V: View> Driver<V> {
    pub fn new() -> Self {
        Self::initialize(None).expect("view initializes")
    }
    pub(crate) fn initialize(restored: Option<V>) -> Result<Self, String> {
        Self::initialize_in(App::for_driver(), restored)
    }
    pub(crate) fn initialize_in(mut app: App, restored: Option<V>) -> Result<Self, String> {
        let entity = Entity::reserve(&app);
        let mut window = app.window();
        let mut cx = Context {
            app: &mut app,
            entity: entity.clone(),
        };
        let value = match restored {
            Some(mut value) => {
                value.restored(&mut window, &mut cx);
                value
            }
            None => V::new(&mut window, &mut cx),
        };
        *entity.value.borrow_mut() = Some(value);
        Ok(Self {
            app,
            entity,
            last_root: None,
            busy: false,
        })
    }
    pub fn entity(&self) -> Entity<V> {
        self.entity.clone()
    }
    pub fn host(&self) -> Host {
        self.app.host()
    }
    pub(crate) fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }
    pub fn tick(&mut self, events: Vec<wire::Event>) -> wire::Frame {
        self.busy = false;
        self.settle();
        for event in events {
            let editor_event = matches!(
                &event,
                wire::Event::EditorDocument { .. }
                    | wire::Event::EditorRequest { .. }
                    | wire::Event::EditorTransaction { .. }
            );
            let message = match event {
                wire::Event::Observation { .. }
                | wire::Event::Mouse { .. }
                | wire::Event::Keyboard { .. } => None,
                wire::Event::Message(index) => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    if slots::run_message_route(&slots, index, &mut window, &mut self.app) {
                        None
                    } else {
                        slots::take_message::<Callback<V>>(&slots, index)
                    }
                }
                wire::Event::Click { handler, event } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    slots::run_click(&slots, handler, &event.into(), &mut window, &mut self.app);
                    None
                }
                wire::Event::AuxClick { handler, event } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    slots::run_click(&slots, handler, &event.into(), &mut window, &mut self.app);
                    None
                }
                wire::Event::MouseDown { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::MouseUp { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::MouseDownOut { handler, event } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::MouseUpOut { handler, event } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::MousePressure { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::MouseMove { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::MouseExit { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::ScrollWheel { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::Pinch { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::KeyDown { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::KeyUp { handler, event, .. } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::ModifiersChanged { handler, event } => {
                    self.run_route(handler, &event.into_gpui());
                    None
                }
                wire::Event::Hover { handler, hovered } => {
                    self.run_route(handler, &hovered);
                    None
                }
                wire::Event::FileDropExit { handler } => {
                    self.run_route(handler, &gpui::FileDropEvent::Exited);
                    None
                }
                wire::Event::TooltipRequest {
                    request,
                    character_index,
                } => {
                    let slots = self.app.inner.slots.clone();
                    if let Some(build) = slots::tooltip_route(&slots, request) {
                        let mut window = self.app.window();
                        let content = build(
                            character_index.map(|index| index as usize),
                            &mut window,
                            &mut self.app,
                        )
                        .map(|view| {
                            Box::new(Lowering::new(&mut window, &mut self.app).lower(view))
                        });
                        slots::tooltip_response(
                            &slots,
                            wire::TooltipResponse {
                                request,
                                character_index,
                                content,
                            },
                        );
                    }
                    None
                }
                wire::Event::Surface { handler, value } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    if slots::run_route(&slots, handler, &value, &mut window, &mut self.app) {
                        None
                    } else {
                        slots::run_handler::<wire::SurfaceValue, Callback<V>>(
                            &slots, handler, value,
                        )
                    }
                }
                wire::Event::Input { handler, text } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    if slots::run_route(&slots, handler, &text, &mut window, &mut self.app) {
                        None
                    } else {
                        slots::run_handler::<String, Callback<V>>(&slots, handler, text)
                    }
                }
                wire::Event::EditorDocument { handler, message } => {
                    use wire::editor_document::EditorDocumentMessage;
                    if matches!(
                        message,
                        EditorDocumentMessage::Acknowledged { .. }
                            | EditorDocumentMessage::Failed { .. }
                    ) {
                        slots::finish_editor_transfer(&self.app.inner.slots, message.id());
                        continue;
                    }
                    slots::run_handler::<wire::editor_document::EditorDocumentMessage, Callback<V>>(
                        &self.app.inner.slots,
                        handler,
                        message,
                    )
                }
                wire::Event::EditorRequest { handler, request } => {
                    slots::run_handler::<wire::EditorRequest, Callback<V>>(
                        &self.app.inner.slots,
                        handler,
                        request,
                    )
                }
                wire::Event::EditorTransaction { handler, event } => {
                    if let wire::EditorTransactionEvent::Fault { id, .. }
                    | wire::EditorTransactionEvent::Cancelled { id, .. } = &event
                    {
                        slots::finish_editor_transfer(
                            &self.app.inner.slots,
                            &wire::editor_document::EditorTransferId {
                                instance: id.instance,
                                document: id.document.clone(),
                                reset: id.reset,
                                serial: id.sequence,
                                attempt: id.attempt,
                            },
                        );
                    }
                    if let wire::EditorTransactionEvent::Cancelled { id, .. } = &event {
                        if !slots::editor_matches_pending(&self.app.inner.slots, id) {
                            continue;
                        }
                        slots::editor_acknowledge(&self.app.inner.slots, &event);
                    }
                    slots::run_handler::<wire::EditorTransactionEvent, Callback<V>>(
                        &self.app.inner.slots,
                        handler,
                        event,
                    )
                }
                wire::Event::Toggle { handler, on } => {
                    slots::run_handler::<bool, Callback<V>>(&self.app.inner.slots, handler, on)
                }
                wire::Event::Slide { handler, value } => {
                    slots::run_handler::<f32, Callback<V>>(&self.app.inner.slots, handler, value)
                }
                wire::Event::Select { handler, index } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    if slots::run_route(&slots, handler, &index, &mut window, &mut self.app) {
                        None
                    } else {
                        slots::run_handler::<u32, Callback<V>>(&slots, handler, index)
                    }
                }
                wire::Event::RichTextHover { handler, event } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    slots::run_route(&slots, handler, &event, &mut window, &mut self.app);
                    None
                }
                wire::Event::Size {
                    handler,
                    width,
                    height,
                } => {
                    let slots = self.app.inner.slots.clone();
                    let route_event = (px(width), px(height));
                    let mut window = self.app.window();
                    if slots::run_route(&slots, handler, &route_event, &mut window, &mut self.app) {
                        None
                    } else {
                        let event = (width, height);
                        slots::run_handler::<(f32, f32), Callback<V>>(&slots, handler, event)
                    }
                }
                wire::Event::Drag { handler, dx, dy } => {
                    let slots = self.app.inner.slots.clone();
                    let route_event = (px(dx as f32), px(dy as f32));
                    let mut window = self.app.window();
                    if slots::run_route(&slots, handler, &route_event, &mut window, &mut self.app) {
                        None
                    } else {
                        let event = (dx, dy);
                        slots::run_handler::<(f64, f64), Callback<V>>(&slots, handler, event)
                    }
                }
                wire::Event::Pointer { handler, x, y } => slots::run_handler::<
                    (f32, f32),
                    Callback<V>,
                >(
                    &self.app.inner.slots, handler, (x, y)
                ),
                wire::Event::Scroll {
                    handler,
                    dx,
                    dy,
                    pixels,
                } => slots::run_handler::<(f32, f32, bool), Callback<V>>(
                    &self.app.inner.slots,
                    handler,
                    (dx, dy, pixels),
                ),
                wire::Event::ScrollOffset {
                    handler,
                    x,
                    y,
                    relative_x,
                    relative_y,
                } => slots::run_handler::<(f32, f32, f32, f32), Callback<V>>(
                    &self.app.inner.slots,
                    handler,
                    (x, y, relative_x, relative_y),
                ),
                wire::Event::UniformListRange {
                    path,
                    route,
                    start,
                    end,
                } => {
                    if self.app.request_uniform_list_range(
                        path,
                        route,
                        start as usize,
                        end as usize,
                    ) {
                        self.app.notify();
                    }
                    None
                }
                wire::Event::UniformListState {
                    path,
                    route,
                    top_index,
                    scrollable,
                    scrolled_to_end,
                } => {
                    self.app.update_uniform_list_state(
                        &path,
                        route,
                        top_index as usize,
                        scrollable,
                        scrolled_to_end,
                    );
                    None
                }
                wire::Event::ListRequest { handler, request } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    slots::run_route(&slots, handler, &request, &mut window, &mut self.app);
                    self.app.notify();
                    None
                }
                wire::Event::ListScroll { handler, event } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    slots::run_route(&slots, handler, &event, &mut window, &mut self.app);
                    None
                }
                wire::Event::Theme { dark } => {
                    self.app
                        .set_global(if dark { Theme::dark() } else { Theme::light() });
                    self.app.notify();
                    None
                }
                wire::Event::Response { id, result, done } => {
                    self.app.host().fulfill(id, result, done);
                    // Response order is semantic: queued hidden data must be
                    // applied before a later visibility notification.
                    self.settle();
                    None
                }
                // The host dropped the tree the patches build on.
                wire::Event::Resync => {
                    self.last_root = None;
                    slots::clear_pictures(&self.app.inner.slots);
                    self.app.notify();
                    None
                }
            };
            if let Some(callback) = message {
                self.entity.clone().update_app(&mut self.app, |v, w, cx| {
                    callback(v, w, cx);
                    if editor_event {
                        cx.notify();
                    }
                });
                self.settle();
            }
        }
        self.settle();
        let render = self.app.inner.dirty.replace(false)
            || self.last_root.is_none()
            || slots::editor_transferring(&self.app.inner.slots);
        let mut root = if render {
            slots::reset(&self.app.inner.slots);
            let mut window = self.app.window();
            let element = {
                let mut cx = Context {
                    app: &mut self.app,
                    entity: self.entity.clone(),
                };
                let mut view = self.entity.value.borrow_mut();
                view.as_mut()
                    .expect("entity initialized")
                    .render(&mut window, &mut cx)
                    .into_element()
            };
            Lowering::new(&mut window, &mut self.app).lower_element(element)
        } else {
            self.last_root.clone().expect("rendered tree")
        };
        self.busy |= self.app.inner.dirty.get() || executor::ready(&self.app.inner.tasks.borrow());
        let unchanged = self.last_root.as_ref() == Some(&root);
        let mut patches = Vec::new();
        if !unchanged {
            // Patches against the last tree, unless there is none — a first
            // frame or a resync. Property changes remain patches even for
            // tiny trees; replacing their identity would discard native state.
            // Structural edits may use a whole tree when that is smaller.
            if let Some(last) = &mut self.last_root {
                patches = wire::diff(last, &mut root);
                let only_props = patches
                    .iter()
                    .all(|patch| matches!(patch, wire::Patch::Props { .. }));
                if patches.len() > wire::MAX_PATCHES
                    || (!only_props && wire::encoded_size(&patches) >= wire::encoded_size(&root))
                {
                    patches.clear();
                }
            }
            // Remembered without the picture bytes this frame carried: the
            // next view names those pictures by hash alone, and that is
            // the same tree — and the tree the host keeps, which drops the
            // bytes the same way once it has the pictures.
            let mut kept = root.clone();
            kept.for_each_mut(&mut |node| match node {
                wire::Node::Svg {
                    source: wire::SvgSource::Data { bytes, .. },
                    ..
                } => *bytes = None,
                wire::Node::Image { data, .. } | wire::Node::ImageViewer { data, .. } => {
                    *data = None
                }
                _ => {}
            });
            self.last_root = Some(kept);
        }
        let editor_decisions = slots::take_editor_responses(&self.app.inner.slots);
        self.busy |= slots::editor_responses_ready(&self.app.inner.slots);
        wire::Frame {
            upstream_sanitization: Default::default(),
            editor_decisions,
            editor_documents: slots::take_editor_documents(&self.app.inner.slots),
            tooltip_responses: slots::take_tooltip_responses(&self.app.inner.slots),
            mouse_interest: slots::mouse_interest(&self.app.inner.slots),
            event_interest: slots::event_interest(&self.app.inner.slots),
            root: Some(root),
            patches,
            requests: self.app.host().drain_outbox(),
            cancels: self.app.host().drain_cancels(),
            unchanged,
            busy: self.busy,
        }
    }

    fn run_route<E: 'static>(&mut self, handler: u32, event: &E) {
        let slots = self.app.inner.slots.clone();
        let mut window = self.app.window();
        slots::run_route(&slots, handler, event, &mut window, &mut self.app);
    }

    fn settle(&mut self) {
        for _ in 0..MAX_ROUNDS {
            let mut tasks = std::mem::take(&mut *self.app.inner.tasks.borrow_mut());
            let cut_short = executor::poll(&mut tasks);
            self.app.refresh_globals();
            let added = !self.app.inner.tasks.borrow().is_empty();
            tasks.append(&mut self.app.inner.tasks.borrow_mut());
            *self.app.inner.tasks.borrow_mut() = tasks;
            self.busy |= cut_short;
            if !added {
                return;
            }
        }
        self.busy = true;
    }
}
