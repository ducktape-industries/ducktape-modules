use crate::prelude::*;
use crate::testing::TestAppContext;
use crate::{View, modal_overlay, resize_handle, sensor, surface, wire};

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct BehaviorView {
    measured: (f32, f32),
    dragged: (f32, f32),
    dismissed: bool,
    surface_event: String,
}

impl View for BehaviorView {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for BehaviorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = cx.listener(|view, size: &(Pixels, Pixels), _, cx| {
            view.measured = (size.0.into(), size.1.into());
            cx.notify();
        });
        let dragged = cx.listener(|view, delta: &(Pixels, Pixels), _, cx| {
            view.dragged = (delta.0.into(), delta.1.into());
            cx.notify();
        });
        let dismissed = cx.listener(|view, _: &(), _, cx| {
            view.dismissed = true;
            cx.notify();
        });
        let surface_event = cx.listener(|view, event: &wire::SurfaceValue, _, cx| {
            if let wire::SurfaceValue::Str(value) = event {
                view.surface_event = value.clone();
            }
            cx.notify();
        });
        modal_overlay(
            ElementId::Name("behavior-overlay".into()),
            sensor(
                ElementId::Name("behavior-sensor".into()),
                resize_handle(
                    ElementId::Name("behavior-resize".into()),
                    div().child("base").child(
                        surface(
                            ElementId::Name("behavior-surface".into()),
                            "test",
                            vec![wire::SurfaceValue::Bool(true)],
                        )
                        .on_event(surface_event),
                    ),
                )
                .on_drag(dragged),
            )
            .on_show(measured),
            div().child("modal"),
        )
        .label("Behavior dialog")
        .flex()
        .items_center()
        .justify_center()
        .on_dismiss(dismissed)
    }
}

#[test]
fn behavior_elements_lower_typed_routes_and_children() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<BehaviorView>();
    assert!(matches!(
        cx.find("behavior-sensor"),
        Some(wire::Node::Sensor {
            on_show: Some(_),
            ..
        })
    ));
    assert!(matches!(
        cx.find("behavior-resize"),
        Some(wire::Node::ResizeHandle {
            on_drag: Some(_),
            ..
        })
    ));
    assert!(matches!(
        cx.find("behavior-overlay"),
        Some(wire::Node::Overlay {
            label: Some(label),
            on_dismiss: Some(_),
            children,
            ..
        }) if label == "Behavior dialog" && children.len() == 2
    ));
    cx.simulate_measure("behavior-sensor", 321., 123.);
    cx.simulate_drag("behavior-resize", 12., -3.);
    cx.simulate_surface("behavior-surface", wire::SurfaceValue::Str("opened".into()));
    cx.simulate_dismiss("behavior-overlay");
    view.read(|view| {
        assert_eq!(view.measured, (321., 123.));
        assert_eq!(view.dragged, (12., -3.));
        assert_eq!(view.surface_event, "opened");
        assert!(view.dismissed);
    });
}
