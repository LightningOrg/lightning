//! Implementación de [`Responder`] sobre el cliente HTTP de Twilight.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lightning_core::context::Responder;
use lightning_core::{Error, Reply, Result};
use twilight_http::Client;
use twilight_model::http::interaction::InteractionResponseType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ApplicationMarker, ChannelMarker, InteractionMarker};

use crate::convert;

/// A dónde va la respuesta.
enum Target {
    /// Interacción (webhook con token).
    Interaction {
        id: Id<InteractionMarker>,
        token: String,
    },
    /// Canal normal, para comandos de prefijo.
    Channel(Id<ChannelMarker>),
}

/// Qué tipo de mensaje es la respuesta inicial.
#[derive(Clone, Copy)]
pub(crate) enum InitialKind {
    Message,
    UpdateMessage,
}

/// Responde a una invocación concreta.
pub(crate) struct TwilightResponder {
    http: Arc<Client>,
    application_id: Id<ApplicationMarker>,
    target: Target,
    kind: InitialKind,
    responded: AtomicBool,
}

impl TwilightResponder {
    pub(crate) fn interaction(
        http: Arc<Client>,
        application_id: Id<ApplicationMarker>,
        id: Id<InteractionMarker>,
        token: String,
        kind: InitialKind,
    ) -> Self {
        Self {
            http,
            application_id,
            target: Target::Interaction { id, token },
            kind,
            responded: AtomicBool::new(false),
        }
    }

    pub(crate) fn channel(http: Arc<Client>, channel_id: Id<ChannelMarker>) -> Self {
        Self {
            http,
            application_id: Id::new(0),
            target: Target::Channel(channel_id),
            kind: InitialKind::Message,
            responded: AtomicBool::new(false),
        }
    }
}

fn transport(error: impl std::fmt::Display) -> Error {
    Error::Transport(error.to_string())
}

#[lightning_core::async_trait]
impl Responder for TwilightResponder {
    async fn respond(&self, reply: Reply) -> Result<()> {
        if reply.is_empty() {
            return Ok(());
        }

        match &self.target {
            Target::Channel(channel_id) => self.send_message(*channel_id, &reply).await,
            Target::Interaction { id, token } => {
                if self.responded.swap(true, Ordering::SeqCst) {
                    if matches!(self.kind, InitialKind::UpdateMessage) {
                        return self.update_message(token, &reply).await;
                    }

                    return self.followup(reply).await;
                }

                let kind = match self.kind {
                    InitialKind::Message => InteractionResponseType::ChannelMessageWithSource,
                    InitialKind::UpdateMessage => InteractionResponseType::UpdateMessage,
                };
                let response = convert::interaction_response(kind, &reply)?;
                let client = self.http.interaction(self.application_id);

                client
                    .create_response(*id, token, &response)
                    .await
                    .map_err(transport)?;

                Ok(())
            }
        }
    }

    async fn defer(&self, ephemeral: bool) -> Result<()> {
        let Target::Interaction { id, token } = &self.target else {
            return Ok(());
        };

        if self.responded.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let response = match self.kind {
            InitialKind::Message => convert::deferred(ephemeral),
            InitialKind::UpdateMessage => convert::deferred_update(),
        };
        let client = self.http.interaction(self.application_id);

        client
            .create_response(*id, token, &response)
            .await
            .map_err(transport)?;

        Ok(())
    }

    async fn followup(&self, reply: Reply) -> Result<()> {
        if reply.is_empty() {
            return Ok(());
        }

        match &self.target {
            Target::Channel(channel_id) => self.send_message(*channel_id, &reply).await,
            Target::Interaction { token, .. } => {
                let embeds = convert::to_embeds(&reply)?;
                let components = convert::to_components(&reply)?;

                let client = self.http.interaction(self.application_id);
                let mut request = client.create_followup(token);

                if let Some(content) = reply.content.as_deref() {
                    request = request.content(content);
                }
                if !embeds.is_empty() {
                    request = request.embeds(&embeds);
                }
                if !components.is_empty() {
                    request = request.components(&components);
                }

                request.await.map_err(transport)?;

                Ok(())
            }
        }
    }
}

impl TwilightResponder {
    /// Edita el mensaje original de una interacción (botones y menús).
    async fn update_message(&self, token: &str, reply: &Reply) -> Result<()> {
        let embeds = convert::to_embeds(reply)?;
        let components = convert::to_components(reply)?;

        let client = self.http.interaction(self.application_id);
        let mut request = client.update_response(token);

        request = request.embeds(Some(&embeds)).components(Some(&components));

        if let Some(content) = reply.content.as_deref() {
            request = request.content(Some(content));
        }

        request.await.map_err(transport)?;

        Ok(())
    }

    async fn send_message(&self, channel_id: Id<ChannelMarker>, reply: &Reply) -> Result<()> {
        let embeds = convert::to_embeds(reply)?;
        let components = convert::to_components(reply)?;

        let mut request = self.http.create_message(channel_id);

        if let Some(content) = reply.content.as_deref() {
            request = request.content(content);
        }
        if !embeds.is_empty() {
            request = request.embeds(&embeds);
        }
        if !components.is_empty() {
            request = request.components(&components);
        }

        request.await.map_err(transport)?;

        Ok(())
    }
}
