//! Ejemplo completo de Lightning:
//! comandos slash + prefijo, argumentos tipados, estado, middleware, botones y eventos.
//!
//! Ejecuta con:
//!
//! ```powershell
//! $env:DISCORD_TOKEN = "tu-token"
//! cargo run -p examples-basics
//! ```
//!
//! Recuerda activar el intent `MESSAGE_CONTENT` en el portal de desarrolladores
//! si quieres usar los comandos de prefijo (`!ping`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lightning::model::incoming::MemberAdd;
use lightning::prelude::*;

/// Estado compartido entre todos los handlers.
struct Counter(AtomicU64);

impl Counter {
    fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// Argumentos del comando `eco`.
#[derive(CommandArgs)]
struct Eco {
    /// Qué debe repetir el bot
    texto: String,
    /// Cuántas veces repetirlo (1-10)
    veces: Option<u8>,
}

/// Elección de un botón.
#[derive(CommandArgs)]
struct Choice {
    /// Opción elegida
    choice: String,
}

/// Responde con pong y el nombre de quien lo pidió.
#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong ⚡ hola, {}", ctx.user.name))
}

/// Repite un texto. Funciona con `/eco` y con `!eco`.
#[command(aliases = ["rep", "decir"])]
async fn eco(args: Args<Eco>) -> Reply {
    let veces = args.veces.unwrap_or(1).clamp(1, 10);
    Reply::text(args.texto.repeat(veces as usize)).no_mentions()
}

/// Enseña el servidor actual. Solo funciona en guilds.
#[command(guild_only)]
async fn servidor(ctx: Context) -> Reply {
    let guild = ctx.guild_id().map(|id| id.get()).unwrap_or_default();
    Reply::new().embed(
        Embed::new()
            .title("Estás en un servidor")
            .description(format!("`{guild}`"))
            .color(0xF5A623),
    )
}

/// Suma uno al contador global (estado inyectado).
#[command]
async fn contador(state: State<Counter>) -> Reply {
    let total = state.next();
    Reply::text(format!("⚡ contador: **{total}**"))
}

/// Comando con cooldown propio y solo en slash.
#[command(kind = "slash", middleware = [Cooldown::per_user(Duration::from_secs(30))])]
async fn lento() -> Reply {
    Reply::text("este comando solo puede usarse cada 30 s por persona")
}

/// Envía un mensaje con botones.
#[command]
async fn confirmar() -> Reply {
    Reply::text("¿Seguro que quieres continuar?").components(vec![
        ActionRow::new()
            .button(Button::success(r#"confirm:{"choice":"yes"}"#, "Sí, vamos"))
            .button(Button::danger(r#"confirm:{"choice":"no"}"#, "Cancelar")),
    ])
}

/// Handler de dos botones distintos.
#[component("confirm")]
async fn on_confirm(args: Args<Choice>) -> Reply {
    match args.choice.as_str() {
        "yes" => Reply::text("¡hecho! ⚡"),
        _ => Reply::text("cancelado, sin rencor"),
    }
}

/// Se ejecuta cuando alguien entra al servidor.
#[event(MemberAdd)]
async fn bienvenida(evento: &MemberAdd) {
    println!("⚡ entró {}", evento.user.name);
}

#[lightning::main]
async fn main() {
    Lightning::new()
        .token_from_env()
        .prefix("!")
        // En desarrollo, registra los comandos en tu servidor:
        // .guild(Id::new(123456789012345678))
        .state(Counter(AtomicU64::new(0)))
        .middleware(Cooldown::per_user(Duration::from_secs(3)))
        .run()
        .await
        .expect("el bot se ha caído");
}
