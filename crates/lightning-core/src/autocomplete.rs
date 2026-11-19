//! Typed autocomplete for slash command options.
//!
//! Once a command option has an [`AutocompleteHandler`], Discord sends an
//! `ApplicationCommandAutocomplete` interaction every time the user types in
//! that option. Lightning routes it to your function and answers with the
//! choices it returns.
//!
//! ```ignore
//! use lightning::prelude::*;
//!
//! #[derive(CommandArgs)]
//! struct Search {
//!     /// Fruit to look up
//!     fruit: String,
//! }
//!
//! #[command]
//! async fn search(args: Args<Search>) -> Reply {
//!     Reply::text(format!("you picked {}", args.fruit))
//! }
//!
//! #[autocomplete(command = "search", option = "fruit")]
//! async fn fruits(focused: Focused) -> Vec<AutocompleteChoice> {
//!     ["apple", "apricot", "banana"]
//!         .into_iter()
//!         .filter(|fruit| fruit.contains(&focused.value))
//!         .map(|fruit| AutocompleteChoice::text(fruit, fruit))
//!         .collect()
//! }
//! ```
//!
//! Options inside subcommands and groups are addressed with `group` and
//! `subcommand` in addition to `command` and `option`:
//!
//! ```ignore
//! #[autocomplete(command = "config", group = "set", subcommand = "channel", option = "channel")]
//! async fn channels(focused: Focused) -> Vec<AutocompleteChoice> { /* … */ }
//! ```
//!
//! Handlers only run for slash commands: autocomplete does not exist on the
//! prefix surface. They also skip the middleware pipeline, so they stay fast.

use std::str::FromStr;

use crate::BoxFuture;
use crate::context::{Context, FromContext};
use crate::error::{Error, Result};

/// Value attached to an [`AutocompleteChoice`].
///
/// The variant must match the type of the option being completed: `String`
/// options take [`ChoiceValue::String`], integer options take
/// [`ChoiceValue::Integer`] and number options take [`ChoiceValue::Number`].
#[derive(Debug, Clone, PartialEq)]
pub enum ChoiceValue {
    /// Text choice.
    String(String),
    /// 64-bit integer choice.
    Integer(i64),
    /// Decimal choice.
    Number(f64),
}

impl ChoiceValue {
    /// Borrows the choice as text, if it is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }
}

impl From<String> for ChoiceValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for ChoiceValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<i64> for ChoiceValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for ChoiceValue {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<u32> for ChoiceValue {
    fn from(value: u32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<f64> for ChoiceValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<f32> for ChoiceValue {
    fn from(value: f32) -> Self {
        Self::Number(f64::from(value))
    }
}

/// One suggestion shown under the option the user is typing.
///
/// Discord limits autocomplete responses to 25 choices; Lightning truncates
/// longer lists automatically. Names are truncated to 100 characters.
#[derive(Debug, Clone, PartialEq)]
pub struct AutocompleteChoice {
    /// Text shown in the Discord UI.
    pub name: String,
    /// Value sent back to the command when the choice is selected.
    pub value: ChoiceValue,
}

impl AutocompleteChoice {
    /// Creates a choice with a custom display name and value.
    pub fn new(name: impl Into<String>, value: impl Into<ChoiceValue>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Creates a text choice where the display name and value are the same.
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: ChoiceValue::String(value.into()),
        }
    }

    /// Creates an integer choice.
    pub fn integer(name: impl Into<String>, value: i64) -> Self {
        Self {
            name: name.into(),
            value: ChoiceValue::Integer(value),
        }
    }

    /// Creates a decimal choice.
    pub fn number(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value: ChoiceValue::Number(value),
        }
    }
}

/// Converts a handler's return value into choices.
///
/// Implemented for `Vec<AutocompleteChoice>`, `Vec<String>`, `Vec<&str>`,
/// arrays of strings and `Result<T>`; `()` means "no suggestions".
pub trait IntoChoices {
    /// Performs the conversion.
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>>;
}

impl IntoChoices for Vec<AutocompleteChoice> {
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>> {
        Ok(self)
    }
}

impl IntoChoices for Vec<String> {
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>> {
        Ok(self
            .into_iter()
            .map(|value| AutocompleteChoice::text(value.clone(), value))
            .collect())
    }
}

impl IntoChoices for Vec<&str> {
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>> {
        Ok(self
            .into_iter()
            .map(|value| AutocompleteChoice::text(value, value))
            .collect())
    }
}

impl<const N: usize> IntoChoices for [&str; N] {
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>> {
        Ok(self
            .into_iter()
            .map(|value| AutocompleteChoice::text(value, value))
            .collect())
    }
}

impl<T: IntoChoices> IntoChoices for Result<T> {
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>> {
        self?.into_choices()
    }
}

impl IntoChoices for () {
    fn into_choices(self) -> Result<Vec<AutocompleteChoice>> {
        Ok(Vec::new())
    }
}

/// The option the user is currently typing.
///
/// Discord sends it as text even for integer and number options, so use
/// [`Focused::parse`] when you need a typed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focused {
    /// Name of the option, without the slash (`query`).
    pub name: String,
    /// Text typed by the user so far.
    pub value: String,
}

impl Focused {
    /// Creates a focused option (mainly useful in tests).
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Parses the typed text into any [`FromStr`] type.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidArgument`] when the text does not fit the type.
    pub fn parse<T>(&self) -> Result<T>
    where
        T: FromStr,
        T::Err: std::fmt::Display,
    {
        self.value
            .parse()
            .map_err(|error: T::Err| Error::InvalidArgument {
                name: self.name.clone(),
                reason: error.to_string(),
            })
    }

    /// Parses the typed text as an integer, returning `None` on failure.
    pub fn as_i64(&self) -> Option<i64> {
        self.value.parse().ok()
    }

    /// Parses the typed text as a decimal, returning `None` on failure.
    pub fn as_f64(&self) -> Option<f64> {
        self.value.parse().ok()
    }
}

#[crate::async_trait]
impl<'a> FromContext<'a> for Focused {
    async fn from_context(ctx: &'a Context) -> Result<Self> {
        ctx.focused
            .clone()
            .ok_or(Error::MissingResource("focused option"))
    }
}

/// Handler signature generated by `#[autocomplete]`.
pub type AutocompleteFn = for<'a> fn(&'a Context) -> BoxFuture<'a, Result<Vec<AutocompleteChoice>>>;

/// Binds an autocomplete handler to a command option.
///
/// You normally do not build this by hand: `#[lightning::autocomplete]`
/// generates it and `inventory` registers it automatically.
#[derive(Clone, Copy)]
pub struct AutocompleteHandler {
    /// Command name without the slash (`search`).
    pub command: &'static str,
    /// Subcommand group, when the option lives inside one (`set`).
    pub group: Option<&'static str>,
    /// Subcommand name, when the option lives inside one (`channel`).
    pub subcommand: Option<&'static str>,
    /// Option name inside that command (`query`).
    pub option: &'static str,
    /// Function that produces the choices.
    pub run: AutocompleteFn,
}

impl std::fmt::Debug for AutocompleteHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutocompleteHandler")
            .field("command", &self.command)
            .field("group", &self.group)
            .field("subcommand", &self.subcommand)
            .field("option", &self.option)
            .finish_non_exhaustive()
    }
}

impl AutocompleteHandler {
    /// `true` if this handler answers this command path and option.
    pub fn matches(
        &self,
        command: &str,
        group: Option<&str>,
        subcommand: Option<&str>,
        option: &str,
    ) -> bool {
        self.command == command
            && self.group == group
            && self.subcommand == subcommand
            && self.option == option
    }
}

inventory::collect!(AutocompleteHandler);
