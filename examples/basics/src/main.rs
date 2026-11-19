//! Complete Lightning example:
//! slash + prefix commands, typed arguments, state, middleware, buttons, events
//! and voice channels (join, play audio, text chat and moderation).
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

use lightning::EventContext;
use lightning::model::incoming::{MemberAdd, VoiceStateUpdate};
use lightning::model::{ChannelMarker, UserMarker};
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

/// Arguments for the autocompleted `fruit` command.
#[derive(CommandArgs)]
struct FruitQuery {
    /// Fruit to look up
    fruit: String,
}

/// A button choice.
#[derive(CommandArgs)]
struct Choice {
    /// Selected option
    choice: String,
}

/// Arguments for voice moderation commands.
#[derive(CommandArgs)]
struct Target {
    /// Member to act on
    user: Id<UserMarker>,
}

/// A member and a destination channel.
#[derive(CommandArgs)]
struct MoveTarget {
    /// Member to move
    user: Id<UserMarker>,
    /// Voice channel to move them to
    channel: Id<ChannelMarker>,
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

/// Fruits suggested by `/fruit`, and the command that uses them.
const FRUITS: &[&str] = &[
    "apple",
    "apricot",
    "banana",
    "blueberry",
    "cherry",
    "grape",
    "kiwi",
    "lemon",
    "lime",
    "mango",
    "orange",
    "peach",
    "pear",
    "pineapple",
    "strawberry",
    "watermelon",
];

/// Looks up a fruit; `/fruit fruit:` autocompletes as you type.
#[command]
async fn fruit(args: Args<FruitQuery>) -> Reply {
    Reply::text(format!("You picked **{}**.", args.fruit))
}

/// Serves the fruit suggestions while the user types.
#[autocomplete(command = "fruit", option = "fruit")]
async fn fruit_autocomplete(focused: Focused) -> Vec<AutocompleteChoice> {
    let query = focused.value.to_ascii_lowercase();

    FRUITS
        .iter()
        .filter(|fruit| fruit.contains(&query))
        .map(|fruit| AutocompleteChoice::text(*fruit, *fruit))
        .collect()
}

/// Manage the bot configuration.
#[command]
async fn config() -> Reply {
    Reply::text("Use `/config show` or `/config set greeting`.")
}

/// Shows the current configuration.
#[subcommand(command = "config")]
async fn show() -> Reply {
    Reply::text("The configuration looks good.")
}

/// Arguments for `/config set greeting`.
#[derive(CommandArgs)]
struct Greeting {
    /// New greeting text
    text: String,
}

/// Changes the greeting.
#[subcommand(
    command = "config",
    group = "set",
    group_description = "Change settings"
)]
async fn greeting(args: Args<Greeting>) -> Reply {
    Reply::text(format!("Greeting changed to **{}**.", args.text))
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

/// Counts the people in your voice channel.
#[command(guild_only, name = "voice-count")]
async fn voice_count(ctx: Context, states: VoiceStates, user: UserId) -> Result<Reply> {
    let guild = ctx.guild_id()?;
    let Some(channel) = states.channel_of(guild, user.0) else {
        return Ok(Reply::text("Join a voice channel first.").ephemeral());
    };

    Ok(Reply::text(format!(
        "**{}** people in <#{}>",
        states.count(channel),
        channel.get()
    )))
}

/// Joins the voice channel you are in.
#[command(guild_only)]
async fn join(ctx: Context, voice: Voice, states: VoiceStates, user: UserId) -> Result<Reply> {
    let guild = ctx.guild_id()?;
    let Some(channel) = states.channel_of(guild, user.0) else {
        return Ok(Reply::text("Join a voice channel first.").ephemeral());
    };

    voice.join(guild, channel).await?;

    Ok(Reply::text(format!("Connected to <#{}>.", channel.get())))
}

/// Leaves the voice channel.
#[command(guild_only)]
async fn leave(ctx: Context, voice: Voice) -> Result<Reply> {
    voice.leave(ctx.guild_id()?).await?;
    Ok(Reply::text("Left the voice channel."))
}

/// Plays the bundled beep in your voice channel.
#[command(guild_only)]
async fn play(ctx: Context, voice: Voice) -> Result<Reply> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/beep.wav");
    let _ = voice.play_file(ctx.guild_id()?, path).await?;

    Ok(Reply::text("Playing `beep.wav`."))
}

/// Writes in the text chat of your voice channel.
#[command(guild_only)]
async fn say(ctx: Context, voice: Voice, states: VoiceStates, user: UserId) -> Result<Reply> {
    let guild = ctx.guild_id()?;
    let Some(channel) = states.channel_of(guild, user.0) else {
        return Ok(Reply::text("Join a voice channel first.").ephemeral());
    };

    voice
        .say(channel, "Hello from the voice channel chat!")
        .await?;

    Ok(Reply::text("Message sent.").ephemeral())
}

/// Moves a member to another voice channel (needs Move Members).
#[command(guild_only, permissions = 16777216)]
async fn move_member(ctx: Context, voice: Voice, args: Args<MoveTarget>) -> Result<Reply> {
    voice
        .move_to(ctx.guild_id()?, args.user, args.channel)
        .await?;

    Ok(Reply::text(format!(
        "Moved <@{}> to <#{}>.",
        args.user.get(),
        args.channel.get()
    )))
}

/// Disconnects a member from voice (needs Move Members).
#[command(guild_only, permissions = 16777216)]
async fn disconnect(ctx: Context, voice: Voice, args: Args<Target>) -> Result<Reply> {
    voice.disconnect(ctx.guild_id()?, args.user).await?;

    Ok(Reply::text(format!("Disconnected <@{}>.", args.user.get())))
}

/// Server-mutes a member (needs Mute Members).
#[command(guild_only, permissions = 4194304)]
async fn mute(ctx: Context, voice: Voice, args: Args<Target>) -> Result<Reply> {
    voice.server_mute(ctx.guild_id()?, args.user, true).await?;

    Ok(Reply::text(format!("Muted <@{}>.", args.user.get())))
}

/// Runs when someone joins the server.
#[event(MemberAdd)]
async fn welcome(event: &MemberAdd) {
    println!("{} joined", event.user.name);
}

/// Logs voice movements and shows how many people share the channel.
#[event(VoiceStateUpdate)]
async fn voice_log(event: &VoiceStateUpdate, ctx: EventContext) {
    if let Some(channel) = event.channel_id {
        println!(
            "{} is now in {} ({} connected)",
            event.user_id,
            channel,
            ctx.voice_states.count(channel)
        );
    }
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
