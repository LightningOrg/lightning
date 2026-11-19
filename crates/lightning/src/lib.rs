//! **Lightning** — Discord bots at lightning speed.
//!
//! Lightning is a Rust framework for building Discord bots around three ideas:
//!
//! 1. **One command, two surfaces**: the same `#[command]` answers to `/ping`
//!    and `!ping`, with typed arguments defined once.
//! 2. **Zero boilerplate**: commands register themselves with `inventory`; you
//!    only write the function.
//! 3. **Everything is testable**: sending messages is a trait, so you can test
//!    your handlers without touching Discord.
//!
//! # Hello, world
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! #[command]
//! async fn ping(ctx: Context) -> Reply {
//!     Reply::text(format!("pong, {}", ctx.user.name))
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
//! # Typed arguments
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! #[derive(CommandArgs)]
//! struct Echo {
//!     /// What the bot should repeat
//!     text: String,
//!     /// How many times
//!     times: Option<u8>,
//! }
//!
//! #[command]
//! async fn echo(ctx: Context, args: Args<Echo>) -> Reply {
//!     let times = args.times.unwrap_or(1);
//!     Reply::text(args.text.repeat(times as usize))
//! }
//! ```
//!
//! # Subcommands
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! /// Manage the bot configuration
//! #[command]
//! async fn config() {}
//!
//! /// Show the current configuration
//! #[subcommand(command = "config")]
//! async fn show() -> Reply {
//!     Reply::text("everything is fine")
//! }
//!
//! /// Change a setting
//! #[subcommand(command = "config", group = "set")]
//! async fn channel() -> Reply {
//!     Reply::text("channel updated")
//! }
//! ```
//!
//! Parent options are replaced by subcommands; the parent handler is the prefix
//! fallback (`!config`). `Context` exposes `group`, `subcommand` and
//! `command_path()`.
//!
//! # Autocomplete
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! #[derive(CommandArgs)]
//! struct Search {
//!     /// Fruit to look up
//!     fruit: String,
//! }
//!
//! #[command]
//! async fn search(args: Args<Search>) -> Reply {
//!     Reply::text(format!("you picked {}", args.fruit))
//! }
//!
//! #[autocomplete(command = "search", option = "fruit")]
//! async fn fruits(focused: Focused) -> Vec<AutocompleteChoice> {
//!     ["apple", "banana"]
//!         .into_iter()
//!         .filter(|fruit| fruit.contains(&focused.value))
//!         .map(|fruit| AutocompleteChoice::text(fruit, fruit))
//!         .collect()
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
pub mod voice;

mod bot;
mod convert;
mod events;
mod responder;
mod runtime;
mod voice_states;

pub use bot::Lightning;
pub use events::{ComponentHandler, EventContext, EventHandler};
pub use voice::{Voice, VoiceConnection};
pub use voice_states::VoiceStates;

pub use lightning_core::{
    ActionRow, Args, Author, AutocompleteChoice, AutocompleteFn, AutocompleteHandler, BoxFuture,
    Button, ButtonStyle, ChannelId, ChoiceValue, Command, CommandArgs, CommandKind, Context,
    Cooldown, DmOnly, Embed, Error, Extras, Focused, FromContext, FromOptionValue, FromToken,
    Guild, GuildOnly, Interaction, IntoChoices, IntoReply, Member, Message, Middleware, Modal,
    Next, OptionKind, OptionSpec, Reply, Responder, Result, State, StateMap, Subcommand, Surface,
    UserId, Values, run_pipeline,
};

/// Helpers for the macros (argument errors).
pub use lightning_core::args;
/// The `async-trait` attribute macro, re-exported for your middleware.
pub use lightning_core::async_trait;
/// Re-export of the core middleware module.
pub use lightning_core::middleware;

pub use lightning_macros::{
    CommandArgs, autocomplete, command, component, event, main, subcommand,
};

#[doc(hidden)]
pub use inventory;
pub use serde_json;

/// Twilight types you will need every day.
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

/// Gateway events.
pub use twilight_gateway::Event;

/// Re-export of the Gateway (shards, events…).
pub use twilight_gateway;
/// Re-export of the HTTP client, in case you need the API directly.
pub use twilight_http::Client;

/// What you need in 99% of your files.
pub mod prelude {
    pub use crate::model::{DiscordMessage, Event, Id, Intents, PartialMember, User};
    pub use crate::{
        ActionRow, Args, AutocompleteChoice, Button, ButtonStyle, ChoiceValue, Command,
        CommandArgs, CommandKind, Context, Cooldown, Embed, Error, Focused, Lightning, Middleware,
        Modal, Next, OptionKind, Reply, Result, State, Subcommand, UserId, Voice, VoiceConnection,
        VoiceStates,
    };
    pub use crate::{autocomplete, command, component, event, subcommand};
}

#[doc(hidden)]
pub mod __private {
    pub use crate::bot::runtime_main;
    pub use crate::{BoxFuture, Command, CommandKind, FromContext, IntoReply, Middleware, Result};
}
