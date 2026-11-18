//! Handlers de eventos y de componentes, registrados con `inventory`.

use std::sync::Arc;

use lightning_core::command::RunFn;
use lightning_core::{BoxFuture, StateMap};
use twilight_gateway::Event;
use twilight_http::Client;
use twilight_model::id::Id;
use twilight_model::id::marker::ApplicationMarker;

/// Acceso a servicios del runtime dentro de un handler de evento.
#[derive(Clone)]
pub struct EventContext {
    /// Cliente HTTP ya autenticado.
    pub http: Arc<Client>,
    /// Id de la aplicación.
    pub application_id: Id<ApplicationMarker>,
    /// Estados inyectados con `.state(...)`.
    pub state: Arc<StateMap>,
    /// Shard que recibió el evento.
    pub shard_id: u32,
}

impl std::fmt::Debug for EventContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventContext")
            .field("application_id", &self.application_id)
            .field("shard_id", &self.shard_id)
            .finish_non_exhaustive()
    }
}

impl EventContext {
    /// Recupera un estado inyectado.
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.state.get::<T>()
    }
}

/// Handler de un evento del Gateway, generado por `#[event]`.
#[doc(hidden)]
pub struct EventHandler {
    /// Nombre del evento (informativo).
    pub name: &'static str,
    /// Función que filtra y ejecuta el handler.
    pub handle: for<'a> fn(&'a Event, EventContext) -> BoxFuture<'a, ()>,
}

/// Handler de un componente interactivo, generado por `#[component]`.
#[doc(hidden)]
pub struct ComponentHandler {
    /// Prefijo del `custom_id` que atiende.
    pub id: &'static str,
    /// Handler.
    pub run: RunFn,
}

inventory::collect!(EventHandler);
inventory::collect!(ComponentHandler);
