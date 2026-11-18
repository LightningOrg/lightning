//! Complete Lightning example:
//! slash + prefix commands, typed arguments, state, middleware, buttons and events.
//!
//! Run it with:
//!
//! ```powershell
//! $env:DISCORD_TOKEN = "your-token"
//! cargo run -p examples-basics
//! ```
//!
//! Remember to enable the `MESSAGE_CONTENT` intent in the developer portal if
//! you want to use prefix commands (`!ping`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lightning::model::incoming::MemberAdd;
use lightning::prelude::*;

/// State shared by every handler.
struct Counter(AtomicU64);

impl Counter {
    fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// Arguments for the `echo` command.
#[derive(CommandArgs)]
struct Echo {
    /// What the bot should repeat
    text: String,
    /// How many times to repeat it (1-10)
    times: Option<u8>,
}

/// A button choice.
#[derive(CommandArgs)]
struct Choice {
    /// Selected option
    choice: String,
}

/// Replies with pong and the name of whoever asked.
#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong, {}!", ctx.user.name))
}

/// Repeats a text. Works with `/echo` and `!echo`.
#[command(aliases = ["say", "repeat"])]
async fn echo(args: Args<Echo>) -> Reply {
    let times = args.times.unwrap_or(1).clamp(1, 10);
    Reply::text(args.text.repeat(times as usize)).no_mentions()
}

/// Shows the current server. Only works in guilds.
#[command(guild_only)]
async fn server(ctx: Context) -> Reply {
    let guild = ctx.guild_id().map(|id| id.get()).unwrap_or_default();
    Reply::new().embed(
        Embed::new()
            .title("You are in a server")
            .description(format!("`{guild}`"))
            .color(0xF5A623),
    )
}

/// Adds one to the global counter (injected state).
#[command]
async fn counter(state: State<Counter>) -> Reply {
    let total = state.next();
    Reply::text(format!("counter: **{total}**"))
}

/// Command with its own cooldown, slash only.
#[command(kind = "slash", middleware = [Cooldown::per_user(Duration::from_secs(30))])]
async fn slow() -> Reply {
    Reply::text("this command can only be used every 30 s per person")
}

/// Sends a message with buttons.
#[command]
async fn confirm() -> Reply {
    Reply::text("Are you sure you want to continue?").components(vec![
        ActionRow::new()
            .button(Button::success(
                r#"confirm:{"choice":"yes"}"#,
                "Yes, let's go",
            ))
            .button(Button::danger(r#"confirm:{"choice":"no"}"#, "Cancel")),
    ])
}

/// Handler for both buttons.
#[component("confirm")]
async fn on_confirm(args: Args<Choice>) -> Reply {
    match args.choice.as_str() {
        "yes" => Reply::text("done!"),
        _ => Reply::text("cancelled, no hard feelings"),
    }
}

/// Runs when someone joins the server.
#[event(MemberAdd)]
async fn welcome(event: &MemberAdd) {
    println!("{} joined", event.user.name);
}

#[lightning::main]
async fn main() {
    Lightning::new()
        .token_from_env()
        .prefix("!")
        // In development, register commands in your guild:
        // .guild(Id::new(123456789012345678))
        .state(Counter(AtomicU64::new(0)))
        .middleware(Cooldown::per_user(Duration::from_secs(3)))
        .run()
        .await
        .expect("the bot crashed");
}
