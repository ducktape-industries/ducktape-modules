//! Commands applied only to the requesting guest's mounted widget tree.
use serde::{Deserialize, Serialize};

/// Full authored typed ancestry of one mounted element, including the target.
pub type WidgetTarget = Vec<crate::ElementIdWire>;

/// Payload of `host.widget`. Mutation requests return an encoded unit;
/// `Focused` returns an encoded bool. Targets are the tree's qualified keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WidgetCommand {
    /// Queue a guest-defined action after native edits on this editor settle.
    /// The host sends EditorInteraction::Action through its transaction lane.
    EditorAction {
        target: WidgetTarget,
        #[serde(deserialize_with = "crate::editor_transaction::decode_document")]
        tag: String,
    },
    FocusPrevious,
    FocusNext,
    Focus {
        target: WidgetTarget,
    },
    FocusHandle {
        handle: u64,
    },
    Focused {
        target: WidgetTarget,
    },
    CursorFront {
        target: WidgetTarget,
    },
    CursorEnd {
        target: WidgetTarget,
    },
    Cursor {
        target: WidgetTarget,
        position: u32,
    },
    SelectAll {
        target: WidgetTarget,
    },
    Select {
        target: WidgetTarget,
        start: u32,
        end: u32,
    },
    Snap {
        target: WidgetTarget,
        x: f32,
        y: f32,
    },
    SnapEnd {
        target: WidgetTarget,
    },
    ScrollTo {
        target: WidgetTarget,
        x: f32,
        y: f32,
    },
    ScrollToKey {
        target: WidgetTarget,
        key: u64,
    },
    ScrollBy {
        target: WidgetTarget,
        x: f32,
        y: f32,
    },
}

impl WidgetCommand {
    /// Bound a decoded request without changing its target identity.
    pub fn validate(&mut self) -> Result<(), String> {
        let target = match self {
            Self::FocusPrevious | Self::FocusNext | Self::FocusHandle { .. } => return Ok(()),
            Self::EditorAction { target, .. }
            | Self::Focus { target }
            | Self::Focused { target }
            | Self::CursorFront { target }
            | Self::CursorEnd { target }
            | Self::Cursor { target, .. }
            | Self::SelectAll { target }
            | Self::Select { target, .. }
            | Self::Snap { target, .. }
            | Self::SnapEnd { target }
            | Self::ScrollTo { target, .. }
            | Self::ScrollBy { target, .. }
            | Self::ScrollToKey { target, .. } => target,
        };
        if target.is_empty() || target.len() > crate::MAX_DEPTH {
            return Err("widget target path is empty or too deep".into());
        }
        for id in target {
            id.validate_host()?;
        }
        if let Self::EditorAction { tag, .. } = self {
            let invalid_tag = tag.is_empty() || tag.len() > crate::MAX_STRING_BYTES;
            if invalid_tag {
                return Err("editor action tag exceeds bounds".into());
            }
        }
        match self {
            Self::Snap { x, y, .. } | Self::ScrollTo { x, y, .. } | Self::ScrollBy { x, y, .. }
                if !x.is_finite() || !y.is_finite() =>
            {
                return Err("widget offsets must be finite".into());
            }
            _ => {}
        }
        if let Self::Snap { x, y, .. } = self {
            *x = x.clamp(0.0, 1.0);
            *y = y.clamp(0.0, 1.0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: crate::ElementIdWire) -> WidgetTarget {
        vec![id]
    }

    #[test]
    fn editor_action_keeps_its_target_and_rejects_unbounded_tags() {
        let mut command = WidgetCommand::EditorAction {
            target: target(crate::ElementIdWire::Name("Other/body".into())),
            tag: "send".into(),
        };
        assert!(command.validate().is_ok());
        assert_eq!(
            crate::decode::<WidgetCommand>(&crate::encode(&command)).unwrap(),
            command
        );
        let WidgetCommand::EditorAction { tag, .. } = &mut command else {
            unreachable!()
        };
        *tag = "x".repeat(crate::MAX_STRING_BYTES + 1);
        assert!(command.validate().is_err());
    }

    #[test]
    fn widget_targets_are_rejected_whole_instead_of_redirected() {
        let key = crate::ElementIdWire::Name("가".repeat(crate::MAX_STRING_BYTES / 3 + 1).into());
        let mut command = WidgetCommand::Focus {
            target: target(key.clone()),
        };
        assert!(
            command.validate().is_err(),
            "oversized target must be refused"
        );
        assert_eq!(command, WidgetCommand::Focus { target: target(key) });
        let expected = WidgetCommand::Focus {
            target: vec![
                crate::ElementIdWire::Integer(1),
                crate::ElementIdWire::Name("draft".into()),
            ],
        };
        let mut decoded: WidgetCommand = crate::decode(&crate::encode(&expected)).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn widget_offsets_reject_nonfinite_values_and_preserve_scroll_direction() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for mut command in [
                WidgetCommand::Snap {
                    target: target(crate::ElementIdWire::Name("list".into())),
                    x: value,
                    y: 0.0,
                },
                WidgetCommand::ScrollTo {
                    target: target(crate::ElementIdWire::Name("list".into())),
                    x: 0.0,
                    y: value,
                },
                WidgetCommand::ScrollBy {
                    target: target(crate::ElementIdWire::Name("list".into())),
                    x: value,
                    y: 0.0,
                },
            ] {
                assert!(
                    command.validate().is_err(),
                    "nonfinite offset must be refused"
                );
            }
        }
        let mut snap = WidgetCommand::Snap {
            target: target(crate::ElementIdWire::Name("list".into())),
            x: -1.0,
            y: 2.0,
        };
        snap.validate().unwrap();
        assert_eq!(
            snap,
            WidgetCommand::Snap {
                target: target(crate::ElementIdWire::Name("list".into())),
                x: 0.0,
                y: 1.0
            }
        );
        let expected = WidgetCommand::ScrollBy {
            target: target(crate::ElementIdWire::Name("list".into())),
            x: -24.0,
            y: 10.0,
        };
        let mut command = expected.clone();
        command.validate().unwrap();
        assert_eq!(command, expected);
    }
}
