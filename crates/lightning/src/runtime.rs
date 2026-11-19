//! The engine: registration, shards, event dispatch and command execution.

use std::sync::Arc;

use lightning_core::command::Command;
use lightning_core::context::Surface;
use lightning_core::middleware::{self, Middleware};
use lightning_core::{
    AutocompleteHandler, Context, Error, Focused, OptionKind, Reply, Result, StateMap, Subcommand,
};
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

/// A subcommand plus its already-built middleware layers.
pub(crate) struct SubcommandSlot {
    subcommand: Subcommand,
    layers: Arc<[Arc<dyn Middleware>]>,
}

/// State shared by every shard.
pub(crate) struct Runtime {
    pub http: Arc<Client>,
    pub application_id: Id<ApplicationMarker>,
    pub commands: Vec<CommandSlot>,
    pub subcommands: Vec<SubcommandSlot>,
    pub components: Vec<ComponentHandler>,
    pub autocompletes: Vec<AutocompleteHandler>,
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
    pub subcommands: Vec<Subcommand>,
    pub components: Vec<ComponentHandler>,
    pub autocompletes: Vec<AutocompleteHandler>,
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
            subcommands,
            components,
            autocompletes,
            prefix,
            state,
            middleware,
            shard_total,
            voice,
        } = config;

        for command in &commands {
            let has_children = subcommands
                .iter()
                .any(|subcommand| subcommand.command == command.name);

            if has_children && !command.options.is_empty() {
                warn!(
                    command = command.name,
                    "the command mixes its own options with subcommands; Discord does not allow that, so the options are ignored"
                );
            }
        }

        for handler in &autocompletes {
            let Some(command) = commands
                .iter()
                .find(|command| command.name == handler.command)
            else {
                warn!(
                    command = handler.command,
                    option = handler.option,
                    "autocomplete handler points to an unknown command"
                );
                continue;
            };

            if !command.kind.allows(true) {
                warn!(
                    command = handler.command,
                    option = handler.option,
                    "autocomplete requires a slash command"
                );
                continue;
            }

            if handler.group.is_some() && handler.subcommand.is_none() {
                warn!(
                    command = handler.command,
                    option = handler.option,
                    "an autocomplete group requires a subcommand"
                );
                continue;
            }

            let options = if let Some(name) = handler.subcommand {
                match subcommands
                    .iter()
                    .find(|subcommand| subcommand.matches(handler.command, handler.group, name))
                {
                    Some(subcommand) => subcommand.options,
                    None => {
                        warn!(
                            command = handler.command,
                            group = handler.group.unwrap_or("-"),
                            subcommand = name,
                            option = handler.option,
                            "autocomplete handler points to an unknown subcommand"
                        );
                        continue;
                    }
                }
            } else {
                if subcommands
                    .iter()
                    .any(|subcommand| subcommand.command == command.name)
                {
                    warn!(
                        command = handler.command,
                        option = handler.option,
                        "autocomplete handler points to a parent option, but the command only exposes subcommands"
                    );
                    continue;
                }
                command.options
            };

            match options.iter().find(|option| option.name == handler.option) {
                None => warn!(
                    command = handler.command,
                    option = handler.option,
                    "autocomplete handler points to an unknown option"
                ),
                Some(option) if matches!(option.kind, OptionKind::Boolean) => warn!(
                    command = handler.command,
                    option = handler.option,
                    "Discord cannot autocomplete boolean options"
                ),
                Some(_) => {}
            }
        }

        let commands: Vec<CommandSlot> = commands
            .into_iter()
            .map(|command| {
                let layers: Arc<[Arc<dyn Middleware>]> = command.build_layers().into();
                CommandSlot { command, layers }
            })
            .collect();

        let mut seen = std::collections::HashSet::new();
        let subcommands = subcommands
            .into_iter()
            .filter_map(|subcommand| {
                if !commands
                    .iter()
                    .any(|slot| slot.command.name == subcommand.command)
                {
                    warn!(
                        command = subcommand.command,
                        subcommand = subcommand.name,
                        "subcommand points to an unknown command"
                    );
                    return None;
                }

                if !seen.insert((subcommand.command, subcommand.group, subcommand.name)) {
                    warn!(
                        command = subcommand.command,
                        group = subcommand.group.unwrap_or("-"),
                        subcommand = subcommand.name,
                        "duplicate subcommand, skipping"
                    );
                    return None;
                }

                let layers: Arc<[Arc<dyn Middleware>]> = subcommand.build_layers().into();
                Some(SubcommandSlot { subcommand, layers })
            })
            .collect();

        Self {
            http,
            application_id,
            commands,
            subcommands,
            components,
            autocompletes,
            prefix,
            state,
            middleware: middleware.into(),
            shard_total,
            voice,
        }
    }

    /// Publishes the slash commands to Discord.
    pub(crate) async fn register(&self, guild_id: Option<Id<GuildMarker>>) -> Result<()> {
        let subcommands: Vec<Subcommand> = self
            .subcommands
            .iter()
            .map(|slot| slot.subcommand.clone())
            .collect();
        let commands = convert::to_commands(
            self.commands.iter().map(|slot| &slot.command),
            &subcommands,
            &self.autocompletes,
        );

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
                // Cheap pre-filter before spawning: most messages are plain
                // chat, and cloning them just to discard the task is wasteful.
                let is_command = self.prefix.as_deref().is_some_and(|prefix| {
                    !created.message.author.bot && created.message.content.starts_with(prefix)
                });

                if is_command {
                    let runtime = Arc::clone(self);
                    let message = created.message.clone();
                    tokio::spawn(async move {
                        runtime.handle_message(message, shard_id).await;
                    });
                }
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
            self.autocomplete(interaction).await;
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

    /// Answers an autocomplete interaction with typed choices.
    ///
    /// Middleware is deliberately skipped: autocomplete runs on every
    /// keystroke, so cooldowns and similar layers would be wrong here.
    async fn autocomplete(&self, interaction: Interaction) {
        let Some(InteractionData::ApplicationCommand(data)) = interaction.data.clone() else {
            self.respond_raw(&interaction, convert::autocomplete(&[]))
                .await;
            return;
        };

        let Some(focused) = convert::focused_option(&data.options) else {
            debug!(command = %data.name, "autocomplete without a focused option");
            self.respond_raw(&interaction, convert::autocomplete(&[]))
                .await;
            return;
        };

        let handler = self
            .autocompletes
            .iter()
            .find(|handler| {
                handler.matches(
                    &data.name,
                    focused.group.as_deref(),
                    focused.subcommand.as_deref(),
                    &focused.name,
                )
            })
            .copied();

        let Some(handler) = handler else {
            debug!(
                command = %data.name,
                option = %focused.name,
                "no autocomplete handler for this option"
            );
            self.respond_raw(&interaction, convert::autocomplete(&[]))
                .await;
            return;
        };

        let options = Some(convert::option_map(&data.options));
        let Some(mut ctx) = self.interaction_context(
            &interaction,
            handler.command,
            Surface::Autocomplete,
            options,
            Vec::new(),
            InitialKind::Message,
        ) else {
            return;
        };
        ctx.group = handler.group;
        ctx.subcommand = handler.subcommand;
        ctx.focused = Some(Focused::new(focused.name, focused.value));

        let choices = match (handler.run)(&ctx).await {
            Ok(choices) => choices,
            Err(error) => {
                warn!(command = %data.name, %error, "autocomplete handler failed");
                Vec::new()
            }
        };

        self.respond_raw(&interaction, convert::autocomplete(&choices))
            .await;
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

        let has_subcommands = self
            .subcommands
            .iter()
            .any(|subcommand| subcommand.subcommand.command == slot.command.name);
        let path = convert::selected_subcommand(&data.options);

        let selected = if has_subcommands {
            let Some(path) = path else {
                self.reject(&interaction, "choose a subcommand for this command.")
                    .await;
                return;
            };

            let selected =
                self.find_subcommand(slot.command.name, path.group.as_deref(), &path.name);

            let Some(selected) = selected else {
                self.reject(&interaction, "that subcommand does not exist.")
                    .await;
                return;
            };

            Some(selected)
        } else {
            None
        };

        let options = Some(convert::option_map(&data.options));
        let Some(mut ctx) = self.interaction_context(
            &interaction,
            slot.command.name,
            Surface::Slash,
            options,
            Vec::new(),
            InitialKind::Message,
        ) else {
            return;
        };

        if let Some(subcommand) = selected {
            ctx.group = subcommand.subcommand.group;
            ctx.subcommand = Some(subcommand.subcommand.name);
            self.execute_subcommand(slot, subcommand, ctx).await;
        } else {
            self.execute(slot, ctx).await;
        }
    }

    fn find_subcommand(
        &self,
        command: &str,
        group: Option<&str>,
        name: &str,
    ) -> Option<&SubcommandSlot> {
        self.subcommands
            .iter()
            .find(|slot| slot.subcommand.matches(command, group, name))
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
            group: None,
            subcommand: None,
            surface,
            user,
            member: interaction.member.clone(),
            guild_id: interaction.guild_id,
            channel_id,
            message_id: interaction.message.as_ref().map(|message| message.id),
            interaction: Some(interaction.clone()),
            message: interaction.message.clone(),
            options,
            focused: None,
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
            group: None,
            subcommand: None,
            surface: Surface::Slash,
            user: user.clone(),
            member: interaction.member.clone(),
            guild_id: interaction.guild_id,
            channel_id: None,
            message_id: None,
            interaction: None,
            message: None,
            options: None,
            focused: None,
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

        let has_subcommands = self
            .subcommands
            .iter()
            .any(|subcommand| subcommand.subcommand.command == slot.command.name);
        let (selected, consumed) = if has_subcommands {
            match self.find_prefix_subcommand(slot.command.name, args) {
                Some((subcommand, consumed)) => (Some(subcommand), consumed),
                None => (None, 0),
            }
        } else {
            (None, 0)
        };

        let channel_id = message.channel_id;
        let responder = Arc::new(TwilightResponder::channel(
            Arc::clone(&self.http),
            channel_id,
        ));

        let mut ctx = Context {
            command: slot.command.name,
            group: None,
            subcommand: None,
            surface: Surface::Prefix,
            user: message.author.clone(),
            member: message.member.clone(),
            guild_id: message.guild_id,
            channel_id: Some(channel_id),
            message_id: Some(message.id),
            interaction: None,
            message: Some(message),
            options: None,
            focused: None,
            tokens: args[consumed..].to_vec(),
            values: Vec::new(),
            shard_id: 0,
            responder,
            state: Arc::clone(&self.state),
            extras: self.extras(),
        };

        if let Some(subcommand) = selected {
            ctx.group = subcommand.subcommand.group;
            ctx.subcommand = Some(subcommand.subcommand.name);
            self.execute_subcommand(slot, subcommand, ctx).await;
        } else {
            self.execute(slot, ctx).await;
        }
    }

    fn find_prefix_subcommand<'a>(
        &'a self,
        command: &str,
        tokens: &[String],
    ) -> Option<(&'a SubcommandSlot, usize)> {
        let first = tokens.first()?;

        if let Some(subcommand) = self.find_subcommand(command, None, first) {
            return Some((subcommand, 1));
        }

        let second = tokens.get(1)?;
        self.find_subcommand(command, Some(first), second)
            .map(|subcommand| (subcommand, 2))
    }

    // -----------------------------------------------------------------------
    // Execution
    // -----------------------------------------------------------------------

    async fn execute(self: &Arc<Self>, slot: &CommandSlot, ctx: Context) {
        self.execute_with(&slot.command, slot.command.run(), &slot.layers, ctx)
            .await;
    }

    async fn execute_subcommand(
        self: &Arc<Self>,
        parent: &CommandSlot,
        slot: &SubcommandSlot,
        ctx: Context,
    ) {
        self.execute_with(&parent.command, slot.subcommand.run(), &slot.layers, ctx)
            .await;
    }

    async fn execute_with(
        self: &Arc<Self>,
        command: &Command,
        run: Option<lightning_core::command::RunFn>,
        layers: &[Arc<dyn Middleware>],
        ctx: Context,
    ) {
        if command.guild_only && ctx.guild_id.is_none() {
            let _ = ctx.reply(Error::GuildOnly.to_reply()).await;
            return;
        }

        let Some(run) = run else {
            warn!(command = %ctx.command_path(), "the command has no handler");
            return;
        };

        let reply = middleware::run_pipeline(&self.middleware, layers, run, ctx.clone()).await;

        self.reply_to(ctx, reply).await;
    }

    async fn reply_to(&self, ctx: Context, reply: Result<Reply>) {
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                warn!(command = %ctx.command_path(), %error, "the handler failed");
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
    use std::sync::Arc;

    use lightning_core::command::Command;
    use lightning_core::{StateMap, Subcommand};
    use twilight_http::Client;
    use twilight_model::id::Id;

    use super::{Runtime, RuntimeConfig, tokenize};
    use crate::voice::Voice;
    use crate::voice_states::VoiceStates;

    #[test]
    fn tokenizes_while_respecting_quotes() {
        assert_eq!(tokenize("echo hello world"), vec!["echo", "hello", "world"]);
        assert_eq!(
            tokenize(r#"echo "hello world""#),
            vec!["echo", "hello world"]
        );
        assert_eq!(tokenize(r#"  "a b"  c  "#), vec!["a b", "c"]);
    }

    fn runtime_with_subcommands() -> Runtime {
        let http = Arc::new(Client::new("test".to_owned()));
        let voice = Voice::new(Arc::clone(&http), VoiceStates::new(), 1);

        Runtime::new(
            http,
            RuntimeConfig {
                application_id: Id::new(1),
                commands: vec![Command::new("config", "Manage configuration")],
                subcommands: vec![
                    Subcommand::new("config", "show", "Show the configuration"),
                    Subcommand::new("config", "channel", "Change the channel")
                        .group("set", "Change settings"),
                ],
                components: Vec::new(),
                autocompletes: Vec::new(),
                prefix: Some("!".to_owned()),
                state: Arc::new(StateMap::new()),
                middleware: Vec::new(),
                shard_total: 1,
                voice,
            },
        )
    }

    #[tokio::test]
    async fn prefix_routing_finds_direct_and_grouped_subcommands() {
        let runtime = runtime_with_subcommands();

        let tokens = vec!["show".to_owned(), "extra".to_owned()];
        let (subcommand, consumed) = runtime
            .find_prefix_subcommand("config", &tokens)
            .expect("the direct subcommand should match");
        assert_eq!(subcommand.subcommand.name, "show");
        assert_eq!(consumed, 1);

        let tokens = vec!["set".to_owned(), "channel".to_owned(), "extra".to_owned()];
        let (subcommand, consumed) = runtime
            .find_prefix_subcommand("config", &tokens)
            .expect("the grouped subcommand should match");
        assert_eq!(subcommand.subcommand.name, "channel");
        assert_eq!(consumed, 2);

        let tokens = vec!["set".to_owned()];
        assert!(runtime.find_prefix_subcommand("config", &tokens).is_none());
    }
}
