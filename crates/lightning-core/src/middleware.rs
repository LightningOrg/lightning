//! *tower*-style middleware: layers that wrap command execution.
//!
//! ```no_run
//! use lightning_core::{async_trait, Context, Middleware, Next, Reply, Result};
//! use std::time::Duration;
//!
//! struct AdminsOnly;
//!
//! #[async_trait]
//! impl Middleware for AdminsOnly {
//!     async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
//!         if ctx.user.id.get() == 1 {
//!             return Err(lightning_core::Error::Forbidden);
//!         }
//!         next.run(ctx).await
//!     }
//! }
//!
//! // Register it like this:
//! // .middleware(AdminsOnly)
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

/// A middleware layer.
#[crate::async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// Processes the invocation and decides whether to call `next` or stop.
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply>;
}

/// The rest of the middleware chain (including the final handler).
pub struct Next {
    layers: Arc<[Arc<dyn Middleware>]>,
    index: usize,
    terminal: RunFn,
}

impl Next {
    /// Builds the full chain.
    pub(crate) fn new(layers: Arc<[Arc<dyn Middleware>]>, terminal: RunFn) -> Self {
        Self {
            layers,
            index: 0,
            terminal,
        }
    }

    /// Moves on to the next layer, or runs the handler.
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

/// Lets you use closures as middleware.
///
/// ```
/// use lightning_core::{Context, Next, Reply, Result};
///
/// let logging = |ctx: Context, next: Next| async move {
///     println!("running {}", ctx.command);
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

/// Only allows execution inside a guild.
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

/// Only allows execution in direct messages.
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

/// Per-user, per-command usage limit.
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
    /// One use every `window` per user **and** per command.
    pub fn per_user(window: Duration) -> Self {
        Self {
            window,
            buckets: DashMap::new(),
        }
    }

    /// Time left for the user, if they are on cooldown.
    pub fn remaining(&self, command: &'static str, user: Id<UserMarker>) -> Option<Duration> {
        let last = *self.buckets.get(&(command, user))?;
        let elapsed = last.elapsed();

        (elapsed < self.window).then(|| self.window - elapsed)
    }

    /// Drops expired buckets (useful from a cron job).
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

/// Runs the full pipeline: global layers + command layers.
///
/// Used by the runtime; also handy in tests that want to exercise the whole
/// middleware chain.
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
