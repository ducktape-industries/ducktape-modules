use crate::prelude::*;
use crate::testing::TestAppContext;
use crate::{modal_overlay, resize_handle, sensor, wire, View};

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct BehaviorView {
    measured: (f32, f32),
    dragged: (f64, f64),
    dismissed: bool,
}

impl View for BehaviorView {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for BehaviorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = cx.listener(|view, size: &(f32, f32), _, cx| {
            view.measured = *size;
            cx.notify();
        });
        let dragged = cx.listener(|view, delta: &(f64, f64), _, cx| {
            view.dragged = *delta;
            cx.notify();
        });
        let dismissed = cx.listener(|view, _: &(), _, cx| {
            view.dismissed = true;
            cx.notify();
        });
        modal_overlay(
            ElementId::Name("behavior-overlay".into()),
            sensor(
                ElementId::Name("behavior-sensor".into()),
                resize_handle(
                    ElementId::Name("behavior-resize".into()),
                    div().child("base"),
                )
                .on_drag(dragged),
            )
            .on_show(measured),
            div().child("modal"),
        )
        .label("Behavior dialog")
        .centered()
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
    cx.simulate_dismiss("behavior-overlay");
    view.read(|view| {
        assert_eq!(view.measured, (321., 123.));
        assert_eq!(view.dragged, (12., -3.));
        assert!(view.dismissed);
    });
}
