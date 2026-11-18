//! The engine: registration, shards, event dispatch and command execution.

use std::sync::Arc;

use lightning_core::command::Command;
use lightning_core::context::Surface;
use lightning_core::middleware::{self, Middleware};
use lightning_core::{Context, Error, Reply, Result, StateMap};
use serde_json::Map;
use tracing::{debug, info, warn};
use twilight_gateway::EventTypeFlags;
use twilight_gateway::{Event, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client;
use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::Message;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::id::Id;
use twilight_model::id::marker::{ApplicationMarker, GuildMarker};

use crate::convert;
use crate::events::{ComponentHandler, EventContext, EventHandler};
use crate::responder::{InitialKind, TwilightResponder};
use crate::voice::Voice;

/// A command plus its already-built middleware layers.
pub(crate) struct CommandSlot {
    command: Command,
    layers: Arc<[Arc<dyn Middleware>]>,
}

/// State shared by every shard.
pub(crate) struct Runtime {
    pub http: Arc<Client>,
    pub application_id: Id<ApplicationMarker>,
    pub commands: Vec<CommandSlot>,
    pub components: Vec<ComponentHandler>,
    pub prefix: Option<String>,
    pub state: Arc<StateMap>,
    pub middleware: Arc<[Arc<dyn Middleware>]>,
    pub shard_total: u32,
    pub voice: Voice,
}

/// Everything needed to build the runtime.
pub(crate) struct RuntimeConfig {
    pub application_id: Id<ApplicationMarker>,
    pub commands: Vec<Command>,
    pub components: Vec<ComponentHandler>,
    pub prefix: Option<String>,
    pub state: Arc<StateMap>,
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub shard_total: u32,
    pub voice: Voice,
}

impl Runtime {
    pub(crate) fn new(http: Arc<Client>, config: RuntimeConfig) -> Self {
        let RuntimeConfig {
            application_id,
            commands,
            components,
            prefix,
            state,
            middleware,
            shard_total,
            voice,
        } = config;

        let commands = commands
            .into_iter()
            .map(|command| {
                let layers: Arc<[Arc<dyn Middleware>]> = command.build_layers().into();
                CommandSlot { command, layers }
            })
            .collect();

        Self {
            http,
            application_id,
            commands,
            components,
            prefix,
            state,
            middleware: middleware.into(),
            shard_total,
            voice,
        }
    }

    /// Publishes the slash commands to Discord.
    pub(crate) async fn register(&self, guild_id: Option<Id<GuildMarker>>) -> Result<()> {
        let commands = convert::to_commands(self.commands.iter().map(|slot| &slot.command));

        if commands.is_empty() {
            info!("there are no slash commands to register");
            return Ok(());
        }

        let interaction = self.http.interaction(self.application_id);

        if let Some(guild_id) = guild_id {
            interaction
                .set_guild_commands(guild_id, &commands)
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;
            info!(
                count = commands.len(),
                guild = %guild_id,
                "commands registered in guild"
            );
        } else {
            interaction
                .set_global_commands(&commands)
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;
            info!(count = commands.len(), "global commands registered");
        }

        Ok(())
    }

    /// Processes a Gateway event.
    pub(crate) async fn dispatch(self: &Arc<Self>, event: Event, shard_id: u32) {
        match &event {
            Event::InteractionCreate(created) => {
                // Commands are spawned: a handler (for example a voice join)
                // must never block the shard, or the very events it is waiting
                // for could not be read.
                let runtime = Arc::clone(self);
                let interaction = created.0.clone();
                tokio::spawn(async move {
                    runtime.handle_interaction(interaction, shard_id).await;
                });
            }
            Event::MessageCreate(created) => {
                let runtime = Arc::clone(self);
                let message = created.message.clone();
                tokio::spawn(async move {
                    runtime.handle_message(message, shard_id).await;
                });
            }
            Event::Ready(ready) => {
                self.voice.set_user_id(ready.user.id);
                self.banner(&ready.user.name, ready.guilds.len(), shard_id);
            }
            Event::GuildCreate(created) => {
                if let GuildCreate::Available(guild) = &**created {
                    self.voice.states().seed(guild.id, &guild.voice_states);
                }
            }
            Event::VoiceStateUpdate(update) => {
                self.voice.handle_state(&update.0).await;
            }
            Event::VoiceServerUpdate(server) => {
                self.voice.handle_server(server).await;
            }
            _ => {}
        }

        let context = EventContext {
            http: Arc::clone(&self.http),
            application_id: self.application_id,
            state: Arc::clone(&self.state),
            shard_id,
            voice: self.voice.clone(),
            voice_states: self.voice.states().clone(),
        };

        for handler in inventory::iter::<EventHandler> {
            (handler.handle)(&event, context.clone()).await;
        }
    }

    fn banner(&self, bot: &str, guilds: usize, shard_id: u32) {
        if shard_id > 0 {
            return;
        }

        println!(
            "\n  Lightning v{}\n  ─────────────────────────────────────\n  Bot         {}\n  Servers     {}\n  Commands    {}\n  Shards      {}\n",
            env!("CARGO_PKG_VERSION"),
            bot,
            guilds,
            self.commands.len(),
            self.shard_total,
        );
    }

    // -----------------------------------------------------------------------
    // Interactions
    // -----------------------------------------------------------------------

    async fn handle_interaction(self: Arc<Self>, interaction: Interaction, shard_id: u32) {
        if interaction.kind == InteractionType::Ping {
            self.respond_raw(&interaction, convert::pong()).await;
            return;
        }

        if interaction.kind == InteractionType::ApplicationCommandAutocomplete {
            debug!("autocomplete not supported yet: replying with an empty list");
            self.respond_raw(&interaction, convert::empty_autocomplete())
                .await;
            return;
        }

        let Some(data) = interaction.data.clone() else {
            return;
        };

        match data {
            InteractionData::ApplicationCommand(data) => {
                self.slash(interaction, *data, shard_id).await;
            }
            InteractionData::MessageComponent(data) => {
                self.component(interaction, *data).await;
            }
            InteractionData::ModalSubmit(data) => {
                self.modal(interaction, *data).await;
            }
            _ => {}
        }
    }

    /// Sends a low-level response (ping, autocomplete…).
    async fn respond_raw(
        &self,
        interaction: &Interaction,
        response: twilight_model::http::interaction::InteractionResponse,
    ) {
        let client = self.http.interaction(self.application_id);

        if let Err(error) = client
            .create_response(interaction.id, &interaction.token, &response)
            .await
        {
            warn!(%error, "could not respond to the interaction");
        }
    }

    async fn slash(self: &Arc<Self>, interaction: Interaction, data: CommandData, _shard_id: u32) {
        let slot = self
            .commands
            .iter()
            .find(|slot| slot.command.matches(&data.name) && slot.command.kind.allows(true));

        let Some(slot) = slot else {
            warn!(command = %data.name, "unknown slash command");
            self.reject(
                &interaction,
                "that command does not exist (did you restart the bot?)",
            )
            .await;
            return;
        };

        let options = Some(convert::option_map(&data.options));
        let Some(ctx) = self.interaction_context(
            &interaction,
            slot.command.name,
            Surface::Slash,
            options,
            Vec::new(),
            InitialKind::Message,
        ) else {
            return;
        };

        self.execute(slot, ctx).await;
    }

    async fn component(
        self: &Arc<Self>,
        interaction: Interaction,
        data: MessageComponentInteractionData,
    ) {
        let (id, payload) = convert::split_custom_id(&data.custom_id);

        let Some(handler) = self.components.iter().find(|handler| handler.id == id) else {
            warn!(custom_id = %data.custom_id, "component without a handler");
            return;
        };

        let Some(ctx) = self.interaction_context(
            &interaction,
            handler.id,
            Surface::Component,
            payload,
            data.values.clone(),
            InitialKind::UpdateMessage,
        ) else {
            return;
        };

        let run = handler.run;
        let reply = middleware::run_pipeline(&self.middleware, &[], run, ctx.clone()).await;

        self.reply_to(ctx, reply).await;
    }

    async fn modal(self: &Arc<Self>, interaction: Interaction, data: ModalInteractionData) {
        let (id, _) = convert::split_custom_id(&data.custom_id);

        let Some(handler) = self.components.iter().find(|handler| handler.id == id) else {
            warn!(custom_id = %data.custom_id, "modal without a handler");
            return;
        };

        let serialized = serde_json::to_value(&data.components).unwrap_or_default();
        let options = convert::modal_map(&serialized);

        let Some(ctx) = self.interaction_context(
            &interaction,
            handler.id,
            Surface::Modal,
            Some(options),
            Vec::new(),
            InitialKind::Message,
        ) else {
            return;
        };

        let run = handler.run;
        let reply = middleware::run_pipeline(&self.middleware, &[], run, ctx.clone()).await;

        self.reply_to(ctx, reply).await;
    }

    fn interaction_context(
        &self,
        interaction: &Interaction,
        command: &'static str,
        surface: Surface,
        options: Option<Map<String, serde_json::Value>>,
        values: Vec<String>,
        kind: InitialKind,
    ) -> Option<Context> {
        let user = interaction.author()?.clone();
        #[allow(deprecated)]
        let channel_id = interaction
            .channel
            .as_ref()
            .map(|channel| channel.id)
            .or(interaction.channel_id);

        let responder = Arc::new(TwilightResponder::interaction(
            Arc::clone(&self.http),
            self.application_id,
            interaction.id,
            interaction.token.clone(),
            kind,
        ));

        Some(Context {
            command,
            surface,
            user,
            member: interaction.member.clone(),
            guild_id: interaction.guild_id,
            channel_id,
            message_id: interaction.message.as_ref().map(|message| message.id),
            interaction: Some(interaction.clone()),
            message: interaction.message.clone(),
            options,
            tokens: Vec::new(),
            values,
            shard_id: 0,
            responder,
            state: Arc::clone(&self.state),
            extras: self.extras(),
        })
    }

    async fn reject(&self, interaction: &Interaction, message: &str) {
        let Some(user) = interaction.author() else {
            return;
        };

        let responder = Arc::new(TwilightResponder::interaction(
            Arc::clone(&self.http),
            self.application_id,
            interaction.id,
            interaction.token.clone(),
            InitialKind::Message,
        ));

        let context = Context {
            command: "reject",
            surface: Surface::Slash,
            user: user.clone(),
            member: interaction.member.clone(),
            guild_id: interaction.guild_id,
            channel_id: None,
            message_id: None,
            interaction: None,
            message: None,
            options: None,
            tokens: Vec::new(),
            values: Vec::new(),
            shard_id: 0,
            responder,
            state: Arc::clone(&self.state),
            extras: self.extras(),
        };

        if let Err(error) = context.reply(Reply::text(message).ephemeral()).await {
            warn!(%error, "could not reject the interaction");
        }
    }

    // -----------------------------------------------------------------------
    // Prefix commands
    // -----------------------------------------------------------------------

    async fn handle_message(self: Arc<Self>, message: Message, _shard_id: u32) {
        let Some(prefix) = self.prefix.clone() else {
            return;
        };
        if message.author.bot {
            return;
        }
        let Some(rest) = message.content.strip_prefix(prefix.as_str()) else {
            return;
        };

        let tokens = tokenize(rest);
        let Some((name, args)) = tokens.split_first() else {
            return;
        };

        let slot = self
            .commands
            .iter()
            .find(|slot| slot.command.matches(name) && slot.command.kind.allows(false));

        let Some(slot) = slot else {
            debug!(%name, "unknown prefix command");
            return;
        };

        let channel_id = message.channel_id;
        let responder = Arc::new(TwilightResponder::channel(
            Arc::clone(&self.http),
            channel_id,
        ));

        let ctx = Context {
            command: slot.command.name,
            surface: Surface::Prefix,
            user: message.author.clone(),
            member: message.member.clone(),
            guild_id: message.guild_id,
            channel_id: Some(channel_id),
            message_id: Some(message.id),
            interaction: None,
            message: Some(message),
            options: None,
            tokens: args.to_vec(),
            values: Vec::new(),
            shard_id: 0,
            responder,
            state: Arc::clone(&self.state),
            extras: self.extras(),
        };

        self.execute(slot, ctx).await;
    }

    // -----------------------------------------------------------------------
    // Execution
    // -----------------------------------------------------------------------

    async fn execute(self: &Arc<Self>, slot: &CommandSlot, ctx: Context) {
        if slot.command.guild_only && ctx.guild_id.is_none() {
            let _ = ctx.reply(Error::GuildOnly.to_reply()).await;
            return;
        }

        let Some(run) = slot.command.run() else {
            warn!(command = %slot.command.name, "the command has no handler");
            return;
        };

        let reply =
            middleware::run_pipeline(&self.middleware, &slot.layers, run, ctx.clone()).await;

        self.reply_to(ctx, reply).await;
    }

    async fn reply_to(&self, ctx: Context, reply: Result<Reply>) {
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                warn!(command = %ctx.command, %error, "the handler failed");
                error.to_reply()
            }
        };

        if reply.is_empty() {
            return;
        }

        if let Err(error) = ctx.reply(reply).await {
            warn!(%error, "could not send the reply");
        }
    }

    fn extras(&self) -> Arc<StateMap> {
        let mut extras = StateMap::new();
        extras.insert_arc(Arc::clone(&self.http));
        extras.insert(self.voice.clone());
        extras.insert(self.voice.states().clone());
        Arc::new(extras)
    }
}

/// Starts a shard and does not return until the connection closes.
pub(crate) async fn run_shard(
    runtime: Arc<Runtime>,
    token: String,
    intents: Intents,
    shard_id: u32,
    total: u32,
) {
    let mut shard = Shard::new(ShardId::new(shard_id, total), token, intents);

    // Voice state updates are sent through the shard that owns the guild.
    runtime.voice.register_shard(shard_id, shard.sender());

    loop {
        let Some(item) = shard.next_event(EventTypeFlags::all()).await else {
            break;
        };

        match item {
            Ok(event) => runtime.dispatch(event, shard_id).await,
            Err(error) => {
                warn!(shard = shard_id, %error, "error receiving event");
            }
        }
    }

    info!(shard = shard_id, "shard disconnected");
}

/// Splits text while respecting double quotes: `!echo "hello world"` → `echo`, `hello world`.
pub(crate) fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;

    for character in input.chars() {
        match character {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::tokenize;

    #[test]
    fn tokenizes_while_respecting_quotes() {
        assert_eq!(tokenize("echo hello world"), vec!["echo", "hello", "world"]);
        assert_eq!(
            tokenize(r#"echo "hello world""#),
            vec!["echo", "hello world"]
        );
        assert_eq!(tokenize(r#"  "a b"  c  "#), vec!["a b", "c"]);
    }
}
