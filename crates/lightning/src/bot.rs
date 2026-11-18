//! The bot builder and the runtime bootstrap.

use std::sync::Arc;

use lightning_core::command::Command;
use lightning_core::{Error, Middleware, Result, StateMap};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use twilight_gateway::Intents;
use twilight_http::Client;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::events::ComponentHandler;
use crate::runtime::{Runtime, RuntimeConfig, run_shard};

/// Builder and entry point for a Lightning bot.
///
/// ```
/// # use lightning::prelude::*;
/// let bot = Lightning::new()
///     .prefix("!")
///     .intents(Intents::GUILDS | Intents::GUILD_MESSAGES);
///
/// // bot.run().await?;
/// ```
pub struct Lightning {
    token: Option<String>,
    intents: Intents,
    prefix: Option<String>,
    state: StateMap,
    middleware: Vec<Arc<dyn Middleware>>,
    commands: Vec<Command>,
    guild_id: Option<Id<GuildMarker>>,
    shards: Option<u32>,
    register: bool,
}

impl Default for Lightning {
    fn default() -> Self {
        Self::new()
    }
}

impl Lightning {
    /// Starts building a bot with sensible defaults.
    pub fn new() -> Self {
        Self {
            token: None,
            intents: Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES,
            prefix: None,
            state: StateMap::new(),
            middleware: Vec::new(),
            commands: Vec::new(),
            guild_id: None,
            shards: None,
            register: true,
        }
    }

    /// Bot token. If omitted, it is read from `DISCORD_TOKEN`.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Reads the token from the `DISCORD_TOKEN` environment variable.
    ///
    /// This is the default behaviour; it is explicit so the code reads itself.
    pub fn token_from_env(self) -> Self {
        self
    }

    /// Gateway intents.
    ///
    /// If you use prefix commands, Lightning adds `MESSAGE_CONTENT` for you.
    pub fn intents(mut self, intents: Intents) -> Self {
        self.intents = intents;
        self
    }

    /// Enables prefix commands (`!ping`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Injects state accessible through the `State<T>` extractor.
    pub fn state<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.state.insert(value);
        self
    }

    /// Injects state that already lives in an `Arc` (useful for databases).
    pub fn state_arc<T: Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.state.insert_arc(value);
        self
    }

    /// Adds a global middleware layer.
    pub fn middleware<M: Middleware>(mut self, layer: M) -> Self {
        self.middleware.push(Arc::new(layer));
        self
    }

    /// Adds a hand-written command (`#[command]` items register themselves).
    pub fn command(mut self, command: Command) -> Self {
        self.commands.push(command);
        self
    }

    /// Adds several commands.
    pub fn commands(mut self, commands: impl IntoIterator<Item = Command>) -> Self {
        self.commands.extend(commands);
        self
    }

    /// Registers slash commands in a specific guild.
    ///
    /// Great for development: changes show up instantly instead of waiting for
    /// Discord's global cache.
    pub fn guild(mut self, guild_id: impl Into<Id<GuildMarker>>) -> Self {
        self.guild_id = Some(guild_id.into());
        self
    }

    /// Forces a shard count (defaults to Discord's recommendation).
    pub fn shards(mut self, count: u32) -> Self {
        self.shards = Some(count.max(1));
        self
    }

    /// Disables automatic slash command registration.
    pub fn skip_registration(mut self) -> Self {
        self.register = false;
        self
    }

    /// Boots the bot: registers commands, opens the shards and processes events
    /// until you press `Ctrl+C`.
    pub async fn run(self) -> Result<()> {
        let token = self
            .token
            .or_else(|| std::env::var("DISCORD_TOKEN").ok())
            .ok_or_else(|| {
                Error::custom("missing bot token: use .token(\"...\") or set DISCORD_TOKEN")
            })?;

        let mut intents = self.intents;
        if self.prefix.is_some() {
            intents |= Intents::MESSAGE_CONTENT;
        }

        let http = Arc::new(Client::new(token.clone()));

        let application = http
            .current_user_application()
            .await
            .map_err(|error| Error::Transport(error.to_string()))?
            .model()
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;
        let application_id = application.id;

        let mut commands: Vec<Command> = inventory::iter::<Command>.into_iter().cloned().collect();
        commands.extend(self.commands);

        let mut seen = std::collections::HashSet::new();
        commands.retain(|command| {
            if seen.insert(command.name) {
                true
            } else {
                warn!(command = command.name, "duplicate command, skipping");
                false
            }
        });

        let components: Vec<ComponentHandler> = inventory::iter::<ComponentHandler>
            .into_iter()
            .map(|handler| ComponentHandler {
                id: handler.id,
                run: handler.run,
            })
            .collect();

        let shard_total = match self.shards {
            Some(count) => count,
            None => http
                .gateway()
                .authed()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?
                .model()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?
                .shards
                .max(1),
        };

        let runtime = Arc::new(Runtime::new(
            Arc::clone(&http),
            RuntimeConfig {
                application_id,
                commands,
                components,
                prefix: self.prefix.clone(),
                state: Arc::new(self.state),
                middleware: self.middleware,
                shard_total,
            },
        ));

        if self.register {
            runtime.register(self.guild_id).await?;
        }

        info!(
            bot = %application.name,
            shards = shard_total,
            prefix = self.prefix.as_deref().unwrap_or("-"),
            "connecting to Discord"
        );

        let mut tasks: Vec<JoinHandle<()>> = Vec::with_capacity(shard_total as usize);

        for shard_id in 0..shard_total {
            let runtime = Arc::clone(&runtime);
            let token = token.clone();

            tasks.push(tokio::spawn(async move {
                run_shard(runtime, token, intents, shard_id, shard_total).await;
            }));
        }

        tokio::signal::ctrl_c()
            .await
            .map_err(|error| Error::Internal(error.to_string()))?;

        info!("shutting down Lightning");

        for task in tasks {
            task.abort();
        }

        Ok(())
    }
}

/// Entry point generated by `#[lightning::main]`.
#[doc(hidden)]
pub fn runtime_main<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    use tracing_subscriber::EnvFilter;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("lightning=info,info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create the Tokio runtime");

    runtime.block_on(future);
}
