# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-18

Voice channels land in Lightning.

### Added

- `Voice` extractor: join and leave channels, play local files or any songbird
  input, stop/pause/resume playback and write in the voice channel text chat
  with `voice.say(...)`.
- Voice moderation: `move_to`, `disconnect`, `server_mute`, `server_deafen` and
  stage-channel `suppress`.
- `VoiceStates` extractor: live cache seeded from `GuildCreate` and updated on
  every `VoiceStateUpdate`, with `count`, `members`, `members_with_state` and
  `channel_of`.
- `EventContext` now exposes `voice` and `voice_states`.
- `Error::Voice` for voice-subsystem failures.
- Example bot: `voice-count`, `join`, `leave`, `play` (bundled beep), `say`,
  `move_member`, `disconnect`, `mute` and a `VoiceStateUpdate` logger.

### Changed

- Commands run in spawned tasks, so a slow handler (for example
  `voice.join(...).await`) no longer blocks the shard that must read the voice
  events it is waiting for.
- Prefix messages are pre-filtered: only messages that start with the prefix
  allocate a task.
- Default intents now include `GUILD_VOICE_STATES`.

### Notes

- Audio builds libopus from source: CMake and a C compiler are required.
- Receiving audio (transcription, per-user volume) is not implemented yet.

## [0.1.0] - 2026-11-18

Initial release.

- Slash and prefix commands from one function (`#[command]`).
- Typed arguments (`#[derive(CommandArgs)]`), axum-style extractors.
- tower-style middleware, cooldowns, shared state.
- Buttons, select values and modals (`#[component]`).
- Gateway events (`#[event]`), automatic registration with `inventory`.
- Automatic sharding and a testable `Responder` trait.
