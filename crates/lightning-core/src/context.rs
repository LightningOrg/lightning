//! The [`Context`]: everything a handler needs to know about the invocation.

use std::sync::Arc;

use serde_json::{Map, Value};
use twilight_model::channel::message::Message as DiscordMessage;
use twilight_model::guild::PartialMember;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker};
use twilight_model::user::User;

use crate::args::{Args, CommandArgs};
use crate::autocomplete::Focused;
use crate::error::{Error, Result};
use crate::reply::Reply;
use crate::state::{State, StateMap};

/// How the invocation arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Slash command (`/ping`).
    Slash,
    /// Prefix command (`!ping`).
    Prefix,
    /// Button or menu (`custom_id`).
    Component,
    /// Modal submit.
    Modal,
    /// Autocomplete for a slash command option.
    Autocomplete,
}

impl Surface {
    /// `true` if the invocation came from an interaction.
    pub const fn is_interaction(self) -> bool {
        !matches!(self, Self::Prefix)
    }

    /// `true` if it is a slash command.
    pub const fn is_slash(self) -> bool {
        matches!(self, Self::Slash)
    }

    /// `true` if it is a prefix command.
    pub const fn is_prefix(self) -> bool {
        matches!(self, Self::Prefix)
    }

    /// `true` if it is an autocomplete interaction.
    pub const fn is_autocomplete(self) -> bool {
        matches!(self, Self::Autocomplete)
    }
}

/// Something that knows how to send a reply. The runtime implements it with
/// HTTP; tests implement it with a `Vec`.
#[crate::async_trait]
pub trait Responder: Send + Sync + 'static {
    /// Sends the first response. If `defer` already happened, the runtime decides
    /// between editing the original response or sending a followup.
    async fn respond(&self, reply: Reply) -> Result<()>;

    /// Acknowledges the interaction (staying under Discord's 3 s timeout).
    async fn defer(&self, ephemeral: bool) -> Result<()>;

    /// Sends an additional message.
    async fn followup(&self, reply: Reply) -> Result<()>;
}

/// Runtime state and services (HTTP client, app id…).
pub type Extras = StateMap;

/// Context of an invocation.
///
/// It is cheap to clone (`Arc` inside), so you can move it into tasks.
#[derive(Clone)]
pub struct Context {
    /// Name of the command being executed.
    pub command: &'static str,
    /// Surface of the invocation.
    pub surface: Surface,
    /// Author of the invocation.
    pub user: User,
    /// Guild member, when applicable.
    pub member: Option<PartialMember>,
    /// Guild, when applicable.
    pub guild_id: Option<Id<GuildMarker>>,
    /// Channel where the command was invoked.
    pub channel_id: Option<Id<ChannelMarker>>,
    /// Source message (components) or prefix command message.
    pub message_id: Option<Id<MessageMarker>>,
    /// The raw interaction.
    pub interaction: Option<twilight_model::application::interaction::Interaction>,
    /// Source message, for prefix commands.
    pub message: Option<DiscordMessage>,
    /// Options already normalized to JSON (slash, components and modals).
    pub options: Option<Map<String, Value>>,
    /// Option the user is typing, during autocomplete.
    pub focused: Option<Focused>,
    /// Tokens of a prefix command.
    pub tokens: Vec<String>,
    /// Values of a select menu.
    pub values: Vec<String>,
    /// Id of the shard that received the event.
    pub shard_id: u32,
    /// Output channel.
    pub responder: Arc<dyn Responder>,
    /// User-injected state.
    pub state: Arc<StateMap>,
    /// Runtime-internal services.
    pub extras: Arc<Extras>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("command", &self.command)
            .field("surface", &self.surface)
            .field("user", &self.user.id)
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// Retrieves an injected state.
    pub fn try_state<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.state.get::<T>()
    }

    /// Retrieves an injected state or fails with [`Error::MissingState`].
    pub fn state<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.try_state::<T>()
            .ok_or(Error::MissingState(std::any::type_name::<T>()))
    }

    /// Retrieves a runtime service (for example the HTTP client).
    pub fn ext<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.extras.get::<T>()
    }

    /// Guild id or [`Error::GuildOnly`].
    pub fn guild_id(&self) -> Result<Id<GuildMarker>> {
        self.guild_id.ok_or(Error::GuildOnly)
    }

    /// Channel id or [`Error::MissingResource`].
    pub fn channel_id(&self) -> Result<Id<ChannelMarker>> {
        self.channel_id
            .ok_or(Error::MissingResource("output channel"))
    }

    /// `true` if the invocation happened inside a guild.
    pub fn is_guild(&self) -> bool {
        self.guild_id.is_some()
    }

    /// `true` if this is an interaction invocation.
    pub fn is_interaction(&self) -> bool {
        self.surface.is_interaction()
    }

    /// Replies to the user.
    pub async fn reply(&self, reply: impl Into<Reply>) -> Result<()> {
        self.responder.respond(reply.into()).await
    }

    /// Acknowledges an interaction (no-op for prefix commands).
    pub async fn defer(&self) -> Result<()> {
        self.defer_ephemeral(false).await
    }

    /// Acknowledges an interaction with an ephemeral reply.
    pub async fn defer_ephemeral(&self, ephemeral: bool) -> Result<()> {
        if self.surface.is_interaction() {
            self.responder.defer(ephemeral).await
        } else {
            Ok(())
        }
    }

    /// Sends an extra message.
    pub async fn followup(&self, reply: impl Into<Reply>) -> Result<()> {
        self.responder.followup(reply.into()).await
    }

    /// Creates a test context with a fake responder.
    ///
    /// ```
    /// use lightning_core::Context;
    /// use std::sync::Arc;
    ///
    /// # struct Noop;
    /// # #[lightning_core::async_trait]
    /// # impl lightning_core::context::Responder for Noop {
    /// #     async fn respond(&self, _: lightning_core::Reply) -> lightning_core::Result<()> { Ok(()) }
    /// #     async fn defer(&self, _: bool) -> lightning_core::Result<()> { Ok(()) }
    /// #     async fn followup(&self, _: lightning_core::Reply) -> lightning_core::Result<()> { Ok(()) }
    /// # }
    /// let ctx = Context::test("ping", Arc::new(Noop));
    /// assert_eq!(ctx.command, "ping");
    /// ```
    pub fn test(command: &'static str, responder: Arc<dyn Responder>) -> Self {
        Self {
            command,
            surface: Surface::Slash,
            user: test_user(),
            member: None,
            guild_id: None,
            channel_id: Some(Id::new(1)),
            message_id: None,
            interaction: None,
            message: None,
            options: None,
            focused: None,
            tokens: Vec::new(),
            values: Vec::new(),
            shard_id: 0,
            responder,
            state: Arc::new(StateMap::new()),
            extras: Arc::new(Extras::new()),
        }
    }
}

fn test_user() -> User {
    User {
        accent_color: None,
        avatar: None,
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator: 0,
        email: None,
        flags: None,
        global_name: Some("Test".into()),
        id: Id::new(42),
        locale: None,
        mfa_enabled: None,
        name: "test".into(),
        premium_type: None,
        primary_guild: None,
        public_flags: None,
        system: None,
        verified: None,
    }
}

/// Extracts a value from the [`Context`]. The axum *extractor* equivalent.
#[crate::async_trait]
pub trait FromContext<'a>: Sized {
    /// Builds the value from the context.
    async fn from_context(ctx: &'a Context) -> Result<Self>;
}

#[crate::async_trait]
impl<'a> FromContext<'a> for Context {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(ctx.clone())
    }
}

#[crate::async_trait]
impl<'a, T> FromContext<'a> for Args<T>
where
    T: CommandArgs,
{
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        if let Some(options) = &ctx.options {
            return T::from_options(options.clone()).map(Args);
        }

        T::from_prefix(&ctx.tokens).map(Args)
    }
}

#[crate::async_trait]
impl<'a, T> FromContext<'a> for State<T>
where
    T: Send + Sync + 'static,
{
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.state::<T>().map(State)
    }
}

/// Extractor for the current guild.
#[derive(Debug, Clone, Copy)]
pub struct Guild(pub Id<GuildMarker>);

#[crate::async_trait]
impl<'a> FromContext<'a> for Guild {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.guild_id().map(Guild)
    }
}

/// Extractor for the member who invoked the command.
#[derive(Debug, Clone)]
pub struct Member(pub PartialMember);

#[crate::async_trait]
impl<'a> FromContext<'a> for Member {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.member
            .clone()
            .map(Member)
            .ok_or(Error::MissingResource("member"))
    }
}

/// Extractor for the author.
#[derive(Debug, Clone)]
pub struct Author(pub User);

#[crate::async_trait]
impl<'a> FromContext<'a> for Author {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(Author(ctx.user.clone()))
    }
}

/// Extractor for the channel.
#[derive(Debug, Clone, Copy)]
pub struct ChannelId(pub Id<ChannelMarker>);

#[crate::async_trait]
impl<'a> FromContext<'a> for ChannelId {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.channel_id().map(ChannelId)
    }
}

/// Extractor for the source message.
#[derive(Debug, Clone)]
pub struct Message(pub DiscordMessage);

#[crate::async_trait]
impl<'a> FromContext<'a> for Message {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.message
            .clone()
            .map(Message)
            .ok_or(Error::MissingResource("message"))
    }
}

/// Extractor for the raw interaction.
#[derive(Debug, Clone)]
pub struct Interaction(pub twilight_model::application::interaction::Interaction);

#[crate::async_trait]
impl<'a> FromContext<'a> for Interaction {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.interaction
            .clone()
            .map(Interaction)
            .ok_or(Error::MissingResource("interaction"))
    }
}

/// Extractor for select menu values.
#[derive(Debug, Clone)]
pub struct Values(pub Vec<String>);

#[crate::async_trait]
impl<'a> FromContext<'a> for Values {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(Values(ctx.values.clone()))
    }
}

/// Extractor for the user id.
#[derive(Debug, Clone, Copy)]
pub struct UserId(pub Id<UserMarker>);

#[crate::async_trait]
impl<'a> FromContext<'a> for UserId {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(UserId(ctx.user.id))
    }
}
