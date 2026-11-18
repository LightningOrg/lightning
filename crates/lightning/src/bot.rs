//! El constructor del bot y el arranque del runtime.

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

/// Constructor y punto de entrada de un bot de Lightning.
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
    /// Empieza a construir un bot con valores sensatos.
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

    /// Token del bot. Si no se indica, se lee de `DISCORD_TOKEN`.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Lee el token de la variable de entorno `DISCORD_TOKEN`.
    ///
    /// Es el comportamiento por defecto; queda explícito para que el código
    /// se lea solo.
    pub fn token_from_env(self) -> Self {
        self
    }

    /// Intents del Gateway.
    ///
    /// Si usas comandos de prefijo, Lightning añade `MESSAGE_CONTENT` por ti.
    pub fn intents(mut self, intents: Intents) -> Self {
        self.intents = intents;
        self
    }

    /// Activa los comandos de prefijo (`!ping`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Inyecta un estado accesible con el extractor `State<T>`.
    pub fn state<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.state.insert(value);
        self
    }

    /// Inyecta un estado que ya vive en un `Arc` (útil para bases de datos).
    pub fn state_arc<T: Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.state.insert_arc(value);
        self
    }

    /// Añade una capa de middleware global.
    pub fn middleware<M: Middleware>(mut self, layer: M) -> Self {
        self.middleware.push(Arc::new(layer));
        self
    }

    /// Añade un comando escrito a mano (los `#[command]` se auto-registran).
    pub fn command(mut self, command: Command) -> Self {
        self.commands.push(command);
        self
    }

    /// Añade varios comandos.
    pub fn commands(mut self, commands: impl IntoIterator<Item = Command>) -> Self {
        self.commands.extend(commands);
        self
    }

    /// Registra los comandos de barra en un servidor concreto.
    ///
    /// Ideal en desarrollo: los cambios aparecen al instante en vez de esperar
    /// a la caché global de Discord.
    pub fn guild(mut self, guild_id: impl Into<Id<GuildMarker>>) -> Self {
        self.guild_id = Some(guild_id.into());
        self
    }

    /// Fuerza un número de shards (por defecto, el recomendado por Discord).
    pub fn shards(mut self, count: u32) -> Self {
        self.shards = Some(count.max(1));
        self
    }

    /// Desactiva el registro automático de comandos de barra.
    pub fn skip_registration(mut self) -> Self {
        self.register = false;
        self
    }

    /// Arranca el bot: registra comandos, abre los shards y procesa eventos
    /// hasta que pulses `Ctrl+C`.
    pub async fn run(self) -> Result<()> {
        let token = self
            .token
            .or_else(|| std::env::var("DISCORD_TOKEN").ok())
            .ok_or_else(|| {
                Error::custom("falta el token del bot: usa .token(\"...\") o define DISCORD_TOKEN")
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
                warn!(command = command.name, "comando duplicado, se ignora");
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
            prefix = self.prefix.as_deref().unwrap_or("—"),
            "conectando a Discord"
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

        info!("apagando Lightning ⚡");

        for task in tasks {
            task.abort();
        }

        Ok(())
    }
}

/// Punto de entrada que genera `#[lightning::main]`.
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
        .expect("no se pudo crear el runtime de Tokio");

    runtime.block_on(future);
}
