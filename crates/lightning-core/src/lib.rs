//! # ⚡ lightning-core
//!
//! El núcleo de **Lightning**, un framework de Rust para construir bots de
//! Discord con la velocidad del rayo.
//!
//! Esta crate no habla con la red: define el modelo de comandos, el contexto de
//! ejecución, los extractores tipados, la pila de middleware y el formato de
//! respuesta. El runtime real (Gateway + HTTP) vive en la crate [`lightning`].
//!
//! La gracia de separar el núcleo es que **el envío de mensajes es un trait**
//! ([`Responder`]): puedes ejecutar tus handlers contra un responder falso y
//! testear tu bot sin tocar Discord.
//!
//! ```
//! use lightning_core::Reply;
//!
//! let reply = Reply::text("⚡ pong!").ephemeral();
//! assert_eq!(reply.content.as_deref(), Some("⚡ pong!"));
//! ```

#![warn(missing_docs)]

use std::future::Future;
use std::pin::Pin;

pub mod args;
pub mod command;
pub mod context;
pub mod error;
pub mod middleware;
pub mod reply;
pub mod state;

pub use args::{Args, CommandArgs, FromOptionValue, FromToken, OptionKind, OptionSpec};
pub use command::{Command, CommandKind};
pub use context::{
    Author, ChannelId, Context, Extras, FromContext, Guild, Interaction, Member, Message,
    Responder, Surface, UserId, Values,
};
pub use error::{Error, Result};
pub use middleware::{Cooldown, DmOnly, GuildOnly, Middleware, Next, run_pipeline};
pub use reply::{ActionRow, Button, ButtonStyle, Embed, IntoReply, Modal, Reply};
pub use state::{State, StateMap};

/// Un futuro dinámico y `Send`, el pegamento entre handlers y runtime.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Re-exportado para que las macros generen código sin depender del crate.
pub use async_trait::async_trait;
pub use inventory;
pub use serde_json;
pub use twilight_model;

/// Tipos que necesitan las macros y el runtime. No forma parte de la API estable.
#[doc(hidden)]
pub mod __private {
    pub use crate::{Error, Result};
    pub use std::boxed::Box;
    pub use std::string::String;
    pub use std::vec::Vec;
}
