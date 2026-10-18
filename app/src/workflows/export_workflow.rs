use std::result::Result;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_yaml::Value;
use warpui::{AppContext, SingletonEntity};

use super::workflow::{Argument, ArgumentType, Workflow};
use super::workflow_enum::EnumVariants;
use crate::cloud_object::model::persistence::CloudModel;

/// Separate structure for exporting arguments. This new structure holds explicit enum information,
/// unlike the `Argument` struct which just holds the enum_id. It is also flatter than the normal `Argument`
/// struct, making it easier to use serde's built-in serialize and deserialize methods.
#[derive(Serialize, Deserialize, Debug)]
struct ExportArgument {
    pub name: String,
    #[serde(flatten, deserialize_with = "deserialize_arg_type")]
    pub arg_type: ExportArgumentType,
    pub description: Option<String>,
    pub default_value: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
#[serde(tag = "arg_type")]
enum ExportArgumentType {
    #[default]
    Text,
    Enum {
        enum_name: String,

        // This field is Some() for static enums
        #[serde(skip_serializing_if = "Option::is_none")]
        enum_variants: Option<Vec<String>>,

        // This field is Some() for dynamic enums
        #[serde(skip_serializing_if = "Option::is_none")]
        enum_command: Option<String>,
    },
}

impl ExportArgument {
    /// Create a new ExportArgument given an Argument
    fn new(argument: &Argument, app: &AppContext) -> Self {
        let arg_type = match argument.arg_type {
            ArgumentType::Text => ExportArgumentType::Text,
            ArgumentType::Enum { enum_id } => CloudModel::as_ref(app)
                .get_workflow_enum(&enum_id)
                .map(|workflow_enum| {
                    let model = &workflow_enum.model().string_model;
                    let enum_name = model.name.clone();

                    let mut enum_variants = None;
                    let mut enum_command = None;

                    match &model.variants {
                        EnumVariants::Static(variants) => enum_variants = Some(variants.clone()),
                        EnumVariants::Dynamic(command) => enum_command = Some(command.clone()),
                    };
                    ExportArgumentType::Enum {
                        enum_name,
                        enum_variants,
                        enum_command,
                    }
                })
                .unwrap_or(ExportArgumentType::Text),
        };

        ExportArgument {
            name: argument.name.clone(),
            arg_type,
            description: argument.description.clone(),
            default_value: argument.default_value.clone(),
        }
    }
}

/// Custom serialize function used to export a workflow to YAML
pub fn export_serialize<S>(
    workflow: &Workflow,
    serializer: S,
    app: &AppContext,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut state = serializer.serialize_struct("Workflow", 9)?;

    let export_args: Vec<ExportArgument> = workflow
        .arguments()
        .iter()
        .map(|arg| ExportArgument::new(arg, app))
        .collect();
    match workflow {
        Workflow::Command {
            name,
            description,
            command,
            tags,
            source_url,
            author,
            author_url,
            shells,
            ..
        } => {
            if let Some(description) = description {
                state.serialize_field("description", description)?;
            }
            state.serialize_field("name", name)?;
            state.serialize_field("command", command)?;
            state.serialize_field("description", description)?;
            state.serialize_field("arguments", &export_args)?;
            state.serialize_field("tags", tags)?;
            state.serialize_field("source_url", source_url)?;
            state.serialize_field("author", author)?;
            state.serialize_field("author_url", author_url)?;
            state.serialize_field("shells", shells)?;
        }
        Workflow::AgentMode {
            name,
            description,
            query,
            ..
        } => {
            state.serialize_field("type", "agent_mode")?;
            state.serialize_field("name", name)?;
            state.serialize_field("query", query)?;
            state.serialize_field("description", description)?;
            state.serialize_field("arguments", &export_args)?;
        }
    }

    state.end()
}

/// Custom deserialization for argument types, used to both `flatten` the argument type
/// and allow for the specification of `default` behavior.
///
/// We need to specify default behavior to remain compatible with old workflow formats.
///
/// Necessary because serde currently does not support the use of `flatten` with a `default`,
/// related GitHub issue here: https://github.com/serde-rs/serde/issues/1626
fn deserialize_arg_type<'de, D>(deserializer: D) -> Result<ExportArgumentType, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Value = Deserialize::deserialize(deserializer)?;

    let arg_type = match value.get("arg_type").and_then(|value| value.as_str()) {
        Some("Text") => ExportArgumentType::Text,
        Some("Enum") => {
            let enum_name = value
                .get("enum_name")
                .and_then(|s| s.as_str().map(|s| s.to_string()));

            let enum_variants = value
                .get("enum_variants")
                .and_then(|v| v.as_sequence())
                .and_then(|seq| {
                    seq.iter()
                        .map(|s| s.as_str().map(|s| s.to_string()))
                        .collect()
                });
            let enum_command = value
                .get("enum_command")
                .and_then(|v| v.as_str().map(|s| s.to_string()));

            // If we don't have an enum name, default to a text argument
            match enum_name {
                Some(enum_name) => ExportArgumentType::Enum {
                    enum_name,
                    enum_variants,
                    enum_command,
                },
                None => ExportArgumentType::default(),
            }
        }
        _ => ExportArgumentType::default(),
    };

    Ok(arg_type)
}
