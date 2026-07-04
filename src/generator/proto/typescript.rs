use heck::ToLowerCamelCase;
use prost_types::FieldDescriptorProto;

use crate::error::Result;
use crate::generator::typescript::{
    RenderedExternalModelFragments, RenderedModel, WireValueConversion, generic_model_annotation,
    render_named_generic_function_start, typescript_authored_type_annotation,
    typescript_generated_field_name, typescript_ident,
};
use crate::language::Language;
use crate::planning::{
    PlannedProtoType, PlannedProtoTypeInfo, PlannedSpec, PlannedType, PlannedTypeFamily,
    relative_descriptor_name,
};
use crate::spec::{ExternalTypeSpec, RecordSpec, TypeReplacementSpec};

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelBackend;

impl crate::generator::ModelBackend for ModelBackend {
    type TypeRef = PlannedProtoType;
    type WireType = PlannedType;
    type ModelFragments = RenderedExternalModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, _api_plan: &PlannedSpec) -> Result<()> {
        Ok(())
    }

    fn render_models(&self) -> Result<RenderedExternalModelFragments> {
        Ok(RenderedExternalModelFragments::default())
    }

    fn model_type_annotation(&self, proto_type: &PlannedProtoType) -> Option<String> {
        match proto_type {
            PlannedProtoType::Message(proto) => {
                Some(message_typescript_interface_ref(&proto.proto))
            }
            PlannedProtoType::Enum(enum_type) => enum_type
                .replacement
                .as_ref()
                .and_then(typescript_replacement_type_name)
                .or_else(|| Some(enum_type.name.clone())),
        }
    }

    fn wire_type_identifier(&self, proto_type: &PlannedProtoType) -> Option<String> {
        match proto_type {
            PlannedProtoType::Message(proto) => Some(proto.proto.full_name.clone()),
            PlannedProtoType::Enum(_) => None,
        }
    }

    fn wire_conversion(
        &self,
        model_type: &PlannedType,
        planned_record: Option<&RecordSpec<PlannedTypeFamily>>,
    ) -> Option<WireValueConversion> {
        enum_wire_conversion(model_type)
            .or_else(|| message_override_conversion(model_type))
            .or_else(|| {
                planned_record.and_then(|record| generated_wire_conversion(model_type, record))
            })
    }
}

impl ModelBackend {
    pub(in crate::generator) fn render_model_wire_functions(
        &self,
        output: &mut String,
        model: &RenderedModel,
    ) -> bool {
        let mut wrote_conversion = false;
        if let Some(function_name) = model.from_wire_function_name.as_deref() {
            render_model_from_proto_function(output, model, function_name);
            wrote_conversion = true;
        }
        if let Some(function_name) = model.to_wire_function_name.as_deref() {
            if wrote_conversion {
                output.push('\n');
            }
            render_model_to_proto_function(output, model, function_name);
            wrote_conversion = true;
        }
        wrote_conversion
    }
}

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

pub(in crate::generator) fn message_override_conversion(
    model_type: &PlannedType,
) -> Option<WireValueConversion> {
    if let Some(proto) = model_type.proto_message()
        && let Some(language_override) = &proto.replacement
        && let Some(type_name) = typescript_replacement_type_name(language_override)
    {
        return Some(WireValueConversion {
            annotation: type_name,
            from_wire: format!(
                "{}({{wire}})",
                typescript_from_proto_converter(&proto.proto.full_name, language_override)
            ),
            to_wire: format!(
                "{}({{value}})",
                typescript_to_proto_converter(&proto.proto.full_name, language_override)
            ),
            function_name_to_wire: Some(format!(
                "{}({{name}})",
                typescript_to_proto_converter(&proto.proto.full_name, language_override)
            )),
            from_wire_function_name: None,
            to_wire_function_name: None,
            uses_rendered_model_annotation: false,
        });
    }
    if let Some(proto) = model_type.proto_message()
        && let Some(authored_type) = &proto.authored_type
    {
        return Some(WireValueConversion {
            annotation: typescript_authored_type_annotation(authored_type),
            from_wire: format!(
                "{}({{wire}})",
                typescript_default_from_proto_name(&proto.proto.full_name)
            ),
            to_wire: format!(
                "{}({{value}})",
                typescript_default_to_proto_name(&proto.proto.full_name)
            ),
            function_name_to_wire: Some(format!(
                "{}({{name}})",
                typescript_default_to_proto_name(&proto.proto.full_name)
            )),
            from_wire_function_name: None,
            to_wire_function_name: None,
            uses_rendered_model_annotation: false,
        });
    }
    None
}

fn generated_wire_conversion(
    model_type: &PlannedType,
    planned_model: &RecordSpec<PlannedTypeFamily>,
) -> Option<WireValueConversion> {
    let model_name = if planned_model.data.proto.is_some() {
        planned_model.name.clone()
    } else {
        model_type.proto_message()?.model_name.clone()
    };
    let from_wire_function_name = model_from_proto_function_name(&model_name);
    let to_wire_function_name = model_to_proto_function_name(&model_name);
    Some(WireValueConversion {
        annotation: model_name.clone(),
        from_wire: format!("{from_wire_function_name}({{wire}})"),
        to_wire: format!("{to_wire_function_name}({{value}}) ?? {{}}"),
        function_name_to_wire: Some(format!(
            "{to_wire_function_name}({{ name: {{name}} }}) ?? {{}}"
        )),
        from_wire_function_name: Some(from_wire_function_name),
        to_wire_function_name: Some(to_wire_function_name),
        uses_rendered_model_annotation: true,
    })
}

fn model_to_proto_function_name(model_name: &str) -> String {
    format!("{}ToProto", model_name.to_lower_camel_case())
}

fn model_from_proto_function_name(model_name: &str) -> String {
    format!("{}FromProto", model_name.to_lower_camel_case())
}

pub(crate) fn field_name(field: &FieldDescriptorProto, explicit_name: Option<&str>) -> String {
    match explicit_name {
        Some(name) => typescript_generated_field_name(name),
        None => {
            let name = field
                .json_name
                .as_deref()
                .or_else(|| field.name.as_deref())
                .expect("descriptor fields should be named");
            typescript_ident(name)
        }
    }
}

fn enum_wire_conversion(value_type: &PlannedType) -> Option<WireValueConversion> {
    let PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Enum(enum_type))) =
        value_type
    else {
        return None;
    };
    if let Some(replacement) = &enum_type.replacement
        && let Some(type_name) = typescript_replacement_type_name(replacement)
    {
        return Some(WireValueConversion {
            annotation: type_name.clone(),
            from_wire: format!(
                "{}({{wire}})",
                typescript_from_proto_converter(&enum_type.proto.full_name, replacement)
            ),
            to_wire: format!(
                "{}({{value}})",
                typescript_to_proto_converter(&enum_type.proto.full_name, replacement)
            ),
            function_name_to_wire: None,
            from_wire_function_name: None,
            to_wire_function_name: None,
            uses_rendered_model_annotation: false,
        });
    }

    None
}

fn render_model_to_proto_function(output: &mut String, model: &RenderedModel, function_name: &str) {
    if model.type_parameters.is_empty() {
        output.push_str("export function ");
        output.push_str(function_name);
        output.push_str("(\n");
        output.push_str("  model: ");
        output.push_str(&model.name);
        output.push_str(" | null | undefined,\n");
    } else {
        render_named_generic_function_start(
            output,
            &format!("export function {function_name}"),
            &model.type_parameters,
            0,
        );
        output.push_str("  model: ");
        output.push_str(&generic_model_annotation(
            &model.name,
            &model.type_parameters,
        ));
        output.push_str(" | null | undefined,\n");
    }
    output.push_str("): ");
    output.push_str(&model.proto_ref);
    output.push_str(" | undefined {\n");
    output.push_str("  if (model == null) {\n");
    output.push_str("    return undefined;\n");
    output.push_str("  }\n");
    if model.fields.is_empty() && model.sourced_fields.is_empty() {
        output.push_str("  return {};\n");
    } else {
        output.push_str("  return {\n");
        for field in &model.fields {
            output.push_str("    ");
            output.push_str(&field.wire_name);
            output.push_str(": ");
            output.push_str(&field.to_wire_expr);
            output.push_str(",\n");
        }
        for field in &model.sourced_fields {
            output.push_str("    ");
            output.push_str(&field.name);
            output.push_str(": ");
            output.push_str(&field.to_wire_expr);
            output.push_str(",\n");
        }
        output.push_str("  };\n");
    }
    output.push_str("}\n");
}

fn render_model_from_proto_function(
    output: &mut String,
    model: &RenderedModel,
    function_name: &str,
) {
    if model.type_parameters.is_empty() {
        output.push_str("export function ");
        output.push_str(function_name);
        output.push_str("(\n");
        output.push_str("  proto: ");
        output.push_str(&model.proto_ref);
        output.push_str(" | null | undefined,\n");
    } else {
        render_named_generic_function_start(
            output,
            &format!("export function {function_name}"),
            &model.type_parameters,
            0,
        );
        output.push_str("  proto: ");
        output.push_str(&model.proto_ref);
        output.push_str(" | null | undefined,\n");
    }
    output.push_str("): ");
    output.push_str(&generic_model_annotation(
        &model.name,
        &model.type_parameters,
    ));
    output.push_str(" | undefined {\n");
    output.push_str("  if (proto == null) {\n");
    output.push_str("    return undefined;\n");
    output.push_str("  }\n");
    if model.fields.is_empty() {
        output.push_str("  return {};\n");
    } else {
        output.push_str("  return {\n");
        for field in &model.fields {
            if field.flattened_fields.is_empty() {
                output.push_str("    ");
                output.push_str(&field.name);
                output.push_str(": ");
                output.push_str(&from_proto_expr(&field.from_wire_expr));
                output.push_str(",\n");
            } else {
                for flattened_field in &field.flattened_fields {
                    output.push_str("    ");
                    output.push_str(&flattened_field.name);
                    output.push_str(": ");
                    output.push_str(&from_proto_expr(&flattened_field.from_wire_expr));
                    output.push_str(",\n");
                }
            }
        }
        output.push_str("  };\n");
    }
    output.push_str("}\n");
}

fn from_proto_expr(field_expr: &str) -> String {
    field_expr.replace("{wire}", "proto")
}
