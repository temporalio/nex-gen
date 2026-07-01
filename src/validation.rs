use std::collections::BTreeMap;

use prost_types::FieldDescriptorProto;
use prost_types::field_descriptor_proto::{Label, Type};

use crate::descriptors::{DescriptorIndex, MessageMetadata};
use crate::error::{Error, Result};
use crate::language::Language;
use crate::python;
use crate::spec::{
    ApiSpec, AuthoredFieldTypeSpec, GeneratedModelSpec, LanguageStringSpec, TypeOverrideSpec,
};
use crate::typescript;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct MessageUsage {
    input: bool,
    output: bool,
}

pub(crate) fn validate_type_overrides(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    language: Language,
) -> Result<()> {
    let usages = language_message_usages(spec, descriptors, language)?;
    for (type_name, type_override) in &spec.types {
        if let Some(message) = descriptors.message(type_name) {
            validate_message_type_override(
                type_name,
                type_override,
                message,
                descriptors,
                usages.get(type_name).copied().unwrap_or_default(),
                language,
            )?;
        } else if descriptors.enumeration(type_name).is_some() {
            validate_enum_type_override(type_name, type_override)?;
        } else if descriptors.file_count() == 0 {
            continue;
        } else {
            return Err(Error::UnknownTypeOverride {
                type_name: type_name.clone(),
            });
        }
    }

    Ok(())
}

fn language_message_usages(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    _language: Language,
) -> Result<BTreeMap<String, MessageUsage>> {
    let mut usages: BTreeMap<String, MessageUsage> = BTreeMap::new();

    for service in &spec.services {
        for operation in &service.operations {
            let Some(input_proto) = operation.input_proto() else {
                continue;
            };
            let input_message = descriptors.message(input_proto).ok_or_else(|| {
                Error::UnknownOperationInputProto {
                    service: service.name.clone(),
                    operation: operation.name.clone(),
                    type_name: input_proto.to_string(),
                }
            })?;
            usages
                .entry(input_message.full_name.clone())
                .or_default()
                .input = true;

            if operation.output_transform().is_some() || operation.output_resource().is_some() {
                continue;
            }

            let Some(output_proto) = operation.output_proto() else {
                continue;
            };
            let output_message = descriptors.message(output_proto).ok_or_else(|| {
                Error::UnknownOperationOutputProto {
                    service: service.name.clone(),
                    operation: operation.name.clone(),
                    type_name: output_proto.to_string(),
                }
            })?;
            usages
                .entry(output_message.full_name.clone())
                .or_default()
                .output = true;
        }
    }

    Ok(usages)
}

fn validate_message_type_override(
    message_name: &str,
    type_override: &TypeOverrideSpec,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
    usage: MessageUsage,
    language: Language,
) -> Result<()> {
    for field_name in &type_override.required_fields {
        validate_model_required_field(message_name, field_name, message, descriptors)?;
    }
    for field_name in &type_override.omitted_fields {
        validate_model_override_field(message_name, field_name, message)?;
    }
    if let Some(generated_model) = type_override.generated_model() {
        for field_name in &generated_model.declared_fields {
            validate_model_override_field(message_name, field_name, message)?;
        }
        validate_generated_model_fields(
            message_name,
            type_override,
            &generated_model.field_names,
            &generated_model.field_annotations,
            &generated_model.field_flattened_annotations,
            &generated_model.field_sources,
            message,
            usage,
            language,
        )?;
        validate_authored_field_types(
            message_name,
            type_override,
            generated_model,
            message,
            descriptors,
        )?;
        validate_invocation_fields(
            message_name,
            type_override,
            generated_model,
            message,
            descriptors,
            usage,
        )?;
    }
    for field_name in type_override
        .required_fields
        .intersection(&type_override.omitted_fields)
    {
        return Err(Error::ConflictingTypeOverrideField {
            message: message_name.to_string(),
            field: (*field_name).clone(),
        });
    }

    Ok(())
}

fn validate_generated_model_fields(
    message_name: &str,
    type_override: &TypeOverrideSpec,
    field_names: &BTreeMap<String, String>,
    field_annotations: &BTreeMap<String, LanguageStringSpec>,
    field_flattened_annotations: &BTreeMap<String, LanguageStringSpec>,
    field_sources: &BTreeMap<String, String>,
    message: &MessageMetadata,
    usage: MessageUsage,
    language: Language,
) -> Result<()> {
    for field_name in field_names.keys() {
        validate_model_override_field(message_name, field_name, message)?;
        if type_override.omitted_fields.contains(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
    }

    for field_name in field_annotations.keys() {
        validate_model_override_field(message_name, field_name, message)?;
        if type_override.omitted_fields.contains(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
    }

    for field_name in field_flattened_annotations.keys() {
        validate_model_override_field(message_name, field_name, message)?;
        if type_override.omitted_fields.contains(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
        if !type_override.flatten_in_api() {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
                property: "flattenedType",
                reason:
                    "flattened-type is only supported on records marked `@nexus.flatten-in-api`"
                        .to_string(),
            });
        }
    }

    for field_name in field_sources.keys() {
        validate_model_override_field(message_name, field_name, message)?;
        if type_override.omitted_fields.contains(field_name) {
            return Err(Error::ConflictingTypeOverrideFieldProperties {
                message: message_name.to_string(),
                field: field_name.to_string(),
                property: "source",
                conflicting_property: "omit",
            });
        }
        if usage.output {
            return Err(Error::UnsupportedSourcedTypeField {
                message: message_name.to_string(),
                field: field_name.to_string(),
                reason: "sourced fields are only supported on input-only generated models"
                    .to_string(),
            });
        }
    }

    let mut seen_generated_names: BTreeMap<String, String> = BTreeMap::new();
    for field in &message.descriptor.field {
        let proto_name = field
            .name
            .as_deref()
            .expect("descriptor fields should be named");
        if !type_override.is_field_omitted(proto_name) && !field_names.contains_key(proto_name) {
            return Err(Error::UndeclaredTypeOverrideField {
                message: message_name.to_string(),
                field: proto_name.to_string(),
            });
        }
        if type_override.is_field_hidden(proto_name) {
            continue;
        }

        let generated_name = field_name_for_language(
            language,
            field,
            field_names.get(proto_name).map(String::as_str),
        );
        if let Some(existing) =
            seen_generated_names.insert(generated_name.clone(), proto_name.to_string())
        {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: proto_name.to_string(),
                property: "name",
                reason: format!(
                    "generated field name `{generated_name}` conflicts with field `{existing}`"
                ),
            });
        }
    }

    Ok(())
}

fn validate_authored_field_types(
    message_name: &str,
    type_override: &TypeOverrideSpec,
    generated_model: &GeneratedModelSpec,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
) -> Result<()> {
    for (field_name, authored_type) in &generated_model.field_wit_types {
        if generated_model.function(field_name).is_some() {
            continue;
        }

        let field = validate_model_override_field(message_name, field_name, message)?;
        if type_override.omitted_fields.contains(field_name) {
            continue;
        }

        validate_authored_field_type(message_name, field_name, authored_type, field, descriptors)?;
    }

    Ok(())
}

fn validate_invocation_fields(
    message_name: &str,
    type_override: &TypeOverrideSpec,
    generated_model: &GeneratedModelSpec,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
    usage: MessageUsage,
) -> Result<()> {
    if generated_model.functions.is_empty() {
        return Ok(());
    }

    if usage.output {
        if let Some(field) = generated_model.functions.keys().next() {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: field.clone(),
                property: "function",
                reason: "function fields are only supported on input-only generated models"
                    .to_string(),
            });
        }
    }

    let mut primary_field_name: Option<&str> = None;
    let mut seen_args_fields = BTreeMap::new();

    for (field_name, function) in &generated_model.functions {
        if function.primary {
            if let Some(existing) = primary_field_name {
                return Err(Error::InvalidTypeOverrideField {
                    message: message_name.to_string(),
                    field: field_name.clone(),
                    property: "function",
                    reason: format!(
                        "only one primary function field is supported; `{existing}` is already primary"
                    ),
                });
            }
            primary_field_name = Some(field_name);
        }

        if let Some((existing, _)) =
            seen_args_fields.insert(function.args_field.as_str(), (field_name, "function"))
        {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.clone(),
                property: "function",
                reason: format!(
                    "argsField `{}` is already used by function field `{existing}`",
                    function.args_field
                ),
            });
        }
    }

    for (field_name, function) in &generated_model.functions {
        let callable_field = validate_model_override_field(message_name, field_name, message)?;
        if type_override.omitted_fields.contains(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.clone(),
            });
        }
        if function.args_field == *field_name {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.clone(),
                property: "function",
                reason: "argsField must point to a different field".to_string(),
            });
        }

        validate_named_invocation_field(
            message_name,
            field_name,
            callable_field,
            descriptors,
            "function",
        )?;

        let args_field =
            validate_model_override_field(message_name, &function.args_field, message)?;
        if type_override.omitted_fields.contains(&function.args_field) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: function.args_field.clone(),
            });
        }
        if type_override.required_fields.contains(&function.args_field) {
            return Err(Error::ConflictingTypeOverrideFieldProperties {
                message: message_name.to_string(),
                field: function.args_field.clone(),
                property: "function",
                conflicting_property: "required",
            });
        }
        if generated_model
            .field_sources
            .contains_key(&function.args_field)
        {
            return Err(Error::ConflictingTypeOverrideFieldProperties {
                message: message_name.to_string(),
                field: function.args_field.clone(),
                property: "function",
                conflicting_property: "source",
            });
        }
        validate_invocation_args_field(
            message_name,
            &function.args_field,
            args_field,
            descriptors,
            "function",
        )?;
    }

    Ok(())
}

fn validate_named_invocation_field(
    message_name: &str,
    field_name: &str,
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
    property: &'static str,
) -> Result<()> {
    match field_type(field) {
        Some(Type::String) => Ok(()),
        Some(Type::Message) => {
            let Some(type_name) = field.type_name.as_deref() else {
                return Err(Error::InvalidTypeOverrideField {
                    message: message_name.to_string(),
                    field: field_name.to_string(),
                    property,
                    reason: "field message type is missing a descriptor name".to_string(),
                });
            };
            let Some(message) = descriptors.message(type_name.trim_start_matches('.')) else {
                return Err(Error::InvalidTypeOverrideField {
                    message: message_name.to_string(),
                    field: field_name.to_string(),
                    property,
                    reason: "field message type is not available in the descriptors".to_string(),
                });
            };
            let has_name_field = message.descriptor.field.iter().any(|field| {
                field.name.as_deref() == Some("name")
                    && field_label(field) != Some(Label::Repeated)
                    && field_type(field) == Some(Type::String)
            });
            if has_name_field {
                Ok(())
            } else {
                Err(Error::InvalidTypeOverrideField {
                    message: message_name.to_string(),
                    field: field_name.to_string(),
                    property,
                    reason: "field messages must expose a singular string `name` field".to_string(),
                })
            }
        }
        _ => Err(Error::InvalidTypeOverrideField {
            message: message_name.to_string(),
            field: field_name.to_string(),
            property,
            reason: "fields must be either a string field or a message with a `name` field"
                .to_string(),
        }),
    }
}

fn validate_invocation_args_field(
    message_name: &str,
    field_name: &str,
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
    property: &'static str,
) -> Result<()> {
    if field_label(field) == Some(Label::Repeated) {
        return Err(Error::InvalidTypeOverrideField {
            message: message_name.to_string(),
            field: field_name.to_string(),
            property,
            reason: "argsField must point to a singular Payloads field".to_string(),
        });
    }

    let Some(type_name) = field.type_name.as_deref() else {
        return Err(Error::InvalidTypeOverrideField {
            message: message_name.to_string(),
            field: field_name.to_string(),
            property,
            reason: "argsField must point to temporal.api.common.v1.Payloads".to_string(),
        });
    };
    let normalized_type_name = type_name.trim_start_matches('.');
    if normalized_type_name == "temporal.api.common.v1.Payloads" {
        return Ok(());
    }

    if descriptors.message(normalized_type_name).is_none() {
        return Err(Error::InvalidTypeOverrideField {
            message: message_name.to_string(),
            field: field_name.to_string(),
            property,
            reason: "argsField message type is not available in the descriptors".to_string(),
        });
    }

    Err(Error::InvalidTypeOverrideField {
        message: message_name.to_string(),
        field: field_name.to_string(),
        property,
        reason: "argsField must point to temporal.api.common.v1.Payloads".to_string(),
    })
}

fn validate_authored_field_type(
    message_name: &str,
    field_name: &str,
    authored_type: &AuthoredFieldTypeSpec,
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
) -> Result<()> {
    if authored_field_matches_proto(authored_type, field, descriptors)? {
        return Ok(());
    }

    Err(Error::InvalidTypeOverrideField {
        message: message_name.to_string(),
        field: field_name.to_string(),
        property: "type",
        reason: format!(
            "WIT field type `{}` does not match proto field type `{}`; use `@nexus.flattened-type` if only the flattened API should differ",
            authored_type.to_wit_string(),
            proto_field_type_string(field, descriptors)?,
        ),
    })
}

fn authored_field_matches_proto(
    authored_type: &AuthoredFieldTypeSpec,
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
) -> Result<bool> {
    let authored_type = authored_type.validation_type();
    if field_is_map(field, descriptors) {
        let AuthoredFieldTypeSpec::Map(authored_key, authored_value) =
            authored_type.without_option()
        else {
            return Ok(false);
        };

        let Some(entry_name) = field.type_name.as_deref() else {
            return Ok(false);
        };
        let Some(entry) = descriptors.message(entry_name.trim_start_matches('.')) else {
            return Ok(false);
        };
        let Some(key_field) = entry
            .descriptor
            .field
            .iter()
            .find(|field| field.name.as_deref() == Some("key"))
        else {
            return Ok(false);
        };
        let Some(value_field) = entry
            .descriptor
            .field
            .iter()
            .find(|field| field.name.as_deref() == Some("value"))
        else {
            return Ok(false);
        };
        return Ok(
            authored_field_matches_proto(authored_key, key_field, descriptors)?
                && authored_field_matches_proto(authored_value, value_field, descriptors)?,
        );
    }

    if field_label(field) == Some(Label::Repeated) {
        let AuthoredFieldTypeSpec::List(inner) = authored_type.without_option() else {
            return Ok(false);
        };
        return authored_field_matches_singular_proto(inner, field);
    }

    authored_field_matches_singular_proto(authored_type.without_option(), field)
}

fn authored_field_matches_singular_proto(
    authored_type: &AuthoredFieldTypeSpec,
    field: &FieldDescriptorProto,
) -> Result<bool> {
    let authored_type = authored_type.validation_type();
    let matches = match field_type(field) {
        Some(Type::Double | Type::Float) => {
            matches!(authored_type, AuthoredFieldTypeSpec::Float)
        }
        Some(
            Type::Int64
            | Type::Uint64
            | Type::Fixed64
            | Type::Sfixed64
            | Type::Sint64
            | Type::Int32
            | Type::Fixed32
            | Type::Uint32
            | Type::Sfixed32
            | Type::Sint32,
        ) => matches!(authored_type, AuthoredFieldTypeSpec::Int),
        Some(Type::Bool) => matches!(authored_type, AuthoredFieldTypeSpec::Bool),
        Some(Type::String) => matches!(authored_type, AuthoredFieldTypeSpec::String),
        Some(Type::Bytes) => matches!(authored_type, AuthoredFieldTypeSpec::Bytes),
        Some(Type::Enum | Type::Message | Type::Group) => match authored_type {
            AuthoredFieldTypeSpec::Proto(proto_name) => field
                .type_name
                .as_deref()
                .map(|name| proto_name == name.trim_start_matches('.'))
                .unwrap_or(false),
            _ => false,
        },
        None => false,
    };

    Ok(matches)
}

fn proto_field_type_string(
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
) -> Result<String> {
    if field_is_map(field, descriptors) {
        let Some(entry_name) = field.type_name.as_deref() else {
            return Ok("map".to_string());
        };
        let Some(entry) = descriptors.message(entry_name.trim_start_matches('.')) else {
            return Ok("map".to_string());
        };
        let Some(key_field) = entry
            .descriptor
            .field
            .iter()
            .find(|field| field.name.as_deref() == Some("key"))
        else {
            return Ok("map".to_string());
        };
        let Some(value_field) = entry
            .descriptor
            .field
            .iter()
            .find(|field| field.name.as_deref() == Some("value"))
        else {
            return Ok("map".to_string());
        };
        return Ok(format!(
            "map<{}, {}>",
            proto_field_type_string(key_field, descriptors)?,
            proto_field_type_string(value_field, descriptors)?,
        ));
    }

    let singular = match field_type(field) {
        Some(Type::Double | Type::Float) => "float64".to_string(),
        Some(
            Type::Int64
            | Type::Uint64
            | Type::Fixed64
            | Type::Sfixed64
            | Type::Sint64
            | Type::Int32
            | Type::Fixed32
            | Type::Uint32
            | Type::Sfixed32
            | Type::Sint32,
        ) => "s64".to_string(),
        Some(Type::Bool) => "bool".to_string(),
        Some(Type::String) => "string".to_string(),
        Some(Type::Bytes) => "bytes".to_string(),
        Some(Type::Enum | Type::Message | Type::Group) => field
            .type_name
            .as_deref()
            .map(|name| name.trim_start_matches('.').to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        None => "unknown".to_string(),
    };

    if field_label(field) == Some(Label::Repeated) {
        Ok(format!("list<{singular}>"))
    } else if field_has_presence(field, field_type(field)) {
        Ok(format!("option<{singular}>"))
    } else {
        Ok(singular)
    }
}

fn field_name_for_language(
    language: Language,
    field: &FieldDescriptorProto,
    explicit_name: Option<&str>,
) -> String {
    match language {
        Language::Python => python::python_field_name(
            explicit_name
                .or_else(|| field.name.as_deref())
                .expect("descriptor fields should be named"),
        ),
        Language::TypeScript => typescript::field_name(field, explicit_name),
        _ => explicit_name
            .or_else(|| field.name.as_deref())
            .expect("descriptor fields should be named")
            .to_string(),
    }
}

fn validate_enum_type_override(
    enumeration_name: &str,
    type_override: &TypeOverrideSpec,
) -> Result<()> {
    if !type_override.required_fields.is_empty() {
        return Err(Error::UnsupportedEnumTypeOverrideProperty {
            enumeration: enumeration_name.to_string(),
            property: "required",
        });
    }
    if !type_override.omitted_fields.is_empty() {
        return Err(Error::UnsupportedEnumTypeOverrideProperty {
            enumeration: enumeration_name.to_string(),
            property: "omit",
        });
    }
    if let Some(generated_model) = type_override.generated_model() {
        if !generated_model.field_names.is_empty()
            || !generated_model.declared_fields.is_empty()
            || !generated_model.field_annotations.is_empty()
            || !generated_model.field_flattened_annotations.is_empty()
            || !generated_model.field_wit_types.is_empty()
            || !generated_model.field_sources.is_empty()
            || !generated_model.functions.is_empty()
        {
            return Err(Error::UnsupportedTypeOverrideProperty {
                type_name: enumeration_name.to_string(),
                property: "fields",
            });
        }
    }

    Ok(())
}

fn validate_model_override_field<'a>(
    message_name: &str,
    field_name: &str,
    message: &'a MessageMetadata,
) -> Result<&'a FieldDescriptorProto> {
    message
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some(field_name))
        .ok_or_else(|| Error::UnknownTypeOverrideField {
            message: message_name.to_string(),
            field: field_name.to_string(),
        })
}

fn validate_model_required_field(
    message_name: &str,
    field_name: &str,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
) -> Result<()> {
    let field = validate_model_override_field(message_name, field_name, message)?;
    let field_type = field_type(field);

    if field_label(field) == Some(Label::Repeated) {
        let reason = if field_is_map(field, descriptors) {
            "map fields cannot be marked required"
        } else {
            "repeated fields cannot be marked required"
        };
        return Err(Error::UnsupportedRequiredTypeField {
            message: message_name.to_string(),
            field: field_name.to_string(),
            reason: reason.to_string(),
        });
    }

    if field_has_presence(field, field_type) || field_supports_required_without_presence(field_type)
    {
        return Ok(());
    }

    Err(Error::UnsupportedRequiredTypeField {
        message: message_name.to_string(),
        field: field_name.to_string(),
        reason: "field must support presence or be a string/bytes scalar".to_string(),
    })
}

fn field_is_map(field: &FieldDescriptorProto, descriptors: &DescriptorIndex) -> bool {
    if field_label(field) != Some(Label::Repeated) || field_type(field) != Some(Type::Message) {
        return false;
    }

    let Some(entry_name) = field.type_name.as_deref() else {
        return false;
    };
    let Some(entry) = descriptors.message(entry_name.trim_start_matches('.')) else {
        return false;
    };

    entry
        .descriptor
        .options
        .as_ref()
        .and_then(|options| options.map_entry)
        .unwrap_or(false)
}

fn field_has_presence(field: &FieldDescriptorProto, field_type: Option<Type>) -> bool {
    matches!(field_type, Some(Type::Message))
        || field.proto3_optional.unwrap_or(false)
        || field.oneof_index.is_some()
}

fn field_supports_required_without_presence(field_type: Option<Type>) -> bool {
    matches!(field_type, Some(Type::String | Type::Bytes))
}

fn field_label(field: &FieldDescriptorProto) -> Option<Label> {
    field.label.and_then(|label| Label::try_from(label).ok())
}

fn field_type(field: &FieldDescriptorProto) -> Option<Type> {
    field
        .r#type
        .and_then(|field_type| Type::try_from(field_type).ok())
}
