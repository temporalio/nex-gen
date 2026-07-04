use std::collections::BTreeSet;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
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

impl crate::generator::ModelBackend for ModelBackend {
    type TypeRef = PlannedJsonType;
    type WireType = PlannedJsonType;
    type ModelFragments = RenderedModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.json_models = api_plan
            .external_types
            .values()
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
    output.push_str(GENERATED_CODE_ATTRIBUTE);
    output.push('\n');
    output.push_str("public class ");
    output.push_str(&model_type_ref(model));
    output.push_str("\n{\n");

    if typed_map_value_schema(&schema)?.is_none() {
        render_model_constructor(output, model, &schema)?;
        render_model_properties(output, &schema)?;
    }
    render_extension_data_property(output, &schema)?;

    output.push_str("}\n\n");
    Ok(())
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
        output.push_str("    [JsonPropertyName(");
        output.push_str(&csharp_string_literal(json_name));
        output.push_str(")]\n");
        output.push_str("    public ");
        output.push_str(&schema_type(
            property,
            !required.contains(json_name.as_str()),
        )?);
        output.push(' ');
        output.push_str(&csharp_type_name(json_name));
        output.push_str(" { get; ");
        if required.contains(json_name.as_str()) && property.const_value.is_none() {
            output.push('}');
        } else {
            output.push_str("init; }");
        }
        if let Some(value) = property
            .const_value
            .as_ref()
            .or(property.default.as_ref())
            .and_then(csharp_value_literal)
        {
            output.push_str(" = ");
            output.push_str(&value);
            output.push(';');
        }
        output.push('\n');
    }
    Ok(())
}

fn render_extension_data_property(output: &mut String, schema: &Schema) -> Result<()> {
    if typed_map_value_schema(schema)?.is_none() && !is_open_object(schema) {
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

fn schema_type(schema: &Schema, optional: bool) -> Result<String> {
    let base = schema_base_type(schema)?;
    Ok(if optional { nullable_type(&base) } else { base })
}

fn schema_base_type(schema: &Schema) -> Result<String> {
    if let Some(reference) = &schema.reference {
        return Ok(csharp_type_name(&reference_model_name(reference)));
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
        .replace("~1", "/")
        .replace("~0", "~")
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
