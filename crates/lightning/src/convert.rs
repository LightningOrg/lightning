//! Conversion between Lightning's model and Twilight's types.

use lightning_core::command::Command;
use lightning_core::{
    AutocompleteChoice, AutocompleteHandler, ChoiceValue, Error, OptionKind, Reply, Result,
    Subcommand,
};
use serde_json::{Map, Value, json};
use twilight_model::application::command::{
    Command as TwilightCommand, CommandOption, CommandOptionChoice, CommandOptionChoiceValue,
    CommandOptionType, CommandType,
};
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::channel::message::Component;
use twilight_model::channel::message::Embed;
use twilight_model::channel::message::MessageFlags;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::Id;

/// Converts framework commands to Discord's model.
///
/// Commands with [`Subcommand`] entries publish those entries as nested
/// options. Options with an [`AutocompleteHandler`] are published with
/// `autocomplete: true`, which is what makes Discord send the interaction.
#[allow(deprecated)]
pub(crate) fn to_commands<'a>(
    commands: impl Iterator<Item = &'a Command>,
    subcommands: &[Subcommand],
    autocompletes: &[AutocompleteHandler],
) -> Vec<TwilightCommand> {
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
            options: command_options(command, subcommands, autocompletes),
            version: Id::new(1),
        })
        .collect()
}

fn command_options(
    command: &Command,
    subcommands: &[Subcommand],
    autocompletes: &[AutocompleteHandler],
) -> Vec<CommandOption> {
    let children: Vec<&Subcommand> = subcommands
        .iter()
        .filter(|subcommand| subcommand.command == command.name)
        .collect();

    if children.is_empty() {
        return command
            .options
            .iter()
            .map(|spec| {
                let autocomplete =
                    autocomplete_enabled(autocompletes, command.name, None, None, spec);
                scalar_option(spec, autocomplete)
            })
            .collect();
    }

    let mut options = Vec::new();

    for subcommand in children.iter().filter(|child| child.group.is_none()) {
        options.push(subcommand_option(command, subcommand, autocompletes));
    }

    let mut groups: Vec<(&'static str, Vec<&Subcommand>)> = Vec::new();
    for subcommand in children
        .iter()
        .filter_map(|child| child.group.map(|g| (g, *child)))
    {
        match groups.iter_mut().find(|(name, _)| *name == subcommand.0) {
            Some((_, members)) => members.push(subcommand.1),
            None => groups.push((subcommand.0, vec![subcommand.1])),
        }
    }

    for (name, members) in groups {
        let description = members
            .iter()
            .map(|member| member.group_description)
            .find(|description| !description.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| humanize(name));

        options.push(CommandOption {
            autocomplete: None,
            channel_types: None,
            choices: None,
            description,
            description_localizations: None,
            file_types: None,
            kind: CommandOptionType::SubCommandGroup,
            max_length: None,
            max_value: None,
            min_length: None,
            min_value: None,
            name: name.to_owned(),
            name_localizations: None,
            options: Some(
                members
                    .into_iter()
                    .map(|subcommand| subcommand_option(command, subcommand, autocompletes))
                    .collect(),
            ),
            required: None,
        });
    }

    options
}

fn subcommand_option(
    command: &Command,
    subcommand: &Subcommand,
    autocompletes: &[AutocompleteHandler],
) -> CommandOption {
    CommandOption {
        autocomplete: None,
        channel_types: None,
        choices: None,
        description: if subcommand.description.is_empty() {
            subcommand.name.to_owned()
        } else {
            subcommand.description.to_owned()
        },
        description_localizations: None,
        file_types: None,
        kind: CommandOptionType::SubCommand,
        max_length: None,
        max_value: None,
        min_length: None,
        min_value: None,
        name: subcommand.name.to_owned(),
        name_localizations: None,
        options: Some(
            subcommand
                .options
                .iter()
                .map(|spec| {
                    let autocomplete = autocomplete_enabled(
                        autocompletes,
                        command.name,
                        subcommand.group,
                        Some(subcommand.name),
                        spec,
                    );
                    scalar_option(spec, autocomplete)
                })
                .collect(),
        ),
        required: None,
    }
}

fn autocomplete_enabled(
    autocompletes: &[AutocompleteHandler],
    command: &str,
    group: Option<&str>,
    subcommand: Option<&str>,
    spec: &lightning_core::OptionSpec,
) -> bool {
    // Discord only accepts autocomplete on string, integer and number options.
    !matches!(spec.kind, OptionKind::Boolean)
        && autocompletes
            .iter()
            .any(|handler| handler.matches(command, group, subcommand, spec.name))
}

fn scalar_option(spec: &lightning_core::OptionSpec, autocomplete: bool) -> CommandOption {
    CommandOption {
        autocomplete: autocomplete.then_some(true),
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

fn humanize(name: &str) -> String {
    name.replace(['_', '-'], " ")
}

pub(crate) fn to_embeds(reply: &Reply) -> Result<Vec<Embed>> {
    reply
        .embeds
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|error| Error::Internal(format!("invalid embed: {error}")))
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
                .map_err(|error| Error::Internal(format!("invalid component: {error}")))
        })
        .collect()
}

/// Initial response to an interaction.
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

/// Deferred response data.
pub(crate) fn deferred(ephemeral: bool) -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::DeferredChannelMessageWithSource,
        data: Some(InteractionResponseData {
            flags: ephemeral.then_some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
    }
}

/// Response data that updates the original message (buttons).
pub(crate) fn deferred_update() -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::DeferredUpdateMessage,
        data: None,
    }
}

/// Reply to an API `Ping`.
pub(crate) fn pong() -> InteractionResponse {
    InteractionResponse {
        kind: InteractionResponseType::Pong,
        data: None,
    }
}

/// Autocomplete reply with up to 25 typed choices.
pub(crate) fn autocomplete(choices: &[AutocompleteChoice]) -> InteractionResponse {
    let choices = choices
        .iter()
        .take(25)
        .map(|choice| CommandOptionChoice {
            name: truncate(&choice.name, 100),
            name_localizations: None,
            value: match &choice.value {
                ChoiceValue::String(text) => CommandOptionChoiceValue::String(truncate(text, 100)),
                ChoiceValue::Integer(number) => CommandOptionChoiceValue::Integer(*number),
                ChoiceValue::Number(number) => CommandOptionChoiceValue::Number(*number),
            },
        })
        .collect();

    InteractionResponse {
        kind: InteractionResponseType::ApplicationCommandAutocompleteResult,
        data: Some(InteractionResponseData {
            choices: Some(choices),
            ..Default::default()
        }),
    }
}

/// Discord counts characters, not bytes, and rejects longer values.
fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The option the user is typing, with its place inside the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusedOption {
    pub group: Option<String>,
    pub subcommand: Option<String>,
    pub name: String,
    pub value: String,
}

/// Finds the option the user is typing, looking through subcommands too.
pub(crate) fn focused_option(options: &[CommandDataOption]) -> Option<FocusedOption> {
    focused_in(options, None, None)
}

fn focused_in(
    options: &[CommandDataOption],
    group: Option<&str>,
    subcommand: Option<&str>,
) -> Option<FocusedOption> {
    for option in options {
        match &option.value {
            CommandOptionValue::Focused(value, _) => {
                return Some(FocusedOption {
                    group: group.map(str::to_owned),
                    subcommand: subcommand.map(str::to_owned),
                    name: option.name.clone(),
                    value: value.clone(),
                });
            }
            CommandOptionValue::SubCommand(inner) => {
                if let Some(found) = focused_in(inner, group, Some(&option.name)) {
                    return Some(found);
                }
            }
            CommandOptionValue::SubCommandGroup(inner) => {
                if let Some(found) = focused_in(inner, Some(&option.name), subcommand) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }

    None
}

/// Subcommand selected by a slash interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubcommandPath {
    pub group: Option<String>,
    pub name: String,
}

/// Finds the subcommand or group/subcommand pair selected by a slash command.
pub(crate) fn selected_subcommand(options: &[CommandDataOption]) -> Option<SubcommandPath> {
    for option in options {
        match &option.value {
            CommandOptionValue::SubCommand(_) => {
                return Some(SubcommandPath {
                    group: None,
                    name: option.name.clone(),
                });
            }
            CommandOptionValue::SubCommandGroup(inner) => {
                let name = inner.iter().find_map(|child| {
                    matches!(child.value, CommandOptionValue::SubCommand(_))
                        .then(|| child.name.clone())
                })?;

                return Some(SubcommandPath {
                    group: Some(option.name.clone()),
                    name,
                });
            }
            _ => {}
        }
    }

    None
}

/// Flattens interaction options into a JSON map.
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

/// Extracts modal fields from their JSON.
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

/// Splits a `custom_id` into `(name, payload)`.
pub(crate) fn split_custom_id(custom_id: &str) -> (&str, Option<Map<String, Value>>) {
    match custom_id.split_once(':') {
        Some((name, rest)) => (name, serde_json::from_str(rest).ok()),
        None => (custom_id, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_core::{ActionRow, Button, Embed as FrameworkEmbed, OptionSpec, Subcommand};

    static OPTIONS: &[OptionSpec] = &[
        OptionSpec {
            name: "text",
            description: "What to repeat",
            kind: OptionKind::String,
            required: true,
        },
        OptionSpec {
            name: "times",
            description: "How many times",
            kind: OptionKind::Integer,
            required: false,
        },
    ];

    fn no_choices<'a>(
        _: &'a lightning_core::Context,
    ) -> lightning_core::BoxFuture<'a, lightning_core::Result<Vec<AutocompleteChoice>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    static AUTOCOMPLETE: AutocompleteHandler = AutocompleteHandler {
        command: "echo",
        group: None,
        subcommand: None,
        option: "text",
        run: no_choices,
    };

    #[test]
    fn commands_serialize_the_way_discord_expects() {
        let command = Command::new("echo", "Repeats a text").options(OPTIONS);
        let converted = to_commands(std::iter::once(&command), &[], &[]);
        let json = serde_json::to_value(&converted[0]).unwrap();

        assert_eq!(json["name"], "echo");
        assert_eq!(json["description"], "Repeats a text");
        assert_eq!(json["type"], 1);
        assert_eq!(json["options"][0]["type"], 3);
        assert_eq!(json["options"][0]["required"], true);
        assert_eq!(json["options"][1]["type"], 4);
        assert_eq!(json["options"][1]["required"], false);
    }

    #[test]
    fn options_with_a_handler_are_published_as_autocomplete() {
        let command = Command::new("echo", "Repeats a text").options(OPTIONS);
        let converted = to_commands(std::iter::once(&command), &[], &[AUTOCOMPLETE]);
        let json = serde_json::to_value(&converted[0]).unwrap();

        assert_eq!(json["options"][0]["autocomplete"], true);
        assert!(json["options"][1]["autocomplete"].is_null());
    }

    #[test]
    fn subcommands_and_groups_become_nested_options() {
        let command = Command::new("config", "Manage configuration").options(OPTIONS);
        let show = Subcommand::new("config", "show", "Show the configuration");
        let channel = Subcommand::new("config", "channel", "Change the channel")
            .group("set", "Change a setting")
            .options(OPTIONS);

        let converted = to_commands(std::iter::once(&command), &[show, channel], &[]);
        let json = serde_json::to_value(&converted[0]).unwrap();

        assert_eq!(json["options"][0]["type"], 1);
        assert_eq!(json["options"][0]["name"], "show");
        assert_eq!(json["options"][1]["type"], 2);
        assert_eq!(json["options"][1]["name"], "set");
        assert_eq!(json["options"][1]["description"], "Change a setting");
        assert_eq!(json["options"][1]["options"][0]["type"], 1);
        assert_eq!(json["options"][1]["options"][0]["name"], "channel");
        assert_eq!(json["options"][1]["options"][0]["options"][0]["type"], 3);
    }

    #[test]
    fn subcommand_options_can_autocomplete() {
        static NESTED: AutocompleteHandler = AutocompleteHandler {
            command: "config",
            group: Some("set"),
            subcommand: Some("channel"),
            option: "text",
            run: no_choices,
        };

        let command = Command::new("config", "Manage configuration");
        let channel = Subcommand::new("config", "channel", "Change the channel")
            .group("set", "Change a setting")
            .options(OPTIONS);

        let converted = to_commands(std::iter::once(&command), &[channel], &[NESTED]);
        let json = serde_json::to_value(&converted[0]).unwrap();

        assert_eq!(
            json["options"][0]["options"][0]["options"][0]["autocomplete"],
            true
        );
    }

    #[test]
    fn autocomplete_replies_with_typed_choices() {
        let response = autocomplete(&[
            AutocompleteChoice::text("Apple", "apple"),
            AutocompleteChoice::integer("Two", 2),
            AutocompleteChoice::number("Half", 0.5),
        ]);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["type"], 8);
        assert_eq!(json["data"]["choices"][0]["name"], "Apple");
        assert_eq!(json["data"]["choices"][0]["value"], "apple");
        assert_eq!(json["data"]["choices"][1]["value"], 2);
        assert_eq!(json["data"]["choices"][2]["value"], 0.5);
    }

    #[test]
    fn autocomplete_is_limited_to_25_choices() {
        let choices: Vec<_> = (0..30)
            .map(|index| AutocompleteChoice::text(format!("Choice {index}"), index.to_string()))
            .collect();

        let response = autocomplete(&choices);
        let data = response.data.unwrap();

        assert_eq!(data.choices.unwrap().len(), 25);
    }

    #[test]
    fn finds_the_focused_option_inside_subcommands() {
        let options = vec![CommandDataOption {
            name: "user".into(),
            value: CommandOptionValue::SubCommand(vec![CommandDataOption {
                name: "query".into(),
                value: CommandOptionValue::Focused("app".into(), CommandOptionType::String),
            }]),
        }];

        assert_eq!(
            focused_option(&options),
            Some(FocusedOption {
                group: None,
                subcommand: Some("user".to_owned()),
                name: "query".to_owned(),
                value: "app".to_owned(),
            })
        );
    }

    #[test]
    fn finds_the_focused_option_inside_a_group() {
        let options = vec![CommandDataOption {
            name: "set".into(),
            value: CommandOptionValue::SubCommandGroup(vec![CommandDataOption {
                name: "channel".into(),
                value: CommandOptionValue::SubCommand(vec![CommandDataOption {
                    name: "query".into(),
                    value: CommandOptionValue::Focused("gen".into(), CommandOptionType::String),
                }]),
            }]),
        }];

        assert_eq!(
            focused_option(&options),
            Some(FocusedOption {
                group: Some("set".to_owned()),
                subcommand: Some("channel".to_owned()),
                name: "query".to_owned(),
                value: "gen".to_owned(),
            })
        );
    }

    #[test]
    fn reads_the_selected_subcommand_and_group() {
        let direct = vec![CommandDataOption {
            name: "show".into(),
            value: CommandOptionValue::SubCommand(Vec::new()),
        }];
        assert_eq!(
            selected_subcommand(&direct),
            Some(SubcommandPath {
                group: None,
                name: "show".to_owned(),
            })
        );

        let nested = vec![CommandDataOption {
            name: "set".into(),
            value: CommandOptionValue::SubCommandGroup(vec![CommandDataOption {
                name: "channel".into(),
                value: CommandOptionValue::SubCommand(Vec::new()),
            }]),
        }];
        assert_eq!(
            selected_subcommand(&nested),
            Some(SubcommandPath {
                group: Some("set".to_owned()),
                name: "channel".to_owned(),
            })
        );
    }

    #[test]
    fn embeds_convert_to_the_model() {
        let reply = Reply::text("hello").embed(
            FrameworkEmbed::new()
                .title("Title")
                .description("Body")
                .color(0xF5A623),
        );

        let embeds = to_embeds(&reply).expect("the embed should be valid");

        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0].title.as_deref(), Some("Title"));
        assert_eq!(embeds[0].color, Some(0xF5A623));
    }

    #[test]
    fn buttons_convert_to_the_model() {
        let reply = Reply::text("are you sure?").components(vec![
            ActionRow::new()
                .button(Button::success("confirm:yes", "Yes"))
                .button(Button::danger("confirm:no", "No")),
        ]);

        let components = to_components(&reply).expect("the buttons should be valid");

        assert_eq!(components.len(), 1);
    }

    #[test]
    fn modals_convert_to_the_model() {
        let reply = Reply::new().modal(lightning_core::Modal::new("form", "Title").text_input(
            "name",
            "Name",
            "Your name",
        ));

        let response =
            interaction_response(InteractionResponseType::ChannelMessageWithSource, &reply)
                .expect("the modal should be valid");
        let data = response.data.expect("the modal carries data");

        assert_eq!(response.kind, InteractionResponseType::Modal);
        assert_eq!(data.custom_id.as_deref(), Some("form"));
        assert_eq!(data.title.as_deref(), Some("Title"));
        assert_eq!(data.components.unwrap().len(), 1);
    }

    #[test]
    fn splits_custom_ids() {
        let (id, payload) = split_custom_id(r#"confirm:{"choice":"yes"}"#);

        assert_eq!(id, "confirm");
        assert_eq!(payload.unwrap()["choice"], "yes");

        let (id, payload) = split_custom_id("cancel");
        assert_eq!(id, "cancel");
        assert!(payload.is_none());
    }
}
