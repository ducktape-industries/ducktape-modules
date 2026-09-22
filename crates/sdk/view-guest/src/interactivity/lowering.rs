use super::*;

impl Interactivity {
    pub(crate) fn into_wire(
        self,
        lowering: &mut Lowering<'_>,
    ) -> (Option<wire::ElementIdWire>, wire::Interactivity) {
        let id = self
            .id
            .map(wire::ElementIdWire::from_gpui)
            .transpose()
            .expect("element ID must be portable across the view boundary");
        let tooltip = self.tooltip.map(|tooltip| {
            let request = lowering.tooltip(tooltip.build);
            wire::Tooltip {
                request,
                content: None,
                hoverable: tooltip.hoverable,
                delay_ms: self
                    .tooltip_show_delay
                    .unwrap_or(Duration::from_millis(500))
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
            }
        });
        let wire = wire::Interactivity {
            role: self.role,
            aria: self.aria,
            focusable: self.focusable,
            tab_stop: self.tab_stop,
            tab_index: self.tab_index,
            tab_group: self.tab_group,
            focus: self.focus,
            in_focus: self.in_focus,
            focus_visible: self.focus_visible,
            key_context: self.key_context,
            focus_handle: self.focus_handle.map(|handle| handle.id),
            occlude: self.occlude,
            block_mouse_except_scroll: self.block_mouse_except_scroll,
            window_control_area: self.window_control_area.map(|area| match area {
                WindowControlArea::Drag => wire::WindowControlArea::Drag,
                WindowControlArea::Close => wire::WindowControlArea::Close,
                WindowControlArea::Max => wire::WindowControlArea::Max,
                WindowControlArea::Min => wire::WindowControlArea::Min,
            }),
            hover_listener_mode: match self.hover_listener_mode {
                gpui::HoverListenerMode::InputModalityAware => {
                    wire::HoverListenerMode::InputModalityAware
                }
                gpui::HoverListenerMode::InputModalityIndependent => {
                    wire::HoverListenerMode::InputModalityIndependent
                }
            },
            group: self.group,
            hover: self.hover,
            active: self.active,
            group_hover: self
                .group_hover
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            group_active: self
                .group_active
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            on_click: self.on_click.map(|listener| lowering.click(listener)),
            on_aux_click: self.on_aux_click.map(|listener| lowering.click(listener)),
            on_mouse_down: route_mouse_down(self.mouse_down, lowering),
            capture_mouse_down: route_plain(self.capture_mouse_down, lowering),
            on_mouse_down_out: route_plain(self.mouse_down_out, lowering),
            on_mouse_up: route_mouse_up(self.mouse_up, lowering),
            capture_mouse_up: route_plain(self.capture_mouse_up, lowering),
            on_mouse_up_out: route_mouse_up(self.mouse_up_out, lowering),
            on_mouse_pressure: route_plain(self.mouse_pressure, lowering),
            capture_mouse_pressure: route_plain(self.capture_mouse_pressure, lowering),
            on_mouse_move: route_plain(self.mouse_move, lowering),
            on_mouse_exit: route_plain(self.mouse_exit, lowering),
            on_scroll_wheel: route_plain(self.scroll_wheel, lowering),
            on_pinch: route_plain(self.pinch, lowering),
            capture_pinch: route_plain(self.capture_pinch, lowering),
            on_key_down: route_plain(self.key_down, lowering),
            capture_key_down: route_plain(self.capture_key_down, lowering),
            on_key_up: route_plain(self.key_up, lowering),
            capture_key_up: route_plain(self.capture_key_up, lowering),
            on_modifiers_changed: route_plain(self.modifiers_changed, lowering),
            on_hover: self.on_hover.map(|listener| lowering.route(listener)),
            on_file_drop_exit: route_plain(self.on_file_drop_exit, lowering),
            tooltip,
        };
        (id, wire)
    }
}

fn route_plain<E: 'static>(
    listeners: Vec<EventListener<E>>,
    lowering: &mut Lowering<'_>,
) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(move |event: &E, window, app| {
            for listener in &listeners {
                listener(event, window, app);
            }
        })
    })
}

fn route_mouse_down(listeners: Vec<MouseDownBinding>, lowering: &mut Lowering<'_>) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(move |event: &gpui::MouseDownEvent, window, app| {
            for binding in &listeners {
                if binding.button.is_none_or(|button| button == event.button) {
                    (binding.listener)(event, window, app);
                }
            }
        })
    })
}

fn route_mouse_up(listeners: Vec<MouseUpBinding>, lowering: &mut Lowering<'_>) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(move |event: &gpui::MouseUpEvent, window, app| {
            for binding in &listeners {
                if binding.button.is_none_or(|button| button == event.button) {
                    (binding.listener)(event, window, app);
                }
            }
        })
    })
}
