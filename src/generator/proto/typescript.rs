use heck::ToLowerCamelCase;

use crate::language::Language;
use crate::planning::{
    PlannedProtoType, PlannedProtoTypeInfo, PlannedSpec, PlannedType, relative_descriptor_name,
};
use crate::spec::{ExternalTypeSpec, TypeReplacementSpec};

pub(crate) fn message_typescript_interface_ref(proto: &PlannedProtoTypeInfo) -> String {
    let relative_name = relative_descriptor_name(&proto.full_name, &proto.package);
    let mut parts = relative_name.split('.').collect::<Vec<_>>();
    let leaf = parts
        .pop()
        .expect("descriptor names should not be empty")
        .to_string();
    if parts.is_empty() {
        format!("{}.I{leaf}", proto.package)
    } else {
        format!("{}.{}.I{leaf}", proto.package, parts.join("."))
    }
}

pub(crate) fn model_typescript_interface_ref(
    model_type: &PlannedType,
    api_plan: &PlannedSpec,
) -> Option<String> {
    match model_type {
        PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(proto))) => {
            Some(message_typescript_interface_ref(&proto.proto))
        }
        PlannedType::Record(record) => record_proto_info(model_type, api_plan)
            .map(message_typescript_interface_ref)
            .or_else(|| Some(record.model_name.clone())),
        _ => None,
    }
}

pub(crate) fn model_typescript_type_id(
    model_type: &PlannedType,
    api_plan: &PlannedSpec,
) -> Option<String> {
    match model_type {
        PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(proto))) => {
            Some(proto.proto.full_name.clone())
        }
        PlannedType::Record(record) => record_proto_info(model_type, api_plan)
            .map(|proto| proto.full_name.clone())
            .or_else(|| Some(record.full_name.clone())),
        _ => None,
    }
}

pub(crate) fn record_proto_info<'a>(
    model_type: &PlannedType,
    api_plan: &'a PlannedSpec,
) -> Option<&'a PlannedProtoTypeInfo> {
    let PlannedType::Record(record) = model_type else {
        return None;
    };
    api_plan
        .records
        .get(&record.full_name)
        .and_then(|record| record.data.proto.as_ref())
}

pub(crate) fn typescript_default_from_proto_name(name: &str) -> String {
    format!(
        "{}FromProto",
        name.rsplit('.')
            .next()
            .expect("converter name source should have a final segment")
            .to_lower_camel_case()
    )
}

pub(crate) fn typescript_default_to_proto_name(name: &str) -> String {
    format!(
        "{}ToProto",
        name.rsplit('.')
            .next()
            .expect("converter name source should have a final segment")
            .to_lower_camel_case()
    )
}

pub(crate) fn typescript_from_proto_converter(
    name: &str,
    replacement: &TypeReplacementSpec,
) -> String {
    replacement
        .from_proto
        .for_language(Language::TypeScript)
        .map(str::to_string)
        .unwrap_or_else(|| typescript_default_from_proto_name(name))
}

pub(crate) fn typescript_to_proto_converter(
    name: &str,
    replacement: &TypeReplacementSpec,
) -> String {
    replacement
        .to_proto
        .for_language(Language::TypeScript)
        .map(str::to_string)
        .unwrap_or_else(|| typescript_default_to_proto_name(name))
}

pub(crate) fn typescript_replacement_type_name(
    replacement: &TypeReplacementSpec,
) -> Option<String> {
    replacement
        .type_name
        .for_language(Language::TypeScript)
        .map(str::to_string)
}
