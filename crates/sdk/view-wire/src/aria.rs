//! Serializable accessibility declarations for native GPUI interactivity.
use gpui::SharedString;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Aria {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyshortcuts: Option<SharedString>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub active_descendant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value_step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_in_set: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_of_set: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toggled: Option<gpui::Toggled>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
