use heck::ToSnakeCase;

use crate::error::Result;
use crate::generator::ExternalModelBackend;
use crate::generator::python::{
    PythonFieldDefaultKind, PythonImports, RenderedField, RenderedModel, RenderedModelFragments,
    RenderedRecordWireBlock, ResolvedFieldKind, ResolvedFieldType, WireValueConversion,
    python_authored_type_annotation, python_string_literal,
};
use crate::language::Language;
use crate::planning::{
    PlannedProtoType, PlannedProtoTypeInfo, PlannedSpec, PlannedType, PlannedTypeFamily,
    relative_descriptor_name,
};
use crate::spec::{ExternalTypeSpec, RecordFieldSpec, RecordSpec, TypeReplacementSpec};

#[derive(Debug)]
struct RenderedWireRead {
    setup_lines: Vec<String>,
    expr: String,
}

#[derive(Debug)]
struct RenderedWireWrite {
    lines: Vec<String>,
}

enum WireReadPolicy {
    Required { missing_error: String },
    Optional,
    Default { default_expr: String },
}

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelBackend;

impl ExternalModelBackend for ModelBackend {
    type ModelFragments = RenderedModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, _api_plan: &PlannedSpec) -> Result<()> {
        Ok(())
    }

    fn render_models(&self) -> Result<RenderedModelFragments> {
        Ok(RenderedModelFragments::default())
    }

    fn model_type_annotation(&self, model_type: &PlannedType) -> Option<String> {
        match model_type {
            PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(message))) => {
                message_python_ref(&message.proto).map(|reference| reference.type_ref)
            }
            PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Enum(enumeration))) => {
                enumeration
                    .replacement
                    .as_ref()
                    .and_then(python_replacement_type_name)
                    .or_else(|| Some(enumeration.name.clone()))
            }
            PlannedType::Record(record) => Some(record.model_name.clone()),
            _ => None,
        }
    }

    fn wire_type_identifier(&self, model_type: &PlannedType) -> Option<String> {
        match model_type {
            PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(message))) => {
                Some(message.proto.full_name.clone())
            }
            PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Enum(_))) => None,
            PlannedType::Record(record) => Some(record.full_name.clone()),
            _ => None,
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
    pub(in crate::generator) fn render_record_wire_block(
        &self,
        model: &RenderedModel,
        planned_model: &RecordSpec<PlannedTypeFamily>,
    ) -> Option<RenderedRecordWireBlock> {
        render_record_wire_block(model, planned_model)
    }

    pub(in crate::generator) fn service_wire_model_ref(
        &self,
        model_type: &PlannedType,
    ) -> Option<PythonReference> {
        external_message_python_ref(model_type)
    }

    pub(in crate::generator) fn enum_field_type(
        &self,
        value_type: &PlannedType,
    ) -> Option<ResolvedFieldType> {
        enum_field_type(value_type)
    }
}

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

fn record_python_ref(planned_model: &RecordSpec<PlannedTypeFamily>) -> Option<PythonReference> {
    planned_model
        .data
        .proto
        .as_ref()
        .and_then(message_python_ref)
}

fn message_override_conversion(model_type: &PlannedType) -> Option<WireValueConversion> {
    let PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(proto))) =
        model_type
    else {
        return None;
    };
    if let Some(language_override) = &proto.replacement
        && let Some(type_name) = python_replacement_type_name(language_override)
    {
        let from_proto = python_from_proto_converter(&proto.proto.full_name, language_override);
        let to_proto = python_to_proto_converter(&proto.proto.full_name, language_override);
        return Some(WireValueConversion {
            annotation: type_name,
            from_wire: format!("{from_proto}({{wire}}, payload_converter=payload_converter)"),
            to_wire: format!("{to_proto}({{value}}, payload_converter=payload_converter)"),
            imports: PythonImports::default(),
            supports_unpacked_input: false,
        });
    }
    if let Some(authored_type) = &proto.authored_type {
        let from_proto = python_default_from_proto_name(&proto.proto.full_name);
        let to_proto = python_default_to_proto_name(&proto.proto.full_name);
        return Some(WireValueConversion {
            annotation: python_authored_type_annotation(authored_type),
            from_wire: format!("{from_proto}({{wire}}, payload_converter=payload_converter)"),
            to_wire: format!("{to_proto}({{value}}, payload_converter=payload_converter)"),
            imports: PythonImports::default(),
            supports_unpacked_input: false,
        });
    }
    None
}

fn generated_message_model_name(
    model_type: &PlannedType,
    planned_model: &RecordSpec<PlannedTypeFamily>,
) -> Option<String> {
    if planned_model.data.proto.is_some() {
        return Some(planned_model.name.clone());
    }
    let PlannedType::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(proto))) =
        model_type
    else {
        return None;
    };
    Some(proto.model_name.clone())
}

fn generated_wire_conversion(
    model_type: &PlannedType,
    planned_model: &RecordSpec<PlannedTypeFamily>,
) -> Option<WireValueConversion> {
    if let Some(model_name) = generated_message_model_name(model_type, planned_model) {
        return Some(WireValueConversion {
            annotation: model_name.clone(),
            from_wire: format!(
                "{model_name}.from_proto({{wire}}, payload_converter=payload_converter)"
            ),
            to_wire: "{value}.to_proto(payload_converter=payload_converter)".to_string(),
            imports: PythonImports::default(),
            supports_unpacked_input: true,
        });
    }
    match model_type {
        PlannedType::Record(record) => Some(WireValueConversion {
            annotation: record.model_name.clone(),
            from_wire: "{wire}".to_string(),
            to_wire: "{value}".to_string(),
            imports: PythonImports::default(),
            supports_unpacked_input: true,
        }),
        _ => None,
    }
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
        wire_conversion: enum_wire_conversion(value_type),
    })
}

fn enum_wire_conversion(value_type: &PlannedType) -> Option<WireValueConversion> {
    let proto_enum = enum_value(value_type)?;
    let conversion = proto_enum.conversion?;
    Some(WireValueConversion {
        annotation: proto_enum.annotation,
        from_wire: format!("{}({{wire}})", conversion.from_proto),
        to_wire: format!("{}({{value}})", conversion.to_proto),
        imports: PythonImports::default(),
        supports_unpacked_input: false,
    })
}

fn field_read(
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
            expr: optional_from_proto_expr(field, resolved_value_type, proto_name, value_expr),
        },
        WireReadPolicy::Default { default_expr } => RenderedWireRead {
            setup_lines: Vec::new(),
            expr: defaulted_from_proto_expr(field, proto_name, value_expr, default_expr),
        },
    }
}

fn field_write(
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

fn function_field_write(
    proto_name: &str,
    _field: &RecordFieldSpec<PlannedTypeFamily>,
    value_expr: &str,
    converter: &str,
    resolved_type: &ResolvedFieldType,
    optional_guard: bool,
) -> RenderedWireWrite {
    let converted_value = format!("{converter}({value_expr}, payload_converter=payload_converter)");
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
    resolved_type: &ResolvedFieldType,
    proto_name: &str,
    value_expr: String,
) -> String {
    if field_has_proto_presence(field) {
        format!("{value_expr} if proto.HasField(\"{proto_name}\") else None")
    } else if let Some(present_expr) =
        no_presence_default_value_present_expr(field, resolved_type, proto_name)
    {
        format!("{value_expr} if {present_expr} else None")
    } else {
        value_expr
    }
}

fn no_presence_default_value_present_expr(
    field: &RecordFieldSpec<PlannedTypeFamily>,
    resolved_type: &ResolvedFieldType,
    proto_name: &str,
) -> Option<String> {
    match resolved_type.kind {
        ResolvedFieldKind::Enum => Some(format!("proto.{proto_name} != 0")),
        ResolvedFieldKind::Scalar => match field.field_type.without_option() {
            PlannedType::Bool | PlannedType::String | PlannedType::Bytes => {
                Some(format!("bool(proto.{proto_name})"))
            }
            PlannedType::Int(_) | PlannedType::Float => Some(format!("proto.{proto_name} != 0")),
            _ => None,
        },
        _ => None,
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
                .wire_conversion
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
                .wire_conversion
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
            .wire_conversion
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
                    .wire_conversion
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
                    .wire_conversion
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
                .wire_conversion
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
    if let Some(enum_conversion) = &resolved_type.wire_conversion {
        enum_conversion.from_wire_expr(expr)
    } else {
        format!("{}({expr})", resolved_type.annotation)
    }
}

fn enum_to_proto_expr(resolved_type: &ResolvedFieldType, expr: &str) -> String {
    if let Some(enum_conversion) = &resolved_type.wire_conversion {
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

fn render_record_wire_block(
    model: &RenderedModel,
    planned_model: &RecordSpec<PlannedTypeFamily>,
) -> Option<RenderedRecordWireBlock> {
    if !model.capabilities.from_wire && !model.capabilities.to_wire {
        return None;
    }

    let proto_ref = record_python_ref(planned_model)?;
    let mut output = String::new();
    let mut wrote_method = false;
    if model.capabilities.from_wire {
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
        output.push_str(&proto_ref.type_ref);
        output.push_str(",\n");
        output.push_str("        *,\n");
        output.push_str(
            "        payload_converter: temporalio.converter.PayloadConverter | None = None,\n",
        );
        output.push_str("    ) -> ");
        output.push_str(&model.name);
        output.push_str(":\n");
        output.push_str("        _ = payload_converter\n");
        if model.fields.is_empty() {
            output.push_str("        return cls()\n");
        } else {
            for ((field_name, planned_field), rendered_field) in planned_model
                .fields
                .iter()
                .filter(|(_, field)| {
                    field.visibility != crate::spec::RecordFieldVisibility::Omitted
                })
                .map(|(name, field)| (name.as_str(), field))
                .zip(model.fields.iter())
            {
                let read = field_read(
                    field_name,
                    &rendered_field.attr_name,
                    planned_field,
                    &rendered_field.wire_value_type,
                    field_read_policy(&model.name, rendered_field),
                );
                for line in &read.setup_lines {
                    output.push_str("        ");
                    output.push_str(line);
                    output.push('\n');
                }
            }

            output.push_str("        return cls(\n");
            for ((field_name, planned_field), rendered_field) in planned_model
                .fields
                .iter()
                .filter(|(_, field)| {
                    field.visibility != crate::spec::RecordFieldVisibility::Omitted
                })
                .map(|(name, field)| (name.as_str(), field))
                .zip(model.fields.iter())
            {
                let read = field_read(
                    field_name,
                    &rendered_field.attr_name,
                    planned_field,
                    &rendered_field.wire_value_type,
                    field_read_policy(&model.name, rendered_field),
                );
                output.push_str("            ");
                output.push_str(&rendered_field.attr_name);
                output.push_str("=");
                output.push_str(&read.expr);
                output.push_str(",\n");
            }
            output.push_str("        )\n");
        }
        wrote_method = true;
    }
    if model.capabilities.from_wire {
        output.push('\n');
        output.push_str("    @classmethod\n");
        output.push_str("    def _temporal_from_wire(\n");
        output.push_str("        cls,\n");
        output.push_str("        wire: ");
        output.push_str(&proto_ref.type_ref);
        output.push_str(",\n");
        output.push_str("        *,\n");
        output.push_str(
            "        payload_converter: temporalio.converter.PayloadConverter | None = None,\n",
        );
        output.push_str("    ) -> ");
        output.push_str(&model.name);
        output.push_str(":\n");
        output
            .push_str("        return cls.from_proto(wire, payload_converter=payload_converter)\n");
        wrote_method = true;
    }
    if model.capabilities.to_wire {
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
        output.push_str("    def to_proto(\n");
        output.push_str("        self,\n");
        output.push_str("        *,\n");
        output.push_str(
            "        payload_converter: temporalio.converter.PayloadConverter | None = None,\n",
        );
        output.push_str("    ) -> ");
        output.push_str(&proto_ref.type_ref);
        output.push_str(":\n");
        output.push_str("        _ = payload_converter\n");
        output.push_str("        message = ");
        output.push_str(&proto_ref.type_ref);
        output.push_str("()\n");
        for ((field_name, planned_field), rendered_field) in planned_model
            .fields
            .iter()
            .filter(|(_, field)| field.visibility != crate::spec::RecordFieldVisibility::Omitted)
            .map(|(name, field)| (name.as_str(), field))
            .zip(model.fields.iter())
        {
            let value_expr = format!("self.{}", rendered_field.attr_name);
            let write = field_write_for_rendered_field(
                field_name,
                planned_field,
                rendered_field,
                &value_expr,
            );
            for line in &write.lines {
                output.push_str("        ");
                output.push_str(line);
                output.push('\n');
            }
        }
        output.push_str("        return message\n");

        output.push('\n');
        output.push_str("    def _temporal_to_wire(\n");
        output.push_str("        self,\n");
        output.push_str("        *,\n");
        output.push_str(
            "        payload_converter: temporalio.converter.PayloadConverter | None = None,\n",
        );
        output.push_str("    ) -> ");
        output.push_str(&proto_ref.type_ref);
        output.push_str(":\n");
        output.push_str("        return self.to_proto(payload_converter=payload_converter)\n");
    }

    Some(RenderedRecordWireBlock {
        imports: PythonImports {
            module_imports: [proto_ref.module_path, "temporalio.converter".to_string()]
                .into_iter()
                .collect(),
            ..PythonImports::default()
        },
        class_body_lines: output.lines().map(str::to_string).collect(),
    })
}

fn field_read_policy(model_name: &str, rendered_field: &RenderedField) -> WireReadPolicy {
    match &rendered_field.default_kind {
        PythonFieldDefaultKind::Required => {
            let missing_error = python_string_literal(&format!(
                "missing required field {model_name}.{}",
                rendered_field.attr_name
            ));
            WireReadPolicy::Required { missing_error }
        }
        PythonFieldDefaultKind::None => WireReadPolicy::Optional,
        PythonFieldDefaultKind::EmptyDict => WireReadPolicy::Default {
            default_expr: "{}".to_string(),
        },
        PythonFieldDefaultKind::EmptyList => WireReadPolicy::Default {
            default_expr: "[]".to_string(),
        },
        PythonFieldDefaultKind::Expression(default_expr) => WireReadPolicy::Default {
            default_expr: default_expr.clone(),
        },
    }
}

fn field_write_for_rendered_field(
    field_name: &str,
    planned_field: &RecordFieldSpec<PlannedTypeFamily>,
    rendered_field: &RenderedField,
    value_expr: &str,
) -> RenderedWireWrite {
    let optional_guard = matches!(
        rendered_field.default_kind,
        PythonFieldDefaultKind::None
            | PythonFieldDefaultKind::EmptyDict
            | PythonFieldDefaultKind::EmptyList
    );
    let converter = planned_field
        .function
        .as_ref()
        .and_then(|function| function.converter.as_deref())
        .filter(|_| {
            !matches!(
                planned_field.field_type,
                PlannedType::Map(_, _) | PlannedType::List(_)
            ) && planned_field.default_value.is_none()
        });
    match converter {
        Some(converter) => function_field_write(
            field_name,
            planned_field,
            value_expr,
            converter,
            &rendered_field.wire_value_type,
            optional_guard,
        ),
        None => field_write(
            field_name,
            planned_field,
            value_expr,
            &rendered_field.wire_value_type,
            optional_guard,
        ),
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
