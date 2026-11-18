//! Testing without Discord.
//!
//! Because sending messages is the [`lightning_core::Responder`] trait, you can
//! capture your handlers' replies:
//!
//! ```
//! use lightning::prelude::*;
//! use lightning::testing::mock_context;
//!
//! #[command]
//! async fn ping(ctx: Context) -> Reply {
//!     Reply::text(format!("pong {}", ctx.user.name))
//! }
//!
//! # #[tokio::test]
//! # async fn example() {
//! let (ctx, responder) = mock_context("ping");
//! let reply = ping(ctx).await;
//! ctx.reply(reply).await.unwrap();
//!
//! assert_eq!(responder.sent()[0].content.as_deref(), Some("pong test"));
//! # }
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use lightning_core::context::Responder;
use lightning_core::{Context, Reply, Result};

/// Responder that records everything in memory.
#[derive(Debug, Default)]
pub struct MockResponder {
    sent: Mutex<Vec<Reply>>,
    deferred: AtomicBool,
}

impl MockResponder {
    /// Creates an empty responder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Copy of every message sent so far.
    pub fn sent(&self) -> Vec<Reply> {
        self.sent.lock().expect("poisoned lock").clone()
    }

    /// Takes and clears the messages sent so far.
    pub fn take(&self) -> Vec<Reply> {
        std::mem::take(&mut *self.sent.lock().expect("poisoned lock"))
    }

    /// Was `defer` called?
    pub fn is_deferred(&self) -> bool {
        self.deferred.load(Ordering::SeqCst)
    }

    /// Last message sent.
    pub fn last(&self) -> Option<Reply> {
        self.sent.lock().expect("poisoned lock").last().cloned()
    }
}

#[lightning_core::async_trait]
impl Responder for MockResponder {
    async fn respond(&self, reply: Reply) -> Result<()> {
        self.sent.lock().expect("poisoned lock").push(reply);

        Ok(())
    }

    async fn defer(&self, _ephemeral: bool) -> Result<()> {
        self.deferred.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn followup(&self, reply: Reply) -> Result<()> {
        self.respond(reply).await
    }
}

/// Creates a test [`Context`] and its responder.
pub fn mock_context(command: &'static str) -> (Context, Arc<MockResponder>) {
    let responder = Arc::new(MockResponder::new());
    let context = Context::test(command, responder.clone());

    (context, responder)
}
