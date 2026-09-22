use std::sync::Arc;

use serde::{Deserialize, Serialize};
use view_guest::prelude::*;
use view_guest::{Driver, ElementId, Input, View, Window, wire};

#[derive(Default, Serialize, Deserialize)]
struct Form {
    value: String,
    submits: usize,
}

impl View for Form {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = ElementId::NamedChild(
            Arc::new(ElementId::Name("chat".into())),
            "search".into(),
        );
        div().child(
            Input::new(id)
                .value(self.value.clone())
                .placeholder("Search")
                .label("Search messages")
                .w(px(240.))
                .on_input(cx.listener(|view, text: &String, _, cx| {
                    view.value = text.clone();
                    cx.notify();
                }))
                .on_submit(cx.listener(|view, _: &(), _, cx| {
                    view.submits += 1;
                    cx.notify();
                })),
        )
    }
}

fn input(frame: &wire::Frame) -> (&wire::ElementIdWire, u32, u32, &gpui::StyleRefinement) {
    let wire::Node::Container { children, .. } = frame.root.as_ref().expect("root") else {
        panic!("root container")
    };
    let wire::Node::Input {
        id,
        on_input,
        on_submit: Some(on_submit),
        style,
        ..
    } = &children[0]
    else {
        panic!("input child")
    };
    (id, *on_input, *on_submit, style)
}

#[test]
fn input_lowers_typed_identity_style_and_frame_owned_callbacks() {
    let mut first = Driver::<Form>::new();
    let mut second = Driver::<Form>::new();
    let first_frame = first.tick(vec![]);
    let second_frame = second.tick(vec![]);
    let (id, first_input, first_submit, style) = input(&first_frame);
    let (second_id, second_input, second_submit, _) = input(&second_frame);
    assert_eq!(id, second_id);
    assert_eq!(first_input, second_input);
    assert_eq!(first_submit, second_submit);
    assert_eq!(
        id,
        &wire::ElementIdWire::NamedChild {
            base: wire::ElementIdAtom::Name("chat".into()),
            names: vec!["search".into()],
        }
    );
    assert_eq!(style, &gpui::StyleRefinement::default().w(px(240.)));

    first.tick(vec![wire::Event::Input {
        handler: first_input,
        text: "hello".into(),
    }]);
    first.tick(vec![wire::Event::Message(first_submit)]);
    first.entity().read(|form| {
        assert_eq!(form.value, "hello");
        assert_eq!(form.submits, 1);
    });
    second.entity().read(|form| {
        assert_eq!(form.value, "");
        assert_eq!(form.submits, 0);
    });

    first.tick(vec![wire::Event::Input {
        handler: second_input,
        text: "stale".into(),
    }]);
    first.entity().read(|form| assert_eq!(form.value, "stale"));
}
