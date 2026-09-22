use super::*;

#[test]
fn patches_reconstruct_the_rendered_tree_and_picture_bytes_are_not_retained() {
    #[derive(Serialize, Deserialize)]
    struct Picture(u32);
    impl View for Picture {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(0)
        }
    }
    impl Render for Picture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("picture-view")
                .child(img(Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    b"shared-image".to_vec(),
                ))))
                .children((0..20).map(|i| {
                    div()
                        .id(format!("row/{i}"))
                        .child(format!("Row {i}: {}", if i == 0 { self.0 } else { 0 }))
                }))
        }
    }
    let mut driver = Driver::<Picture>::new();
    let first = driver.tick(vec![]);
    let mut mounted = first.root.unwrap();
    let mut pictures = 0;
    mounted.for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            assert!(data.is_some());
            *data = None;
            pictures += 1;
        }
    });
    assert_eq!(pictures, 1);
    assert_eq!(driver.last_root.as_ref(), Some(&mounted));
    assert!(driver.tick(vec![]).unchanged);
    driver.entity().update_app(driver.app_mut(), |view, _, cx| {
        view.0 = 1;
        cx.notify();
    });
    let frame = driver.tick(vec![]);
    assert!(!frame.unchanged);
    assert!(!frame.patches.is_empty());
    wire::apply(&mut mounted, frame.patches).unwrap();
    // The host stores picture data separately after applying a patch.
    mounted.for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            *data = None;
        }
    });
    assert_eq!(driver.last_root.as_ref(), Some(&mounted));

    let resent = driver.tick(vec![wire::Event::Resync]);
    assert!(
        resent.root.as_ref().is_some_and(|root| {
            let mut found = false;
            root.clone().for_each_mut(&mut |node| {
                if matches!(
                    node,
                    wire::Node::Image {
                        data: Some(wire::ImageData::Encoded(_)),
                        ..
                    }
                ) {
                    found = true;
                }
            });
            found
        }),
        "resync must resend bytes from a dropped first frame"
    );
}

#[test]
fn primitive_sources_fallbacks_transformations_and_typed_ids_survive_lowering() {
    #[derive(Serialize, Deserialize)]
    struct Primitives;
    impl View for Primitives {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }
    impl Render for Primitives {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let empty = gpui::RenderImage::new(Vec::new());
            div().children([
                img(Arc::new(empty))
                    .with_fallback(|| "fallback".into_any_element())
                    .id(gpui::ElementId::Integer(9))
                    .into_any_element(),
                svg()
                    .data(b"<svg/>")
                    .with_transformation(Transformation::translate(gpui::point(px(3.), px(4.))))
                    .id(gpui::ElementId::Integer(10))
                    .into_any_element(),
            ])
        }
    }

    let frame = Driver::<Primitives>::new().tick(vec![]);
    let children = frame.root.unwrap().children().to_vec();
    let wire::Node::Image {
        id,
        data,
        fallback,
        state_children,
        ..
    } = &children[0]
    else {
        panic!()
    };
    assert_eq!(id, &Some(wire::ElementIdWire::Integer(9)));
    assert!(matches!(data, Some(wire::ImageData::Refusal(reason)) if reason.contains("no frames")));
    assert!(*fallback);
    assert!(
        matches!(&state_children[0], wire::Node::Text (crate::wire::TextNode { content, .. }) if content == "fallback")
    );
    let wire::Node::Svg {
        id,
        source,
        transformation,
        ..
    } = &children[1]
    else {
        panic!()
    };
    assert_eq!(id, &Some(wire::ElementIdWire::Integer(10)));
    assert!(
        matches!(source, wire::SvgSource::Data { bytes: Some(bytes), .. } if bytes == b"<svg/>")
    );
    assert_eq!(transformation.translate, [3., 4.]);
}
