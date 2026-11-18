//! Shared state and dependency injection.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

type BoxedState = Arc<dyn Any + Send + Sync>;

/// Typed state map: `TypeId` → `Arc<T>`.
///
/// This is Lightning's "grid": everything you inject with
/// `Lightning::new().state(...)` ends up here and is extracted with [`State`].
#[derive(Default, Clone)]
pub struct StateMap {
    entries: HashMap<TypeId, BoxedState>,
}

impl StateMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a value, replacing the previous one of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.entries.insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Stores a value that already lives in an `Arc` (useful for expensive states).
    pub fn insert_arc<T: Send + Sync + 'static>(&mut self, value: Arc<T>) {
        self.entries.insert(TypeId::of::<T>(), value);
    }

    /// Retrieves the state of type `T`.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.entries
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.clone().downcast::<T>().ok())
    }

    /// `true` if the map contains type `T`.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.entries.contains_key(&TypeId::of::<T>())
    }

    /// Number of registered states.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no state has been registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// State extractor: ask for `State<MyDatabase>` in the signature and you're done.
pub struct State<T>(pub Arc<T>);

impl<T> State<T> {
    /// Returns the inner `Arc`.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> Deref for State<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::fmt::Debug for State<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").finish_non_exhaustive()
    }
}
