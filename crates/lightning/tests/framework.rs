//! Tests de integración del framework: macros, argumentos, estado y middleware.

use std::sync::Arc;
use std::time::Duration;

use lightning::prelude::*;
use lightning::testing::mock_context;

#[derive(CommandArgs, Debug, PartialEq)]
struct Eco {
    /// Qué repetir
    texto: String,
    /// Cuántas veces
    veces: Option<u8>,
}

#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong {}", ctx.user.name))
}

#[command]
async fn eco(args: Args<Eco>) -> Reply {
    let veces = args.veces.unwrap_or(1);
    Reply::text(args.texto.repeat(veces as usize))
}

fn ok_handler<'a>(_ctx: &'a Context) -> lightning::BoxFuture<'a, Result<Reply>> {
    Box::pin(async { Ok(Reply::text("ok")) })
}

#[test]
fn las_opciones_se_generan_desde_la_struct() {
    let options = <Eco as CommandArgs>::OPTIONS;

    assert_eq!(options.len(), 2);
    assert_eq!(options[0].name, "texto");
    assert!(options[0].required);
    assert_eq!(options[0].kind, OptionKind::String);
    assert_eq!(options[1].name, "veces");
    assert!(!options[1].required);
    assert_eq!(options[1].kind, OptionKind::Integer);
}

#[test]
fn las_descripciones_salen_del_doc_comment() {
    let options = <Eco as CommandArgs>::OPTIONS;
    assert_eq!(options[0].description, "Qué repetir");
}

#[test]
fn parseo_de_prefijo_posicional() {
    let args = <Eco as CommandArgs>::from_prefix(&["hola".to_owned(), "3".to_owned()]).unwrap();

    assert_eq!(args.texto, "hola");
    assert_eq!(args.veces, Some(3));
}

#[test]
fn parseo_de_prefijo_sin_opcionales() {
    let args = <Eco as CommandArgs>::from_prefix(&["hola".to_owned()]).unwrap();

    assert_eq!(args.texto, "hola");
    assert_eq!(args.veces, None);
}

#[test]
fn parseo_de_prefijo_detecta_faltantes() {
    let error = <Eco as CommandArgs>::from_prefix(&[]).unwrap_err();
    assert!(error.to_string().contains("texto"));
}

#[test]
fn parseo_de_opciones_de_discord() {
    let mut options = lightning::serde_json::Map::new();
    options.insert("texto".into(), lightning::serde_json::json!("hey"));
    options.insert("veces".into(), lightning::serde_json::json!(2));

    let args = <Eco as CommandArgs>::from_options(options).unwrap();

    assert_eq!(args.texto, "hey");
    assert_eq!(args.veces, Some(2));
}

#[tokio::test]
async fn el_comando_se_encuentra_en_el_registro() {
    let command = lightning::inventory::iter::<Command>
        .into_iter()
        .find(|command| command.name == "ping")
        .expect("el comando ping debería estar registrado");

    assert_eq!(command.kind, CommandKind::Both);

    let (ctx, responder) = mock_context("ping");
    let reply = (command.run().expect("tiene handler"))(&ctx).await.unwrap();

    ctx.reply(reply).await.unwrap();

    assert_eq!(
        responder.last().unwrap().content.as_deref(),
        Some("pong test")
    );
}

#[tokio::test]
async fn el_cooldown_bloquea_la_segunda_llamada() {
    let cooldown: Arc<dyn Middleware> = Arc::new(Cooldown::per_user(Duration::from_secs(60)));
    let layers = [cooldown];

    let (ctx, _) = mock_context("eco");

    let first = lightning::run_pipeline(&layers, &[], ok_handler, ctx.clone()).await;
    assert!(first.is_ok());

    let second = lightning::run_pipeline(&layers, &[], ok_handler, ctx).await;
    assert!(matches!(second, Err(Error::Cooldown { .. })));
}

struct Bloqueado;

#[lightning::async_trait]
impl Middleware for Bloqueado {
    async fn handle(&self, _ctx: Context, _next: Next) -> Result<Reply> {
        Err(Error::Forbidden)
    }
}

#[tokio::test]
async fn un_middleware_puede_cortar_la_cadena() {
    let layer: Arc<dyn Middleware> = Arc::new(Bloqueado);
    let (ctx, _) = mock_context("ping");

    let result = lightning::run_pipeline(&[layer], &[], ok_handler, ctx).await;

    assert!(matches!(result, Err(Error::Forbidden)));
}
