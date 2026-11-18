//! Framework errors.
//!
//! Every handler can return `Result<_, Error>`: Lightning turns the error into a
//! readable ephemeral reply for the user and logs it through `tracing`.

use crate::reply::Reply;

/// Result alias used across the framework.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error produced by a command, extractor or middleware.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// User-defined error.
    #[error("{0}")]
    Custom(String),

    /// A required argument is missing.
    #[error("missing required argument `{0}`")]
    MissingArgument(String),

    /// An argument could not be converted to the expected type.
    #[error("invalid value for `{name}`: {reason}")]
    InvalidArgument {
        /// Argument name.
        name: String,
        /// Reason for the failure.
        reason: String,
    },

    /// The command only works inside a guild.
    #[error("this command only works inside a server")]
    GuildOnly,

    /// The command only works in direct messages.
    #[error("this command only works in direct messages")]
    DmOnly,

    /// The user is not allowed to run the command.
    #[error("you don't have permission to use this command")]
    Forbidden,

    /// A usage limit was hit.
    #[error("too many invocations: try again in {retry_after_ms} ms")]
    Cooldown {
        /// Milliseconds to wait.
        retry_after_ms: u64,
    },

    /// The command does not exist on this surface (slash, prefix, component…).
    #[error("this command is not available on this surface")]
    UnsupportedSurface,

    /// An extractor could not find the resource it needed.
    #[error("missing `{0}` in the context")]
    MissingResource(&'static str),

    /// No state of that type was injected.
    #[error("state `{0}` was not injected")]
    MissingState(&'static str),

    /// Network or Discord API failure.
    #[error("error talking to Discord: {0}")]
    Transport(String),

    /// Voice subsystem failure (joining, playing audio, moderation…).
    #[error("voice error: {0}")]
    Voice(String),

    /// Internal framework failure.
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Builds a custom error from anything convertible to `String`.
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom(message.into())
    }

    /// Builds the [`Reply`] the user sees when the handler fails.
    pub fn to_reply(&self) -> Reply {
        Reply::text(format!("❌ {self}")).ephemeral()
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Internal(value.to_string())
    }
}
