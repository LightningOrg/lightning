//! Building replies: messages, embeds, buttons, select menus and modals.
//!
//! `Reply` is what handlers return. It is deliberately *builder*-shaped:
//!
//! ```
//! use lightning_core::{ActionRow, Button, Embed, Reply, SelectMenu};
//!
//! let reply = Reply::text("Pick a color")
//!     .embed(Embed::new().title("Confirmation").color(0xF5A623))
//!     .components(vec![
//!         ActionRow::new().select(
//!             SelectMenu::text("poll", "Choose one")
//!                 .option("red", "Red")
//!                 .option("blue", "Blue"),
//!         ),
//!         ActionRow::new().button(Button::success("confirm:yes", "Let's go")),
//!     ]);
//!
//! assert_eq!(reply.embeds.len(), 1);
//! assert_eq!(reply.components.len(), 2);
//! ```

use serde_json::{Map, Value, json};
use twilight_model::channel::ChannelType;

/// Visual style of a button, following the Discord API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Blue (`blurple`).
    Primary,
    /// Grey.
    Secondary,
    /// Green.
    Success,
    /// Red.
    Danger,
    /// External link (does not send an interaction).
    Link,
}

impl ButtonStyle {
    fn code(self) -> u8 {
        match self {
            Self::Primary => 1,
            Self::Secondary => 2,
            Self::Success => 3,
            Self::Danger => 4,
            Self::Link => 5,
        }
    }
}

/// A button inside an [`ActionRow`].
#[derive(Debug, Clone)]
pub struct Button {
    value: Map<String, Value>,
}

impl Button {
    /// Creates a button with a style and a `custom_id`.
    pub fn new(style: ButtonStyle, custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        let mut value = Map::new();
        value.insert("type".into(), json!(2));
        value.insert("style".into(), json!(style.code()));
        value.insert("label".into(), json!(label.into()));
        value.insert("custom_id".into(), json!(custom_id.into()));

        Self { value }
    }

    /// Blue button.
    pub fn primary(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Primary, custom_id, label)
    }

    /// Grey button.
    pub fn secondary(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Secondary, custom_id, label)
    }

    /// Green button.
    pub fn success(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Success, custom_id, label)
    }

    /// Red button.
    pub fn danger(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Danger, custom_id, label)
    }

    /// Link button: opens a URL in the browser.
    pub fn link(url: impl Into<String>, label: impl Into<String>) -> Self {
        let mut button = Self::new(ButtonStyle::Link, String::new(), label);
        button.value.remove("custom_id");
        button.value.insert("url".into(), json!(url.into()));
        button
    }

    /// Disables the button.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.value.insert("disabled".into(), json!(disabled));
        self
    }

    /// Adds an emoji (unicode or `name:id`).
    pub fn emoji(mut self, emoji: impl Into<String>) -> Self {
        self.value
            .insert("emoji".into(), json!({ "name": emoji.into() }));
        self
    }

    /// JSON representation ready for the API.
    pub fn to_value(self) -> Value {
        Value::Object(self.value)
    }
}

/// Kind of select menu, matching Discord's component types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    /// Classic dropdown with custom string options (type 3).
    Text,
    /// User picker (type 5).
    User,
    /// Role picker (type 6).
    Role,
    /// User-or-role picker (type 7).
    Mentionable,
    /// Channel picker (type 8).
    Channel,
}

impl SelectKind {
    /// Numeric code used by the Discord API.
    pub const fn code(self) -> u8 {
        match self {
            Self::Text => 3,
            Self::User => 5,
            Self::Role => 6,
            Self::Mentionable => 7,
            Self::Channel => 8,
        }
    }
}

/// A dropdown (select menu) inside an [`ActionRow`].
///
/// All five Discord kinds share the same builder: [`SelectMenu::text`] is the
/// classic dropdown and takes options, while the rest are populated by Discord
/// with users, roles or channels.
#[derive(Debug, Clone)]
pub struct SelectMenu {
    value: Map<String, Value>,
}

impl SelectMenu {
    /// Creates a select menu of any kind with a `custom_id`.
    pub fn new(kind: SelectKind, custom_id: impl Into<String>) -> Self {
        let mut value = Map::new();
        value.insert("type".into(), json!(kind.code()));
        value.insert("custom_id".into(), json!(custom_id.into()));

        Self { value }
    }

    /// Classic string select menu (type 3).
    ///
    /// It needs at least one [`option`](Self::option), with at most 25.
    #[doc(alias = "string select")]
    pub fn text(custom_id: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self::new(SelectKind::Text, custom_id).placeholder(placeholder)
    }

    /// User select menu (type 5).
    pub fn user(custom_id: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self::new(SelectKind::User, custom_id).placeholder(placeholder)
    }

    /// Role select menu (type 6).
    pub fn role(custom_id: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self::new(SelectKind::Role, custom_id).placeholder(placeholder)
    }

    /// User-or-role select menu (type 7).
    pub fn mentionable(custom_id: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self::new(SelectKind::Mentionable, custom_id).placeholder(placeholder)
    }

    /// Channel select menu (type 8).
    pub fn channel(custom_id: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self::new(SelectKind::Channel, custom_id).placeholder(placeholder)
    }

    /// Text shown while nothing is selected.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.value
            .insert("placeholder".into(), json!(placeholder.into()));
        self
    }

    /// Adds an option to a text select menu.
    pub fn option(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.push_option(json!({
            "label": label.into(),
            "value": value.into(),
        }));
        self
    }

    /// Adds an option with a description.
    pub fn option_with(
        mut self,
        value: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.push_option(json!({
            "label": label.into(),
            "value": value.into(),
            "description": description.into(),
        }));
        self
    }

    /// Adds an option with an emoji (unicode or `name:id`).
    pub fn option_emoji(
        mut self,
        value: impl Into<String>,
        label: impl Into<String>,
        emoji: impl Into<String>,
    ) -> Self {
        self.push_option(json!({
            "label": label.into(),
            "value": value.into(),
            "emoji": { "name": emoji.into() },
        }));
        self
    }

    /// Adds an option selected by default.
    pub fn option_default(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.push_option(json!({
            "label": label.into(),
            "value": value.into(),
            "default": true,
        }));
        self
    }

    /// Adds many `(value, label)` options.
    pub fn options<I, V, L>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = (V, L)>,
        V: Into<String>,
        L: Into<String>,
    {
        for (value, label) in options {
            self = self.option(value, label);
        }
        self
    }

    /// Minimum number of options that must be selected.
    pub fn min_values(mut self, min: u8) -> Self {
        self.value.insert("min_values".into(), json!(min));
        self
    }

    /// Maximum number of options that may be selected.
    pub fn max_values(mut self, max: u8) -> Self {
        self.value.insert("max_values".into(), json!(max));
        self
    }

    /// Disables the select menu.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.value.insert("disabled".into(), json!(disabled));
        self
    }

    /// Restricts a channel select to some channel types.
    pub fn channel_types(mut self, types: impl IntoIterator<Item = ChannelType>) -> Self {
        let types: Vec<Value> = types.into_iter().map(|kind| json!(kind)).collect();
        self.value
            .insert("channel_types".into(), Value::Array(types));
        self
    }

    /// JSON representation ready for the API.
    pub fn to_value(self) -> Value {
        Value::Object(self.value)
    }

    fn push_option(&mut self, option: Value) {
        self.value
            .entry("options")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("options is always an array")
            .push(option);
    }
}

impl From<SelectMenu> for Value {
    fn from(menu: SelectMenu) -> Self {
        menu.to_value()
    }
}

impl From<Button> for Value {
    fn from(button: Button) -> Self {
        button.to_value()
    }
}

/// Row of components (buttons, select menus or inputs).
#[derive(Debug, Clone, Default)]
pub struct ActionRow {
    components: Vec<Value>,
}

impl ActionRow {
    /// Empty row.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds any component that can be serialized.
    pub fn component(mut self, component: impl Into<Value>) -> Self {
        self.components.push(component.into());
        self
    }

    /// Adds a button.
    pub fn button(self, button: Button) -> Self {
        self.component(button.to_value())
    }

    /// Adds a select menu.
    pub fn select(self, menu: SelectMenu) -> Self {
        self.component(menu.to_value())
    }

    /// Turns the row into JSON.
    pub fn to_value(self) -> Value {
        json!({ "type": 1, "components": self.components })
    }
}

/// Discord embed with a fluent API.
#[derive(Debug, Clone, Default)]
pub struct Embed {
    value: Map<String, Value>,
}

impl Embed {
    /// Empty embed (Discord needs at least one field to display it).
    pub fn new() -> Self {
        Self::default()
    }

    /// Embed title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.value.insert("title".into(), json!(title.into()));
        self
    }

    /// Description (markdown allowed).
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.value
            .insert("description".into(), json!(description.into()));
        self
    }

    /// Color in `0xRRGGBB` format.
    pub fn color(mut self, color: u32) -> Self {
        self.value.insert("color".into(), json!(color));
        self
    }

    /// Title URL.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.value.insert("url".into(), json!(url.into()));
        self
    }

    /// Adds a field.
    pub fn field(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        inline: bool,
    ) -> Self {
        let field = json!({
            "name": name.into(),
            "value": value.into(),
            "inline": inline,
        });

        self.value
            .entry("fields")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("fields is always an array")
            .push(field);

        self
    }

    /// Embed footer.
    pub fn footer(mut self, text: impl Into<String>) -> Self {
        self.value
            .insert("footer".into(), json!({ "text": text.into() }));
        self
    }

    /// Embed author.
    pub fn author(mut self, name: impl Into<String>) -> Self {
        self.value
            .insert("author".into(), json!({ "name": name.into() }));
        self
    }

    /// Main image.
    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.value
            .insert("image".into(), json!({ "url": url.into() }));
        self
    }

    /// Thumbnail (top-right corner).
    pub fn thumbnail(mut self, url: impl Into<String>) -> Self {
        self.value
            .insert("thumbnail".into(), json!({ "url": url.into() }));
        self
    }

    /// ISO-8601 timestamp.
    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.value
            .insert("timestamp".into(), json!(timestamp.into()));
        self
    }

    /// JSON representation ready for the API.
    pub fn to_value(self) -> Value {
        let mut value = self.value;
        value
            .entry("type")
            .or_insert_with(|| Value::String("rich".into()));
        Value::Object(value)
    }
}

/// Modal (popup form) sent in response to an interaction.
#[derive(Debug, Clone)]
pub struct Modal {
    custom_id: String,
    title: String,
    components: Vec<Value>,
}

impl Modal {
    /// Creates a modal with its `custom_id` and title.
    pub fn new(custom_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            custom_id: custom_id.into(),
            title: title.into(),
            components: Vec::new(),
        }
    }

    /// Adds a required text input.
    pub fn text_input(
        mut self,
        custom_id: impl Into<String>,
        label: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        let input = json!({
            "type": 4,
            "custom_id": custom_id.into(),
            "label": label.into(),
            "style": 1,
            "placeholder": placeholder.into(),
            "required": true,
            "max_length": 1024,
        });
        self.components
            .push(json!({ "type": 1, "components": [input] }));
        self
    }

    /// Modal `custom_id`.
    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }

    /// Modal title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// JSON with `custom_id`, `title` and `components`.
    pub fn to_value(&self) -> Value {
        json!({
            "custom_id": self.custom_id,
            "title": self.title,
            "components": self.components,
        })
    }

    /// Only the component rows (what `InteractionResponseData` expects).
    pub fn components(&self) -> Value {
        Value::Array(self.components.clone())
    }
}

/// Handler response.
///
/// An empty `Reply` means "send nothing" (useful after a `defer`).
#[derive(Debug, Clone, Default)]
pub struct Reply {
    /// Message text.
    pub content: Option<String>,
    /// Embeds, already serialized.
    pub embeds: Vec<Value>,
    /// Component rows, already serialized.
    pub components: Vec<Value>,
    /// Allowed mentions (`AllowedMentions` JSON).
    pub allowed_mentions: Option<Value>,
    /// Whether only the invoker sees the reply.
    pub ephemeral: bool,
    /// Whether the message is read aloud by TTS.
    pub tts: bool,
    /// When present, the interaction is answered with a modal.
    pub modal: Option<Modal>,
}

impl Reply {
    /// Empty reply.
    pub fn new() -> Self {
        Self::default()
    }

    /// Text message.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            ..Self::default()
        }
    }

    /// Adds an embed.
    pub fn embed(mut self, embed: Embed) -> Self {
        self.embeds.push(embed.to_value());
        self
    }

    /// Adds several component rows.
    pub fn components(mut self, rows: Vec<ActionRow>) -> Self {
        self.components
            .extend(rows.into_iter().map(ActionRow::to_value));
        self
    }

    /// Adds raw component JSON (escape hatch).
    pub fn raw_component(mut self, component: Value) -> Self {
        self.components.push(component);
        self
    }

    /// Marks the reply as ephemeral.
    pub fn ephemeral(mut self) -> Self {
        self.ephemeral = true;
        self
    }

    /// Raw allowed mentions (JSON).
    pub fn allowed_mentions(mut self, allowed: Value) -> Self {
        self.allowed_mentions = Some(allowed);
        self
    }

    /// Disallow every mention.
    pub fn no_mentions(mut self) -> Self {
        self.allowed_mentions = Some(json!({ "parse": [] }));
        self
    }

    /// Replies with a modal.
    pub fn modal(mut self, modal: Modal) -> Self {
        self.modal = Some(modal);
        self
    }

    /// `true` if there is nothing to send.
    pub fn is_empty(&self) -> bool {
        self.content.is_none()
            && self.embeds.is_empty()
            && self.components.is_empty()
            && self.modal.is_none()
    }

    /// Builds the `data` object expected by the API.
    pub fn data(&self) -> Map<String, Value> {
        let mut data = Map::new();

        if let Some(content) = &self.content {
            data.insert("content".into(), json!(content));
        }
        if !self.embeds.is_empty() {
            data.insert("embeds".into(), Value::Array(self.embeds.clone()));
        }
        if !self.components.is_empty() {
            data.insert("components".into(), Value::Array(self.components.clone()));
        }
        if let Some(allowed) = &self.allowed_mentions {
            data.insert("allowed_mentions".into(), allowed.clone());
        }
        if self.tts {
            data.insert("tts".into(), json!(true));
        }
        if self.ephemeral {
            // MessageFlags::EPHEMERAL
            data.insert("flags".into(), json!(64u64));
        }

        data
    }
}

impl From<&str> for Reply {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<String> for Reply {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<Embed> for Reply {
    fn from(value: Embed) -> Self {
        Self::new().embed(value)
    }
}

impl From<Modal> for Reply {
    fn from(value: Modal) -> Self {
        Self::new().modal(value)
    }
}

impl From<()> for Reply {
    fn from((): ()) -> Self {
        Self::default()
    }
}

/// Result a handler can return.
pub trait IntoReply {
    /// Converts the value into a reply.
    fn into_reply(self) -> crate::Result<Reply>;
}

impl IntoReply for Reply {
    fn into_reply(self) -> crate::Result<Reply> {
        Ok(self)
    }
}

impl IntoReply for String {
    fn into_reply(self) -> crate::Result<Reply> {
        Ok(Reply::text(self))
    }
}

impl IntoReply for &str {
    fn into_reply(self) -> crate::Result<Reply> {
        Ok(Reply::text(self))
    }
}

impl IntoReply for () {
    fn into_reply(self) -> crate::Result<Reply> {
        Ok(Reply::default())
    }
}

impl<T: IntoReply> IntoReply for crate::Result<T> {
    fn into_reply(self) -> crate::Result<Reply> {
        self?.into_reply()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::channel::ChannelType;

    #[test]
    fn text_selects_serialize_their_options() {
        let menu = SelectMenu::text("poll", "Pick one")
            .option("red", "Red")
            .option_with("green", "Green", "The color of grass")
            .option_default("blue", "Blue")
            .min_values(1)
            .max_values(3)
            .disabled(true);

        let value = menu.to_value();

        assert_eq!(value["type"], 3);
        assert_eq!(value["custom_id"], "poll");
        assert_eq!(value["placeholder"], "Pick one");
        assert_eq!(value["options"][0]["value"], "red");
        assert_eq!(value["options"][1]["description"], "The color of grass");
        assert_eq!(value["options"][2]["default"], true);
        assert_eq!(value["min_values"], 1);
        assert_eq!(value["max_values"], 3);
        assert_eq!(value["disabled"], true);
    }

    #[test]
    fn user_role_and_mentionable_selects_have_no_options() {
        for (menu, kind) in [
            (SelectMenu::user("u", "Pick"), 5),
            (SelectMenu::role("r", "Pick"), 6),
            (SelectMenu::mentionable("m", "Pick"), 7),
        ] {
            let value = menu.to_value();
            assert_eq!(value["type"], kind);
            assert!(value.get("options").is_none());
        }
    }

    #[test]
    fn channel_selects_restrict_channel_types() {
        let value = SelectMenu::channel("where", "Where?")
            .channel_types([ChannelType::GuildText, ChannelType::GuildVoice])
            .to_value();

        assert_eq!(value["type"], 8);
        assert_eq!(value["channel_types"], json!([0, 2]));
    }

    #[test]
    fn action_rows_wrap_selects() {
        let row = ActionRow::new()
            .select(SelectMenu::text("poll", "Pick").options([("a", "A"), ("b", "B")]))
            .to_value();

        assert_eq!(row["type"], 1);
        assert_eq!(row["components"][0]["type"], 3);
        assert_eq!(row["components"][0]["options"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn select_kinds_match_the_api_codes() {
        assert_eq!(SelectKind::Text.code(), 3);
        assert_eq!(SelectKind::User.code(), 5);
        assert_eq!(SelectKind::Role.code(), 6);
        assert_eq!(SelectKind::Mentionable.code(), 7);
        assert_eq!(SelectKind::Channel.code(), 8);
    }
}
