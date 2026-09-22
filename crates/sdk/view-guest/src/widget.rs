//! Widget commands use the mounted host's scoped request channel.
#[cfg(test)]
mod tests {
    use crate::{
        Context, Driver, Host, InteractiveElement, Render, View, Window, host, wire,
    };
    use serde::{Deserialize, Serialize};

    async fn perform(host: Host, command: wire::WidgetCommand) -> Result<Vec<u8>, host::Refusal> {
        host.request("host.widget", &wire::encode(&command)).await
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
                        target: "App/draft".into(),
                    },
                )
                .await
                .unwrap();
                let bytes = perform(
                    host,
                    wire::WidgetCommand::Focused {
                        target: "App/draft".into(),
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
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
            crate::div().id("draft")
        }
    }

    #[test]
    fn widget_futures_wait_for_mutations_before_querying_focus() {
        let mut driver = Driver::<WidgetView>::new();
        let frame = driver.tick(vec![]);
        let [focus] = frame.requests.as_slice() else {
            panic!("one focus request: {:?}", frame.requests)
        };
        assert_eq!(focus.kind, "host.widget");
        assert_eq!(
            wire::decode::<wire::WidgetCommand>(&focus.payload).unwrap(),
            wire::WidgetCommand::Focus {
                target: "App/draft".into()
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
                target: "App/draft".into()
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
        let mut window = Window::new(false, crate::slots::Context::with_host(false, host.clone()));
        // Window mutations enqueue synchronously; explicit request futures wait for acknowledgments.
        window.focus("first");
        window.focus("second");
        let requests = host.drain_outbox();
        assert_eq!(requests.len(), 2);
        for (request, target) in requests.iter().zip(["first", "second"]) {
            assert_eq!(
                wire::decode::<wire::WidgetCommand>(&request.payload).unwrap(),
                wire::WidgetCommand::Focus {
                    target: target.into()
                }
            );
        }
    }
}
