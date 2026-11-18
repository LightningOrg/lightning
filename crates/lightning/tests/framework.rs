//! Framework integration tests: macros, arguments, state and middleware.

use std::sync::Arc;
use std::time::Duration;

use lightning::prelude::*;
use lightning::testing::mock_context;

#[derive(CommandArgs, Debug, PartialEq)]
struct Echo {
    /// What to repeat
    text: String,
    /// How many times
    times: Option<u8>,
}

#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong {}", ctx.user.name))
}

#[command]
async fn echo(args: Args<Echo>) -> Reply {
    let times = args.times.unwrap_or(1);
    Reply::text(args.text.repeat(times as usize))
}

fn ok_handler<'a>(_ctx: &'a Context) -> lightning::BoxFuture<'a, Result<Reply>> {
    Box::pin(async { Ok(Reply::text("ok")) })
}

#[test]
fn options_are_generated_from_the_struct() {
    let options = <Echo as CommandArgs>::OPTIONS;

    assert_eq!(options.len(), 2);
    assert_eq!(options[0].name, "text");
    assert!(options[0].required);
    assert_eq!(options[0].kind, OptionKind::String);
    assert_eq!(options[1].name, "times");
    assert!(!options[1].required);
    assert_eq!(options[1].kind, OptionKind::Integer);
}

#[test]
fn descriptions_come_from_doc_comments() {
    let options = <Echo as CommandArgs>::OPTIONS;
    assert_eq!(options[0].description, "What to repeat");
}

#[test]
fn prefix_parsing_is_positional() {
    let args = <Echo as CommandArgs>::from_prefix(&["hello".to_owned(), "3".to_owned()]).unwrap();

    assert_eq!(args.text, "hello");
    assert_eq!(args.times, Some(3));
}

#[test]
fn prefix_parsing_without_optional_fields() {
    let args = <Echo as CommandArgs>::from_prefix(&["hello".to_owned()]).unwrap();

    assert_eq!(args.text, "hello");
    assert_eq!(args.times, None);
}

#[test]
fn prefix_parsing_detects_missing_arguments() {
    let error = <Echo as CommandArgs>::from_prefix(&[]).unwrap_err();
    assert!(error.to_string().contains("text"));
}

#[test]
fn discord_options_are_parsed() {
    let mut options = lightning::serde_json::Map::new();
    options.insert("text".into(), lightning::serde_json::json!("hey"));
    options.insert("times".into(), lightning::serde_json::json!(2));

    let args = <Echo as CommandArgs>::from_options(options).unwrap();

    assert_eq!(args.text, "hey");
    assert_eq!(args.times, Some(2));
}

#[tokio::test]
async fn the_command_is_found_in_the_registry() {
    let command = lightning::inventory::iter::<Command>
        .into_iter()
        .find(|command| command.name == "ping")
        .expect("the ping command should be registered");

    assert_eq!(command.kind, CommandKind::Both);

    let (ctx, responder) = mock_context("ping");
    let reply = (command.run().expect("it has a handler"))(&ctx)
        .await
        .unwrap();

    ctx.reply(reply).await.unwrap();

    assert_eq!(
        responder.last().unwrap().content.as_deref(),
        Some("pong test")
    );
}

#[tokio::test]
async fn cooldown_blocks_the_second_call() {
    let cooldown: Arc<dyn Middleware> = Arc::new(Cooldown::per_user(Duration::from_secs(60)));
    let layers = [cooldown];

    let (ctx, _) = mock_context("echo");

    let first = lightning::run_pipeline(&layers, &[], ok_handler, ctx.clone()).await;
    assert!(first.is_ok());

    let second = lightning::run_pipeline(&layers, &[], ok_handler, ctx).await;
    assert!(matches!(second, Err(Error::Cooldown { .. })));
}

struct Blocked;

#[lightning::async_trait]
impl Middleware for Blocked {
    async fn handle(&self, _ctx: Context, _next: Next) -> Result<Reply> {
        Err(Error::Forbidden)
    }
}

#[tokio::test]
async fn middleware_can_cut_the_chain() {
    let layer: Arc<dyn Middleware> = Arc::new(Blocked);
    let (ctx, _) = mock_context("ping");

    let result = lightning::run_pipeline(&[layer], &[], ok_handler, ctx).await;

    assert!(matches!(result, Err(Error::Forbidden)));
}
