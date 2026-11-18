# Lightning

[![CI](https://github.com/ImDaiDev/lightning/actions/workflows/ci.yml/badge.svg)](https://github.com/ImDaiDev/lightning/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

**Discord bots at lightning speed.**

Lightning is a Rust framework for building Discord bots with one central idea:
if you can write an `async` function, you can write a command.

```rust
/// Replies with pong
#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong, {}!", ctx.user.name))
}
```

That's it. That command already answers both `/ping` and `!ping`, gets registered
with Discord, and can be tested without a network connection.

> **Status:** alpha (`0.1.0`). The API may change before `1.0`.
> Engine: [twilight](https://twilight.rs) `0.18`.

---

## Why another framework?

| | Lightning | Serenity | Raw Twilight |
|---|---|---|---|
| Slash commands | ✅ `#[command]` macro | ✅ macro | ❌ manual |
| Prefix commands | ✅ **the same command** | ⚠️ separate system | ❌ manual |
| Automatic registration | ✅ `inventory` | ❌ manual list | ❌ manual list |
| Typed arguments | ✅ `Args<T>` + derive | ⚠️ manual parsing | ❌ manual |
| axum-style extractors | ✅ `Context`, `State<T>`, `Guild`… | ❌ | ❌ |
| tower-style middleware | ✅ `next.run(ctx)` | ⚠️ hooks | ❌ |
| Testing without Discord | ✅ `Responder` is a trait | ⚠️ hand-rolled mocks | ❌ |
| Sharding | ✅ automatic | ✅ | ⚠️ manual |

Lightning's personality comes from three decisions:

1. **One command, two surfaces.** One function, one argument struct, but it
   answers to both `/echo` and `!echo`.
2. **Everything is discovered.** You write the function and the framework finds
   it.
3. **Everything is testable.** Sending messages is a trait: in production it
   speaks HTTP with Discord; in tests it points at a `Vec`.

---

## Installation

Lightning is not on crates.io yet. Use the workspace as a path dependency:

```toml
# Cargo.toml
[dependencies]
lightning = { path = "../Lightning/crates/lightning" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Rust **1.85+** (edition 2024).

---

## 30 seconds

```rust
use lightning::prelude::*;

/// Replies with pong
#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong, {}!", ctx.user.name))
}

#[lightning::main]
async fn main() {
    Lightning::new()
        .token_from_env()   // reads DISCORD_TOKEN
        .prefix("!")        // also enables prefix commands
        .run()
        .await
        .expect("the bot crashed");
}
```

```powershell
$env:DISCORD_TOKEN = "your-token"
cargo run -p examples-basics
```

On startup you'll see the banner:

```
  Lightning v0.1.0
  ─────────────────────────────────────
  Bot         Lightning#1234
  Servers     3
  Commands    6
  Shards      1
```

---

## The mental model

### One command, two surfaces

```rust
#[command]
async fn ping(ctx: Context) -> Reply { /* /ping and !ping */ }
```

The `kind` attribute changes this when you need to:

```rust
#[command(kind = "slash")]   // only /ping
#[command(kind = "prefix")]  // only !ping
```

Aliases only affect the prefix surface:

```rust
#[command(aliases = ["say", "repeat"])]
async fn echo(/* … */) {}
```

### Arguments

Declare a normal struct and derive `CommandArgs`. The **field doc comments
become the Discord option descriptions**:

```rust
#[derive(CommandArgs)]
struct Echo {
    /// What the bot should repeat
    text: String,
    /// How many times to repeat it
    times: Option<u8>,
}

#[command]
async fn echo(args: Args<Echo>) -> Reply {
    let times = args.times.unwrap_or(1);
    Reply::text(args.text.repeat(times as usize))
}
```

- `/echo text: hello times: 2` → typed Discord options.
- `!echo hello 2` → positional parser with quotes (`!echo "hello world" 2`).
- `Option<T>` fields are optional on both surfaces.
- Mark a field `#[arg(rest)]` to capture the rest of the line into a `String`.

Supported types: `String`, integers, `f32`/`f64`, `bool`, `char`, `Id<T>`,
`Option<T>`, `Vec<T>` and `serde_json::Value`.

### Extractors

The function signature is the documentation: ask for what you need and
Lightning injects it.

| Extractor | What you get |
|---|---|
| `Context` | user, guild, channel, interaction… |
| `Args<T>` | typed arguments |
| `State<T>` | a state injected with `.state(…)` |
| `Guild` | guild id (fails in DMs) |
| `Member` | the member who invoked the command |
| `Author` | author as a `User` |
| `ChannelId` | current channel |
| `Message` | source message (prefix) |
| `Interaction` | the raw interaction |
| `Values` | values from a select menu |

### Middleware

Layers that wrap execution, following the same idea as tower.

```rust
use std::time::Duration;

let bot = Lightning::new()
    .middleware(Cooldown::per_user(Duration::from_secs(3)));
```

You can write your own with a closure…

```rust
let logging = |ctx: Context, next: Next| async move {
    println!("-> {}", ctx.command);
    next.run(ctx).await
};
```

…or with the trait, if you need state:

```rust
#[lightning::async_trait]
impl Middleware for AdminsOnly {
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
        if !is_admin(&ctx) {
            return Err(Error::Forbidden);
        }
        next.run(ctx).await
    }
}
```

Per command too:

```rust
#[command(middleware = [Cooldown::per_user(Duration::from_secs(30))])]
async fn slow() -> Reply { /* … */ }
```

### State

```rust
struct Db(sqlx::PgPool);

let pool = /* your pool */;

Lightning::new().state(Db(pool));
// or, if you prefer sharing the Arc:
// Lightning::new().state_arc(std::sync::Arc::new(Db(pool)));
```

```rust
#[command]
async fn users(db: State<Db>) -> Reply {
    let total = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&db.0)
        .await?;
    Reply::text(format!("{total} users"))
}
```

### Buttons and modals

```rust
#[command]
async fn confirm() -> Reply {
    Reply::text("Are you sure?").components(vec![
        ActionRow::new()
            .button(Button::success(r#"confirm:{"choice":"yes"}"#, "Yes"))
            .button(Button::danger(r#"confirm:{"choice":"no"}"#, "No")),
    ])
}

#[component("confirm")]  // handles any custom_id starting with `confirm`
async fn on_confirm(args: Args<Choice>) -> Reply {
    match args.choice.as_str() {
        "yes" => Reply::text("done!"),
        _ => Reply::text("cancelled"),
    }
}
```

The part after `:` is parsed as JSON and delivered as `Args<T>`, so a button's
state travels **typed**. Modals are answered with
`Reply::modal(Modal::new("id", "Title").text_input(...))`.

### Events

```rust
use lightning::model::incoming::MemberAdd;

#[event(MemberAdd)]
async fn welcome(event: &MemberAdd) {
    println!("{} joined", event.user.name);
}
```

Add a second parameter to receive an `EventContext` with HTTP and state:

```rust
#[event(MessageCreate)]
async fn log(event: &MessageCreate, ctx: EventContext) {
    let _ = ctx.http;
}
```

### Sharding

Lightning asks Discord for the recommended shard count and spawns one shard per
task, all sharing the same state and command registry. To force a value:

```rust
Lightning::new().shards(4)
```

### Testing

```rust
use lightning::testing::mock_context;

#[tokio::test]
async fn ping_replies() {
    let (ctx, responder) = mock_context("ping");

    let reply = ping(ctx.clone()).await;
    ctx.reply(reply).await.unwrap();

    assert_eq!(
        responder.last().unwrap().content.as_deref(),
        Some("pong, test!"),
    );
}
```

No network, no token, no Discord.

---

## `#[command]` reference

| Attribute | Example | Description |
|---|---|---|
| `name` | `name = "echo"` | Command name |
| `description` | `description = "Repeats"` | Falls back to the doc comment |
| `kind` | `kind = "slash"` | `slash`, `prefix` or `both` (default) |
| `aliases` | `aliases = ["say"]` | Prefix aliases |
| `guild_only` | `guild_only` | Guilds only |
| `nsfw` | `nsfw` | Marks the command as NSFW |
| `permissions` | `permissions = 8` | Default permissions (bits) |
| `middleware` | `middleware = [MyLayer]` | Command-specific layers |
| `auto_register` | `auto_register = "false"` | Disables auto-registration |

---

## Repository layout

```
Lightning/
├── crates/
│   ├── lightning-core/    # model, context, extractors, middleware, Reply
│   ├── lightning-macros/  # #[command], #[event], #[component], #[main], derive
│   └── lightning/         # runtime: Gateway, HTTP, sharding, registration
├── examples/
│   └── basics/            # complete example bot
├── ARCHITECTURE.md        # how it works inside and why
└── Cargo.toml             # workspace
```

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p examples-basics
```

---

## Roadmap

- [ ] Typed autocomplete (`#[autocomplete]`).
- [ ] Subcommands and groups (`#[subcommand]`).
- [ ] First-class select menus.
- [ ] Sharding through `ShardManager` and `twilight-gateway-queue` (today: own shards).
- [ ] `lightning dev` with hot reload.
- [ ] Publishing to crates.io.

---

## License

MIT OR Apache-2.0. Pick whichever you prefer:

- [`LICENSE-MIT`](LICENSE-MIT)
- [`LICENSE-APACHE`](LICENSE-APACHE)
