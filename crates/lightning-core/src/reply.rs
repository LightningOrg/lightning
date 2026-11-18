//! Building replies: messages, embeds, buttons and modals.
//!
//! `Reply` is what handlers return. It is deliberately *builder*-shaped:
//!
//! ```
//! use lightning_core::{Button, Embed, Reply};
//!
//! let reply = Reply::text("Are you sure?")
//!     .embed(Embed::new().title("Confirmation").color(0xF5A623))
//!     .components(vec![
//!         lightning_core::ActionRow::new()
//!             .button(Button::danger("confirm:no", "Cancel"))
//!             .button(Button::success("confirm:yes", "Let's go")),
//!     ]);
//!
//! assert_eq!(reply.embeds.len(), 1);
//! ```

use serde_json::{Map, Value, json};

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

/// Row of components (buttons or inputs).
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
