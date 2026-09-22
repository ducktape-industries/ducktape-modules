//! Copied combo presentation and routes; the host retains native search state.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComboOptions {
    pub icon: Option<ComboIcon>,
    pub input: Option<u32>,
    pub hover: Option<u32>,
    pub open: Option<u32>,
    pub close: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComboIcon {
    pub code_point: char,
    pub style: gpui::TextStyleRefinement,
    pub spacing: f32,
    pub right: bool,
}
impl ComboOptions {
    pub(super) fn sanitize(&mut self, budgets: &mut Budgets) {
        if let Some(icon) = &mut self.icon {
            icon.spacing = bounded(icon.spacing);
            style_sanitize::sanitize_text(&mut icon.style, budgets);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combo_hostile_options_and_indices_are_bounded() {
        let node = Node::ComboBox {
            id: ElementIdWire::Name("combo".into()),
            state_key: "state".into(),
            options: (0..MAX_OPTIONS + 2).map(|i| i.to_string()).collect(),
            selected: Some(MAX_OPTIONS as u32),
            reset: 0,
            placeholder: "é".repeat(MAX_STRING_BYTES),
            label: None,
            on_select: 0,
            style: gpui::StyleRefinement::default(),
            settings: Box::new(ComboOptions {
                icon: Some(ComboIcon {
                    code_point: '⌕',
                    style: {
                        let mut style = gpui::TextStyleRefinement::default();
                        style.font_size = Some(gpui::px(f32::MAX).into());
                        style
                    },
                    spacing: f32::NAN,
                    right: false,
                }),
                ..Default::default()
            }),
        };
        let mut frame = Frame {
            root: Some(node),
            ..Default::default()
        };
        sanitize(&mut frame).unwrap();
        let Node::ComboBox {
            options,
            selected,
            placeholder,
            settings,
            ..
        } = frame.root.unwrap()
        else {
            panic!()
        };
        assert_eq!(options.len(), MAX_OPTIONS);
        assert_eq!(selected, None);
        assert!(placeholder.len() <= MAX_STRING_BYTES);
        let icon = settings.icon.unwrap();
        assert_eq!(icon.style.font_size, Some(gpui::px(MAX_TEXT_PIXELS).into()));
        assert_eq!(icon.spacing, 0.0);
    }
}
