//! El [`Context`]: todo lo que un handler necesita saber de la invocación.

use std::sync::Arc;

use serde_json::{Map, Value};
use twilight_model::channel::message::Message as DiscordMessage;
use twilight_model::guild::PartialMember;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker};
use twilight_model::user::User;

use crate::args::{Args, CommandArgs};
use crate::error::{Error, Result};
use crate::reply::Reply;
use crate::state::{State, StateMap};

/// Por dónde llegó la invocación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Comando de barra (`/ping`).
    Slash,
    /// Comando de prefijo (`!ping`).
    Prefix,
    /// Botón o menú (`custom_id`).
    Component,
    /// Envío de un modal.
    Modal,
}

impl Surface {
    /// `true` si la invocación vino de una interacción.
    pub const fn is_interaction(self) -> bool {
        !matches!(self, Self::Prefix)
    }

    /// `true` si es un comando de barra.
    pub const fn is_slash(self) -> bool {
        matches!(self, Self::Slash)
    }

    /// `true` si es un comando de prefijo.
    pub const fn is_prefix(self) -> bool {
        matches!(self, Self::Prefix)
    }
}

/// Algo que sabe enviar una respuesta. El runtime lo implementa con HTTP; los
/// tests lo implementan con un `Vec`.
#[crate::async_trait]
pub trait Responder: Send + Sync + 'static {
    /// Envía la primera respuesta. Si ya se había hecho `defer`, el runtime
    /// decide entre editar la respuesta original o mandar un followup.
    async fn respond(&self, reply: Reply) -> Result<()>;

    /// Acusa recibo (para no pasarse del timeout de 3 s de Discord).
    async fn defer(&self, ephemeral: bool) -> Result<()>;

    /// Envía un mensaje adicional.
    async fn followup(&self, reply: Reply) -> Result<()>;
}

/// Estados y servicios internos del runtime (cliente HTTP, app id...).
pub type Extras = StateMap;

/// Contexto de una invocación.
///
/// Es barato de clonar (`Arc` por dentro), así que puedes pasarlo a tareas.
#[derive(Clone)]
pub struct Context {
    /// Nombre del comando que se está ejecutando.
    pub command: &'static str,
    /// Superficie de la invocación.
    pub surface: Surface,
    /// Autor de la invocación.
    pub user: User,
    /// Miembro del servidor, si aplica.
    pub member: Option<PartialMember>,
    /// Servidor, si aplica.
    pub guild_id: Option<Id<GuildMarker>>,
    /// Canal donde se invocó.
    pub channel_id: Option<Id<ChannelMarker>>,
    /// Mensaje de origen (componentes) o del comando de prefijo.
    pub message_id: Option<Id<MessageMarker>>,
    /// Interacción original.
    pub interaction: Option<twilight_model::application::interaction::Interaction>,
    /// Mensaje original, en comandos de prefijo.
    pub message: Option<DiscordMessage>,
    /// Opciones ya normalizadas a JSON (slash, componentes y modales).
    pub options: Option<Map<String, Value>>,
    /// Tokens de un comando de prefijo.
    pub tokens: Vec<String>,
    /// Valores de un menú de selección.
    pub values: Vec<String>,
    /// Id del shard que recibió el evento.
    pub shard_id: u32,
    /// Canal de salida.
    pub responder: Arc<dyn Responder>,
    /// Estados inyectados por el usuario.
    pub state: Arc<StateMap>,
    /// Servicios internos del runtime.
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
    /// Recupera un estado inyectado.
    pub fn try_state<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.state.get::<T>()
    }

    /// Recupera un estado inyectado o falla con [`Error::MissingState`].
    pub fn state<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.try_state::<T>()
            .ok_or(Error::MissingState(std::any::type_name::<T>()))
    }

    /// Recupera un servicio del runtime (por ejemplo el cliente HTTP).
    pub fn ext<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.extras.get::<T>()
    }

    /// Id del servidor o [`Error::GuildOnly`].
    pub fn guild_id(&self) -> Result<Id<GuildMarker>> {
        self.guild_id.ok_or(Error::GuildOnly)
    }

    /// Id del canal o [`Error::MissingResource`].
    pub fn channel_id(&self) -> Result<Id<ChannelMarker>> {
        self.channel_id
            .ok_or(Error::MissingResource("canal de salida"))
    }

    /// `true` si la invocación ocurrió en un servidor.
    pub fn is_guild(&self) -> bool {
        self.guild_id.is_some()
    }

    /// `true` si es una invocación por interacción.
    pub fn is_interaction(&self) -> bool {
        self.surface.is_interaction()
    }

    /// Responde al usuario.
    pub async fn reply(&self, reply: impl Into<Reply>) -> Result<()> {
        self.responder.respond(reply.into()).await
    }

    /// Acusa recibo de una interacción (no-op en prefijo).
    pub async fn defer(&self) -> Result<()> {
        self.defer_ephemeral(false).await
    }

    /// Acusa recibo con respuesta efímera.
    pub async fn defer_ephemeral(&self, ephemeral: bool) -> Result<()> {
        if self.surface.is_interaction() {
            self.responder.defer(ephemeral).await
        } else {
            Ok(())
        }
    }

    /// Envía un mensaje extra.
    pub async fn followup(&self, reply: impl Into<Reply>) -> Result<()> {
        self.responder.followup(reply.into()).await
    }

    /// Crea un contexto de prueba con un responder falso.
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

/// Extrae un valor del [`Context`]. Es el equivalente a los *extractors* de axum.
#[crate::async_trait]
pub trait FromContext<'a>: Sized {
    /// Construye el valor a partir del contexto.
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

/// Extractor del servidor actual.
#[derive(Debug, Clone, Copy)]
pub struct Guild(pub Id<GuildMarker>);

#[crate::async_trait]
impl<'a> FromContext<'a> for Guild {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.guild_id().map(Guild)
    }
}

/// Extractor del miembro que invocó el comando.
#[derive(Debug, Clone)]
pub struct Member(pub PartialMember);

#[crate::async_trait]
impl<'a> FromContext<'a> for Member {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.member
            .clone()
            .map(Member)
            .ok_or(Error::MissingResource("miembro"))
    }
}

/// Extractor del autor.
#[derive(Debug, Clone)]
pub struct Author(pub User);

#[crate::async_trait]
impl<'a> FromContext<'a> for Author {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(Author(ctx.user.clone()))
    }
}

/// Extractor del canal.
#[derive(Debug, Clone, Copy)]
pub struct ChannelId(pub Id<ChannelMarker>);

#[crate::async_trait]
impl<'a> FromContext<'a> for ChannelId {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.channel_id().map(ChannelId)
    }
}

/// Extractor del mensaje de origen.
#[derive(Debug, Clone)]
pub struct Message(pub DiscordMessage);

#[crate::async_trait]
impl<'a> FromContext<'a> for Message {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.message
            .clone()
            .map(Message)
            .ok_or(Error::MissingResource("mensaje"))
    }
}

/// Extractor de la interacción original.
#[derive(Debug, Clone)]
pub struct Interaction(pub twilight_model::application::interaction::Interaction);

#[crate::async_trait]
impl<'a> FromContext<'a> for Interaction {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.interaction
            .clone()
            .map(Interaction)
            .ok_or(Error::MissingResource("interacción"))
    }
}

/// Extractor de los valores de un menú de selección.
#[derive(Debug, Clone)]
pub struct Values(pub Vec<String>);

#[crate::async_trait]
impl<'a> FromContext<'a> for Values {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(Values(ctx.values.clone()))
    }
}

/// Extractor del id del usuario.
#[derive(Debug, Clone, Copy)]
pub struct UserId(pub Id<UserMarker>);

#[crate::async_trait]
impl<'a> FromContext<'a> for UserId {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        Ok(UserId(ctx.user.id))
    }
}
