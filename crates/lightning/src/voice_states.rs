//! Live voice-state cache.
//!
//! Discord does not expose an endpoint that lists who is inside a voice
//! channel, so Lightning keeps a small cache fed by `GuildCreate` and
//! `VoiceStateUpdate` events. It is seeded when a guild becomes available and
//! updated on every join, move, mute or disconnect.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use lightning_core::{Context, Error, FromContext, Result};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use twilight_model::voice::VoiceState;

/// Who is connected to which voice channel, right now.
///
/// Cheap to clone: all copies share the same cache.
#[derive(Debug, Default, Clone)]
pub struct VoiceStates {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    states: DashMap<(Id<GuildMarker>, Id<UserMarker>), VoiceState>,
}

impl VoiceStates {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of users connected to `channel_id`.
    ///
    /// Bots connected to the channel are included; use [`Self::members_with_state`]
    /// if you need to filter them out.
    pub fn count(&self, channel_id: Id<ChannelMarker>) -> usize {
        self.len(channel_id)
    }

    /// Number of users connected to `channel_id` (alias of [`Self::count`]).
    pub fn len(&self, channel_id: Id<ChannelMarker>) -> usize {
        self.inner
            .states
            .iter()
            .filter(|entry| entry.value().channel_id == Some(channel_id))
            .count()
    }

    /// `true` if nobody is connected to `channel_id`.
    pub fn is_empty(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.len(channel_id) == 0
    }

    /// Ids of the users connected to `channel_id`.
    pub fn members(&self, channel_id: Id<ChannelMarker>) -> Vec<Id<UserMarker>> {
        self.inner
            .states
            .iter()
            .filter(|entry| entry.value().channel_id == Some(channel_id))
            .map(|entry| entry.key().1)
            .collect()
    }

    /// Full voice states of the users connected to `channel_id`.
    pub fn members_with_state(&self, channel_id: Id<ChannelMarker>) -> Vec<VoiceState> {
        self.inner
            .states
            .iter()
            .filter(|entry| entry.value().channel_id == Some(channel_id))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Channel the user is connected to, if any.
    pub fn channel_of(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<Id<ChannelMarker>> {
        self.inner
            .states
            .get(&(guild_id, user_id))
            .and_then(|state| state.channel_id)
    }

    /// Full voice state of a user, if they are connected.
    pub fn state_of(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<VoiceState> {
        self.inner
            .states
            .get(&(guild_id, user_id))
            .map(|state| state.value().clone())
    }

    /// `true` if the user is connected to `channel_id`.
    pub fn is_connected(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        self.channel_of(guild_id, user_id) == Some(channel_id)
    }

    /// Total number of tracked voice states.
    pub fn total(&self) -> usize {
        self.inner.states.len()
    }

    pub(crate) fn apply(&self, state: &VoiceState) {
        let Some(guild_id) = state.guild_id else {
            return;
        };

        let key = (guild_id, state.user_id);

        if state.channel_id.is_some() {
            self.inner.states.insert(key, state.clone());
        } else {
            self.inner.states.remove(&key);
        }
    }

    pub(crate) fn seed(&self, guild_id: Id<GuildMarker>, states: &[VoiceState]) {
        self.clear_guild(guild_id);

        for state in states {
            if state.channel_id.is_some() {
                self.inner
                    .states
                    .insert((guild_id, state.user_id), state.clone());
            }
        }
    }

    pub(crate) fn clear_guild(&self, guild_id: Id<GuildMarker>) {
        self.inner.states.retain(|(guild, _), _| *guild != guild_id);
    }
}

#[async_trait]
impl<'a> FromContext<'a> for VoiceStates {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.ext::<VoiceStates>()
            .map(|states| (*states).clone())
            .ok_or(Error::MissingResource("voice states"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(guild: u64, user: u64, channel: Option<u64>) -> VoiceState {
        VoiceState {
            channel_id: channel.map(Id::new),
            deaf: false,
            guild_id: Some(Id::new(guild)),
            member: None,
            mute: false,
            request_to_speak_timestamp: None,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
            self_video: false,
            session_id: format!("session-{user}"),
            suppress: false,
            user_id: Id::new(user),
        }
    }

    #[test]
    fn counts_members_per_channel() {
        let states = VoiceStates::new();
        let channel = Id::new(10);

        states.apply(&state(1, 100, Some(10)));
        states.apply(&state(1, 101, Some(10)));
        states.apply(&state(1, 102, Some(11)));

        assert_eq!(states.count(channel), 2);
        assert_eq!(states.count(Id::new(11)), 1);
        assert_eq!(states.count(Id::new(12)), 0);
    }

    #[test]
    fn moving_and_disconnecting_updates_the_cache() {
        let states = VoiceStates::new();
        let guild = Id::new(1);
        let user = Id::new(100);

        states.apply(&state(1, 100, Some(10)));
        assert_eq!(states.channel_of(guild, user), Some(Id::new(10)));

        states.apply(&state(1, 100, Some(11)));
        assert_eq!(states.channel_of(guild, user), Some(Id::new(11)));
        assert_eq!(states.count(Id::new(10)), 0);

        states.apply(&state(1, 100, None));
        assert_eq!(states.channel_of(guild, user), None);
        assert_eq!(states.total(), 0);
    }

    #[test]
    fn seeding_replaces_the_guild_snapshot() {
        let states = VoiceStates::new();

        states.apply(&state(1, 100, Some(10)));
        states.seed(Id::new(1), &[state(1, 200, Some(12)), state(1, 201, None)]);

        assert_eq!(states.total(), 1);
        assert_eq!(states.count(Id::new(12)), 1);
        assert_eq!(states.channel_of(Id::new(1), Id::new(100)), None);
    }

    #[test]
    fn lists_members_of_a_channel() {
        let states = VoiceStates::new();
        let channel = Id::new(10);

        states.apply(&state(1, 100, Some(10)));
        states.apply(&state(1, 101, Some(10)));

        let mut members = states.members(channel);
        members.sort_by_key(|id| id.get());

        assert_eq!(members, vec![Id::new(100), Id::new(101)]);
        assert!(states.is_connected(Id::new(1), Id::new(100), channel));
    }
}
