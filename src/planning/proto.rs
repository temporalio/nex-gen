use heck::ToUpperCamelCase;
use prost_types::FieldDescriptorProto;
use prost_types::field_descriptor_proto::{Label, Type};

use crate::descriptors::{DescriptorIndex, EnumMetadata, MessageMetadata};
use crate::generator::ModelWireCapabilities;
use crate::spec::{ApiSpec, ExternalTypeSpec, IntSpec, RecordSpec, TypeSpec};

use super::OperationLoweredNames;

use super::type_planning::TypePlanningContext;
use super::{
    PlannedProtoEnumType, PlannedProtoMessageType, PlannedProtoType, PlannedProtoTypeInfo,
    PlannedType, materialize_selected_replacement, materialize_selected_text,
};

impl PlannedProtoTypeInfo {
    fn from_message(message: &MessageMetadata, spec: &ApiSpec<OperationLoweredNames>) -> Self {
        Self {
            full_name: message.full_name.clone(),
            package: message.package.clone(),
            file_name: message.file_name.clone(),
            file_options: message.file_options.clone(),
            reference: spec
                .external_type_binding(&message.full_name)
                .map(|binding| materialize_selected_text(binding.reference()))
                .unwrap_or_default(),
            type_name: spec
                .external_type_binding(&message.full_name)
                .map(|binding| materialize_selected_text(binding.type_name()))
                .unwrap_or_default(),
        }
    }

    fn from_enum(enumeration: &EnumMetadata, spec: &ApiSpec<OperationLoweredNames>) -> Self {
        Self {
            full_name: enumeration.full_name.clone(),
            package: enumeration.package.clone(),
            file_name: enumeration.file_name.clone(),
            file_options: enumeration.file_options.clone(),
            reference: spec
                .external_type_binding(&enumeration.full_name)
                .map(|binding| materialize_selected_text(binding.reference()))
                .unwrap_or_default(),
            type_name: spec
                .external_type_binding(&enumeration.full_name)
                .map(|binding| materialize_selected_text(binding.type_name()))
                .unwrap_or_default(),
        }
    }
}

pub(crate) fn message_model_name(full_name: &str) -> String {
    full_name
        .rsplit('.')
        .next()
        .expect("descriptor names should not be empty")
        .to_upper_camel_case()
        .to_string()
}

fn planned_proto_model_name(
    message: &MessageMetadata,
    spec: &ApiSpec<OperationLoweredNames>,
) -> String {
    spec.record_for_proto(&message.full_name)
        .map(|record| record.name.clone())
        .unwrap_or_else(|| message_model_name(&message.full_name))
}

fn enum_name(full_name: &str) -> String {
    full_name
        .rsplit('.')
        .next()
        .expect("descriptor names should not be empty")
        .to_string()
}

pub(crate) fn relative_descriptor_name(full_name: &str, package: &str) -> String {
    if package.is_empty() {
        full_name.to_string()
    } else {
        full_name
            .strip_prefix(&format!("{package}."))
            .unwrap_or(full_name)
            .to_string()
    }
}

fn field_has_presence(field: &FieldDescriptorProto, field_type: Option<Type>) -> bool {
    matches!(field_type, Some(Type::Message))
        || field.proto3_optional.unwrap_or(false)
        || field.oneof_index.is_some()
}

fn field_label(field: &FieldDescriptorProto) -> Option<Label> {
    field.label.and_then(|label| Label::try_from(label).ok())
}

fn field_type(field: &FieldDescriptorProto) -> Option<Type> {
    field
        .r#type
        .and_then(|field_type| Type::try_from(field_type).ok())
}

pub(super) fn planned_type_for_message(
    message: &MessageMetadata,
    requested_capabilities: ModelWireCapabilities,
    planner: &mut TypePlanningContext<'_>,
) -> PlannedType {
    let planned_message = planned_message_reference(message, planner);
    if planned_message.replacement.is_some() || planned_message.authored_type.is_some() {
        return TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(
            planned_message,
        )));
    }
    if let Some(record) = planner.spec.record_for_proto(&message.full_name).cloned() {
        return TypeSpec::Record(planner.plan_record_type(&record, requested_capabilities));
    }
    TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(
        planned_message,
    )))
}

pub(super) fn planned_message_reference(
    message: &MessageMetadata,
    planner: &mut TypePlanningContext<'_>,
) -> PlannedProtoMessageType {
    let replacement = planner
        .spec
        .external_type_binding(&message.full_name)
        .and_then(|binding| binding.replacement())
        .cloned();
    let authored_type = planner
        .spec
        .external_type_binding(&message.full_name)
        .and_then(|binding| binding.authored_type.clone())
        .map(|authored_type| {
            Box::new(planner.planned_authored_type_override_from_authored(&authored_type))
        });
    PlannedProtoMessageType {
        proto: PlannedProtoTypeInfo::from_message(message, &planner.spec),
        model_name: planned_proto_model_name(message, &planner.spec),
        replacement: replacement.as_ref().map(materialize_selected_replacement),
        authored_type,
    }
}

pub(super) fn planned_enum_reference(
    enumeration: &EnumMetadata,
    spec: &ApiSpec<OperationLoweredNames>,
) -> PlannedProtoEnumType {
    let replacement = spec
        .external_type_binding(&enumeration.full_name)
        .and_then(|binding| binding.replacement())
        .cloned();
    PlannedProtoEnumType {
        proto: PlannedProtoTypeInfo::from_enum(enumeration, spec),
        name: enum_name(&enumeration.full_name),
        replacement: replacement.as_ref().map(materialize_selected_replacement),
    }
}

pub(super) fn record_proto_info(
    record: &RecordSpec<OperationLoweredNames>,
    spec: &ApiSpec<OperationLoweredNames>,
    descriptors: &DescriptorIndex,
) -> Option<PlannedProtoTypeInfo> {
    let proto_name = record_proto_name(record)?;
    descriptors
        .message(proto_name)
        .map(|message| PlannedProtoTypeInfo::from_message(message, spec))
}

pub(super) fn record_field_has_presence(
    record: &RecordSpec<OperationLoweredNames>,
    field_name: &str,
    descriptors: &DescriptorIndex,
) -> Option<bool> {
    let proto_name = record_proto_name(record)?;
    let message = descriptors.message(proto_name)?;
    let field = descriptor_field_by_name(message, field_name);
    Some(field_has_presence(field, field_type(field)))
}

pub(super) fn planned_record_field_type(
    record: &RecordSpec<OperationLoweredNames>,
    field_name: &str,
    requested_capabilities: ModelWireCapabilities,
    planner: &mut TypePlanningContext<'_>,
) -> Option<PlannedType> {
    let proto_name = record_proto_name(record)?;
    let message = planner.descriptors.message(proto_name)?.clone();
    let field = descriptor_field_by_name(&message, field_name);
    Some(planned_field_type(field, requested_capabilities, planner))
}

fn record_proto_name(record: &RecordSpec<OperationLoweredNames>) -> Option<&str> {
    let Some(ExternalTypeSpec::Proto(proto_name)) = record.source_type.as_ref() else {
        return None;
    };
    Some(proto_name.as_str())
}

fn descriptor_field_by_name<'a>(
    message: &'a MessageMetadata,
    field_name: &str,
) -> &'a FieldDescriptorProto {
    message
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some(field_name))
        .expect("planned record field should exist in descriptor")
}

pub(super) fn planned_type_from_authored_proto(
    authored_type: &TypeSpec<OperationLoweredNames>,
    planner: &mut TypePlanningContext<'_>,
) -> Option<PlannedType> {
    let TypeSpec::External(ExternalTypeSpec::Proto(proto_name)) = authored_type else {
        return None;
    };
    if planner.descriptors.message(proto_name.as_str()).is_none()
        && planner
            .descriptors
            .enumeration(proto_name.as_str())
            .is_none()
    {
        return None;
    }
    if let Some(binding) = planner.spec.external_type_binding(proto_name.as_str()) {
        if binding.replacement.is_none()
            && let Some(authored_type) = binding.authored_type.clone()
        {
            return Some(planner.planned_type_from_authored(&authored_type));
        }
    }
    None
}

pub(super) fn planned_value_type_from_authored_proto(
    proto_name: &str,
    planner: &mut TypePlanningContext<'_>,
) -> PlannedType {
    if let Some(message) = planner.descriptors.message(proto_name).cloned() {
        planned_type_for_message(&message, ModelWireCapabilities::BIDIRECTIONAL, planner)
    } else if let Some(enumeration) = planner.descriptors.enumeration(proto_name).cloned() {
        TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Enum(
            planned_enum_reference(&enumeration, &planner.spec),
        )))
    } else {
        TypeSpec::String
    }
}

fn planned_field_type(
    field: &FieldDescriptorProto,
    requested_capabilities: ModelWireCapabilities,
    planner: &mut TypePlanningContext<'_>,
) -> PlannedType {
    if let Some((key, value)) = map_field_value_types(field, requested_capabilities, planner) {
        return TypeSpec::Map(Box::new(key), Box::new(value));
    }

    let value = planned_value_type(field, requested_capabilities, planner);
    if field_label(field) == Some(Label::Repeated) {
        TypeSpec::List(Box::new(value))
    } else {
        value
    }
}

fn map_field_value_types(
    field: &FieldDescriptorProto,
    requested_capabilities: ModelWireCapabilities,
    planner: &mut TypePlanningContext<'_>,
) -> Option<(PlannedType, PlannedType)> {
    if field_label(field) != Some(Label::Repeated) || field_type(field) != Some(Type::Message) {
        return None;
    }

    let entry_name = field.type_name.as_deref()?.trim_start_matches('.');
    let entry = planner.descriptors.message(entry_name)?.clone();
    let is_map_entry = entry
        .descriptor
        .options
        .as_ref()
        .and_then(|options| options.map_entry)
        .unwrap_or(false);
    if !is_map_entry {
        return None;
    }

    let key_field = entry
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some("key"))?;
    let value_field = entry
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some("value"))?;

    Some((
        planned_value_type(key_field, requested_capabilities, planner),
        planned_value_type(value_field, requested_capabilities, planner),
    ))
}

fn planned_value_type(
    field: &FieldDescriptorProto,
    _requested_capabilities: ModelWireCapabilities,
    planner: &mut TypePlanningContext<'_>,
) -> PlannedType {
    match field_type(field) {
        Some(Type::Double | Type::Float) => TypeSpec::Float,
        Some(Type::Int64 | Type::Uint64 | Type::Fixed64 | Type::Sfixed64 | Type::Sint64) => {
            TypeSpec::Int(IntSpec::I64)
        }
        Some(Type::Int32 | Type::Fixed32 | Type::Uint32 | Type::Sfixed32 | Type::Sint32) => {
            TypeSpec::Int(IntSpec::I32)
        }
        Some(Type::Bool) => TypeSpec::Bool,
        Some(Type::String) => TypeSpec::String,
        Some(Type::Bytes) => TypeSpec::Bytes,
        Some(Type::Enum) => plan_enum_type(field, planner),
        Some(Type::Message) | Some(Type::Group) => {
            if let Some(message) = field.type_name.as_deref().and_then(|type_name| {
                planner
                    .descriptors
                    .message(type_name.trim_start_matches('.'))
                    .cloned()
            }) {
                planned_type_for_message(&message, ModelWireCapabilities::BIDIRECTIONAL, planner)
            } else {
                TypeSpec::String
            }
        }
        None => TypeSpec::String,
    }
}

fn plan_enum_type(
    field: &FieldDescriptorProto,
    planner: &mut TypePlanningContext<'_>,
) -> PlannedType {
    let Some(enumeration) = field.type_name.as_deref().and_then(|type_name| {
        planner
            .descriptors
            .enumeration(type_name.trim_start_matches('.'))
            .cloned()
    }) else {
        return TypeSpec::String;
    };

    let replacement = planner
        .spec
        .external_type_binding(&enumeration.full_name)
        .and_then(|binding| binding.replacement())
        .cloned();
    TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Enum(
        PlannedProtoEnumType {
            proto: PlannedProtoTypeInfo::from_enum(&enumeration, &planner.spec),
            name: enum_name(&enumeration.full_name),
            replacement: replacement.as_ref().map(materialize_selected_replacement),
        },
    )))
}
