use super::*;
use gpui::Styled as _;

#[test]
fn overflow_is_rejected_instead_of_silently_truncating_interactions() {
    let mut value = EditorPresentation::default();
    value.affordances.menu = Some(EditorMenu {
        anchor: EditorMenuAnchor::Caret,
        items: (0..=MAX_EDITOR_MENU_ITEMS)
            .map(|index| EditorMenuItem {
                tag: index.to_string(),
                label: format!("Action {index}"),
            })
            .collect(),
        selected: 0,
    });
    let mut budget = crate::Budgets::frame();
    budget.text = crate::MAX_STRING_BYTES;
    value.sanitize(&mut budget);
    assert!(
        crate::decode::<EditorPresentation>(&crate::encode(&value)).is_err(),
        "over-budget action lists must reject the frame, not publish a different menu"
    );
}

#[test]
fn native_span_and_line_styles_bound_untrusted_padding() {
    let mut value = EditorPresentation::default();
    value.formats.push(EditorFormat {
        style: gpui::StyleRefinement::default()
            .pt(gpui::px(-4.5))
            .pr(gpui::px(2.0))
            .pb(gpui::px(-1e9))
            .pl(gpui::px(f32::NAN)),
        line_style: gpui::StyleRefinement::default()
            .pt(gpui::px(-4.5))
            .pr(gpui::px(2.0))
            .pb(gpui::px(-1e9))
            .pl(gpui::px(f32::NAN)),
        ..Default::default()
    });
    let mut budget = crate::Budgets::frame();
    budget.text = crate::MAX_STRING_BYTES;
    value.sanitize(&mut budget);
    let format = &value.formats[0];
    for style in [&format.style, &format.line_style] {
        assert_eq!(style.padding.top, Some(gpui::px(0.).into()));
        assert_eq!(style.padding.right, Some(gpui::px(2.).into()));
        assert_eq!(style.padding.bottom, Some(gpui::px(0.).into()));
        assert_eq!(style.padding.left, Some(gpui::px(0.).into()));
    }
}

fn presentation(spans: Vec<EditorSpan>) -> EditorPresentation {
    EditorPresentation {
        formats: vec![EditorFormat::default()],
        spans,
        ..Default::default()
    }
}

#[test]
fn only_explicit_hit_ranges_consume_a_press() {
    let affordances = EditorAffordances {
        hits: vec![EditorHit {
            line: 1,
            start: 2,
            end: 5,
            tag: 7,
        }],
        ..Default::default()
    };
    let position = crate::EditorPosition { line: 1, column: 3 };
    assert_eq!(
        affordances.hit(position),
        Some(EditorInteraction::LinePress { tag: 7, position })
    );
    for position in [
        crate::EditorPosition { line: 0, column: 3 },
        crate::EditorPosition { line: 1, column: 1 },
        crate::EditorPosition { line: 1, column: 5 },
    ] {
        assert_eq!(affordances.hit(position), None);
    }
}

#[test]
fn affordances_cannot_address_missing_lines_or_invalid_menu_items() {
    let menu = EditorMenu {
        anchor: EditorMenuAnchor::Caret,
        items: vec![EditorMenuItem {
            tag: "heading".into(),
            label: "Heading".into(),
        }],
        selected: 0,
    };
    let mut value = EditorPresentation::default();
    value.affordances.menu = Some(menu.clone());
    assert_eq!(value.validate("Title\nbody"), Ok(()));
    value.affordances.menu.as_mut().unwrap().selected = 1;
    assert!(value.validate("Title\nbody").is_err());
    value.affordances.menu = Some(EditorMenu {
        anchor: EditorMenuAnchor::Line(2),
        ..menu
    });
    assert!(value.validate("Title\nbody").is_err());
    value.affordances.menu = None;
    value.affordances.gutters.push(EditorGutter {
        line: 2,
        plus: true,
        handle: true,
    });
    assert!(value.validate("Title\nbody").is_err());
    value.affordances.gutters.clear();
    value.affordances.hits.push(EditorHit {
        line: 1,
        start: 1,
        end: 3,
        tag: 0,
    });
    assert!(value.validate("Title\n한글").is_err());
}

#[test]
fn shared_range_scan_preserves_independent_unicode_ranges_and_line_counts() {
    let text = "한x\r\nz\n\r끝\r";
    let source: Vec<_> = crate::editor_lines(text).collect();
    let ranges: Vec<_> = (0..=4)
        .flat_map(|line| (0..=5).flat_map(move |start| (0..=5).map(move |end| (line, start, end))))
        .collect();
    let valid = |(line, start, end): (u32, u32, u32)| {
        start <= end
            && source.get(line as usize).is_some_and(|text| {
                text.is_char_boundary(start as usize) && text.is_char_boundary(end as usize)
            })
    };
    for &span in &ranges {
        for &hit in &ranges {
            let result = validate_ranges(
                crate::editor_lines(text),
                [span].into_iter(),
                [hit].into_iter(),
                true,
            );
            assert_eq!(
                result.is_ok(),
                valid(span) && valid(hit),
                "{span:?}, {hit:?}"
            );
            if let Ok(count) = result {
                assert_eq!(count, source.len());
            }
        }
    }
    // Hits overlap styling freely, but may not overlap one another.
    let spans = [(0, 0, 4), (2, 0, 3)];
    let hits = [(0, 0, 3), (2, 0, 3)];
    let visited = std::cell::Cell::new(0);
    let lines = crate::editor_lines(text).inspect(|_| visited.set(visited.get() + 1));
    assert_eq!(
        validate_ranges(lines, spans.into_iter(), hits.into_iter(), true),
        Ok(source.len())
    );
    assert_eq!(visited.get(), source.len());
    for invalid in [[(0, 0, 3), (0, 0, 3)], [(2, 0, 3), (0, 0, 3)]] {
        assert_eq!(
            validate_ranges(
                crate::editor_lines(text),
                spans.into_iter(),
                invalid.into_iter(),
                true
            ),
            Err(PresentationError::Range)
        );
    }
    assert_eq!(
        validate_ranges(
            crate::editor_lines(text),
            [].into_iter(),
            [].into_iter(),
            true
        ),
        Ok(source.len())
    );
}

#[test]
fn validates_byte_ranges_against_actual_logical_lines() {
    let source = "Title\n- 한글\n";
    let good = EditorSpan {
        line: 1,
        start: 2,
        end: 8,
        format: 0,
    };
    assert_eq!(presentation(vec![good]).validate(source), Ok(()));
    for bad in [
        EditorSpan { start: 3, ..good },
        EditorSpan { end: 9, ..good },
        EditorSpan {
            start: 8,
            end: 2,
            ..good
        },
        EditorSpan { line: 3, ..good },
    ] {
        assert_eq!(
            presentation(vec![bad]).validate(source),
            Err(PresentationError::Range)
        );
    }
    let empty = EditorSpan {
        line: 2,
        start: 0,
        end: 0,
        format: 0,
    };
    assert_eq!(presentation(vec![good, empty]).validate(source), Ok(()));
}

#[test]
fn rejects_ambiguous_order_overlap_and_missing_formats() {
    let span = EditorSpan {
        line: 0,
        start: 0,
        end: 2,
        format: 0,
    };
    assert_eq!(
        presentation(vec![span, span]).validate("abc"),
        Err(PresentationError::Range)
    );
    assert_eq!(
        presentation(vec![EditorSpan { format: 1, ..span }]).validate("abc"),
        Err(PresentationError::Format)
    );
    assert_eq!(
        presentation(vec![EditorSpan { line: 1, ..span }, span]).validate("abc\ndef"),
        Err(PresentationError::Range)
    );
}

#[test]
fn presentation_limits_do_not_consume_or_truncate_document_text() {
    let source = "a".repeat(1024 * 1024);
    assert_eq!(EditorPresentation::default().validate(&source), Ok(()));
    let span = EditorSpan {
        line: 0,
        start: 0,
        end: 0,
        format: 0,
    };
    assert_eq!(
        presentation(vec![span; MAX_EDITOR_SPANS + 1]).validate(&source),
        Err(PresentationError::Limit)
    );
    let oversized = EditorPresentation {
        formats: vec![EditorFormat::default(); MAX_EDITOR_FORMATS + 1],
        spans: vec![],
        ..Default::default()
    };
    assert_eq!(oversized.validate(&source), Err(PresentationError::Limit));
}

#[test]
fn decoder_rejects_oversized_collections_before_host_validation() {
    let valid = EditorPresentation::default();
    assert!(crate::decode::<EditorPresentation>(&crate::encode(&valid)).is_ok());
    let oversized = EditorPresentation {
        formats: vec![EditorFormat::default(); MAX_EDITOR_FORMATS + 1],
        spans: vec![],
        ..Default::default()
    };
    let bytes = crate::encode(&oversized);
    assert!(crate::decode::<EditorPresentation>(&bytes).is_err());
    let span = EditorSpan {
        line: 0,
        start: 0,
        end: 0,
        format: 0,
    };
    let bytes = crate::encode(&presentation(vec![span; MAX_EDITOR_SPANS + 1]));
    assert!(crate::decode::<EditorPresentation>(&bytes).is_err());
}
