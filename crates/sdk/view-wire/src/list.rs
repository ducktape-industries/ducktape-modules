//! Copied row identity, preserving native equality and virtual-list bit identity.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformListSizing {
    Infer,
    #[default]
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformListHorizontalSizing {
    #[default]
    FitList,
    Unconstrained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformListScrollStrategy {
    Top,
    Center,
    Bottom,
    Nearest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniformListScrollRequest {
    pub index: usize,
    pub strategy: UniformListScrollStrategy,
    pub offset: usize,
    pub strict: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ListKey {
    Bool(bool),
    Integer(i64),
    Float(f64),
}

impl ListKey {
    pub fn virtual_key(self) -> u64 {
        match self {
            Self::Bool(value) => u64::from(value),
            Self::Integer(value) => value.cast_unsigned(),
            Self::Float(value) => value.to_bits(),
        }
    }
}

impl From<bool> for ListKey {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<i64> for ListKey {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<f64> for ListKey {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

pub(super) fn decode_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ListKey>, D::Error> {
    struct Keys;
    impl<'de> serde::de::Visitor<'de> for Keys {
        type Value = Vec<ListKey>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bounded row keys")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut keys = Vec::new();
            while let Some(key) = seq.next_element()? {
                if keys.len() == super::MAX_DECODED_NODES {
                    return Err(serde::de::Error::custom("too many row keys"));
                }
                keys.push(key);
            }
            Ok(keys)
        }
    }
    deserializer.deserialize_seq(Keys)
}

impl PartialEq for ListKey {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

pub(super) fn decode_optional_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<ListKey>>, D::Error> {
    struct Keys;
    impl<'de> serde::de::Visitor<'de> for Keys {
        type Value = Option<Vec<ListKey>>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("optional bounded row keys")
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            decode_keys(d).map(Some)
        }
    }
    deserializer.deserialize_option(Keys)
}

pub(super) fn decode_indices<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<u32>, D::Error> {
    struct Indices;
    impl<'de> serde::de::Visitor<'de> for Indices {
        type Value = Vec<u32>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bounded uniform-list row indices")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut indices = Vec::new();
            while let Some(index) = seq.next_element()? {
                if indices.len() == super::MAX_UNIFORM_LIST_ROWS {
                    return Err(serde::de::Error::custom(
                        "too many uniform-list row indices",
                    ));
                }
                indices.push(index);
            }
            Ok(indices)
        }
    }
    deserializer.deserialize_seq(Indices)
}
