//! `AuthoredValidationPass` verifies descriptor-backed authored intent before
//! target selection discards language alternatives.

use std::collections::BTreeMap;
use std::path::Path;

use prost_types::FieldDescriptorProto;
use prost_types::field_descriptor_proto::{Label, Type};

use crate::descriptors::{DescriptorIndex, MessageMetadata};
use crate::error::{Error, Result};
use crate::generator::proto::typescript as typescript_proto;
use crate::generator::python;
use crate::language::Language;
use crate::spec::{
    ApiSpec, AuthoredFamily, ExternalTypeBindingSpec, ExternalTypeSpec, FunctionArgsSpec,
    FunctionResultSpec, RecordFieldVisibility, RecordSpec, ServiceSpec, TypeSpec,
};
use crate::spec::{ApiSpecLeaf, CompilerPass};

fn validate_generic_model_semantics(spec: &ApiSpec, path: &Path, language: Language) -> Result<()> {
    let invalid = |reason: String| Error::InvalidWit {
        path: path.to_path_buf(),
        reason,
    };
    for (_, record) in spec.records() {
        let parameters = spec.record_type_parameters(&record.full_name, language);
        let mut generated_names = BTreeMap::<String, String>::new();
        for usage in &parameters {
            if let Some(previous) = generated_names.insert(
                usage.parameter.name.clone(),
                usage.parameter.full_name.clone(),
            ) && previous != usage.parameter.full_name
            {
                return Err(invalid(format!(
                    "type parameters `{previous}` and `{}` both generate the name `{}`",
                    usage.parameter.full_name, usage.parameter.name
                )));
            }
        }
        if record.source_type.is_some() && !parameters.is_empty() {
            return Err(invalid(format!(
                "proto-backed record `{}` cannot contain generic type parameters",
                record.full_name
            )));
        }
        for (field_name, function) in record.functions() {
            let uses_parameter = match &function.result {
                FunctionResultSpec::Authored(result) => {
                    !spec.type_parameters(result, language).is_empty()
                }
                FunctionResultSpec::Annotation(_) => false,
            } || match &function.args {
                FunctionArgsSpec::Varargs { prefix, .. } | FunctionArgsSpec::Fixed(prefix) => {
                    prefix
                        .iter()
                        .any(|arg| !spec.type_parameters(&arg.field_type, language).is_empty())
                }
            } || function
                .alternate_type
                .as_ref()
                .is_some_and(|alternate| !spec.type_parameters(alternate, language).is_empty());
            if uses_parameter {
                return Err(invalid(format!(
                    "record `{}` field `{field_name}` uses a type parameter in function-signature metadata, which is not supported",
                    record.full_name
                )));
            }
        }
    }

    for (_, variant) in spec.variants() {
        let parameters = spec.variant_type_parameters(&variant.full_name, language);
        let mut generated_names = BTreeMap::<String, String>::new();
        for usage in parameters {
            if let Some(previous) = generated_names.insert(
                usage.parameter.name.clone(),
                usage.parameter.full_name.clone(),
            ) && previous != usage.parameter.full_name
            {
                return Err(invalid(format!(
                    "variant `{}` uses type parameters `{previous}` and `{}` that both generate the name `{}`",
                    variant.full_name, usage.parameter.full_name, usage.parameter.name
                )));
            }
        }
    }

    for service in &spec.services {
        for operation in &service.operations {
            let mut generated_names = BTreeMap::<String, String>::new();
            let operation_parameters = operation
                .input_type()
                .into_iter()
                .chain(operation.output_type())
                .flat_map(|value| spec.type_parameters(value, language));
            for usage in operation_parameters {
                if let Some(previous) = generated_names.insert(
                    usage.parameter.name.clone(),
                    usage.parameter.full_name.clone(),
                ) && previous != usage.parameter.full_name
                {
                    return Err(invalid(format!(
                        "operation `{}` uses type parameters `{previous}` and `{}` that both generate the name `{}`",
                        operation.name, usage.parameter.full_name, usage.parameter.name
                    )));
                }
            }
        }
        for resource in &service.resources {
            let resource_is_generic =
                resource
                    .fields
                    .iter()
                    .any(|field| !spec.type_parameters(&field.field_type, language).is_empty())
                    || resource.methods.iter().any(|method| {
                        method.params.iter().any(|param| {
                            !spec.type_parameters(&param.field_type, language).is_empty()
                        }) || method.result.as_ref().is_some_and(|result| {
                            !spec
                                .type_parameters(&result.result_type, language)
                                .is_empty()
                        })
                    });
            if resource_is_generic {
                return Err(invalid(format!(
                    "resource `{}` cannot contain generic type parameters",
                    resource.name
                )));
            }

            for method in &resource.methods {
                let Some(operation_name) = &method.operation_name else {
                    continue;
                };
                let Some(operation) = service.operation(operation_name) else {
                    continue;
                };
                let operation_is_generic = operation
                    .input_type()
                    .is_some_and(|input| !spec.type_parameters(input, language).is_empty())
                    || operation
                        .output_type()
                        .is_some_and(|output| !spec.type_parameters(output, language).is_empty());
                if operation_is_generic {
                    return Err(invalid(format!(
                        "resource-bound operation `{operation_name}` cannot use generic type parameters"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct MessageUsage {
    input: bool,
    output: bool,
}

#[derive(Clone, Copy)]
struct ModelConfig<'a> {
    record: &'a RecordSpec,
}

pub(crate) struct AuthoredValidationPass<'a> {
    descriptors: &'a DescriptorIndex,
    language: Language,
}

impl<'a> AuthoredValidationPass<'a> {
    pub(crate) fn new(descriptors: &'a DescriptorIndex, language: Language) -> Self {
        Self {
            descriptors,
            language,
        }
    }

    pub(crate) fn validate_spec(&self, spec: &ApiSpec) -> Result<()> {
        validate_external_type_bindings(spec, self.descriptors, self.language)
    }
}

impl CompilerPass<AuthoredFamily, AuthoredFamily> for AuthoredValidationPass<'_> {
    type Error = Error;

    fn transform_leaf(
        &mut self,
        leaf: ApiSpecLeaf<AuthoredFamily>,
    ) -> Result<ApiSpecLeaf<AuthoredFamily>> {
        self.validate_spec(&leaf.spec)?;
        validate_generic_model_semantics(&leaf.spec, &leaf.source_path, self.language)?;
        Ok(leaf)
    }
}

impl<'a> ModelConfig<'a> {
    fn from_record(record: &'a RecordSpec) -> Self {
        Self { record }
    }

    fn is_field_omitted(&self, field_name: &str) -> bool {
        self.record.field_omitted(field_name)
    }

    fn is_field_hidden(&self, field_name: &str) -> bool {
        self.record.field_omitted(field_name) || self.record.field_source(field_name).is_some()
    }
}

fn validate_external_type_bindings(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    language: Language,
) -> Result<()> {
    let usages = language_message_usages(spec, descriptors, language)?;
    for (type_name, binding) in spec.external_types() {
        if let Some(message) = descriptors.message(type_name) {
            validate_message_external_type_binding(type_name, binding)?;
            if let Some(record) = spec.record_for_proto(type_name) {
                validate_message_model_config(
                    type_name,
                    ModelConfig::from_record(record),
                    message,
                    descriptors,
                    usages.get(type_name).copied().unwrap_or_default(),
                    language,
                )?;
            }
        } else if descriptors.enumeration(type_name).is_some() {
            validate_enum_external_type_binding(type_name, binding)?;
        } else if descriptors.file_count() == 0 {
            continue;
        } else {
            return Err(Error::UnknownTypeOverride {
                type_name: type_name.to_string(),
            });
        }
    }

    Ok(())
}

fn validate_message_external_type_binding(
    _message_name: &str,
    _binding: &ExternalTypeBindingSpec,
) -> Result<()> {
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
            let Some(TypeSpec::External(ExternalTypeSpec::Proto(input_proto))) =
                operation.input_type()
            else {
                continue;
            };
            let Some(input_message) = descriptors.message(input_proto.as_str()) else {
                continue;
            };
            usages
                .entry(input_message.full_name.clone())
                .or_default()
                .input = true;

            if operation.output_transform().is_some()
                || operation_output_resource_name(service, operation).is_some()
            {
                continue;
            }

            let Some(TypeSpec::External(ExternalTypeSpec::Proto(output_proto))) =
                operation.output_type()
            else {
                continue;
            };
            let Some(output_message) = descriptors.message(output_proto.as_str()) else {
                continue;
            };
            usages
                .entry(output_message.full_name.clone())
                .or_default()
                .output = true;
        }
    }

    Ok(usages)
}

fn operation_output_resource_name<'a>(
    service: &'a ServiceSpec,
    operation: &'a crate::spec::OperationSpec,
) -> Option<&'a str> {
    let TypeSpec::Resource(resource_name) = operation.output_type()? else {
        return None;
    };
    service
        .resource(resource_name.as_str())
        .map(|_| resource_name.as_str())
}

fn validate_message_model_config(
    message_name: &str,
    model_config: ModelConfig<'_>,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
    usage: MessageUsage,
    language: Language,
) -> Result<()> {
    for field_name in model_config
        .record
        .fields
        .iter()
        .filter(|(_, field)| field.required)
        .map(|(field_name, _)| field_name)
    {
        validate_model_required_field(message_name, field_name, message, descriptors)?;
    }
    for field_name in model_config
        .record
        .fields
        .iter()
        .filter(|(_, field)| field.visibility == RecordFieldVisibility::Omitted)
        .map(|(field_name, _)| field_name)
    {
        validate_model_override_field(message_name, field_name, message)?;
    }
    for field_name in model_config.record.fields.keys() {
        validate_model_override_field(message_name, field_name, message)?;
    }
    validate_record_fields(message_name, model_config, message, usage, language)?;
    validate_authored_field_types(message_name, model_config, message, descriptors)?;
    validate_invocation_fields(message_name, model_config, message, descriptors, usage)?;
    for (field_name, _) in
        model_config.record.fields.iter().filter(|(_, field)| {
            field.required && field.visibility == RecordFieldVisibility::Omitted
        })
    {
        return Err(Error::ConflictingTypeOverrideField {
            message: message_name.to_string(),
            field: field_name.clone(),
        });
    }

    Ok(())
}

fn validate_record_fields(
    message_name: &str,
    model_config: ModelConfig<'_>,
    message: &MessageMetadata,
    usage: MessageUsage,
    language: Language,
) -> Result<()> {
    for (field_name, field) in &model_config.record.fields {
        validate_model_override_field(message_name, field_name, message)?;
        if field.visibility == RecordFieldVisibility::Omitted
            && (field.doc.is_some()
                || field.annotation.is_some()
                || field.flattened_annotation.is_some()
                || field.default_value.is_some()
                || field.function.is_some()
                || matches!(field.visibility, RecordFieldVisibility::Sourced { .. }))
        {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
    }

    for (field_name, _) in model_config
        .record
        .fields
        .iter()
        .filter(|(_, field)| field.annotation.is_some())
    {
        validate_model_override_field(message_name, field_name, message)?;
        if model_config.is_field_omitted(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
    }

    for (field_name, _) in model_config
        .record
        .fields
        .iter()
        .filter(|(_, field)| field.flattened_annotation.is_some())
    {
        validate_model_override_field(message_name, field_name, message)?;
        if model_config.is_field_omitted(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
        if !model_config.record.flatten_in_api {
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

    for (field_name, _, _) in model_config.record.sourced_fields() {
        validate_model_override_field(message_name, field_name, message)?;
        if model_config.is_field_omitted(field_name) {
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
        if !model_config.is_field_omitted(proto_name)
            && !model_config.record.fields.contains_key(proto_name)
        {
            return Err(Error::UndeclaredTypeOverrideField {
                message: message_name.to_string(),
                field: proto_name.to_string(),
            });
        }
        if model_config.is_field_hidden(proto_name) {
            continue;
        }

        let generated_name = field_name_for_language(
            language,
            field,
            model_config.record.field_name_override(proto_name),
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
    model_config: ModelConfig<'_>,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
) -> Result<()> {
    for (field_name, field_spec) in &model_config.record.fields {
        if field_spec.function.is_some() {
            continue;
        }

        let field = validate_model_override_field(message_name, field_name, message)?;
        if model_config.is_field_omitted(field_name) {
            continue;
        }

        validate_authored_field_type(
            message_name,
            field_name,
            &field_spec.field_type,
            field,
            descriptors,
        )?;
    }

    Ok(())
}

fn validate_invocation_fields(
    message_name: &str,
    model_config: ModelConfig<'_>,
    message: &MessageMetadata,
    descriptors: &DescriptorIndex,
    usage: MessageUsage,
) -> Result<()> {
    if model_config.record.functions().next().is_none() {
        return Ok(());
    }

    if usage.output {
        if let Some((field, _)) = model_config.record.functions().next() {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: field.to_string(),
                property: "function",
                reason: "function fields are only supported on input-only generated models"
                    .to_string(),
            });
        }
    }

    let mut primary_field_name: Option<&str> = None;
    let mut seen_args_fields = BTreeMap::new();

    for (field_name, function) in model_config.record.functions() {
        if function.primary {
            if let Some(existing) = primary_field_name {
                return Err(Error::InvalidTypeOverrideField {
                    message: message_name.to_string(),
                    field: field_name.to_string(),
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
                field: field_name.to_string(),
                property: "function",
                reason: format!(
                    "argsField `{}` is already used by function field `{existing}`",
                    function.args_field
                ),
            });
        }
    }

    for (field_name, function) in model_config.record.functions() {
        let callable_field = validate_model_override_field(message_name, field_name, message)?;
        if model_config.is_field_omitted(field_name) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
            });
        }
        if function.args_field == *field_name {
            return Err(Error::InvalidTypeOverrideField {
                message: message_name.to_string(),
                field: field_name.to_string(),
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
        if model_config.is_field_omitted(&function.args_field) {
            return Err(Error::OmittedCustomizedTypeOverrideField {
                message: message_name.to_string(),
                field: function.args_field.clone(),
            });
        }
        if model_config.record.field_required(&function.args_field) {
            return Err(Error::ConflictingTypeOverrideFieldProperties {
                message: message_name.to_string(),
                field: function.args_field.clone(),
                property: "function",
                conflicting_property: "required",
            });
        }
        if model_config
            .record
            .field_source(&function.args_field)
            .is_some()
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
    authored_type: &TypeSpec,
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
            "authored field type `{}` does not match proto field type `{}`; use `@nexus.flattened-type` if only the flattened API should differ",
            authored_type.to_type_string(),
            proto_field_type_string(field, descriptors)?,
        ),
    })
}

fn authored_field_matches_proto(
    authored_type: &TypeSpec,
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
) -> Result<bool> {
    let authored_type = authored_type.validation_type();
    if field_is_map(field, descriptors) {
        let TypeSpec::Map(authored_key, authored_value) = authored_type.without_option() else {
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
        let TypeSpec::List(inner) = authored_type.without_option() else {
            return Ok(false);
        };
        return authored_field_matches_singular_proto(inner, field);
    }

    authored_field_matches_singular_proto(authored_type.without_option(), field)
}

fn authored_field_matches_singular_proto(
    authored_type: &TypeSpec,
    field: &FieldDescriptorProto,
) -> Result<bool> {
    let authored_type = authored_type.validation_type();
    let matches = match field_type(field) {
        Some(Type::Double | Type::Float) => {
            matches!(authored_type, TypeSpec::Float)
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
        ) => matches!(authored_type, TypeSpec::Int(_)),
        Some(Type::Bool) => matches!(authored_type, TypeSpec::Bool),
        Some(Type::String) => matches!(authored_type, TypeSpec::String),
        Some(Type::Bytes) => matches!(authored_type, TypeSpec::Bytes),
        Some(Type::Enum | Type::Message | Type::Group) => match authored_type {
            TypeSpec::External(ExternalTypeSpec::Proto(proto_name)) => field
                .type_name
                .as_deref()
                .map(|name| proto_name.as_str() == name.trim_start_matches('.'))
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
        Language::TypeScript => typescript_proto::field_name(field, explicit_name),
        _ => explicit_name
            .or_else(|| field.name.as_deref())
            .expect("descriptor fields should be named")
            .to_string(),
    }
}

fn validate_enum_external_type_binding(
    _enumeration_name: &str,
    _binding: &ExternalTypeBindingSpec,
) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn validate(language: Language, wit: &str) -> Result<()> {
        let spec = crate::parser::parse_api_spec_from_wit_for_language_with_inputs(
            language,
            wit,
            PathBuf::from("inline.wit"),
            &[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("advanced/samples/inputs/deps")],
        )?;
        let descriptors = DescriptorIndex::load_many(&[])?;
        AuthoredValidationPass::new(&descriptors, language)
            .apply(crate::spec::ApiSpecTree::single(spec))
            .map(|_| ())
    }

    const GENERIC_WIT: &str = r#"
package temporal:nexus@1.0.0;
world system { export generic-service; }
interface generic-service {
  use nexus:temporal-types/model@1.0.0.{placeholder};
  /// @nexus.type-parameter
  type context-t = placeholder;
  /// @nexus.type-parameter
  type output-t = placeholder;
  record inner { value: context-t, }
  record request { nested: inner, values: list<context-t>, }
  record response { context: context-t, output: output-t, }
  complete: func(request: request) -> response;
}
"#;

    #[test]
    fn rejects_generic_resources_and_proto_backed_records() {
        let resource = GENERIC_WIT.replace(
            "record inner { value: context-t, }",
            "resource holder { constructor(value: context-t); }\n  record inner { value: context-t, }",
        );
        assert!(
            validate(Language::Dotnet, &resource)
                .unwrap_err()
                .to_string()
                .contains("resource")
        );

        let proto_backed = GENERIC_WIT
            .replace(
                "record inner { value: context-t, }",
                "record inner { value: context-t, }\n  variant wrapped { inner(inner), }",
            )
            .replace("nested: inner,", "nested: wrapped,")
            .replace(
                "record request {",
                "/// @nexus.proto \"example.GenericRequest\"\n  record request {",
            );
        assert!(
            validate(Language::Dotnet, &proto_backed)
                .unwrap_err()
                .to_string()
                .contains("proto-backed record")
        );
    }

    #[test]
    fn rejects_generated_type_parameter_name_collisions() {
        let base = r#"
package temporal:nexus@1.0.0;
world system { export generic-service; }
interface left {
  use nexus:temporal-types/model@1.0.0.{placeholder};
  /// @nexus.type-parameter
  type value-t = placeholder;
  record payload { value: value-t, }
}
interface right {
  use nexus:temporal-types/model@1.0.0.{placeholder};
  /// @nexus.type-parameter
  type value-t = placeholder;
  record payload { value: value-t, }
}
interface generic-service {
  use left.{payload as left-payload};
  use right.{payload as right-payload};
  record request { value: left-payload, }
  record response { value: right-payload, }
  execute: func(request: request) -> response;
}
"#;
        assert!(
            validate(Language::TypeScript, base)
                .unwrap_err()
                .to_string()
                .contains("operation `Execute`")
        );

        let variant = base.replace(
            "record request { value: left-payload, }",
            "variant collision { left(left-payload), right(right-payload), }\n  record request { value: left-payload, }",
        );
        assert!(
            validate(Language::TypeScript, &variant)
                .unwrap_err()
                .to_string()
                .contains("variant `generic-service.collision`")
        );
    }
}
