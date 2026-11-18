//! Middleware estilo *tower*: capas que envuelven la ejecución de un comando.
//!
//! ```no_run
//! use lightning_core::{async_trait, Context, Middleware, Next, Reply, Result};
//! use std::time::Duration;
//!
//! struct SoloAdmins;
//!
//! #[async_trait]
//! impl Middleware for SoloAdmins {
//!     async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
//!         if ctx.user.id.get() == 1 {
//!             return Err(lightning_core::Error::Forbidden);
//!         }
//!         next.run(ctx).await
//!     }
//! }
//!
//! // Y se registra así:
//! // .middleware(SoloAdmins)
//! // .middleware(Cooldown::per_user(Duration::from_secs(3)))
//! ```

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use crate::command::RunFn;
use crate::context::Context;
use crate::error::{Error, Result};
use crate::reply::Reply;

/// Una capa de middleware.
#[crate::async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// Procesa la invocación y decide si llama a `next` o corta el flujo.
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply>;
}

/// El resto de la cadena de middleware (incluido el handler final).
pub struct Next {
    layers: Arc<[Arc<dyn Middleware>]>,
    index: usize,
    terminal: RunFn,
}

impl Next {
    /// Crea la cadena completa.
    pub(crate) fn new(layers: Arc<[Arc<dyn Middleware>]>, terminal: RunFn) -> Self {
        Self {
            layers,
            index: 0,
            terminal,
        }
    }

    /// Continúa hacia la siguiente capa o ejecuta el handler.
    pub async fn run(mut self, ctx: Context) -> Result<Reply> {
        if let Some(layer) = self.layers.get(self.index).cloned() {
            self.index += 1;

            return layer.handle(ctx, self).await;
        }

        (self.terminal)(&ctx).await
    }
}

impl std::fmt::Debug for Next {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Next")
            .field("index", &self.index)
            .field("remaining", &self.layers.len().saturating_sub(self.index))
            .finish()
    }
}

/// Permite usar closures como middleware.
///
/// ```
/// use lightning_core::{Context, Next, Reply, Result};
///
/// let logging = |ctx: Context, next: Next| async move {
///     println!("ejecutando {}", ctx.command);
///     next.run(ctx).await
/// };
/// # let _ = logging;
/// ```
#[crate::async_trait]
impl<F, Fut> Middleware for F
where
    F: Fn(Context, Next) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Reply>> + Send,
{
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
        (self)(ctx, next).await
    }
}

/// Solo permite la ejecución dentro de un servidor.
#[derive(Debug, Clone, Copy, Default)]
pub struct GuildOnly;

#[crate::async_trait]
impl Middleware for GuildOnly {
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
        if ctx.is_guild() {
            next.run(ctx).await
        } else {
            Err(Error::GuildOnly)
        }
    }
}

/// Solo permite la ejecución por mensaje directo.
#[derive(Debug, Clone, Copy, Default)]
pub struct DmOnly;

#[crate::async_trait]
impl Middleware for DmOnly {
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
        if ctx.is_guild() {
            Err(Error::DmOnly)
        } else {
            next.run(ctx).await
        }
    }
}

/// Límite de uso por usuario y comando.
///
/// ```
/// use lightning_core::Cooldown;
/// use std::time::Duration;
///
/// let cooldown = Cooldown::per_user(Duration::from_secs(5));
/// # let _ = cooldown;
/// ```
#[derive(Debug)]
pub struct Cooldown {
    window: Duration,
    buckets: DashMap<(&'static str, Id<UserMarker>), Instant>,
}

impl Cooldown {
    /// Un uso cada `window` por usuario **y** comando.
    pub fn per_user(window: Duration) -> Self {
        Self {
            window,
            buckets: DashMap::new(),
        }
    }

    /// Tiempo restante para el usuario, si está en cooldown.
    pub fn remaining(&self, command: &'static str, user: Id<UserMarker>) -> Option<Duration> {
        let last = *self.buckets.get(&(command, user))?;
        let elapsed = last.elapsed();

        (elapsed < self.window).then(|| self.window - elapsed)
    }

    /// Limpia los buckets caducados (útil en un cron).
    pub fn sweep(&self) {
        let window = self.window;
        self.buckets.retain(|_, last| last.elapsed() < window);
    }
}

#[crate::async_trait]
impl Middleware for Cooldown {
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
        if let Some(remaining) = self.remaining(ctx.command, ctx.user.id) {
            return Err(Error::Cooldown {
                retry_after_ms: remaining.as_millis() as u64,
            });
        }

        self.buckets
            .insert((ctx.command, ctx.user.id), Instant::now());

        next.run(ctx).await
    }
}

/// Ejecuta el pipeline completo: capas globales + capas del comando.
///
/// Lo usa el runtime; también es útil en tests que quieran probar la cadena
/// completa de middleware.
pub async fn run_pipeline(
    global: &[Arc<dyn Middleware>],
    command_layers: &[Arc<dyn Middleware>],
    run: RunFn,
    ctx: Context,
) -> Result<Reply> {
    let mut layers: Vec<Arc<dyn Middleware>> =
        Vec::with_capacity(global.len() + command_layers.len());
    layers.extend(global.iter().cloned());
    layers.extend(command_layers.iter().cloned());

    Next::new(Arc::from(layers), run).run(ctx).await
}
