//! Serializable accessibility declarations for native GPUI interactivity.
use gpui::SharedString;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Aria {
    pub author_id: Option<SharedString>,
    pub label: Option<SharedString>,
    pub description: Option<SharedString>,
    pub keyshortcuts: Option<SharedString>,
    pub active_descendant: bool,
    pub value: Option<SharedString>,
    pub placeholder: Option<SharedString>,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub disabled: Option<bool>,
    pub numeric_value: Option<f64>,
    pub numeric_value_step: Option<f64>,
    pub min_numeric_value: Option<f64>,
    pub max_numeric_value: Option<f64>,
    pub level: Option<usize>,
    pub position_in_set: Option<usize>,
    pub size_of_set: Option<usize>,
    pub row_index: Option<usize>,
    pub column_index: Option<usize>,
    pub row_count: Option<usize>,
    pub column_count: Option<usize>,
    pub toggled: Option<gpui::Toggled>,
    pub orientation: Option<gpui::Orientation>,
}
impl Aria {
    pub(crate) fn sanitize(&mut self) {
        for field in [
            &mut self.author_id,
            &mut self.label,
            &mut self.description,
            &mut self.keyshortcuts,
            &mut self.value,
            &mut self.placeholder,
        ]
        .into_iter()
        .flatten()
        {
            let mut text = field.to_string();
            let mut end = text.len().min(1024);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            *field = text.into();
        }
        for value in [
            &mut self.numeric_value,
            &mut self.numeric_value_step,
            &mut self.min_numeric_value,
            &mut self.max_numeric_value,
        ]
        .into_iter()
        .flatten()
        {
            *value = if value.is_finite() {
                value.clamp(-1e12, 1e12)
            } else {
                0.
            };
        }
        for value in [
            &mut self.level,
            &mut self.position_in_set,
            &mut self.size_of_set,
            &mut self.row_index,
            &mut self.column_index,
            &mut self.row_count,
            &mut self.column_count,
        ]
        .into_iter()
        .flatten()
        {
            *value = (*value).min(1_000_000);
        }
    }
}
