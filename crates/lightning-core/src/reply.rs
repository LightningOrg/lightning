//! Construcción de respuestas: mensajes, embeds, botones y modales.
//!
//! `Reply` es el tipo que devuelven los handlers. Es deliberadamente *builder*:
//!
//! ```
//! use lightning_core::{Button, Embed, Reply};
//!
//! let reply = Reply::text("¿Seguro?")
//!     .embed(Embed::new().title("Confirmación").color(0xF5A623))
//!     .components(vec![
//!         lightning_core::ActionRow::new()
//!             .button(Button::danger("confirm:no", "Cancelar"))
//!             .button(Button::success("confirm:yes", "Vamos")),
//!     ]);
//!
//! assert_eq!(reply.embeds.len(), 1);
//! ```

use serde_json::{Map, Value, json};

/// Estilo visual de un botón, según la API de Discord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Azul (`blurple`).
    Primary,
    /// Gris.
    Secondary,
    /// Verde.
    Success,
    /// Rojo.
    Danger,
    /// Enlace externo (no envía interacción).
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

/// Un botón dentro de una [`ActionRow`].
#[derive(Debug, Clone)]
pub struct Button {
    value: Map<String, Value>,
}

impl Button {
    /// Crea un botón con estilo y `custom_id`.
    pub fn new(style: ButtonStyle, custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        let mut value = Map::new();
        value.insert("type".into(), json!(2));
        value.insert("style".into(), json!(style.code()));
        value.insert("label".into(), json!(label.into()));
        value.insert("custom_id".into(), json!(custom_id.into()));

        Self { value }
    }

    /// Botón azul.
    pub fn primary(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Primary, custom_id, label)
    }

    /// Botón gris.
    pub fn secondary(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Secondary, custom_id, label)
    }

    /// Botón verde.
    pub fn success(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Success, custom_id, label)
    }

    /// Botón rojo.
    pub fn danger(custom_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(ButtonStyle::Danger, custom_id, label)
    }

    /// Botón de enlace: abre una URL en el navegador.
    pub fn link(url: impl Into<String>, label: impl Into<String>) -> Self {
        let mut button = Self::new(ButtonStyle::Link, String::new(), label);
        button.value.remove("custom_id");
        button.value.insert("url".into(), json!(url.into()));
        button
    }

    /// Deshabilita el botón.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.value.insert("disabled".into(), json!(disabled));
        self
    }

    /// Añade un emoji (unicode o `nombre:id`).
    pub fn emoji(mut self, emoji: impl Into<String>) -> Self {
        self.value
            .insert("emoji".into(), json!({ "name": emoji.into() }));
        self
    }

    /// Representación JSON lista para la API.
    pub fn to_value(self) -> Value {
        Value::Object(self.value)
    }
}

/// Fila de componentes (botones o inputs).
#[derive(Debug, Clone, Default)]
pub struct ActionRow {
    components: Vec<Value>,
}

impl ActionRow {
    /// Fila vacía.
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade cualquier componente que sepa serializarse.
    pub fn component(mut self, component: impl Into<Value>) -> Self {
        self.components.push(component.into());
        self
    }

    /// Añade un botón.
    pub fn button(self, button: Button) -> Self {
        self.component(button.to_value())
    }

    /// Convierte la fila en JSON.
    pub fn to_value(self) -> Value {
        json!({ "type": 1, "components": self.components })
    }
}

/// Embed de Discord con una API fluida.
#[derive(Debug, Clone, Default)]
pub struct Embed {
    value: Map<String, Value>,
}

impl Embed {
    /// Embed vacío (Discord exige al menos un campo para mostrarlo).
    pub fn new() -> Self {
        Self::default()
    }

    /// Título del embed.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.value.insert("title".into(), json!(title.into()));
        self
    }

    /// Descripción (markdown permitido).
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.value
            .insert("description".into(), json!(description.into()));
        self
    }

    /// Color en formato `0xRRGGBB`.
    pub fn color(mut self, color: u32) -> Self {
        self.value.insert("color".into(), json!(color));
        self
    }

    /// URL del título.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.value.insert("url".into(), json!(url.into()));
        self
    }

    /// Añade un campo.
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
            .expect("fields siempre es un array")
            .push(field);

        self
    }

    /// Pie del embed.
    pub fn footer(mut self, text: impl Into<String>) -> Self {
        self.value
            .insert("footer".into(), json!({ "text": text.into() }));
        self
    }

    /// Autor del embed.
    pub fn author(mut self, name: impl Into<String>) -> Self {
        self.value
            .insert("author".into(), json!({ "name": name.into() }));
        self
    }

    /// Imagen principal.
    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.value
            .insert("image".into(), json!({ "url": url.into() }));
        self
    }

    /// Miniatura (esquina superior derecha).
    pub fn thumbnail(mut self, url: impl Into<String>) -> Self {
        self.value
            .insert("thumbnail".into(), json!({ "url": url.into() }));
        self
    }

    /// Timestamp ISO-8601.
    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.value
            .insert("timestamp".into(), json!(timestamp.into()));
        self
    }

    /// Representación JSON lista para la API.
    pub fn to_value(self) -> Value {
        let mut value = self.value;
        value
            .entry("type")
            .or_insert_with(|| Value::String("rich".into()));
        Value::Object(value)
    }
}

/// Modal (formulario emergente) que se responde a una interacción.
#[derive(Debug, Clone)]
pub struct Modal {
    custom_id: String,
    title: String,
    components: Vec<Value>,
}

impl Modal {
    /// Crea un modal con su `custom_id` y título.
    pub fn new(custom_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            custom_id: custom_id.into(),
            title: title.into(),
            components: Vec::new(),
        }
    }

    /// Añade un campo de texto obligatorio.
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

    /// `custom_id` del modal.
    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }

    /// Título del modal.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// JSON con `custom_id`, `title` y `components`.
    pub fn to_value(&self) -> Value {
        json!({
            "custom_id": self.custom_id,
            "title": self.title,
            "components": self.components,
        })
    }

    /// Solo las filas de componentes (lo que espera `InteractionResponseData`).
    pub fn components(&self) -> Value {
        Value::Array(self.components.clone())
    }
}

/// Respuesta de un handler.
///
/// Un `Reply` vacío significa "no envíes nada" (útil después de un `defer`).
#[derive(Debug, Clone, Default)]
pub struct Reply {
    /// Texto del mensaje.
    pub content: Option<String>,
    /// Embeds ya serializados.
    pub embeds: Vec<Value>,
    /// Filas de componentes ya serializadas.
    pub components: Vec<Value>,
    /// Menciones permitidas (JSON de `AllowedMentions`).
    pub allowed_mentions: Option<Value>,
    /// Si la respuesta solo la ve quien invocó el comando.
    pub ephemeral: bool,
    /// Si el mensaje se lee por TTS.
    pub tts: bool,
    /// Si está presente, se responde con un modal (solo interacciones).
    pub modal: Option<Modal>,
}

impl Reply {
    /// Respuesta vacía.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mensaje de texto.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            ..Self::default()
        }
    }

    /// Añade un embed.
    pub fn embed(mut self, embed: Embed) -> Self {
        self.embeds.push(embed.to_value());
        self
    }

    /// Añade varias filas de componentes.
    pub fn components(mut self, rows: Vec<ActionRow>) -> Self {
        self.components
            .extend(rows.into_iter().map(ActionRow::to_value));
        self
    }

    /// Añade un JSON de componente en crudo (escape hatch).
    pub fn raw_component(mut self, component: Value) -> Self {
        self.components.push(component);
        self
    }

    /// Marca la respuesta como efímera.
    pub fn ephemeral(mut self) -> Self {
        self.ephemeral = true;
        self
    }

    /// Menciones permitidas en crudo (JSON).
    pub fn allowed_mentions(mut self, allowed: Value) -> Self {
        self.allowed_mentions = Some(allowed);
        self
    }

    /// No permitir ninguna mención.
    pub fn no_mentions(mut self) -> Self {
        self.allowed_mentions = Some(json!({ "parse": [] }));
        self
    }

    /// Responde con un modal.
    pub fn modal(mut self, modal: Modal) -> Self {
        self.modal = Some(modal);
        self
    }

    /// `true` si no hay nada que enviar.
    pub fn is_empty(&self) -> bool {
        self.content.is_none()
            && self.embeds.is_empty()
            && self.components.is_empty()
            && self.modal.is_none()
    }

    /// Construye el objeto `data` que espera la API.
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

/// Resultado que puede devolver un handler.
pub trait IntoReply {
    /// Convierte el valor en una respuesta.
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
