//! Testing sin Discord.
//!
//! Como el envío de mensajes es el trait [`lightning_core::Responder`], puedes
//! capturar las respuestas de tus handlers:
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
//! # async fn ejemplo() {
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

/// Responde guardando todo en memoria.
#[derive(Debug, Default)]
pub struct MockResponder {
    sent: Mutex<Vec<Reply>>,
    deferred: AtomicBool,
}

impl MockResponder {
    /// Crea un responder vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Copia de todos los mensajes enviados.
    pub fn sent(&self) -> Vec<Reply> {
        self.sent.lock().expect("lock envenenado").clone()
    }

    /// Extrae y vacía los mensajes enviados.
    pub fn take(&self) -> Vec<Reply> {
        std::mem::take(&mut *self.sent.lock().expect("lock envenenado"))
    }

    /// ¿Se hizo `defer`?
    pub fn is_deferred(&self) -> bool {
        self.deferred.load(Ordering::SeqCst)
    }

    /// Último mensaje enviado.
    pub fn last(&self) -> Option<Reply> {
        self.sent.lock().expect("lock envenenado").last().cloned()
    }
}

#[lightning_core::async_trait]
impl Responder for MockResponder {
    async fn respond(&self, reply: Reply) -> Result<()> {
        self.sent.lock().expect("lock envenenado").push(reply);

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

/// Crea un [`Context`] de prueba y su responder.
pub fn mock_context(command: &'static str) -> (Context, Arc<MockResponder>) {
    let responder = Arc::new(MockResponder::new());
    let context = Context::test(command, responder.clone());

    (context, responder)
}
