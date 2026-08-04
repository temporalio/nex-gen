use std::collections::BTreeSet;
use std::path::PathBuf;

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Number, Value};

use crate::error::{Error, Result};
use crate::generator::ExternalModelBackend;
use crate::generator::dotnet::{
    WireValueConversion, csharp_parameter_name, csharp_string_literal, csharp_type_name,
};
use crate::planning::{PlannedJsonType, PlannedSpec, PlannedTypeFamily};
use crate::spec::{ExternalTypeSpec, RecordSpec};

const GENERATED_CODE_ATTRIBUTE: &str = "[GeneratedCode(\"nex-gen\", null)]";

#[derive(Debug, Deserialize, Default)]
struct Schema {
    #[serde(rename = "$ref")]
    reference: Option<String>,
    #[serde(rename = "type")]
    ty: Option<Value>,
    description: Option<String>,
    properties: Option<IndexMap<String, Schema>>,
    required: Option<Vec<String>>,
    #[serde(rename = "additionalProperties")]
    additional_properties: Option<Value>,
    items: Option<Box<Schema>>,
    #[serde(rename = "oneOf")]
    one_of: Option<Vec<Schema>>,
    default: Option<Value>,
    #[serde(rename = "const")]
    const_value: Option<Value>,
    #[serde(rename = "maxProperties")]
    max_properties: Option<usize>,
    // Numeric bounds. Kept as `serde_json::Number` so an integral bound renders
    // without a spurious `.0` and a fractional one keeps its precision, matching
    // how Go's `%v` prints the same bound.
    minimum: Option<Number>,
    maximum: Option<Number>,
    #[serde(rename = "exclusiveMinimum")]
    exclusive_minimum: Option<Number>,
    #[serde(rename = "exclusiveMaximum")]
    exclusive_maximum: Option<Number>,
    #[serde(rename = "multipleOf")]
    multiple_of: Option<Number>,
}

impl Schema {
    /// The numeric bounds declared on this schema, in the order Go emits them so
    /// a multi-violation payload lists them identically across targets.
    fn numeric_bounds(&self) -> Vec<NumericBound<'_>> {
        [
            (NumericBoundKind::Minimum, self.minimum.as_ref()),
            (NumericBoundKind::Maximum, self.maximum.as_ref()),
            (
                NumericBoundKind::ExclusiveMinimum,
                self.exclusive_minimum.as_ref(),
            ),
            (
                NumericBoundKind::ExclusiveMaximum,
                self.exclusive_maximum.as_ref(),
            ),
            (NumericBoundKind::MultipleOf, self.multiple_of.as_ref()),
        ]
        .into_iter()
        .filter_map(|(kind, bound)| bound.map(|bound| NumericBound { kind, bound }))
        .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericBoundKind {
    Minimum,
    Maximum,
    ExclusiveMinimum,
    ExclusiveMaximum,
    MultipleOf,
}

#[derive(Debug)]
struct NumericBound<'a> {
    kind: NumericBoundKind,
    bound: &'a Number,
}

impl NumericBound<'_> {
    /// The C# boolean expression that is true when `value_expr` **violates** the
    /// bound.
    fn violation_condition(&self, value_expr: &str) -> String {
        let bound = self.bound;
        match self.kind {
            NumericBoundKind::Minimum => format!("{value_expr} < {bound}"),
            NumericBoundKind::Maximum => format!("{value_expr} > {bound}"),
            NumericBoundKind::ExclusiveMinimum => format!("{value_expr} <= {bound}"),
            NumericBoundKind::ExclusiveMaximum => format!("{value_expr} >= {bound}"),
            // `%` on a double is exact for the values the spec-number cap admits,
            // and `multipleOf` bounds are themselves exact in binary far more
            // often than not; a remainder test matches Go's `math.Mod` check.
            NumericBoundKind::MultipleOf => format!("{value_expr} % {bound} != 0"),
        }
    }

    /// The violation reason, worded exactly as Go's equivalent so the same
    /// payload produces the same diagnostic text on every target.
    fn reason_format(&self) -> String {
        let bound = self.bound;
        match self.kind {
            NumericBoundKind::Minimum => format!("must be >= {bound}, got "),
            NumericBoundKind::Maximum => format!("must be <= {bound}, got "),
            NumericBoundKind::ExclusiveMinimum => format!("must be > {bound}, got "),
            NumericBoundKind::ExclusiveMaximum => format!("must be < {bound}, got "),
            NumericBoundKind::MultipleOf => format!("must be a multiple of {bound}, got "),
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelBackend {
    json_models: Vec<PlannedJsonType>,
}

#[derive(Debug, Default)]
pub(in crate::generator) struct RenderedModelFragments {
    pub(in crate::generator) body: String,
}

impl RenderedModelFragments {
    pub(in crate::generator) fn has_models(&self) -> bool {
        !self.body.is_empty()
    }
}

impl ExternalModelBackend<PlannedJsonType> for ModelBackend {
    type ModelFragments = RenderedModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.json_models = api_plan
            .external_types()
            .map(|(_, binding)| binding)
            .filter_map(|binding| match &binding.external_type {
                ExternalTypeSpec::Json(json_type) => Some(json_type.clone()),
                _ => None,
            })
            .collect();
        Ok(())
    }

    fn render_models(&self) -> Result<RenderedModelFragments> {
        render_external_models(&self.json_models)
    }

    fn model_type_annotation(&self, json_type: &PlannedJsonType) -> Option<String> {
        Some(model_type_ref(json_type))
    }

    fn wire_type_identifier(&self, json_type: &PlannedJsonType) -> Option<String> {
        Some(json_type.full_name.clone())
    }

    fn wire_conversion(
        &self,
        json_type: &PlannedJsonType,
        _planned_record: Option<&RecordSpec<PlannedTypeFamily>>,
    ) -> Option<WireValueConversion> {
        Some(WireValueConversion {
            annotation: model_type_ref(json_type),
            to_wire: "{value}".to_string(),
        })
    }
}

fn model_type_ref(json_type: &PlannedJsonType) -> String {
    csharp_type_name(&json_type.model_name)
}

fn render_external_models(json_models: &[PlannedJsonType]) -> Result<RenderedModelFragments> {
    if json_models.is_empty() {
        return Ok(RenderedModelFragments::default());
    }

    let mut output = String::new();
    for (index, model) in json_models.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        render_model(&mut output, model)?;
    }
    Ok(RenderedModelFragments { body: output })
}

fn render_model(output: &mut String, model: &PlannedJsonType) -> Result<()> {
    let schema = decode_schema(model)?;
    render_xml_summary(output, "", schema.description.as_deref());
    if !model_needs_extension_data(&schema)? && !is_open_object(&schema) {
        output.push_str("[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]\n");
    }
    output.push_str(GENERATED_CODE_ATTRIBUTE);
    output.push('\n');
    output.push_str("public class ");
    output.push_str(&model_type_ref(model));
    if model_needs_on_deserialized(&schema)? {
        output.push_str(" : IJsonOnDeserialized");
    }
    output.push_str("\n{\n");

    if typed_map_value_schema(&schema)?.is_none() {
        render_model_constructor(output, model, &schema)?;
        render_model_properties(output, &schema)?;
    }
    render_extension_data_property(output, &schema)?;
    render_constraint_validator(output, &schema)?;
    render_model_validation(output, &schema)?;

    output.push_str("}\n\n");
    Ok(())
}

/// Emits the constraint validator: a public `Validate()` that aggregates every
/// violation into one [`ValidationException`], plus the `CollectViolations` worker
/// it and any containing model share.
///
/// Two entry points because the contract has to hold in both wire directions.
/// `OnDeserialized` calls `Validate()` so an inbound payload can never enter the
/// process in a shape the contract forbids; `Validate()` is public so the service
/// binding can call it before serializing an outbound value. `CollectViolations`
/// takes a path prefix so a nested model reports `page.blocks.order` rather than
/// a bare `order`.
fn render_constraint_validator(output: &mut String, schema: &Schema) -> Result<()> {
    let constrained = constrained_members(schema);
    if constrained.is_empty() {
        return Ok(());
    }

    output.push('\n');
    output.push_str("    /// <summary>\n");
    output.push_str(
        "    /// Validates every constraint the contract declares on this type, throwing a\n",
    );
    output.push_str(
        "    /// single <see cref=\"ValidationException\"/> carrying all violations rather\n",
    );
    output.push_str("    /// than stopping at the first.\n");
    output.push_str("    /// </summary>\n");
    output.push_str("    public void Validate()\n    {\n");
    output.push_str("        var violations = new List<Violation>();\n");
    output.push_str("        CollectViolations(violations, string.Empty);\n");
    output.push_str("        if (violations.Count > 0)\n        {\n");
    output.push_str("            throw new ValidationException(violations);\n");
    output.push_str("        }\n");
    output.push_str("    }\n\n");

    output.push_str(
        "    internal void CollectViolations(List<Violation> violations, string path)\n    {\n",
    );
    for member in &constrained {
        render_member_constraints(output, member);
    }
    output.push_str("    }\n");
    Ok(())
}

/// A member carrying at least one enforceable constraint, paired with how its
/// value is reached in C#.
struct ConstrainedMember<'a> {
    json_name: &'a str,
    /// The C# expression for the member's value inside `CollectViolations`.
    accessor: String,
    /// True when the member is optional or nullable, so the check has to be
    /// guarded against the absent case.
    needs_null_guard: bool,
    numeric_type: &'static str,
    bounds: Vec<NumericBound<'a>>,
}

fn constrained_members(schema: &Schema) -> Vec<ConstrainedMember<'_>> {
    let required = required_fields(schema);
    let Some(properties) = &schema.properties else {
        return Vec::new();
    };
    properties
        .iter()
        .filter_map(|(json_name, property)| {
            let bounds = property.numeric_bounds();
            if bounds.is_empty() {
                return None;
            }
            let is_required = required.contains(json_name.as_str());
            Some(ConstrainedMember {
                json_name,
                accessor: csharp_type_name(json_name),
                needs_null_guard: !is_required || allows_null(property),
                numeric_type: numeric_clr_type(property),
                bounds,
            })
        })
        .collect()
}

/// The CLR type a numeric member's value binds to when unwrapped from its
/// nullable form — `long` for `type: integer`, `double` otherwise.
fn numeric_clr_type(schema: &Schema) -> &'static str {
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("integer") => "long",
        _ => "double",
    }
}

fn render_member_constraints(output: &mut String, member: &ConstrainedMember<'_>) {
    // An optional member is `long?`/`double?`; bind it once so each bound reads
    // the unwrapped value, and skip every check when it is absent.
    let (indent, value_expr) = if member.needs_null_guard {
        let local = csharp_parameter_name(&format!("{}-value", member.json_name));
        output.push_str("        if (");
        output.push_str(&member.accessor);
        output.push_str(" is ");
        output.push_str(member.numeric_type);
        output.push(' ');
        output.push_str(&local);
        output.push_str(")\n        {\n");
        ("            ", local)
    } else {
        ("        ", member.accessor.clone())
    };

    for bound in &member.bounds {
        output.push_str(indent);
        output.push_str("if (");
        output.push_str(&bound.violation_condition(&value_expr));
        output.push_str(")\n");
        output.push_str(indent);
        output.push_str("{\n");
        output.push_str(indent);
        output.push_str("    violations.Add(new Violation(JsonRuntime.JoinPath(path, ");
        output.push_str(&csharp_string_literal(member.json_name));
        output.push_str("), ");
        output.push_str(&csharp_string_literal(&bound.reason_format()));
        output.push_str(" + JsonRuntime.FormatNumber(");
        output.push_str(&value_expr);
        output.push_str(")));\n");
        output.push_str(indent);
        output.push_str("}\n");
    }

    if member.needs_null_guard {
        output.push_str("        }\n");
    }
}

fn render_model_constructor(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
) -> Result<()> {
    let required = required_fields(schema);
    let required_properties = schema
        .properties
        .as_ref()
        .map(|properties| {
            properties
                .iter()
                .filter(|(name, property)| {
                    required.contains(name.as_str()) && property.const_value.is_none()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if required_properties.is_empty() {
        return Ok(());
    }

    output.push_str("    public ");
    output.push_str(&model_type_ref(model));
    output.push('(');
    for (index, (json_name, property)) in required_properties.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(&schema_type(property, false)?);
        output.push(' ');
        output.push_str(&csharp_parameter_name(json_name));
    }
    output.push_str(")\n    {\n");
    for (json_name, _) in required_properties {
        output.push_str("        ");
        output.push_str(&csharp_type_name(json_name));
        output.push_str(" = ");
        output.push_str(&csharp_parameter_name(json_name));
        output.push_str(";\n");
    }
    output.push_str("    }\n\n");
    Ok(())
}

fn render_model_properties(output: &mut String, schema: &Schema) -> Result<()> {
    let required = required_fields(schema);
    let Some(properties) = &schema.properties else {
        return Ok(());
    };
    for (json_name, property) in properties {
        render_xml_summary(output, "    ", property.description.as_deref());
        if !required.contains(json_name.as_str()) {
            render_optional_property(output, json_name, property)?;
            continue;
        }
        if property.const_value.is_some() {
            render_const_property(output, json_name, property)?;
            continue;
        }
        output.push_str("    [JsonPropertyName(");
        output.push_str(&csharp_string_literal(json_name));
        output.push_str(")]\n");
        output.push_str("    [JsonRequired]\n");
        output.push_str("    public ");
        output.push_str(&schema_type(property, false)?);
        output.push(' ');
        output.push_str(&csharp_type_name(json_name));
        output.push_str(" { get; init; }\n");
    }
    Ok(())
}

fn render_optional_property(output: &mut String, json_name: &str, property: &Schema) -> Result<()> {
    let property_type = schema_type(property, true)?;
    output.push_str("    [JsonIgnore]\n");
    output.push_str("    public ");
    output.push_str(&property_type);
    output.push(' ');
    output.push_str(&csharp_type_name(json_name));
    output.push_str("\n    {\n");
    output.push_str("        get => JsonRuntime.ReadOptionalValue<");
    output.push_str(&optional_read_type(property, &property_type)?);
    output.push_str(">(AdditionalProperties, ");
    output.push_str(&csharp_string_literal(json_name));
    if let Some(default_value) = property.default.as_ref().and_then(csharp_value_literal) {
        output.push_str(", ");
        output.push_str(&default_value);
    }
    output.push_str(");\n");
    output.push_str("        init\n        {\n");
    if !allows_null(property) {
        output.push_str("            JsonRuntime.RejectNull(");
        output.push_str(&csharp_string_literal(json_name));
        output.push_str(", value);\n");
    }
    output.push_str("            AdditionalProperties[");
    output.push_str(&csharp_string_literal(json_name));
    output.push_str("] = value;\n");
    output.push_str("        }\n");
    output.push_str("    }\n");
    Ok(())
}

fn render_const_property(output: &mut String, json_name: &str, property: &Schema) -> Result<()> {
    let const_value = property
        .const_value
        .as_ref()
        .and_then(csharp_value_literal)
        .expect("const property should have C# literal");
    let field_name = csharp_parameter_name(&format!("{json_name}-value"));
    output.push_str("    private ");
    output.push_str(&schema_type(property, false)?);
    output.push(' ');
    output.push_str(&field_name);
    output.push_str(" = ");
    output.push_str(&const_value);
    output.push_str(";\n\n");
    output.push_str("    [JsonPropertyName(");
    output.push_str(&csharp_string_literal(json_name));
    output.push_str(")]\n");
    output.push_str("    [JsonRequired]\n");
    output.push_str("    public ");
    output.push_str(&schema_type(property, false)?);
    output.push(' ');
    output.push_str(&csharp_type_name(json_name));
    output.push_str("\n    {\n");
    output.push_str("        get => ");
    output.push_str(&field_name);
    output.push_str(";\n");
    output.push_str("        init\n        {\n");
    output.push_str("            if (value != ");
    output.push_str(&const_value);
    output.push_str(")\n            {\n");
    output.push_str("                throw new JsonException(");
    output.push_str(&csharp_string_literal(&format!(
        "{json_name} must equal {const_value}"
    )));
    output.push_str(");\n");
    output.push_str("            }\n");
    output.push_str("            ");
    output.push_str(&field_name);
    output.push_str(" = value;\n");
    output.push_str("        }\n");
    output.push_str("    }\n");
    Ok(())
}

fn optional_read_type(schema: &Schema, property_type: &str) -> Result<String> {
    if matches!(schema.ty.as_ref().and_then(Value::as_str), Some("array")) {
        let item = schema
            .items
            .as_ref()
            .map(|item| schema_base_type(item))
            .transpose()?
            .unwrap_or_else(|| "object".to_string());
        Ok(format!("List<{item}>?"))
    } else {
        Ok(property_type.to_string())
    }
}

fn render_extension_data_property(output: &mut String, schema: &Schema) -> Result<()> {
    if !model_needs_extension_data(schema)? {
        return Ok(());
    }
    if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        output.push('\n');
    }
    output.push_str("    [JsonExtensionData]\n");
    output.push_str("    public Dictionary<string, object?> AdditionalProperties { get; set; } = new Dictionary<string, object?>();\n");
    Ok(())
}

fn render_model_validation(output: &mut String, schema: &Schema) -> Result<()> {
    if !model_needs_on_deserialized(schema)? {
        return Ok(());
    }
    output.push('\n');
    output.push_str("    void IJsonOnDeserialized.OnDeserialized()\n    {\n");
    if let Some(value_schema) = typed_map_value_schema(schema)? {
        if let Some(max_properties) = schema.max_properties {
            output.push_str("        if (AdditionalProperties.Count > ");
            output.push_str(&max_properties.to_string());
            output.push_str(")\n        {\n");
            output.push_str("            throw new JsonException(");
            output.push_str(&csharp_string_literal(&format!(
                "maxProperties: at most {max_properties} entries"
            )));
            output.push_str(");\n        }\n");
        }
        output.push_str("        foreach (var entry in AdditionalProperties)\n        {\n");
        render_extension_value_validation(output, "entry.Key", "entry.Value", &value_schema, 3)?;
        output.push_str("        }\n");
        output.push_str("    }\n");
        return Ok(());
    }

    let optional_fields = optional_fields(schema);
    if !optional_fields.is_empty() {
        if !is_open_object(schema) {
            output.push_str("        foreach (var key in AdditionalProperties.Keys)\n        {\n");
            output.push_str("            if (");
            for (index, (json_name, _)) in optional_fields.iter().enumerate() {
                if index > 0 {
                    output.push_str(" && ");
                }
                output.push_str("key != ");
                output.push_str(&csharp_string_literal(json_name));
            }
            output.push_str(")\n            {\n");
            output.push_str(
                "                throw new JsonException($\"Unknown field `{key}`.\");\n",
            );
            output.push_str("            }\n");
            output.push_str("        }\n");
        }
        for (json_name, property) in optional_fields {
            let value_name = csharp_parameter_name(&format!("{json_name}-value"));
            output.push_str("        if (AdditionalProperties.TryGetValue(");
            output.push_str(&csharp_string_literal(json_name));
            output.push_str(", out var ");
            output.push_str(&value_name);
            output.push_str("))\n        {\n");
            if !allows_null(property) {
                output.push_str("            JsonRuntime.RejectNull(");
                output.push_str(&csharp_string_literal(json_name));
                output.push_str(", ");
                output.push_str(&value_name);
                output.push_str(");\n");
            }
            render_extension_value_validation(
                output,
                &csharp_string_literal(json_name),
                &value_name,
                property,
                3,
            )?;
            output.push_str("        }\n");
        }
    }
    // Structural checks above reject a malformed payload outright; the contract
    // constraints then run so an inbound value cannot enter the process in a shape
    // the contract forbids.
    if !constrained_members(schema).is_empty() {
        output.push_str("        Validate();\n");
    }
    output.push_str("    }\n");
    Ok(())
}

fn render_extension_value_validation(
    output: &mut String,
    path_expr: &str,
    value_expr: &str,
    schema: &Schema,
    indent_level: usize,
) -> Result<()> {
    if allows_null(schema) {
        return Ok(());
    }
    let indent = "    ".repeat(indent_level);
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => {
            let json_name = format!("json{indent_level}");
            output.push_str(&indent);
            output.push_str("if (");
            output.push_str(value_expr);
            output.push_str(" is JsonElement ");
            output.push_str(&json_name);
            output.push_str(" && ");
            output.push_str(&json_name);
            output.push_str(".ValueKind != JsonValueKind.String)\n");
            output.push_str(&indent);
            output.push_str("{\n");
            output.push_str(&indent);
            output.push_str("    throw new JsonException($\"{");
            output.push_str(path_expr);
            output.push_str("}: expected string\");\n");
            output.push_str(&indent);
            output.push_str("}\n");
            output.push_str(&indent);
            output.push_str("else if (");
            output.push_str(value_expr);
            output.push_str(" is not JsonElement && ");
            output.push_str(value_expr);
            output.push_str(" is not string)\n");
            output.push_str(&indent);
            output.push_str("{\n");
            output.push_str(&indent);
            output.push_str("    throw new JsonException($\"{");
            output.push_str(path_expr);
            output.push_str("}: expected string\");\n");
            output.push_str(&indent);
            output.push_str("}\n");
        }
        Some("integer") => {
            output.push_str(&indent);
            output.push_str("_ = JsonRuntime.ReadJsonValue<long?>(");
            output.push_str(value_expr);
            output.push_str(");\n");
        }
        Some("array") => {
            output.push_str(&indent);
            output.push_str("_ = JsonRuntime.ReadJsonValue<");
            output.push_str(&optional_read_type(schema, &schema_type(schema, true)?)?);
            output.push_str(">(");
            output.push_str(value_expr);
            output.push_str(");\n");
        }
        _ if schema.reference.is_some() => {
            output.push_str(&indent);
            output.push_str("_ = JsonRuntime.ReadJsonValue<");
            output.push_str(&optional_read_type(schema, &schema_type(schema, true)?)?);
            output.push_str(">(");
            output.push_str(value_expr);
            output.push_str(");\n");
        }
        _ => {}
    }
    Ok(())
}

fn decode_schema(model: &PlannedJsonType) -> Result<Schema> {
    serde_json::from_value(model.schema.clone()).map_err(|error| Error::InvalidJsonSchema {
        path: PathBuf::from("<json-generator>"),
        reason: format!(
            "failed to read planned JSON schema `{}`: {error}",
            model.full_name
        ),
    })
}

fn typed_map_value_schema(schema: &Schema) -> Result<Option<Schema>> {
    if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        return Ok(None);
    }

    match &schema.additional_properties {
        Some(Value::Object(_)) => serde_json::from_value(
            schema
                .additional_properties
                .clone()
                .expect("additional properties presence checked"),
        )
        .map(Some)
        .map_err(|error| Error::InvalidJsonSchema {
            path: PathBuf::from("<json-generator>"),
            reason: format!("failed to read `additionalProperties`: {error}"),
        }),
        _ => Ok(None),
    }
}

fn model_needs_extension_data(schema: &Schema) -> Result<bool> {
    Ok(typed_map_value_schema(schema)?.is_some()
        || is_open_object(schema)
        || !optional_fields(schema).is_empty())
}

fn model_needs_on_deserialized(schema: &Schema) -> Result<bool> {
    // A model whose only validation is constraint checking still needs the hook,
    // so an inbound payload is validated on deserialize.
    Ok(!constrained_members(schema).is_empty()
        || typed_map_value_schema(schema)?.is_some()
        || (!optional_fields(schema).is_empty()
            && (!is_open_object(schema)
                || optional_fields(schema)
                    .iter()
                    .any(|(_, property)| !allows_null(property)))))
}

fn optional_fields(schema: &Schema) -> Vec<(&str, &Schema)> {
    let required = required_fields(schema);
    schema
        .properties
        .as_ref()
        .map(|properties| {
            properties
                .iter()
                .filter(|(name, _)| !required.contains(name.as_str()))
                .map(|(name, property)| (name.as_str(), property))
                .collect()
        })
        .unwrap_or_default()
}

fn schema_type(schema: &Schema, optional: bool) -> Result<String> {
    let base = schema_base_type(schema)?;
    Ok(if optional { nullable_type(&base) } else { base })
}

fn schema_base_type(schema: &Schema) -> Result<String> {
    if let Some(reference) = &schema.reference {
        return Ok(reference_type_name(reference));
    }
    if let Some(one_of) = &schema.one_of {
        let non_null = one_of
            .iter()
            .filter(|branch| !schema_type_includes(branch, "null"))
            .collect::<Vec<_>>();
        if one_of
            .iter()
            .any(|branch| schema_type_includes(branch, "null"))
            && let Some(branch) = non_null.first()
        {
            return Ok(nullable_type(&schema_base_type(branch)?));
        }
        return Ok("object".to_string());
    }
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => Ok("string".to_string()),
        Some("integer") => Ok("long".to_string()),
        Some("number") => Ok("double".to_string()),
        Some("boolean") => Ok("bool".to_string()),
        Some("array") => {
            let item = schema
                .items
                .as_ref()
                .map(|item| schema_base_type(item))
                .transpose()?
                .unwrap_or_else(|| "object".to_string());
            Ok(format!("IReadOnlyList<{item}>"))
        }
        Some("object") => {
            if let Some(value_schema) = typed_map_value_schema(schema)? {
                Ok(format!(
                    "IReadOnlyDictionary<string, {}>",
                    schema_base_type(&value_schema)?
                ))
            } else {
                Ok("object".to_string())
            }
        }
        Some("null") => Ok("object?".to_string()),
        _ => Ok("object".to_string()),
    }
}

fn nullable_type(base: &str) -> String {
    if base.ends_with('?') {
        base.to_string()
    } else {
        format!("{base}?")
    }
}

fn required_fields(schema: &Schema) -> BTreeSet<&str> {
    schema
        .required
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect()
}

fn is_open_object(schema: &Schema) -> bool {
    !matches!(schema.additional_properties, Some(Value::Bool(false)))
}

fn allows_null(schema: &Schema) -> bool {
    schema_type_includes(schema, "null")
        || schema.one_of.as_ref().is_some_and(|branches| {
            branches
                .iter()
                .any(|branch| schema_type_includes(branch, "null"))
        })
}

fn schema_type_includes(schema: &Schema, ty: &str) -> bool {
    match &schema.ty {
        Some(Value::String(value)) => value == ty,
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|value| value == ty)),
        _ => false,
    }
}

fn reference_model_name(reference: &str) -> String {
    reference
        .rsplit('/')
        .next()
        .unwrap_or(reference)
        .rsplit('#')
        .next()
        .unwrap_or(reference)
        .replace("~1", "/")
        .replace("~0", "~")
}

fn reference_type_name(reference: &str) -> String {
    let target = reference
        .split_once('#')
        .map(|(_, fragment)| fragment)
        .unwrap_or(reference)
        .strip_prefix("/$defs/")
        .map(|name| name.replace("~1", "/").replace("~0", "~"));
    if let Some(target) = target
        && let Some((module_key, model_name)) = target.rsplit_once('#')
        && !module_key.is_empty()
    {
        let namespace = module_key
            .split('/')
            .map(|segment| csharp_type_name(&segment.to_upper_camel_case()))
            .collect::<Vec<_>>()
            .join(".");
        return format!(
            "global::NexGen.Generated.{}.{}",
            namespace,
            csharp_type_name(model_name)
        );
    }
    csharp_type_name(&reference_model_name(reference))
}

fn csharp_value_literal(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(csharp_string_literal(value)),
        Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Null => Some("null".to_string()),
        _ => None,
    }
}

fn render_xml_summary(output: &mut String, indent: &str, summary: Option<&str>) {
    let Some(summary) = summary else {
        return;
    };
    output.push_str(indent);
    output.push_str("/// <summary>\n");
    for line in summary.trim().lines() {
        output.push_str(indent);
        output.push_str("/// ");
        output.push_str(&xml_doc_escape(line.trim()));
        output.push('\n');
    }
    output.push_str(indent);
    output.push_str("/// </summary>\n");
}

fn xml_doc_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
