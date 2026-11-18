//! Voice channels: join, leave, play audio, talk in the channel chat and
//! moderate members.
//!
//! ```no_run
//! use lightning::prelude::*;
//!
//! #[command(guild_only)]
//! async fn join(ctx: Context, voice: Voice, states: VoiceStates, user: UserId) -> Result<Reply> {
//!     let guild = ctx.guild_id()?;
//!     let channel = states
//!         .channel_of(guild, user.0)
//!         .ok_or_else(|| Error::Voice("join a voice channel first".into()))?;
//!
//!     let connection = voice.join(guild, channel).await?;
//!     connection.play_file("assets/beep.wav").await;
//!
//!     Ok(Reply::text(format!("connected to <#{channel}>")))
//! }
//! ```
//!
//! Audio is powered by [songbird](https://github.com/serenity-rs/songbird), but
//! you do not need to know that: Lightning owns the gateway connection, the
//! shard routing and the event plumbing.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use async_trait::async_trait;
use dashmap::DashMap;
use lightning_core::{Context, Error, FromContext, Reply, Result};
use songbird::error::{JoinError, JoinResult};
use songbird::id::{ChannelId as VoiceChannelId, GuildId as VoiceGuildId, UserId as VoiceUserId};
pub use songbird::input::Input;
use songbird::shards::{Shard as VoiceShard, VoiceUpdate};
pub use songbird::tracks::TrackHandle;
use songbird::{Call, Config};
use tokio::sync::Mutex;
use tracing::warn;
use twilight_gateway::MessageSender;
use twilight_http::Client;
use twilight_model::gateway::payload::incoming::VoiceServerUpdate;
use twilight_model::gateway::payload::outgoing::update_voice_state::UpdateVoiceState;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use twilight_model::voice::VoiceState;

use crate::voice_states::VoiceStates;

/// Handle to Lightning's voice subsystem.
///
/// Cheap to clone: every copy points to the same connections and cache. Ask for
/// it in a handler with the `Voice` extractor.
#[derive(Clone)]
pub struct Voice {
    inner: Arc<Inner>,
}

struct Inner {
    http: Arc<Client>,
    config: Config,
    states: VoiceStates,
    calls: DashMap<Id<GuildMarker>, Arc<Mutex<Call>>>,
    senders: Arc<RwLock<HashMap<u32, MessageSender>>>,
    shard_total: u32,
    user_id: OnceLock<Id<UserMarker>>,
}

impl Voice {
    pub(crate) fn new(http: Arc<Client>, states: VoiceStates, shard_total: u32) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                config: Config::default(),
                states,
                calls: DashMap::new(),
                senders: Arc::new(RwLock::new(HashMap::new())),
                shard_total,
                user_id: OnceLock::new(),
            }),
        }
    }

    /// The live voice-state cache.
    pub fn states(&self) -> &VoiceStates {
        &self.inner.states
    }

    /// Number of shards in use.
    pub fn shard_total(&self) -> u32 {
        self.inner.shard_total
    }

    /// Id of the bot user, once Discord has sent `READY`.
    pub fn user_id(&self) -> Option<Id<UserMarker>> {
        self.inner.user_id.get().copied()
    }

    /// Amount of users connected to a voice channel.
    pub fn count(&self, channel_id: Id<ChannelMarker>) -> usize {
        self.inner.states.count(channel_id)
    }

    /// Joins a voice channel and waits until Discord hands over the voice
    /// credentials.
    ///
    /// # Errors
    ///
    /// Fails if the bot is not ready yet, the shard disappeared or Discord did
    /// not answer within the gateway timeout (10 seconds).
    pub async fn join(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> Result<VoiceConnection> {
        let user_id = self.inner.user_id.get().copied().ok_or_else(|| {
            Error::Voice("the bot has not received READY from Discord yet".into())
        })?;

        let call = self.call_for(guild_id, user_id);

        let join = {
            let mut call = call.lock().await;
            call.join(voice_channel(channel_id))
                .await
                .map_err(voice_error)?
        };

        // The `Join` future waits for VOICE_STATE_UPDATE and VOICE_SERVER_UPDATE.
        // Never await it while holding the Call lock: the event handlers need it.
        join.await.map_err(voice_error)?;

        Ok(VoiceConnection {
            guild_id,
            channel_id,
            call,
        })
    }

    /// Leaves the voice channel of a guild, if connected.
    ///
    /// # Errors
    ///
    /// Fails if the gateway request could not be sent.
    pub async fn leave(&self, guild_id: Id<GuildMarker>) -> Result<()> {
        let Some((_, call)) = self.inner.calls.remove(&guild_id) else {
            return Ok(());
        };

        let mut call = call.lock().await;
        call.leave().await.map_err(voice_error)?;

        Ok(())
    }

    /// Current connection of a guild, if the bot is in a voice channel.
    pub async fn connection(&self, guild_id: Id<GuildMarker>) -> Option<VoiceConnection> {
        let call = self
            .inner
            .calls
            .get(&guild_id)
            .map(|call| Arc::clone(&call))?;

        let channel_id = {
            let call = call.lock().await;
            call.current_channel()?
        };

        Some(VoiceConnection {
            guild_id,
            channel_id: Id::new(channel_id.0.get()),
            call,
        })
    }

    /// `true` if the bot is connected to a voice channel in that guild.
    pub async fn is_connected(&self, guild_id: Id<GuildMarker>) -> bool {
        self.connection(guild_id).await.is_some()
    }

    /// Plays a local audio file (mp3, wav, flac, ogg…) in the guild's voice
    /// channel.
    ///
    /// # Errors
    ///
    /// Fails if the bot is not connected to voice in that guild.
    pub async fn play_file(
        &self,
        guild_id: Id<GuildMarker>,
        path: impl Into<PathBuf>,
    ) -> Result<TrackHandle> {
        let connection = self
            .connection(guild_id)
            .await
            .ok_or_else(|| Error::Voice("the bot is not connected to voice".into()))?;

        Ok(connection.play_file(path).await)
    }

    /// Sends a message to a voice channel's text chat.
    ///
    /// Voice channels also have text chat on Discord (text-in-voice), so this
    /// is the same as sending to any other channel.
    ///
    /// # Errors
    ///
    /// Fails if Discord rejects the message (missing permissions, too long…).
    pub async fn say(&self, channel_id: Id<ChannelMarker>, reply: impl Into<Reply>) -> Result<()> {
        let reply = reply.into();
        let embeds = crate::convert::to_embeds(&reply)?;
        let components = crate::convert::to_components(&reply)?;

        let mut request = self.inner.http.create_message(channel_id);

        if let Some(content) = reply.content.as_deref() {
            request = request.content(content);
        }
        if !embeds.is_empty() {
            request = request.embeds(&embeds);
        }
        if !components.is_empty() {
            request = request.components(&components);
        }

        request
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;

        Ok(())
    }

    /// Moves a member to another voice channel.
    ///
    /// Requires the `MOVE_MEMBERS` permission.
    ///
    /// # Errors
    ///
    /// Fails if Discord rejects the request.
    pub async fn move_to(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.inner
            .http
            .update_user_voice_state(guild_id, user_id, channel_id)
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;

        Ok(())
    }

    /// Disconnects a member from voice.
    ///
    /// Requires the `MOVE_MEMBERS` permission.
    ///
    /// # Errors
    ///
    /// Fails if Discord rejects the request.
    pub async fn disconnect(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Result<()> {
        self.inner
            .http
            .update_guild_member(guild_id, user_id)
            .channel_id(None)
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;

        Ok(())
    }

    /// Server-mutes or unmutes a member.
    ///
    /// Requires the `MUTE_MEMBERS` permission.
    ///
    /// # Errors
    ///
    /// Fails if Discord rejects the request.
    pub async fn server_mute(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        mute: bool,
    ) -> Result<()> {
        self.inner
            .http
            .update_guild_member(guild_id, user_id)
            .mute(mute)
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;

        Ok(())
    }

    /// Server-deafens or undeafens a member.
    ///
    /// Requires the `DEAFEN_MEMBERS` permission.
    ///
    /// # Errors
    ///
    /// Fails if Discord rejects the request.
    pub async fn server_deafen(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        deafen: bool,
    ) -> Result<()> {
        self.inner
            .http
            .update_guild_member(guild_id, user_id)
            .deaf(deafen)
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;

        Ok(())
    }

    /// Suppresses a member in a stage channel.
    ///
    /// Requires the `MUTE_MEMBERS` permission.
    ///
    /// # Errors
    ///
    /// Fails if Discord rejects the request.
    pub async fn suppress(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.inner
            .http
            .update_user_voice_state(guild_id, user_id, channel_id)
            .suppress()
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;

        Ok(())
    }

    fn call_for(&self, guild_id: Id<GuildMarker>, user_id: Id<UserMarker>) -> Arc<Mutex<Call>> {
        self.inner
            .calls
            .entry(guild_id)
            .or_insert_with(|| {
                let shard = VoiceShard::Generic(Arc::new(ShardVoice {
                    senders: self.inner.senders.clone(),
                    shard_total: self.inner.shard_total,
                }));

                Arc::new(Mutex::new(Call::from_config(
                    voice_guild(guild_id),
                    shard,
                    voice_user(user_id),
                    self.inner.config.clone(),
                )))
            })
            .clone()
    }

    pub(crate) fn register_shard(&self, shard_id: u32, sender: MessageSender) {
        if let Ok(mut senders) = self.inner.senders.write() {
            senders.insert(shard_id, sender);
        }
    }

    pub(crate) fn set_user_id(&self, user_id: Id<UserMarker>) {
        let _ = self.inner.user_id.set(user_id);
    }

    pub(crate) async fn handle_state(&self, state: &VoiceState) {
        self.inner.states.apply(state);

        let Some(bot_id) = self.inner.user_id.get().copied() else {
            return;
        };
        if state.user_id != bot_id {
            return;
        }

        let Some(guild_id) = state.guild_id else {
            return;
        };
        let Some(call) = self
            .inner
            .calls
            .get(&guild_id)
            .map(|call| Arc::clone(&call))
        else {
            return;
        };

        let mut call = call.lock().await;
        call.update_state(
            state.session_id.clone(),
            state.channel_id.map(voice_channel),
        );
    }

    pub(crate) async fn handle_server(&self, event: &VoiceServerUpdate) {
        let Some(endpoint) = event.endpoint.clone() else {
            return;
        };
        let Some(call) = self
            .inner
            .calls
            .get(&event.guild_id)
            .map(|call| Arc::clone(&call))
        else {
            return;
        };

        let mut call = call.lock().await;
        call.update_server(endpoint, event.token.clone());
    }
}

impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice")
            .field("shard_total", &self.inner.shard_total)
            .field("calls", &self.inner.calls.len())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<'a> FromContext<'a> for Voice {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.ext::<Voice>()
            .map(|voice| (*voice).clone())
            .ok_or(Error::MissingResource("voice"))
    }
}

/// An active voice connection to one guild.
#[derive(Clone)]
pub struct VoiceConnection {
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    call: Arc<Mutex<Call>>,
}

impl VoiceConnection {
    /// Guild of the connection.
    pub fn guild_id(&self) -> Id<GuildMarker> {
        self.guild_id
    }

    /// Voice channel the bot is connected to.
    pub fn channel_id(&self) -> Id<ChannelMarker> {
        self.channel_id
    }

    /// Raw songbird [`Call`], for advanced control.
    pub fn call(&self) -> Arc<Mutex<Call>> {
        Arc::clone(&self.call)
    }

    /// Plays a local audio file (mp3, wav, flac, ogg…).
    ///
    /// Returns a [`TrackHandle`] to stop, pause or loop the track.
    pub async fn play_file(&self, path: impl Into<PathBuf>) -> TrackHandle {
        self.play_input(Input::from(songbird::input::File::new(path.into())))
            .await
    }

    /// Plays any songbird input (HTTP, raw PCM, your own decoder…).
    pub async fn play_input(&self, input: Input) -> TrackHandle {
        let mut call = self.call.lock().await;
        call.play_input(input)
    }

    /// Stops every track in the connection.
    pub async fn stop(&self) {
        let mut call = self.call.lock().await;
        call.stop();
    }

    /// Number of tracks waiting in the queue.
    pub async fn queue_len(&self) -> usize {
        let call = self.call.lock().await;
        call.queue().len()
    }

    /// Pauses the current track.
    ///
    /// # Errors
    ///
    /// Fails if there is no track playing.
    pub async fn pause(&self) -> Result<()> {
        let call = self.call.lock().await;
        call.queue().pause().map_err(voice_error)
    }

    /// Resumes the current track.
    ///
    /// # Errors
    ///
    /// Fails if there is no track paused.
    pub async fn resume(&self) -> Result<()> {
        let call = self.call.lock().await;
        call.queue().resume().map_err(voice_error)
    }

    /// Leaves the voice channel.
    ///
    /// # Errors
    ///
    /// Fails if the gateway request could not be sent.
    pub async fn leave(&self) -> Result<()> {
        let mut call = self.call.lock().await;
        call.leave().await.map_err(voice_error)
    }
}

impl std::fmt::Debug for VoiceConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceConnection")
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .finish()
    }
}

/// Shard routing: Discord sends voice payloads through the shard that owns the
/// guild, calculated from the guild id.
pub(crate) fn shard_for(guild_id: u64, shard_total: u32) -> u32 {
    ((guild_id >> 22) % u64::from(shard_total.max(1))) as u32
}

struct ShardVoice {
    senders: Arc<RwLock<HashMap<u32, MessageSender>>>,
    shard_total: u32,
}

#[async_trait]
impl VoiceUpdate for ShardVoice {
    async fn update_voice_state(
        &self,
        guild_id: VoiceGuildId,
        channel_id: Option<VoiceChannelId>,
        self_deaf: bool,
        self_mute: bool,
    ) -> JoinResult<()> {
        let raw_guild = guild_id.0.get();
        let shard = shard_for(raw_guild, self.shard_total);

        let sender = self
            .senders
            .read()
            .ok()
            .and_then(|senders| senders.get(&shard).cloned())
            .ok_or(JoinError::NoSender)?;

        let update = UpdateVoiceState::new(
            Id::<GuildMarker>::new(raw_guild),
            channel_id.map(|channel| Id::<ChannelMarker>::new(channel.0.get())),
            self_deaf,
            self_mute,
        );

        sender.command(&update).map_err(|error| {
            warn!(%error, shard, "could not send the voice state update");
            JoinError::NoSender
        })?;

        Ok(())
    }
}

fn voice_guild(id: Id<GuildMarker>) -> VoiceGuildId {
    VoiceGuildId(NonZeroU64::new(id.get()).expect("guild ids are never zero"))
}

fn voice_user(id: Id<UserMarker>) -> VoiceUserId {
    VoiceUserId(NonZeroU64::new(id.get()).expect("user ids are never zero"))
}

fn voice_channel(id: Id<ChannelMarker>) -> VoiceChannelId {
    VoiceChannelId(NonZeroU64::new(id.get()).expect("channel ids are never zero"))
}

fn voice_error(error: impl std::fmt::Display) -> Error {
    Error::Voice(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Voice, VoiceStates, shard_for};
    use std::sync::Arc;
    use twilight_http::Client;
    use twilight_model::id::Id;

    #[test]
    fn routes_guilds_to_the_right_shard() {
        assert_eq!(shard_for(1 << 22, 4), 1);
        assert_eq!(shard_for(3 << 22, 4), 3);
        assert_eq!(shard_for(4 << 22, 4), 0);
        assert_eq!(shard_for(123, 1), 0);
    }

    #[tokio::test]
    async fn joining_requires_ready() {
        let voice = Voice::new(
            Arc::new(Client::new("test-token".to_owned())),
            VoiceStates::new(),
            1,
        );

        let error = voice
            .join(Id::new(1), Id::new(2))
            .await
            .expect_err("the bot is not ready yet");

        assert!(error.to_string().contains("READY"));
    }

    #[tokio::test]
    async fn counts_through_the_handle() {
        let voice = Voice::new(
            Arc::new(Client::new("test-token".to_owned())),
            VoiceStates::new(),
            1,
        );

        assert_eq!(voice.count(Id::new(10)), 0);
        assert_eq!(voice.shard_total(), 1);
        assert!(voice.user_id().is_none());
    }
}
