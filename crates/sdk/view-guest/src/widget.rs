//! Widget commands use the mounted host's scoped request channel.
#[cfg(test)]
mod tests {
    use crate::doors::{Door, Widget};
    use crate::{host, wire, Context, Driver, ElementId, Host, Input, Render, View, Window};
    use serde::{Deserialize, Serialize};

    fn target(name: &str) -> wire::WidgetTarget {
        vec![wire::ElementIdWire::Name(name.into())]
    }

    async fn perform(host: Host, command: wire::WidgetCommand) -> Result<Vec<u8>, host::Refusal> {
        host.request(Widget::KIND, &wire::encode(&command)).await
    }

    #[derive(Serialize, Deserialize)]
    struct WidgetView(bool);
    impl View for WidgetView {
        fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
            let host = cx.host();
            cx.spawn(async move |this, cx| {
                perform(
                    host.clone(),
                    wire::WidgetCommand::Focus {
                        target: target("App/draft"),
                    },
                )
                .await
                .unwrap();
                let bytes = perform(
                    host,
                    wire::WidgetCommand::Focused {
                        target: target("App/draft"),
                    },
                )
                .await
                .unwrap();
                let focused = wire::decode(&bytes).unwrap();
                this.update(cx, |view, cx| {
                    view.0 = focused;
                    cx.notify();
                })
                .unwrap();
            })
            .detach();
            Self(false)
        }
    }
    impl Render for WidgetView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl crate::IntoElement {
            Input::new(ElementId::Name("App/draft".into()))
                .label("Draft")
                .on_input(cx.listener(|_, _: &String, _, _| {}))
        }
    }

    #[test]
    fn widget_futures_wait_for_mutations_before_querying_focus() {
        let mut driver = Driver::<WidgetView>::new();
        let frame = driver.tick(vec![]);
        let [focus] = frame.requests.as_slice() else {
            panic!("one focus request: {:?}", frame.requests)
        };
        assert_eq!(focus.kind, Widget::KIND);
        assert_eq!(
            wire::decode::<wire::WidgetCommand>(&focus.payload).unwrap(),
            wire::WidgetCommand::Focus {
                target: target("App/draft")
            }
        );
        assert!(
            driver.tick(vec![]).requests.is_empty(),
            "query must wait for focus acknowledgment"
        );
        let frame = driver.tick(vec![wire::Event::Response {
            id: focus.id,
            result: Ok(wire::encode(&())),
            done: true,
        }]);
        let [query] = frame.requests.as_slice() else {
            panic!("one focused query: {:?}", frame.requests)
        };
        assert_eq!(
            wire::decode::<wire::WidgetCommand>(&query.payload).unwrap(),
            wire::WidgetCommand::Focused {
                target: target("App/draft")
            }
        );
        driver.tick(vec![wire::Event::Response {
            id: query.id,
            result: Ok(wire::encode(&true)),
            done: true,
        }]);
        driver.entity().read(|view| assert!(view.0));
    }

    #[test]
    fn window_dispatch_enqueues_commands_in_order() {
        let host = Host::default();
        let mut window = Window::new(crate::slots::Context::with_host(host.clone()));
        // Window mutations enqueue synchronously; explicit request futures wait for acknowledgments.
        window.focus("first");
        window.focus("second");
        let requests = host.drain_outbox();
        assert_eq!(requests.len(), 2);
        for (request, name) in requests.iter().zip(["first", "second"]) {
            assert_eq!(
                wire::decode::<wire::WidgetCommand>(&request.payload).unwrap(),
                wire::WidgetCommand::Focus {
                    target: target(name)
                }
            );
        }
    }

    #[test]
    fn focus_handle_schedules_its_opaque_host_command() {
        let mut app = crate::App::for_driver();
        let handle = app.focus_handle();
        let mut window = app.window();
        handle.focus(&mut window, &mut app);
        let requests = app.host().drain_outbox();
        let [request] = requests.as_slice() else {
            panic!("one focus command")
        };
        assert_eq!(
            wire::decode::<wire::WidgetCommand>(&request.payload).unwrap(),
            wire::WidgetCommand::FocusHandle { handle: 0 }
        );
    }
}
