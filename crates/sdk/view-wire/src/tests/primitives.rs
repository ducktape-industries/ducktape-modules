use super::*;

#[test]
fn sensor_reset_values_share_the_frame_budget() {
    let sensor = |key: &str| Node::Sensor {
        id: ElementIdWire::Name(key.into()),
        style: Default::default(),
        reset: Some(SurfaceValue::List(vec![SurfaceValue::Unit; 3000])),
        on_show: Some(3),
        on_resize: None,
        on_hide: None,
        anticipate: None,
        delay: None,
        child: Box::new(text("child")),
    };
    // Not `sanitized_children`: the frame itself is asserted on below.
    let mut frame = Frame {
        root: Some(column(vec![sensor("first"), sensor("second")])),
        ..Frame::default()
    };
    sanitize(&mut frame).unwrap();
    let Some(Node::Container { children, .. }) = &frame.root else {
        panic!("column retained")
    };
    for (index, node) in children.iter().enumerate() {
        let Node::Sensor {
            id,
            reset,
            on_show,
            child,
            ..
        } = node
        else {
            panic!("sensor retained")
        };
        assert_eq!(
            reset.is_some(),
            index == 0,
            "individually valid reset values must share one frame budget"
        );
        assert_eq!(id.name(), Some(if index == 0 { "first" } else { "second" }));
        assert_eq!(*on_show, Some(3));
        assert!(matches!(&**child, Node::Text { content, .. } if content == "child"));
    }
    assert!(
        decode::<Frame>(&encode(&frame)).is_ok(),
        "sanitized aggregate fits the decoder budget"
    );
}

#[test]
fn a_sensor_is_pulled_into_range_and_keeps_its_child() {
    let root = sanitized_root(Node::Sensor {
        id: ElementIdWire::Name("watch".into()),
        style: Default::default(),
        reset: None,
        on_show: Some(0),
        on_resize: Some(0),
        on_hide: Some(1),
        anticipate: Some(f32::INFINITY),
        delay: Some(-5.0),
        child: Box::new(text("a")),
    });
    let Node::Sensor {
        anticipate,
        delay,
        child,
        ..
    } = &root
    else {
        panic!("{root:?}")
    };
    assert_eq!(*anticipate, Some(MAX_PIXELS));
    assert_eq!(*delay, Some(0.0));
    assert_eq!(**child, text("a"));
    assert_eq!(root.count(), 2);
}

/// The form controls: a menu is cut to `MAX_OPTIONS` with a selection
/// past the cut dropped, and a slider's numbers are made finite but not
/// clamped like a size — a value of a million is the app's to send.
#[test]
fn form_controls_are_pulled_into_range() {
    let children = sanitized_children(column(vec![
        Node::PickList {
            settings: Default::default(),
            id: ElementIdWire::Name("App/pick".into()),
            options: (0..MAX_OPTIONS + 3).map(|i| i.to_string()).collect(),
            selected: Some((MAX_OPTIONS + 1) as u32),
            placeholder: Some("é".repeat(MAX_STRING_BYTES)),
            label: None,
            on_select: 0,
            style: gpui::StyleRefinement::default(),
        },
        Node::Slider {
            id: ElementIdWire::Name("App/slide".into()),
            label: None,
            value: f32::NAN,
            min: f32::NEG_INFINITY,
            max: 1_000_000.0,
            step: f32::INFINITY,
            on_change: 1,
            on_release: None,
            axis: Axis::Row,
            style: gpui::StyleRefinement::default(),
        },
        Node::Toggle {
            key: "App/pick".into(),
            kind: ToggleKind::Switch,
            label: "x".repeat(MAX_STRING_BYTES + 1),
            checked: true,
            on_toggle: None,
            style: gpui::StyleRefinement::default(),
        },
    ]));
    let Node::PickList {
        options,
        selected,
        placeholder,
        ..
    } = &children[0]
    else {
        panic!("{:?}", children[0])
    };
    assert_eq!(options.len(), MAX_OPTIONS);
    assert_eq!(*selected, None);
    assert!(placeholder.as_ref().unwrap().len() <= MAX_STRING_BYTES);
    let Node::Slider {
        value,
        min,
        max,
        step,
        ..
    } = &children[1]
    else {
        panic!("{:?}", children[1])
    };
    assert_eq!(
        (*value, *min, *max, *step),
        (0.0, f32::MIN, 1_000_000.0, f32::MAX)
    );
    let Node::Toggle { key, label, .. } = &children[2] else {
        panic!("{:?}", children[2])
    };
    assert_eq!(key, "App/pick");
    assert!(label.len() <= MAX_STRING_BYTES);
}

/// A grid is bounded like a linear layout: its numbers are pulled into
/// the pixel range and children past the node budget are dropped, not
/// stood in for.
#[test]
fn a_container_is_pulled_into_range_and_cut_like_a_layout() {
    let root = sanitized_root(Node::Container {
        id: Some(ElementIdWire::Name("App/cells".into())),
        style: gpui::StyleRefinement::default(),
        interactivity: Interactivity::default(),
        children: (0..MAX_NODES + 5).map(|_| text("x")).collect(),
    });
    let Node::Container { children, .. } = &root else {
        panic!()
    };
    assert_eq!(children.len(), MAX_NODES - 1);
    assert_eq!(root.count(), MAX_NODES);
}

#[test]
fn svg_and_raster_images_share_the_frame_picture_budget() {
    let image = Node::Image {
        id: Some(ElementIdWire::Name("App/raster".into())),
        hash: 8,
        data: Some(ImageData::Encoded(vec![
            0;
            MAX_PICTURE_BYTES_PER_FRAME / 2 + 1
        ])),
        label: None,
        image_style: ImageStyle {
            grayscale: false,
            object_fit: ImageObjectFit::Contain,
        },
        loading: false,
        fallback: false,
        state_children: vec![],
        style: gpui::StyleRefinement::default(),
        interactivity: Interactivity::default(),
    };
    let mut frame = Frame {
        root: Some(column(vec![
            picture(Some(vec![0; MAX_PICTURE_BYTES_PER_FRAME / 2])),
            image,
        ])),
        ..Frame::default()
    };
    sanitize(&mut frame).unwrap();
    let children = frame.root.as_ref().unwrap().children();
    assert!(matches!(
        children[0],
        Node::Svg {
            source: SvgSource::Data { bytes: Some(_), .. },
            ..
        }
    ));
    assert!(
        matches!(children[1], Node::Image { data: None, .. }),
        "SVG consumption must reduce raster admission"
    );
    assert!(decode::<Frame>(&encode(&frame)).is_ok());
}

#[test]
fn primitive_geometry_and_state_children_are_bounded_in_the_main_walk() {
    let image = Node::Image {
        id: Some(ElementIdWire::Integer(1)),
        hash: 1,
        data: Some(ImageData::Refusal("x".repeat(MAX_STRING_BYTES + 1))),
        label: None,
        image_style: ImageStyle {
            grayscale: false,
            object_fit: ImageObjectFit::Contain,
        },
        loading: true,
        fallback: true,
        state_children: vec![text("loading"), text("fallback"), text("extra")],
        style: Default::default(),
        interactivity: Default::default(),
    };
    let svg = Node::Svg {
        id: Some(ElementIdWire::Integer(2)),
        source: SvgSource::External("x".repeat(MAX_STRING_BYTES + 1)),
        transformation: SvgTransformation {
            scale: [f32::NAN, f32::INFINITY],
            translate: [f32::NEG_INFINITY, f32::INFINITY],
            rotate: f32::NAN,
        },
        label: None,
        style: Default::default(),
        interactivity: Default::default(),
    };
    let anchored = Node::Anchored {
        anchor: Anchor::TopLeft,
        fit: AnchoredFitMode::SnapToWindowWithMargin([f32::NAN, f32::INFINITY, -1., 4.]),
        position: Some([f32::INFINITY, f32::NEG_INFINITY]),
        position_mode: AnchoredPositionMode::Local,
        offset: Some([f32::NAN, 3.]),
        children: vec![Node::Deferred {
            priority: usize::MAX,
            content: Box::new(text("child")),
        }],
    };
    let children = sanitized_children(column(vec![image, svg, anchored]));
    let Node::Image {
        data: Some(ImageData::Refusal(reason)),
        state_children,
        ..
    } = &children[0]
    else {
        panic!()
    };
    assert!(reason.len() <= MAX_STRING_BYTES);
    assert_eq!(state_children.len(), 2);
    let Node::Svg {
        source: SvgSource::External(path),
        transformation,
        ..
    } = &children[1]
    else {
        panic!()
    };
    assert!(path.len() <= MAX_STRING_BYTES);
    assert_eq!(transformation.scale, [0., MAX_PIXELS]);
    assert_eq!(transformation.translate, [-MAX_PIXELS, MAX_PIXELS]);
    assert_eq!(transformation.rotate, 0.);
    let Node::Anchored {
        fit,
        position,
        offset,
        children,
        ..
    } = &children[2]
    else {
        panic!()
    };
    assert_eq!(*position, Some([MAX_PIXELS, -MAX_PIXELS]));
    assert_eq!(*offset, Some([0., 3.]));
    assert_eq!(
        *fit,
        AnchoredFitMode::SnapToWindowWithMargin([0., MAX_PIXELS, 0., 4.])
    );
    assert!(matches!(children[0], Node::Deferred { priority: 16, .. }));
}

/// A picture past what is left of the frame's budget is dropped whole,
/// never cut: half an SVG is not an SVG. The head of the frame keeps
/// its pictures; a hash without bytes passes as the reference it is.
#[test]
fn a_frame_past_the_picture_budget_drops_whole_pictures_from_its_tail() {
    const EACH: usize = MAX_PICTURE_BYTES_PER_FRAME / 4 * 3;
    let children = sanitized_children(column(vec![
        picture(Some(vec![b'<'; EACH])),
        picture(Some(vec![b'<'; EACH])),
        picture(None),
        picture(Some(vec![b'<'; MAX_PICTURE_BYTES_PER_FRAME / 4])),
    ]));
    let carried: Vec<Option<usize>> = children
        .iter()
        .map(|child| match child {
            Node::Svg {
                source: SvgSource::Data { bytes, .. },
                ..
            } => bytes.as_ref().map(Vec::len),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        carried,
        [
            Some(EACH),
            None,
            None,
            Some(MAX_PICTURE_BYTES_PER_FRAME / 4)
        ]
    );
}

#[test]
fn gpui_text_keeps_style_refinement_on_the_wire() {
    let text = Node::Text {
        id: Some(ElementIdWire::Name("text".into())),
        style: gpui::StyleRefinement::default(),
        content: "huge".into(),
        heading: None,
        live: None,
    };
    assert_eq!(decode::<Node>(&encode(&text)).unwrap(), text);
}
