//! Persistent streams are identified by their recipe, not recreated per frame.
use crate::task::BoxStream;
use futures::{Stream, StreamExt};
use std::any::TypeId;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

pub struct Recipe<T> {
    pub key: u64,
    pub start: Box<dyn FnOnce() -> BoxStream<T>>,
}
pub struct Subscription<T> {
    recipes: Vec<Recipe<T>>,
}

fn fingerprint(value: impl Hash) -> u64 {
    let mut hash = std::hash::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}

impl<T: 'static> Subscription<T> {
    pub fn into_recipes(self) -> Vec<Recipe<T>> {
        self.recipes
    }
    pub fn none() -> Self {
        Self {
            recipes: Vec::new(),
        }
    }
    pub fn run<S: Stream<Item = T> + 'static>(make: fn() -> S) -> Self {
        Self {
            recipes: vec![Recipe {
                key: fingerprint((TypeId::of::<S>(), make as usize)),
                start: Box::new(move || make().boxed_local()),
            }],
        }
    }
    pub fn batch(subscriptions: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self::none();
        for mut subscription in subscriptions {
            result.recipes.append(&mut subscription.recipes);
        }
        result
    }
    pub fn map<U: 'static, F: Fn(T) -> U + 'static>(self, map: F) -> Subscription<U> {
        let map = Rc::new(map);
        Subscription {
            recipes: self
                .recipes
                .into_iter()
                .map(|recipe| {
                    let map = map.clone();
                    Recipe {
                        key: fingerprint((recipe.key, TypeId::of::<F>())),
                        start: Box::new(move || {
                            (recipe.start)().map(move |value| map(value)).boxed_local()
                        }),
                    }
                })
                .collect(),
        }
    }
}
