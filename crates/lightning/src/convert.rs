//! Conversión entre el modelo de Lightning y los tipos de Twilight.

use lightning_core::command::Command;
use lightning_core::{Error, OptionKind, Reply, Result};
use serde_json::{Map, Value, json};
use twilight_model::application::command::{
    Command as TwilightCommand, CommandOption, CommandOptionType, CommandType,
};
use twilight_model::application::interaction::application_command::CommandDataOption;
use twilight_model::channel::message::Component;
use twilight_model::channel::message::Embed;
use twilight_model::channel::message::MessageFlags;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::Id;

/// Convierte los comandos del framework al modelo de Discord.
#[allow(deprecated)]
pub(crate) fn to_commands<'a>(commands: impl Iterator<Item = &'a Command>) -> Vec<TwilightCommand> {
    commands
        .filter(|command| command.kind.allows(true))
        .map(|command| TwilightCommand {
            application_id: None,
            contexts: None,
            default_member_permissions: command
                .default_permissions
                .and_then(Permissions::from_bits),
            description: if command.description.is_empty() {
                command.name.to_owned()
            } else {
                command.description.to_owned()
            },
            description_localizations: None,
            dm_permission: None,
            guild_id: None,
            id: None,
            integration_types: None,
            kind: CommandType::ChatInput,
            name: command.name.to_owned(),
            name_localizations: None,
            nsfw: command.nsfw.then_some(true),
            options: command.options.iter().map(to_option).collect(),
            version: Id::new(1),
        })
        .collect()
}

fn to_option(spec: &lightning_core::OptionSpec) -> CommandOption {
    CommandOption {
        autocomplete: None,
        channel_types: None,
        choices: None,
        description: spec.description.to_owned(),
        description_localizations: None,
        file_types: None,
        kind: match spec.kind {
            OptionKind::String => CommandOptionType::String,
            OptionKind::Integer => CommandOptionType::Integer,
            OptionKind::Number => CommandOptionType::Number,
            OptionKind::Boolean => CommandOptionType::Boolean,
        },
        max_length: None,
        max_value: None,
        min_length: None,
        min_value: None,
        name: spec.name.to_owned(),
        name_localizations: None,
        options: None,
        required: Some(spec.required),
    }
}

pub(crate) fn to_embeds(reply: &Reply) -> Result<Vec<Embed>> {
    reply
        .embeds
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|error| Error::Internal(format!("embed inválido: {error}")))
        })
        .collect()
}

pub(crate) fn to_components(reply: &Reply) -> Result<Vec<Component>> {
    reply
        .components
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|error| Error::Internal(format!("componente inválido: {error}")))
        })
        .collect()
}

/// Respuesta inicial de una interacción.
pub(crate) fn interaction_response(
    kind: InteractionResponseType,
    reply: &Reply,
) -> Result<InteractionResponse> {
    if let Some(modal) = &reply.modal {
        return Ok(InteractionResponse {
            kind: InteractionResponseType::Modal,
            data: Some(InteractionResponseData {
                custom_id: Some(modal.custom_id().to_owned()),
                title: Some(modal.title().to_owned()),
                components: Some(
                    serde_json::from_value(modal.components())
                        .map_err(|error| Error::Internal(error.to_string()))?,
                ),
                ..Default::default()
            }),
        });
    }

    let embeds = to_embeds(reply)?;
    let components = to_components(reply)?;

    Ok(InteractionResponse {
        kind,
        data: Some(InteractionResponseData {
            allowed_mentions: reply
                .allowed_mentions
                .clone()
                .map(|value| {
                    serde_json::from_value(value)
                        .map_err(|error| Error::Internal(error.to_string()))
                })
                .transpose()?,
            components: (!components.is_empty()).then_some(components),
            content: reply.content.clone(),
            embeds: (!embeds.is_empty()).then_some(embeds),
            flags: reply.ephemeral.then_some(MessageFlags::EPHEMERAL),
            tts: reply.tts.then_some(true),
            ..Default::default()
        }),
    })
}

/// Datos de una respuesta diferida.
pub(crate) fn deferred(ephemeral: bool) -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::DeferredChannelMessageWithSource,
        data: Some(InteractionResponseData {
            flags: ephemeral.then_some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
    }
}

/// Datos de una respuesta que actualiza el mensaje original (botones).
pub(crate) fn deferred_update() -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::DeferredUpdateMessage,
        data: None,
    }
}

/// Respuesta a un `Ping` de la API.
pub(crate) fn pong() -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::Pong,
        data: None,
    }
}

/// Respuesta vacía a un autocompletado (todavía no soportado).
pub(crate) fn empty_autocomplete() -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::ApplicationCommandAutocompleteResult,
        data: Some(InteractionResponseData {
            choices: Some(Vec::new()),
            ..Default::default()
        }),
    }
}

/// Convierte las opciones de una interacción en un mapa JSON plano.
pub(crate) fn option_map(options: &[CommandDataOption]) -> Map<String, Value> {
    let mut map = Map::new();

    for option in options {
        match &option.value {
            twilight_model::application::interaction::application_command::CommandOptionValue::SubCommand(
                inner,
            )
            | twilight_model::application::interaction::application_command::CommandOptionValue::SubCommandGroup(
                inner,
            ) => {
                map.extend(option_map(inner));
            }
            value => {
                map.insert(option.name.clone(), option_value(value));
            }
        }
    }

    map
}

fn option_value(
    value: &twilight_model::application::interaction::application_command::CommandOptionValue,
) -> Value {
    use twilight_model::application::interaction::application_command::CommandOptionValue as V;

    match value {
        V::Attachment(id) => json!(id.get()),
        V::Channel(id) => json!(id.get()),
        V::Mentionable(id) => json!(id.get()),
        V::Role(id) => json!(id.get()),
        V::User(id) => json!(id.get()),
        V::Boolean(flag) => json!(flag),
        V::Focused(text, _) => json!(text),
        V::Integer(number) => json!(number),
        V::Number(number) => json!(number),
        V::String(text) => json!(text),
        V::SubCommand(_) | V::SubCommandGroup(_) => Value::Null,
    }
}

/// Extrae los campos de un modal a partir de su JSON.
pub(crate) fn modal_map(data: &serde_json::Value) -> Map<String, Value> {
    let mut map = Map::new();
    collect_inputs(data, &mut map);
    map
}

fn collect_inputs(value: &Value, map: &mut Map<String, Value>) {
    match value {
        Value::Object(object) => {
            if let (Some(id), Some(input)) = (
                object.get("custom_id").and_then(Value::as_str),
                object.get("value"),
            ) {
                map.insert(id.to_owned(), input.clone());
            }

            for nested in object.values() {
                collect_inputs(nested, map);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_inputs(item, map);
            }
        }
        _ => {}
    }
}

/// Divide un `custom_id` en `(nombre, payload)`.
pub(crate) fn split_custom_id(custom_id: &str) -> (&str, Option<Map<String, Value>>) {
    match custom_id.split_once(':') {
        Some((name, rest)) => (name, serde_json::from_str(rest).ok()),
        None => (custom_id, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_core::{ActionRow, Button, Embed as FrameworkEmbed, OptionSpec};

    static OPTIONS: &[OptionSpec] = &[
        OptionSpec {
            name: "texto",
            description: "Qué repetir",
            kind: OptionKind::String,
            required: true,
        },
        OptionSpec {
            name: "veces",
            description: "Cuántas veces",
            kind: OptionKind::Integer,
            required: false,
        },
    ];

    #[test]
    fn los_comandos_se_serializan_como_los_espera_discord() {
        let command = Command::new("eco", "Repite un texto").options(OPTIONS);
        let converted = to_commands(std::iter::once(&command));
        let json = serde_json::to_value(&converted[0]).unwrap();

        assert_eq!(json["name"], "eco");
        assert_eq!(json["description"], "Repite un texto");
        assert_eq!(json["type"], 1);
        assert_eq!(json["options"][0]["type"], 3);
        assert_eq!(json["options"][0]["required"], true);
        assert_eq!(json["options"][1]["type"], 4);
        assert_eq!(json["options"][1]["required"], false);
    }

    #[test]
    fn los_embeds_se_convierten_al_modelo() {
        let reply = Reply::text("hola").embed(
            FrameworkEmbed::new()
                .title("Título")
                .description("Cuerpo")
                .color(0xF5A623),
        );

        let embeds = to_embeds(&reply).expect("el embed debería ser válido");

        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0].title.as_deref(), Some("Título"));
        assert_eq!(embeds[0].color, Some(0xF5A623));
    }

    #[test]
    fn los_botones_se_convierten_al_modelo() {
        let reply = Reply::text("¿seguro?").components(vec![
            ActionRow::new()
                .button(Button::success("confirm:yes", "Sí"))
                .button(Button::danger("confirm:no", "No")),
        ]);

        let components = to_components(&reply).expect("los botones deberían ser válidos");

        assert_eq!(components.len(), 1);
    }

    #[test]
    fn los_modales_se_convierten() {
        let reply = Reply::new().modal(lightning_core::Modal::new("form", "Título").text_input(
            "nombre",
            "Nombre",
            "Tu nombre",
        ));

        let response =
            interaction_response(InteractionResponseType::ChannelMessageWithSource, &reply)
                .expect("el modal debería ser válido");
        let data = response.data.expect("el modal lleva data");

        assert_eq!(response.kind, InteractionResponseType::Modal);
        assert_eq!(data.custom_id.as_deref(), Some("form"));
        assert_eq!(data.title.as_deref(), Some("Título"));
        assert_eq!(data.components.unwrap().len(), 1);
    }

    #[test]
    fn divide_custom_ids() {
        let (id, payload) = split_custom_id(r#"confirm:{"choice":"yes"}"#);

        assert_eq!(id, "confirm");
        assert_eq!(payload.unwrap()["choice"], "yes");

        let (id, payload) = split_custom_id("cancelar");
        assert_eq!(id, "cancelar");
        assert!(payload.is_none());
    }
}
