//! Estado compartido e inyección de dependencias.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

type BoxedState = Arc<dyn Any + Send + Sync>;

/// Mapa de estados tipado: `TypeId` → `Arc<T>`.
///
/// Es el "grid" de Lightning: todo lo que inyectas con
/// `Lightning::new().state(...)` acaba aquí y se extrae con [`State`].
#[derive(Default, Clone)]
pub struct StateMap {
    entries: HashMap<TypeId, BoxedState>,
}

impl StateMap {
    /// Crea un mapa vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Guarda un valor, reemplazando el anterior del mismo tipo.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.entries.insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Guarda un valor ya envuelto en `Arc` (útil para estados caros de clonar).
    pub fn insert_arc<T: Send + Sync + 'static>(&mut self, value: Arc<T>) {
        self.entries.insert(TypeId::of::<T>(), value);
    }

    /// Recupera el estado del tipo `T`.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.entries
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.clone().downcast::<T>().ok())
    }

    /// `true` si el mapa contiene el tipo `T`.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.entries.contains_key(&TypeId::of::<T>())
    }

    /// Número de estados registrados.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` si no hay ningún estado.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Extractor de estado: pide `State<MiBaseDeDatos>` en la firma y listo.
pub struct State<T>(pub Arc<T>);

impl<T> State<T> {
    /// Devuelve el `Arc` interno.
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
