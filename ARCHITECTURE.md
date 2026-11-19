# Lightning architecture

This document explains how the framework is put together and why. If you are
here to work on the code, start here.

## The three crates

```
┌─────────────────────────────────────────────────────────────┐
│ lightning                (runtime)                          │
│   Gateway, HTTP, sharding, registration, Discord conversion  │
│   Voice: voice states + songbird (join, play, moderation)   │
│   ┌──────────────────────────────────────────────────────┐  │
│   │ lightning-core          (core, no network)           │  │
│   │   Command, Context, extractors, Middleware, Reply    │  │
│   │   Responder (trait) · StateMap · Error               │  │
│   └──────────────────────────────────────────────────────┘  │
│   lightning-macros       (procedural macros)                │
│   #[command] #[autocomplete] #[event] #[component] #[main]  │
│   derive(CommandArgs)                                       │
└─────────────────────────────────────────────────────────────┘
```

- **`lightning-core`** does not depend on `tokio` or `twilight-http`: only on
  `twilight-model` data types. It can be used in tests, on a different runtime,
  or even as the basis for another transport.
- **`lightning`** is the facade users depend on (`use lightning::prelude::*`).
  It also owns the voice layer: a live `VoiceStates` cache and a thin wrapper
  over songbird's driver, wired to Lightning's own shards.
- **`lightning-macros`** generates code that always points at `::lightning::…`,
  which is why users only depend on the facade.

## The journey of an event

```
        Gateway (twilight-gateway, one Shard per task)
                                │  Event
                                ▼
                     Runtime::dispatch
             ┌──────────────────┴───────────────────┐
             ▼                                      ▼
    InteractionCreate                        MessageCreate
             │                                      │
    ┌────────┼─────────┐                    starts with the prefix?
    ▼        ▼         ▼                            │
 slash   component   modal                   tokenize + lookup
    │        │         │                            │
    └────────┴─────────┘                            │
             │                                      │
             ▼                                      ▼
   Context { user, guild, channel, options, tokens, state, responder }
                                │
                                ▼
        run_pipeline(global ++ command layers)
                                │
                    ┌───────────┴───────────┐
                    ▼                       ▼
              Middleware::handle         Next::run
                    │                       │
                    └───────────┬───────────┘
                                ▼
                     Command::run (RunFn)
                                │
                     extractors + handler
                                │
                                ▼
                         Result<Reply>
                                │
                                ▼
                  Responder (real HTTP or mock)
```

## Design decisions

### 1. Twilight as the engine, Lightning as the layer

Twilight already solves the boring and hard-to-get-right parts: Gateway with
resume and heartbeats, per-bucket rate limits, optional caching, payload
validation. What it does not provide is ergonomics: registering commands, routing
interactions, parsing options and replying are all manual work.

Lightning does not reimplement the transport: it wraps it. That way we inherit
Twilight's fixes and focus on the product layer.

### 2. `inventory` for auto-registration

`#[command]` emits, alongside the function, a static entry:

```rust
inventory::submit! {
    lightning::Command::new("ping", "Replies with pong")
        .kind(CommandKind::Both)
        .options(<() as CommandArgs>::OPTIONS)
        .handler(__lightning_run_ping)
}
```

`Command` is `const`-constructible (only `&'static str`, slices and function
pointers), so registration costs nothing at runtime: it is just another binary
section. `run()` collects every entry and publishes them to Discord.

Upsides: no lists, no forgotten `use` statements, commands live where they are
used. Downsides: commands cannot capture environment state (that is what the
`State` system is for), and some aggressive linkers may drop entries (mitigated
by `#[used]`, which `inventory` already applies).

### 3. The responder is a trait

```rust
#[async_trait]
pub trait Responder: Send + Sync + 'static {
    async fn respond(&self, reply: Reply) -> Result<()>;
    async fn defer(&self, ephemeral: bool) -> Result<()>;
    async fn followup(&self, reply: Reply) -> Result<()>;
}
```

In production it is implemented by `TwilightResponder`, which knows whether the
invocation comes from an interaction (token, `create_response`, followups) or a
message (`create_message`). In tests it is implemented by `MockResponder`, which
stores `Reply` values in a `Vec`.

This is the decision that makes the framework genuinely testable: same handler
function, same pipeline and same extractors, no network.

### 4. JSON as the intermediate representation

`Reply` stores text, embeds and components as `serde_json::Value` using Discord's
exact API shape. The runtime converts them to `twilight-model` types when sending
(`serde_json::from_value::<Embed>`).

Why:

- Discord's types change often; the API JSON is the stable contract.
- A single place to fix if Twilight changes a signature.
- Tests validate the final JSON for real (see `convert.rs`).

The cost is one deserialization per message: irrelevant next to API latency.

### 5. The pipeline is built once

`Command::build_layers()` runs at startup, not on every invocation. That lets
stateful layers (`Cooldown` with its `DashMap`) share that state across messages
without globals.

Global layers are concatenated with command layers in declaration order.

### 6. One function, two surfaces

The bridge is the argument struct:

- Discord's `CommandDataOption[]` → `Map<String, Value>` → `T::from_options`.
- `!echo hello 2` → tokenizer → `T::from_prefix`.

`#[derive(CommandArgs)]` generates both parsers from the same fields, so there
are no two definitions that can drift apart. The price is that it only supports
"reasonable" types; for anything exotic you can implement `CommandArgs` by hand.

### 7. Voice rides on the same shard

Joining a voice channel is not an HTTP call: it is gateway opcode 4 on the shard
that owns the guild. Lightning already owns every `Shard`, so `run_shard`
registers a `MessageSender` with the voice subsystem and `Call::join` can send
the update without opening a second gateway connection.

The two payloads Discord answers with (`VOICE_STATE_UPDATE` for the bot and
`VOICE_SERVER_UPDATE`) are fed back into the `Call` from the event loop, which
completes the connection. Audio itself is delegated to
[songbird](https://github.com/serenity-rs/songbird)'s driver; Lightning only
implements its `VoiceUpdate` and `GenericSharder` seams, which keeps songbird
from depending on Twilight at all.

Commands are spawned instead of awaited inside the shard loop. That detail is
what makes `voice.join(..).await` possible: if the shard were busy waiting for
the handler, it could not read the very events the handler is waiting for.

The voice-state cache (`VoiceStates`) is seeded from `GuildCreate` and updated
on every `VoiceStateUpdate`. It is intentionally small: only the states the
count queries need, no members, roles or channels.

### 8. Autocomplete is registered, not configured

`#[autocomplete(command = "search", option = "query")]` emits an
`AutocompleteHandler` entry (command name, option name and `AutocompleteFn`)
through `inventory`, just like a command. Two things follow from that:

- At registration time, `convert::to_commands` walks the handlers and marks the
  matching options with `autocomplete: true`. The user does not repeat the
  option metadata anywhere.
- At runtime, an `ApplicationCommandAutocomplete` interaction is routed by
  `(command name, focused option name)` to the handler. The handler receives a
  `Context` with `surface = Autocomplete` and the `Focused` extractor, and its
  choices are converted to Discord's `CommandOptionChoice` model.

Autocomplete deliberately bypasses the middleware pipeline: it fires on every
keystroke and a cooldown would be both wrong and annoying. Responses are capped
at 25 choices and names/values at 100 characters before they reach Discord.

## The life of a response

1. The handler returns `impl IntoReply` (`Reply`, `String`, `()`,
   `Result<T, Error>`…).
2. `IntoReply::into_reply` normalizes it to `Reply`.
3. If `defer` was called, the responder sends a *followup*; otherwise it sends
   the initial response (type 4, or type 7 for buttons).
4. An empty `Reply` sends nothing: that lets you call `ctx.defer().await?` and
   then decide not to reply.
5. An `Err` becomes an ephemeral `❌ …` message and is logged with `tracing`.

## Known limits (MVP)

- Subcommands: routing exists for flat options; groups are flattened.
- Sharding: own shards, no `ShardManager` or coordinated queue.
- Voice: receiving audio (voice transcription) is not implemented, only
  playback and moderation; audio requires `cmake` to build libopus.
- No guild cache: if you need members or roles, use HTTP or add
  `twilight-cache-inmemory` in your own layer.
- `Reply` does not support attachments or `payload_json`.

## Adding a command surface to the framework

1. `#[command]` generates a `RunFn` and the `inventory` entry.
2. `Runtime::dispatch` finds the command by name.
3. It builds the `Context` (interaction or message).
4. It runs `run_pipeline` and replies through the `Responder`.

If you add a new surface (select menus, for example), reuse `RunFn`, the pipeline
and `Context`: only the way you build the context and the `custom_id` change.
