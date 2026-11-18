//! ⚡ **Lightning** — bots de Discord a la velocidad del rayo.
//!
//! Lightning es un framework de Rust para crear bots de Discord con tres ideas:
//!
//! 1. **Un comando, dos superficies**: el mismo `#[command]` responde a `/ping`
//!    y a `!ping`, con los argumentos tipados una sola vez.
//! 2. **Cero boilerplate**: los comandos se auto-registran con `inventory`; tú
//!    solo escribes la función.
//! 3. **Todo es testeable**: el envío de mensajes es un trait, así que puedes
//!    probar tus handlers sin tocar Discord.
//!
//! # Hola, mundo
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! #[command]
//! async fn ping(ctx: Context) -> Reply {
//!     Reply::text(format!("pong ⚡ {}", ctx.user.name))
//! }
//!
//! #[lightning::main]
//! async fn main() {
//!     Lightning::new()
//!         .token_from_env()
//!         .prefix("!")
//!         .run()
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! # Argumentos tipados
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! #[derive(CommandArgs)]
//! struct Eco {
//!     /// Qué debe repetir el bot
//!     texto: String,
//!     /// Cuántas veces
//!     veces: Option<u8>,
//! }
//!
//! #[command]
//! async fn eco(ctx: Context, args: Args<Eco>) -> Reply {
//!     let veces = args.veces.unwrap_or(1);
//!     Reply::text(args.texto.repeat(veces as usize))
//! }
//! ```
//!
//! # Middleware
//!
//! ```no_run
//! use lightning::prelude::*;
//! use std::time::Duration;
//!
//! let bot = Lightning::new()
//!     .middleware(Cooldown::per_user(Duration::from_secs(3)));
//! # let _ = bot;
//! ```

#![warn(missing_docs)]

pub mod testing;

mod bot;
mod convert;
mod events;
mod responder;
mod runtime;

pub use bot::Lightning;
pub use events::{ComponentHandler, EventContext, EventHandler};

pub use lightning_core::{
    ActionRow, Args, Author, BoxFuture, Button, ButtonStyle, ChannelId, Command, CommandArgs,
    CommandKind, Context, Cooldown, DmOnly, Embed, Error, Extras, FromContext, FromOptionValue,
    FromToken, Guild, GuildOnly, Interaction, IntoReply, Member, Message, Middleware, Modal, Next,
    OptionKind, OptionSpec, Reply, Responder, Result, State, StateMap, Surface, UserId, Values,
    run_pipeline,
};

/// Utilidades para las macros (errores de argumentos).
pub use lightning_core::args;
/// El attribute macro de `async-trait`, re-exportado para tus middleware.
pub use lightning_core::async_trait;
/// Re-export del módulo de middleware del núcleo.
pub use lightning_core::middleware;

pub use lightning_macros::{CommandArgs, command, component, event, main};

#[doc(hidden)]
pub use inventory;
pub use serde_json;

/// Tipos de Twilight que vas a necesitar a diario.
pub mod model {
    pub use twilight_gateway::{Event, Intents};
    pub use twilight_model::channel::message::Message as DiscordMessage;
    pub use twilight_model::channel::message::{Component, Embed as DiscordEmbed};
    pub use twilight_model::gateway::payload::incoming;
    pub use twilight_model::guild::PartialMember;
    pub use twilight_model::id::Id;
    pub use twilight_model::id::marker::{
        ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker, MessageMarker,
        RoleMarker, UserMarker,
    };
    pub use twilight_model::user::User;
}

pub use twilight_model::gateway::Intents;
pub use twilight_model::id::Id;

/// Eventos del Gateway.
pub use twilight_gateway::Event;

/// Re-export del Gateway (shards, eventos...).
pub use twilight_gateway;
/// Re-export del cliente HTTP por si necesitas usar la API directamente.
pub use twilight_http::Client;

/// Lo que necesitas en el 99% de los archivos.
pub mod prelude {
    pub use crate::model::{DiscordMessage, Event, Id, Intents, PartialMember, User};
    pub use crate::{
        ActionRow, Args, Button, ButtonStyle, Command, CommandArgs, CommandKind, Context, Cooldown,
        Embed, Error, Lightning, Middleware, Modal, Next, OptionKind, Reply, Result, State,
    };
    pub use crate::{command, component, event};
}

#[doc(hidden)]
pub mod __private {
    pub use crate::bot::runtime_main;
    pub use crate::{BoxFuture, Command, CommandKind, FromContext, IntoReply, Middleware, Result};
}
