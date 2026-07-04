use std::collections::BTreeSet;

use heck::ToSnakeCase;

use crate::generator::python::{
    EnumValueConversion, PythonImports, RenderedModel, RenderedModelFragments, RenderedWireRead,
    RenderedWireWrite, ResolvedFieldKind, ResolvedFieldType, WireReadPolicy,
    python_parameter_annotation, python_string_literal, render_python_default_expr,
    render_python_docstring,
};
use crate::language::Language;
use crate::planning::{
    PlannedProtoType, PlannedProtoTypeInfo, PlannedSpec, PlannedType, PlannedTypeFamily,
    relative_descriptor_name,
};
use crate::spec::{ExternalTypeSpec, RecordFieldSpec, RecordSpec, TypeReplacementSpec};

#[derive(Debug)]
pub(crate) struct PythonReference {
    pub(crate) module_path: String,
    pub(crate) type_ref: String,
}

pub(crate) fn message_python_ref(type_info: &PlannedProtoTypeInfo) -> Option<PythonReference> {
    let module_path =
        python_module_path_for_file_name(type_info.file_name.as_deref(), &type_info.package)?;
    let relative_name = relative_descriptor_name(&type_info.full_name, &type_info.package);
    Some(PythonReference {
        type_ref: format!("{module_path}.{relative_name}"),
        module_path,
    })
}

pub(crate) fn enum_python_ref(type_info: &PlannedProtoTypeInfo) -> Option<PythonReference> {
    let module_path =
        python_module_path_for_file_name(type_info.file_name.as_deref(), &type_info.package)?;
    let relative_name = relative_descriptor_name(&type_info.full_name, &type_info.package);
    Some(PythonReference {
        type_ref: format!("{module_path}.{relative_name}.ValueType"),
        module_path,
    })
}

pub(crate) fn external_message_python_ref(model_type: &PlannedType) -> Option<PythonReference> {
    match model_type {
        PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(proto))) => {
            message_python_ref(&proto.proto)
        }
        _ => None,
    }
}

pub(crate) fn service_message_python_ref(
    model_type: &PlannedType,
    api_plan: &PlannedSpec,
) -> Option<PythonReference> {
    external_message_python_ref(model_type).or_else(|| {
        let PlannedType::Record(record) = model_type else {
            return None;
        };
        api_plan
            .records
            .get(&record.full_name)
            .and_then(|record| record.data.proto.as_ref())
            .and_then(message_python_ref)
    })
}

pub(crate) fn model_python_ref(
    model_type: &PlannedType,
    planned_model: &RecordSpec<PlannedTypeFamily>,
    proto_ref_override: Option<&str>,
) -> Option<PythonReference> {
    if let Some(proto_ref) = proto_ref_override {
        return Some(PythonReference {
            module_path: python_module_path(proto_ref).to_string(),
            type_ref: proto_ref.to_string(),
        });
    }

    model_type
        .proto_message()
        .map(|message| &message.proto)
        .or(planned_model.data.proto.as_ref())
        .and_then(message_python_ref)
}

pub(in crate::generator) enum PythonProtoMessageOverride<'a> {
    Replacement {
        annotation: String,
        from_proto: String,
        to_proto: String,
    },
    Authored {
        authored_type: &'a PlannedType,
        from_proto: String,
        to_proto: String,
    },
}

#[derive(Debug, Clone)]
pub(in crate::generator) enum PythonProtoMessageConversion {
    GeneratedModel {
        model_name: String,
    },
    NativeModel,
    Override {
        from_proto: String,
        to_proto: String,
    },
}

impl PythonProtoMessageConversion {
    pub(in crate::generator) fn from_proto_expr(&self, proto_expr: &str) -> String {
        match self {
            PythonProtoMessageConversion::GeneratedModel { model_name } => {
                format!("{model_name}.from_proto({proto_expr})")
            }
            PythonProtoMessageConversion::NativeModel => proto_expr.to_string(),
            PythonProtoMessageConversion::Override { from_proto, .. } => {
                format!("{from_proto}({proto_expr})")
            }
        }
    }

    pub(in crate::generator) fn to_proto_expr(&self, value_expr: &str) -> String {
        match self {
            PythonProtoMessageConversion::GeneratedModel { .. } => {
                format!("{value_expr}.to_proto()")
            }
            PythonProtoMessageConversion::NativeModel => value_expr.to_string(),
            PythonProtoMessageConversion::Override { to_proto, .. } => {
                format!("{to_proto}({value_expr})")
            }
        }
    }

    pub(in crate::generator) fn supports_unpacked_input(&self) -> bool {
        matches!(
            self,
            PythonProtoMessageConversion::GeneratedModel { .. }
                | PythonProtoMessageConversion::NativeModel
        )
    }
}

pub(in crate::generator) fn message_override(
    model_type: &PlannedType,
) -> Option<PythonProtoMessageOverride<'_>> {
    if let Some(proto) = model_type.proto_message()
        && let Some(language_override) = &proto.replacement
        && let Some(type_name) = python_replacement_type_name(language_override)
    {
        return Some(PythonProtoMessageOverride::Replacement {
            annotation: type_name,
            from_proto: python_from_proto_converter(&proto.proto.full_name, language_override),
            to_proto: python_to_proto_converter(&proto.proto.full_name, language_override),
        });
    }
    if let Some(proto) = model_type.proto_message()
        && let Some(authored_type) = &proto.authored_type
    {
        return Some(PythonProtoMessageOverride::Authored {
            authored_type,
            from_proto: python_default_from_proto_name(&proto.proto.full_name),
            to_proto: python_default_to_proto_name(&proto.proto.full_name),
        });
    }
    None
}

pub(in crate::generator) fn generated_message_model_name(
    model_type: &PlannedType,
    planned_model: &RecordSpec<PlannedTypeFamily>,
) -> Option<String> {
    if planned_model.data.proto.is_some() {
        return Some(planned_model.name.clone());
    }
    model_type
        .proto_message()
        .map(|proto| proto.model_name.clone())
}

pub(in crate::generator) struct PythonProtoEnumValue {
    pub(in crate::generator) annotation: String,
    pub(in crate::generator) module_import: Option<String>,
    pub(in crate::generator) conversion: Option<PythonProtoEnumConversion>,
}

pub(in crate::generator) struct PythonProtoEnumConversion {
    pub(in crate::generator) from_proto: String,
    pub(in crate::generator) to_proto: String,
}

pub(in crate::generator) fn enum_value(value_type: &PlannedType) -> Option<PythonProtoEnumValue> {
    let PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Enum(enum_type))) =
        value_type
    else {
        return None;
    };
    if let Some(replacement) = &enum_type.replacement
        && let Some(type_name) = python_replacement_type_name(replacement)
    {
        return Some(PythonProtoEnumValue {
            annotation: type_name,
            module_import: None,
            conversion: Some(PythonProtoEnumConversion {
                from_proto: python_from_proto_converter(&enum_type.proto.full_name, replacement),
                to_proto: python_to_proto_converter(&enum_type.proto.full_name, replacement),
            }),
        });
    }

    Some(PythonProtoEnumValue {
        annotation: enum_type.name.clone(),
        module_import: enum_python_ref(&enum_type.proto).map(|reference| reference.module_path),
        conversion: None,
    })
}

pub(in crate::generator) fn enum_field_type(value_type: &PlannedType) -> Option<ResolvedFieldType> {
    let proto_enum = enum_value(value_type)?;
    let mut imports = PythonImports::default();
    if let Some(module_import) = proto_enum.module_import {
        imports.module_imports.insert(module_import);
    }
    Some(ResolvedFieldType {
        annotation: proto_enum.annotation,
        imports,
        kind: ResolvedFieldKind::Enum,
        message_conversion: None,
        enum_conversion: proto_enum.conversion.map(|conversion| EnumValueConversion {
            from_wire: conversion.from_proto,
            to_wire: conversion.to_proto,
        }),
    })
}

pub(in crate::generator) fn field_read(
    proto_name: &str,
    attr_name: &str,
    field: &RecordFieldSpec<PlannedTypeFamily>,
    resolved_value_type: &ResolvedFieldType,
    policy: WireReadPolicy,
) -> RenderedWireRead {
    let proto_expr = format!("proto.{proto_name}");
    let value_expr = match &field.field_type {
        PlannedType::Map(_, _) => map_value_from_proto_expr(resolved_value_type, proto_name),
        PlannedType::List(_) => repeated_from_proto_expr(resolved_value_type, proto_name),
        _ => from_proto_value_expr(resolved_value_type, &proto_expr),
    };

    match policy {
        WireReadPolicy::Required { missing_error } => {
            let mut setup_lines = Vec::new();
            if field_has_proto_presence(field) {
                setup_lines.push(format!("if not proto.HasField(\"{proto_name}\"):"));
                setup_lines.push(format!("    raise ValueError({missing_error})"));
            } else if matches!(&field.field_type, PlannedType::String | PlannedType::Bytes) {
                setup_lines.push(format!("if not proto.{proto_name}:"));
                setup_lines.push(format!("    raise ValueError({missing_error})"));
            }
            setup_lines.push(format!("{attr_name} = {value_expr}"));
            RenderedWireRead {
                setup_lines,
                expr: attr_name.to_string(),
            }
        }
        WireReadPolicy::Optional => RenderedWireRead {
            setup_lines: Vec::new(),
            expr: optional_from_proto_expr(field, proto_name, value_expr),
        },
        WireReadPolicy::Default { default_expr } => RenderedWireRead {
            setup_lines: Vec::new(),
            expr: defaulted_from_proto_expr(field, proto_name, value_expr, default_expr),
        },
    }
}

pub(in crate::generator) fn field_write(
    proto_name: &str,
    field: &RecordFieldSpec<PlannedTypeFamily>,
    value_expr: &str,
    resolved_value_type: &ResolvedFieldType,
    optional_guard: bool,
) -> RenderedWireWrite {
    let lines = match &field.field_type {
        PlannedType::Map(_, _) => {
            map_value_to_proto_lines(resolved_value_type, value_expr, proto_name, optional_guard)
        }
        PlannedType::List(_) => {
            repeated_to_proto_lines(resolved_value_type, value_expr, proto_name, optional_guard)
        }
        _ => value_to_proto_lines(resolved_value_type, value_expr, proto_name, optional_guard),
    };
    RenderedWireWrite { lines }
}

pub(in crate::generator) fn function_field_write(
    proto_name: &str,
    _field: &RecordFieldSpec<PlannedTypeFamily>,
    value_expr: &str,
    converter: &str,
    resolved_type: &ResolvedFieldType,
    optional_guard: bool,
) -> RenderedWireWrite {
    let converted_value = format!("{converter}({value_expr})");
    let mut lines = Vec::new();
    if optional_guard {
        lines.push(format!("if {value_expr} is not None:"));
    }
    let indent = if optional_guard { "    " } else { "" };
    match resolved_type.kind {
        ResolvedFieldKind::Message => lines.push(format!(
            "{indent}message.{proto_name}.CopyFrom({converted_value})"
        )),
        _ => lines.push(format!("{indent}message.{proto_name} = {converted_value}")),
    }
    RenderedWireWrite { lines }
}

fn field_has_proto_presence(field: &RecordFieldSpec<PlannedTypeFamily>) -> bool {
    field.data.has_presence.unwrap_or(!field.required)
}

fn optional_from_proto_expr(
    field: &RecordFieldSpec<PlannedTypeFamily>,
    proto_name: &str,
    value_expr: String,
) -> String {
    if field_has_proto_presence(field) {
        format!("{value_expr} if proto.HasField(\"{proto_name}\") else None")
    } else {
        value_expr
    }
}

fn defaulted_from_proto_expr(
    field: &RecordFieldSpec<PlannedTypeFamily>,
    proto_name: &str,
    value_expr: String,
    default_expr: String,
) -> String {
    if field_has_proto_presence(field) {
        format!("{value_expr} if proto.HasField(\"{proto_name}\") else {default_expr}")
    } else {
        value_expr
    }
}

fn repeated_from_proto_expr(resolved_type: &ResolvedFieldType, proto_name: &str) -> String {
    match resolved_type.kind {
        ResolvedFieldKind::Message => format!(
            "[{} for value in proto.{proto_name}]",
            resolved_type
                .message_conversion
                .as_ref()
                .expect("message conversion should be present")
                .from_wire_expr("value")
        ),
        ResolvedFieldKind::Enum => format!(
            "[{} for value in proto.{proto_name}]",
            enum_from_proto_expr(resolved_type, "value")
        ),
        _ => format!("list(proto.{proto_name})"),
    }
}

fn map_value_from_proto_expr(map_value_type: &ResolvedFieldType, proto_name: &str) -> String {
    match map_value_type.kind {
        ResolvedFieldKind::Message => format!(
            "{{key: {} for key, value in proto.{proto_name}.items()}}",
            map_value_type
                .message_conversion
                .as_ref()
                .expect("message conversion should be present")
                .from_wire_expr("value")
        ),
        ResolvedFieldKind::Enum => format!(
            "{{key: {} for key, value in proto.{proto_name}.items()}}",
            enum_from_proto_expr(map_value_type, "value")
        ),
        _ => format!("{{key: value for key, value in proto.{proto_name}.items()}}"),
    }
}

fn from_proto_value_expr(resolved_type: &ResolvedFieldType, proto_expr: &str) -> String {
    match resolved_type.kind {
        ResolvedFieldKind::Message => resolved_type
            .message_conversion
            .as_ref()
            .expect("message conversion should be present")
            .from_wire_expr(proto_expr),
        ResolvedFieldKind::Enum => enum_from_proto_expr(resolved_type, proto_expr),
        _ => proto_expr.to_string(),
    }
}

fn repeated_to_proto_lines(
    resolved_type: &ResolvedFieldType,
    value_expr: &str,
    proto_name: &str,
    optional_guard: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if optional_guard {
        lines.push(format!("if {value_expr}:"));
    }
    let indent = if optional_guard { "    " } else { "" };
    match resolved_type.kind {
        ResolvedFieldKind::Message => {
            lines.push(format!("{indent}for value in {value_expr}:"));
            lines.push(format!("{indent}    item = message.{proto_name}.add()"));
            lines.push(format!(
                "{indent}    item.CopyFrom({})",
                resolved_type
                    .message_conversion
                    .as_ref()
                    .expect("message conversion should be present")
                    .to_wire_expr("value")
            ));
        }
        ResolvedFieldKind::Enum => lines.push(format!(
            "{indent}message.{proto_name}.extend({} for value in {value_expr})",
            enum_to_proto_expr(resolved_type, "value")
        )),
        _ => lines.push(format!("{indent}message.{proto_name}.extend({value_expr})")),
    }
    lines
}

fn map_value_to_proto_lines(
    map_value_type: &ResolvedFieldType,
    value_expr: &str,
    proto_name: &str,
    optional_guard: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if optional_guard {
        lines.push(format!("if {value_expr}:"));
    }
    let indent = if optional_guard { "    " } else { "" };
    match map_value_type.kind {
        ResolvedFieldKind::Message => {
            lines.push(format!("{indent}for key, value in {value_expr}.items():"));
            lines.push(format!(
                "{indent}    message.{proto_name}[key].CopyFrom({})",
                map_value_type
                    .message_conversion
                    .as_ref()
                    .expect("message conversion should be present")
                    .to_wire_expr("value")
            ));
        }
        ResolvedFieldKind::Enum => lines.push(format!(
            "{indent}message.{proto_name}.update({{key: {} for key, value in {value_expr}.items()}})",
            enum_to_proto_expr(map_value_type, "value")
        )),
        _ => lines.push(format!("{indent}message.{proto_name}.update({value_expr})")),
    }
    lines
}

fn value_to_proto_lines(
    resolved_type: &ResolvedFieldType,
    value_expr: &str,
    proto_name: &str,
    optional_guard: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if optional_guard {
        lines.push(format!("if {value_expr} is not None:"));
    }
    let indent = if optional_guard { "    " } else { "" };
    match resolved_type.kind {
        ResolvedFieldKind::Message => lines.push(format!(
            "{indent}message.{proto_name}.CopyFrom({})",
            resolved_type
                .message_conversion
                .as_ref()
                .expect("message conversion should be present")
                .to_wire_expr(value_expr)
        )),
        ResolvedFieldKind::Enum => lines.push(format!(
            "{indent}message.{proto_name} = {}",
            enum_to_proto_expr(resolved_type, value_expr)
        )),
        _ => lines.push(format!("{indent}message.{proto_name} = {value_expr}")),
    }
    lines
}

fn enum_from_proto_expr(resolved_type: &ResolvedFieldType, expr: &str) -> String {
    if let Some(enum_conversion) = &resolved_type.enum_conversion {
        enum_conversion.from_wire_expr(expr)
    } else {
        format!("{}({expr})", resolved_type.annotation)
    }
}

fn enum_to_proto_expr(resolved_type: &ResolvedFieldType, expr: &str) -> String {
    if let Some(enum_conversion) = &resolved_type.enum_conversion {
        enum_conversion.to_wire_expr(expr)
    } else {
        format!("int({expr})")
    }
}

pub(crate) fn python_default_from_proto_name(name: &str) -> String {
    format!(
        "{}_from_proto",
        name.rsplit('.')
            .next()
            .expect("converter name source should have a final segment")
            .to_snake_case()
    )
}

pub(crate) fn python_default_to_proto_name(name: &str) -> String {
    format!(
        "{}_to_proto",
        name.rsplit('.')
            .next()
            .expect("converter name source should have a final segment")
            .to_snake_case()
    )
}

pub(crate) fn python_from_proto_converter(name: &str, replacement: &TypeReplacementSpec) -> String {
    replacement
        .from_proto
        .for_language(Language::Python)
        .map(str::to_string)
        .unwrap_or_else(|| python_default_from_proto_name(name))
}

pub(crate) fn python_to_proto_converter(name: &str, replacement: &TypeReplacementSpec) -> String {
    replacement
        .to_proto
        .for_language(Language::Python)
        .map(str::to_string)
        .unwrap_or_else(|| python_default_to_proto_name(name))
}

pub(crate) fn python_replacement_type_name(replacement: &TypeReplacementSpec) -> Option<String> {
    replacement
        .type_name
        .for_language(Language::Python)
        .map(str::to_string)
}

pub(in crate::generator) fn render_model_proto_methods(
    output: &mut String,
    model: &RenderedModel,
) -> bool {
    let mut wrote_method = false;
    if model.capabilities.from_wire {
        let proto_ref = model
            .proto_ref
            .as_deref()
            .expect("from_proto models should have a proto reference");
        if !model.fields.is_empty() {
            output.push('\n');
        }
        output.push_str("    @classmethod\n");
        output.push_str("    def from_proto(\n");
        output.push_str("        cls,\n");
        output.push_str(if model.fields.is_empty() {
            "        _proto: "
        } else {
            "        proto: "
        });
        output.push_str(proto_ref);
        output.push_str(",\n");
        output.push_str("    ) -> ");
        output.push_str(&model.name);
        output.push_str(":\n");
        if model.fields.is_empty() {
            output.push_str("        return cls()\n");
        } else {
            for field in &model.fields {
                for line in &field.from_wire.setup_lines {
                    output.push_str("        ");
                    output.push_str(line);
                    output.push('\n');
                }
            }

            output.push_str("        return cls(\n");
            for field in &model.fields {
                output.push_str("            ");
                output.push_str(&field.attr_name);
                output.push_str("=");
                output.push_str(&field.from_wire.expr);
                output.push_str(",\n");
            }
            output.push_str("        )\n");
        }
        wrote_method = true;
    }
    if model.capabilities.to_wire {
        let proto_ref = model
            .proto_ref
            .as_deref()
            .expect("to_proto models should have a proto reference");
        if model.fields.is_empty() {
            if wrote_method {
                output.push('\n');
            }
        } else {
            output.push('\n');
            if wrote_method {
                output.push('\n');
            }
        }
        output.push_str("    def to_proto(self) -> ");
        output.push_str(proto_ref);
        output.push_str(":\n");
        output.push_str("        message = ");
        output.push_str(proto_ref);
        output.push_str("()\n");
        for field in &model.fields {
            for line in &field.to_wire.lines {
                output.push_str("        ");
                output.push_str(line);
                output.push('\n');
            }
        }
        for field in &model.sourced_fields {
            for line in &field.to_wire.lines {
                output.push_str("        ");
                output.push_str(line);
                output.push('\n');
            }
        }
        output.push_str("        return message\n");
        wrote_method = true;
    }
    wrote_method
}

pub(in crate::generator) fn render_models(models: &[&RenderedModel]) -> RenderedModelFragments {
    let mut body = String::new();
    for (index, model) in models.iter().enumerate() {
        render_model(&mut body, model);
        if index + 1 != models.len() {
            body.push_str("\n\n");
        }
    }

    let mut registrations = String::new();
    render_nexus_type_registrations(&mut registrations, models);

    let mut module_imports = BTreeSet::new();
    if models.iter().any(|model| model.native) {
        module_imports.insert("nex_gen_runtime".to_string());
    }
    for model in models {
        if let Some(proto_module_path) = &model.proto_module_path {
            module_imports.insert(proto_module_path.clone());
        }
        for field in &model.fields {
            module_imports.extend(field.imports.module_imports.iter().cloned());
        }
    }

    RenderedModelFragments {
        body,
        registrations,
        module_imports,
        exported_names: models.iter().map(|model| model.name.clone()).collect(),
    }
}

fn render_model(output: &mut String, model: &RenderedModel) {
    if model_needs_keyword_only_dataclass(model) {
        output.push_str("@dataclasses.dataclass(slots=True, kw_only=True)\n");
    } else {
        output.push_str("@dataclasses.dataclass(slots=True)\n");
    }
    output.push_str("class ");
    output.push_str(&model.name);
    output.push_str(":\n");
    render_python_docstring(output, "    ", None, &[], None, model.experimental);

    if model.fields.is_empty() {
        if !render_model_proto_methods(output, model) {
            output.push_str("    pass\n");
        }
        return;
    }

    for field in &model.fields {
        output.push_str("    ");
        output.push_str(&field.attr_name);
        output.push_str(": ");
        output.push_str(&python_parameter_annotation(
            &field.annotation,
            &field.default_kind,
        ));
        if let Some(default_expr) = &field.default_expr {
            render_python_default_expr(output, default_expr, "    ");
        }
        output.push('\n');
    }

    render_model_proto_methods(output, model);
}

fn model_needs_keyword_only_dataclass(model: &RenderedModel) -> bool {
    let mut saw_defaulted_field = false;
    for field in &model.fields {
        if field.default_expr.is_some() {
            saw_defaulted_field = true;
        } else if saw_defaulted_field {
            return true;
        }
    }
    false
}

fn render_nexus_type_registrations(output: &mut String, models: &[&RenderedModel]) {
    for model in models.iter().filter(|model| model.native) {
        output.push_str("nex_gen_runtime.register_nexus_type(");
        output.push_str(&model.name);
        output.push_str(", ");
        output.push_str(&python_string_literal(&model.full_name));
        output.push_str(")\n");
    }
}

fn python_module_path_for_file_name(file_name: Option<&str>, package: &str) -> Option<String> {
    if let Some(file_name) = file_name {
        let mut module_path = file_name.replace('/', ".");
        if let Some(stripped) = module_path.strip_suffix(".proto") {
            module_path = format!("{stripped}_pb2");
        }
        if let Some(suffix) = module_path.strip_prefix("temporal.") {
            module_path = format!("temporalio.{suffix}");
        }
        return Some(module_path);
    }

    if package.is_empty() {
        None
    } else if let Some(suffix) = package.strip_prefix("temporal.") {
        Some(format!("temporalio.{suffix}"))
    } else {
        Some(package.to_string())
    }
}

fn python_module_path(reference: &str) -> &str {
    reference
        .rsplit_once('.')
        .map(|(module, _)| module)
        .unwrap_or(reference)
}
