use crate::generator::dotnet::csharp_type_name;
use crate::language::Language;
use crate::planning::{PlannedProtoType, PlannedProtoTypeInfo, PlannedSpec, PlannedType};
use crate::spec::{ExternalTypeSpec, TypeReplacementSpec};

pub(crate) fn dotnet_message_type(model_type: &PlannedType) -> String {
    if let Some(proto) = model_type.proto_message()
        && let Some(replacement) = &proto.replacement
        && let Some(type_name) = dotnet_replacement_type_name(replacement)
    {
        return type_name;
    }
    match model_type {
        PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(_))) => {
            dotnet_proto_type_name_for_message(model_type)
        }
        PlannedType::Record(record) => csharp_type_name(&record.model_name),
        PlannedType::Resource(resource) => csharp_type_name(&resource.type_name),
        _ => panic!("dotnet message type should be model-shaped"),
    }
}

pub(crate) fn dotnet_replacement_type_name(replacement: &TypeReplacementSpec) -> Option<String> {
    replacement
        .type_name
        .for_language(Language::Dotnet)
        .map(str::to_string)
}

pub(crate) fn dotnet_proto_or_local_type(
    info: &PlannedProtoTypeInfo,
    local_name: Option<&str>,
) -> String {
    if info.file_name.is_some() {
        dotnet_proto_type_name_for_info(info)
    } else {
        csharp_type_name(local_name.unwrap_or(&info.full_name))
    }
}

pub(crate) fn dotnet_proto_type_name_for_message(model_type: &PlannedType) -> String {
    dotnet_proto_type_name_for_info(&model_type.proto_message().expect("proto message").proto)
}

pub(crate) fn dotnet_proto_type_name_for_info(info: &PlannedProtoTypeInfo) -> String {
    info.file_options
        .as_ref()
        .and_then(|options| options.csharp_namespace.as_deref())
        .filter(|namespace| !namespace.is_empty())
        .map(|namespace| format!("{namespace}.{}", dotnet_proto_relative_type_name(info)))
        .or_else(|| {
            info.type_name
                .for_language(Language::Dotnet)
                .map(str::to_string)
        })
        .unwrap_or_else(|| dotnet_proto_type_name_fallback(&info.full_name))
}

pub(crate) fn dotnet_planned_record_proto_type_name(
    model_type: &PlannedType,
    api_plan: &PlannedSpec,
) -> Option<String> {
    let full_name = model_type.model_full_name()?;
    api_plan
        .records
        .get(full_name)
        .and_then(|model| model.data.proto.as_ref())
        .map(dotnet_proto_type_name_for_info)
}

pub(crate) fn dotnet_to_proto_converter(model_type: &PlannedType) -> Option<&str> {
    model_type
        .proto_message()?
        .replacement
        .as_ref()
        .and_then(|replacement| replacement.to_proto.for_language(Language::Dotnet))
}

fn dotnet_proto_relative_type_name(info: &PlannedProtoTypeInfo) -> String {
    let relative_name = info
        .full_name
        .strip_prefix(&format!("{}.", info.package))
        .unwrap_or(&info.full_name);
    let mut parts = relative_name.split('.');
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut type_name = csharp_type_name(first);
    for part in parts {
        type_name.push_str(".Types.");
        type_name.push_str(&csharp_type_name(part));
    }
    type_name
}

pub(crate) fn dotnet_proto_type_name_fallback(full_name: &str) -> String {
    full_name
        .split('.')
        .map(csharp_type_name)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use prost_types::FileOptions;

    use super::{dotnet_proto_type_name_fallback, dotnet_proto_type_name_for_info};
    use crate::language::Language;
    use crate::planning::PlannedProtoTypeInfo;
    use crate::spec::LanguageStringSpec;

    #[test]
    fn proto_type_name_fallback_pascal_cases_dotted_parts() {
        assert_eq!(
            dotnet_proto_type_name_fallback("acme.foo.v1.LocalRetryPolicy"),
            "Acme.Foo.V1.LocalRetryPolicy"
        );
        assert_eq!(
            dotnet_proto_type_name_fallback("company.widgets.v1.Widget"),
            "Company.Widgets.V1.Widget"
        );
    }

    #[test]
    fn proto_type_name_uses_csharp_namespace_file_option() {
        let info = PlannedProtoTypeInfo {
            full_name: "temporal.api.workflow.v1.VersioningOverride.PinnedOverride".to_string(),
            package: "temporal.api.workflow.v1".to_string(),
            file_name: Some("temporal/api/workflow/v1/message.proto".to_string()),
            file_options: Some(FileOptions {
                csharp_namespace: Some("Temporalio.Api.Workflow.V1".to_string()),
                ..Default::default()
            }),
            reference: LanguageStringSpec::default(),
            type_name: LanguageStringSpec::default(),
        };

        assert_eq!(
            dotnet_proto_type_name_for_info(&info),
            "Temporalio.Api.Workflow.V1.VersioningOverride.Types.PinnedOverride"
        );
    }

    #[test]
    fn proto_type_name_prefers_csharp_namespace_file_option_over_wit_override() {
        let info = PlannedProtoTypeInfo {
            full_name: "temporal.api.common.v1.Payload".to_string(),
            package: "temporal.api.common.v1".to_string(),
            file_name: Some("temporal/api/common/v1/message.proto".to_string()),
            file_options: Some(FileOptions {
                csharp_namespace: Some("Temporalio.Api.Common.V1".to_string()),
                ..Default::default()
            }),
            reference: LanguageStringSpec::default(),
            type_name: LanguageStringSpec {
                default: None,
                by_language: BTreeMap::from([(
                    Language::Dotnet,
                    "Should.Not.Be.Used.Payload".to_string(),
                )]),
                default_import: None,
                imports: BTreeMap::new(),
            },
        };

        assert_eq!(
            dotnet_proto_type_name_for_info(&info),
            "Temporalio.Api.Common.V1.Payload"
        );
    }
}
