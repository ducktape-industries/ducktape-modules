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

pub const MAX_LIST_ITEMS: usize = 100_000;
pub const MAX_LIST_ROWS: usize = 64;
pub const MAX_LIST_COMMANDS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListAlignment {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListSizingBehavior {
    Infer,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListOffset {
    pub item_ix: usize,
    pub offset_in_item: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ListCommand {
    Reset {
        count: usize,
    },
    Splice {
        start: usize,
        end: usize,
        count: usize,
    },
    Remeasure {
        start: usize,
        end: usize,
    },
    ScrollTo(ListOffset),
    ScrollToEnd,
    ScrollToRevealItem(usize),
    SetFollowMode {
        tail: bool,
    },
    PauseFollowingTail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRequest {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListScroll {
    pub visible_start: usize,
    pub visible_end: usize,
    pub count: usize,
    pub is_scrolled: bool,
    pub is_following_tail: bool,
    pub offset: ListOffset,
}

pub(super) fn decode_commands<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ListCommand>, D::Error> {
    bounded_vec(deserializer, MAX_LIST_COMMANDS, "too many list commands")
}

pub(super) fn decode_path<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<crate::ElementIdWire>, D::Error> {
    bounded_vec(deserializer, crate::MAX_DEPTH, "list ancestry is too deep")
}

fn bounded_vec<'de, D, T>(
    deserializer: D,
    limit: usize,
    message: &'static str,
) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Values<T>(usize, &'static str, std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Values<T> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.1)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == self.0 {
                    return Err(serde::de::Error::custom(self.1));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Values(limit, message, std::marker::PhantomData))
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

#[cfg(test)]
mod variable_tests {
    use super::*;
    use crate::{Frame, Node, decode, encode, sanitize};

    fn node(commands: Vec<ListCommand>, children: usize) -> Node {
        Node::List {
            state: 7,
            path: Vec::new(),
            item_count: usize::MAX,
            alignment: ListAlignment::Bottom,
            overdraw: f32::INFINITY,
            sizing: ListSizingBehavior::Auto,
            following_tail: true,
            revision: 3,
            commands,
            request_handler: 1,
            scroll_handler: Some(2),
            range_start: 99_990,
            style: gpui::StyleRefinement::default(),
            children: (0..children)
                .map(|_| Node::Space {
                    style: gpui::StyleRefinement::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn one_wire_walk_clamps_count_geometry_commands_and_rows() {
        let mut frame = Frame {
            root: Some(node(
                (0..MAX_LIST_COMMANDS + 5)
                    .map(|_| {
                        ListCommand::ScrollTo(ListOffset {
                            item_ix: usize::MAX,
                            offset_in_item: f32::INFINITY,
                        })
                    })
                    .collect(),
                MAX_LIST_ROWS + 20,
            )),
            ..Frame::default()
        };
        sanitize(&mut frame).unwrap();
        let Node::List {
            item_count,
            overdraw,
            commands,
            children,
            ..
        } = frame.root.unwrap()
        else {
            panic!()
        };
        assert_eq!(item_count, MAX_LIST_ITEMS);
        assert_eq!(overdraw, 4096.);
        assert_eq!(commands.len(), MAX_LIST_COMMANDS);
        assert!(commands.iter().all(|command| matches!(
            command,
            ListCommand::ScrollTo(ListOffset {
                item_ix: MAX_LIST_ITEMS,
                offset_in_item: 8192.
            })
        )));
        assert!(children.len() <= MAX_LIST_ROWS);
    }

    #[test]
    fn sibling_lists_share_the_native_item_allocation_budget() {
        let mut frame = Frame {
            root: Some(Node::Container {
                id: None,
                style: Default::default(),
                interactivity: Default::default(),
                children: vec![node(Vec::new(), 0), node(Vec::new(), 0)],
            }),
            ..Default::default()
        };
        sanitize(&mut frame).unwrap();
        let counts: Vec<_> = frame
            .root
            .unwrap()
            .children()
            .iter()
            .map(|child| {
                let Node::List { item_count, .. } = child else {
                    unreachable!()
                };
                *item_count
            })
            .collect();
        assert_eq!(counts, vec![MAX_LIST_ITEMS, 0]);
    }

    #[test]
    fn a_list_cannot_address_another_authored_ancestor() {
        let mut root = node(Vec::new(), 0);
        let Node::List { path, .. } = &mut root else {
            unreachable!()
        };
        path.push(crate::ElementIdWire::Name("other-room".into()));
        let mut frame = Frame {
            root: Some(root),
            ..Default::default()
        };
        assert_eq!(sanitize(&mut frame), Err("list authored path is invalid"));
    }

    #[test]
    fn decoder_refuses_oversized_command_vectors() {
        let commands = vec![ListCommand::ScrollToEnd; MAX_LIST_COMMANDS + 1];
        let frame = Frame {
            root: Some(node(commands, 0)),
            ..Frame::default()
        };
        assert!(decode::<Frame>(&encode(&frame)).is_err());
    }
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
