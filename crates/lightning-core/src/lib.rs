//! # lightning-core
//!
//! The core of **Lightning**, a Rust framework for building Discord bots at
//! lightning speed.
//!
//! This crate does not talk to the network: it defines the command model, the
//! execution context, the typed extractors, the middleware stack and the reply
//! format. The actual runtime (Gateway + HTTP) lives in the `lightning` crate.
//!
//! The point of separating the core is that **sending messages is a trait**
//! ([`Responder`]): you can run your handlers against a fake responder and test
//! your bot without touching Discord.
//!
//! ```
//! use lightning_core::Reply;
//!
//! let reply = Reply::text("pong!").ephemeral();
//! assert_eq!(reply.content.as_deref(), Some("pong!"));
//! ```

#![warn(missing_docs)]

use std::future::Future;
use std::pin::Pin;

pub mod args;
pub mod autocomplete;
pub mod command;
pub mod context;
pub mod error;
pub mod middleware;
pub mod reply;
pub mod state;
pub mod subcommand;

pub use args::{Args, CommandArgs, FromOptionValue, FromToken, OptionKind, OptionSpec};
pub use autocomplete::{
    AutocompleteChoice, AutocompleteFn, AutocompleteHandler, ChoiceValue, Focused, IntoChoices,
};
pub use command::{Command, CommandKind};
pub use context::{
    Author, ChannelId, Context, Extras, FromContext, Guild, Interaction, Member, Message,
    Responder, SelectValue, Selected, Surface, UserId, Values,
};
pub use error::{Error, Result};
pub use middleware::{Cooldown, DmOnly, GuildOnly, Middleware, Next, run_pipeline};
pub use reply::{
    ActionRow, Button, ButtonStyle, Embed, IntoReply, Modal, Reply, SelectKind, SelectMenu,
};
pub use state::{State, StateMap};
pub use subcommand::Subcommand;

/// A dynamic, `Send` future: the glue between handlers and the runtime.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Re-exported so the macros can generate code without depending on the crate.
pub use async_trait::async_trait;
pub use inventory;
pub use serde_json;
pub use twilight_model;

/// Types the macros and the runtime need. Not part of the stable API.
#[doc(hidden)]
pub mod __private {
    pub use crate::{Error, Result};
    pub use std::boxed::Box;
    pub use std::string::String;
    pub use std::vec::Vec;
}
