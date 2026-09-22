//! Copied native pick-list metrics and handles. Fonts remain host-owned.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PickOptions {
    pub handle: Option<PickHandle>,
    pub on_open: Option<u32>,
    pub on_close: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(
    clippy::large_enum_variant,
    reason = "the public wire schema keeps native icon payloads inline"
)]
pub enum PickHandle {
    Arrow { size: Option<f32> },
    Static(PickIcon),
    Dynamic { closed: PickIcon, open: PickIcon },
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickIcon {
    pub code_point: char,
    pub style: gpui::TextStyleRefinement,
}

pub(super) fn text_size(value: &mut Option<f32>) {
    if let Some(value) = value {
        *value = bounded(*value).clamp(f32::EPSILON, MAX_TEXT_PIXELS);
    }
}
impl PickIcon {
    fn sanitize(&mut self, budgets: &mut Budgets) {
        style_sanitize::sanitize_text(&mut self.style, budgets);
    }
}
impl PickOptions {
    pub(super) fn sanitize(&mut self, budgets: &mut Budgets) {
        match &mut self.handle {
            Some(PickHandle::Arrow { size }) => text_size(size),
            Some(PickHandle::Static(icon)) => icon.sanitize(budgets),
            Some(PickHandle::Dynamic { closed, open }) => {
                closed.sanitize(budgets);
                open.sanitize(budgets);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pick_metrics_are_bounded_and_fonts_share_text_budget() {
        let style = gpui::TextStyleRefinement {
            font_family: Some("ééé".into()),
            font_size: Some(gpui::px(f32::MAX).into()),
            line_height: Some(gpui::relative(f32::MAX)),
            ..Default::default()
        };
        let icon = PickIcon {
            code_point: '▼',
            style,
        };
        let mut options = PickOptions {
            handle: Some(PickHandle::Dynamic {
                closed: icon.clone(),
                open: icon,
            }),
            ..Default::default()
        };
        let mut budget = Budgets::frame();
        budget.text = 9;
        options.sanitize(&mut budget);
        let Some(PickHandle::Dynamic { closed, open }) = options.handle else {
            panic!()
        };
        assert_eq!(
            closed.style.font_size,
            Some(gpui::px(MAX_TEXT_PIXELS).into())
        );
        assert_eq!(closed.style.line_height, Some(gpui::relative(8.)));
        assert_eq!(closed.style.font_family, Some("ééé".into()));
        assert_eq!(open.style.font_family, Some("é".into()));
        assert_eq!(budget.text, 1);
    }
}
