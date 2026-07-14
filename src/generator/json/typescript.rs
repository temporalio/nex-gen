use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use heck::{ToShoutySnakeCase, ToUpperCamelCase};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::generator::ExternalModelBackend;
use crate::generator::typescript::{
    RenderedExternalModelFragments, WireValueConversion, typescript_generated_field_name,
};
use crate::planning::{PlannedJsonType, PlannedSpec, PlannedTypeFamily};
use crate::spec::{ExternalTypeSpec, ModulePath, RecordSpec};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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
}

pub(in crate::generator) fn model_type_ref(json_type: &PlannedJsonType) -> String {
    json_type.model_name.clone()
}

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelBackend {
    json_models: Vec<PlannedJsonType>,
    tree_leaf: bool,
    runtime_import_module: String,
}

impl ExternalModelBackend<PlannedJsonType> for ModelBackend {
    type ModelFragments = RenderedExternalModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.tree_leaf = !api_plan.module_path.is_root();
        self.runtime_import_module = if self.tree_leaf {
            root_typescript_runtime_module(&api_plan.module_path)
        } else {
            "./json".to_string()
        };
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

    fn render_models(&self) -> Result<RenderedExternalModelFragments> {
        let json_models = self.json_models.iter().collect::<Vec<_>>();
        render_external_models(json_models.as_slice(), &self.runtime_import_module)
    }

    fn render_support_files(&self) -> Result<BTreeMap<PathBuf, String>> {
        if self.tree_leaf || self.json_models.is_empty() {
            return Ok(BTreeMap::new());
        }

        Ok(BTreeMap::from([(
            PathBuf::from("json.ts"),
            render_support_file(),
        )]))
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
            from_wire: "{wire}".to_string(),
            to_wire: "{value}".to_string(),
            function_name_to_wire: None,
            from_wire_function_name: None,
            to_wire_function_name: None,
            uses_rendered_model_annotation: false,
        })
    }
}

pub(in crate::generator) fn render_support_file() -> String {
    render_json_runtime_module()
}

fn root_typescript_runtime_module(module_path: &ModulePath) -> String {
    format!("{}json", "../".repeat(module_path.0.len()))
}

fn render_external_models(
    json_models: &[&PlannedJsonType],
    runtime_import_module: &str,
) -> Result<RenderedExternalModelFragments> {
    if json_models.is_empty() {
        return Ok(RenderedExternalModelFragments::default());
    }

    let mut output = String::new();
    render_default_constants(&mut output, json_models)?;
    if !output.is_empty() {
        output.push('\n');
    }

    for model in json_models {
        output.push('\n');
        render_model_interface(&mut output, model)?;
    }

    for model in json_models {
        output.push('\n');
        render_model_parser(&mut output, model, json_models)?;
        output.push('\n');
        render_model_serializer(&mut output, model)?;
    }

    let uses_refs = json_models
        .iter()
        .any(|model| schema_uses_ref(&decode_schema(model).ok()));

    Ok(RenderedExternalModelFragments {
        imports: render_json_model_imports(uses_refs, runtime_import_module),
        body: output,
        exported_names: json_models
            .iter()
            .map(|model| model.model_name.clone())
            .collect(),
    })
}

fn render_json_model_imports(uses_refs: bool, runtime_import_module: &str) -> String {
    let mut imports = String::new();
    imports.push_str("import type { Violation } from \"");
    imports.push_str(runtime_import_module);
    imports.push_str("\";\n");
    imports.push_str("import { ValidationError, isPlainObject");
    if uses_refs {
        imports.push_str(", collect");
    }
    imports.push_str(" } from \"");
    imports.push_str(runtime_import_module);
    imports.push_str("\";\n");
    imports
}

fn render_json_runtime_module() -> String {
    let mut output = String::new();
    output.push_str("// Generated by nex-gen. DO NOT EDIT!\n\n");
    render_validator_core(&mut output);
    output.push('\n');
    render_collect_helper(&mut output);
    output
}

fn render_validator_core(output: &mut String) {
    output.push_str("/** A single constraint failure, located by JSON path. */\n");
    output.push_str("export interface Violation {\n");
    output.push_str("  readonly path: string;\n");
    output.push_str("  readonly reason: string;\n");
    output.push_str("}\n\n");
    output.push_str("export class ValidationError extends Error {\n");
    output.push_str("  public constructor(public readonly violations: Violation[]) {\n");
    output.push_str("    super(\n");
    output.push_str(
        "      `${violations.length} validation error(s): ` + violations.map((v) => `${v.path}: ${v.reason}`).join('; '),\n",
    );
    output.push_str("    );\n");
    output.push_str("    this.name = 'ValidationError';\n");
    output.push_str("  }\n");
    output.push_str("}\n\n");
    output.push_str(
        "export function isPlainObject(value: unknown): value is Record<string, unknown> {\n",
    );
    output.push_str(
        "  return typeof value === 'object' && value !== null && !Array.isArray(value);\n",
    );
    output.push_str("}\n");
}

fn render_default_constants(output: &mut String, models: &[&PlannedJsonType]) -> Result<()> {
    #[derive(Debug)]
    struct Constant {
        name: String,
        value: String,
        exported: bool,
    }

    let mut default_fields = Vec::new();
    let mut const_fields = Vec::new();
    for model in models {
        let schema = decode_schema(model)?;
        let Some(properties) = &schema.properties else {
            continue;
        };
        for (field_name, property) in properties {
            if let Some(default) = &property.default {
                default_fields.push((
                    model.model_name.clone(),
                    field_name.clone(),
                    typescript_value_literal(default)?,
                ));
            }
            if let Some(const_value) = &property.const_value {
                const_fields.push((
                    model.model_name.clone(),
                    field_name.clone(),
                    typescript_value_literal(const_value)?,
                ));
            }
        }
    }

    if default_fields.is_empty() && const_fields.is_empty() {
        return Ok(());
    }

    let mut constants = Vec::new();
    for (model_name, field_name, value) in const_fields {
        constants.push(Constant {
            name: const_const_name(
                &model_name,
                &field_name,
                models,
                ConstNameCollisionKind::Const,
            )?,
            value,
            exported: false,
        });
    }
    for (model_name, field_name, value) in default_fields {
        constants.push(Constant {
            name: default_const_name(
                &model_name,
                &field_name,
                models,
                ConstNameCollisionKind::Default,
            )?,
            value,
            exported: true,
        });
    }

    output.push('\n');
    for constant in constants {
        if constant.exported {
            output.push_str("export ");
        }
        output.push_str("const ");
        output.push_str(&constant.name);
        output.push_str(" = ");
        output.push_str(&constant.value);
        output.push_str(";\n");
    }
    Ok(())
}

fn render_model_interface(output: &mut String, model: &PlannedJsonType) -> Result<()> {
    let schema = decode_schema(model)?;
    render_doc_comment(output, "", schema.description.as_deref());
    output.push_str("export interface ");
    output.push_str(&model.model_name);
    output.push_str(" {\n");

    if let Some(value_schema) = typed_map_value_schema(&schema)? {
        output.push_str("  additionalProperties: Record<string, ");
        output.push_str(&type_annotation(&value_schema)?);
        output.push_str(">;\n");
        output.push_str("}\n");
        return Ok(());
    }

    let required = required_fields(&schema);
    if let Some(properties) = &schema.properties {
        for (json_name, property) in properties {
            render_doc_comment(output, "  ", property.description.as_deref());
            output.push_str("  ");
            if property.const_value.is_some() {
                output.push_str("readonly ");
            }
            output.push_str(&typescript_object_key(&typescript_generated_field_name(
                json_name,
            )));
            if !required.contains(json_name) {
                output.push('?');
            }
            output.push_str(": ");
            output.push_str(&type_annotation(property)?);
            output.push_str(";\n");
        }
    }

    if is_open_object(&schema) {
        output.push_str("  additionalProperties: Record<string, ");
        output.push_str(&additional_properties_annotation(&schema)?);
        output.push_str(">;\n");
    }

    output.push_str("}\n");
    Ok(())
}

fn render_model_parser(
    output: &mut String,
    model: &PlannedJsonType,
    models: &[&PlannedJsonType],
) -> Result<()> {
    let schema = decode_schema(model)?;
    if is_open_object(&schema) {
        render_declared_field_set(output, model, &schema);
        output.push('\n');
    }
    output.push_str("export function parse");
    output.push_str(&model.model_name);
    output.push_str("(raw: unknown): ");
    output.push_str(&model.model_name);
    output.push_str(" {\n");
    output.push_str("  const violations: Violation[] = [];\n");
    output.push_str("  if (!isPlainObject(raw)) {\n");
    output.push_str("    throw new ValidationError([{ path: '', reason: 'expected object' }]);\n");
    output.push_str("  }\n\n");

    if let Some(value_schema) = typed_map_value_schema(&schema)? {
        render_typed_map_parser_body(output, &schema, &value_schema);
        output.push_str("}\n");
        return Ok(());
    }

    let required = required_fields(&schema);
    let mut parsed_fields = Vec::new();
    if let Some(properties) = &schema.properties {
        for (json_name, property) in properties {
            render_property_parser(
                output,
                model,
                models,
                json_name,
                property,
                required.contains(json_name),
            )?;
            parsed_fields.push((
                json_name.clone(),
                typescript_generated_field_name(json_name),
            ));
            output.push('\n');
        }
    }

    if schema.additional_properties.as_ref() == Some(&Value::Bool(false)) {
        render_closed_object_unknown_key_check(output, &parsed_fields);
    } else if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        render_open_object_collection(output, model);
    }

    output.push_str("  if (violations.length) {\n");
    output.push_str("    throw new ValidationError(violations);\n");
    output.push_str("  }\n");
    output.push_str("  const out: ");
    output.push_str(&model.model_name);
    output.push_str(" = { ");
    let mut required_out = parsed_fields
        .iter()
        .filter(|(json_name, _)| required.contains(json_name))
        .map(|(_, field_name)| field_name.clone())
        .collect::<Vec<_>>();
    if is_open_object(&schema) {
        required_out.push("additionalProperties".to_string());
    }
    output.push_str(&required_out.join(", "));
    output.push_str(" };\n");
    for (json_name, field_name) in &parsed_fields {
        if !required.contains(json_name) {
            output.push_str("  if (");
            output.push_str(field_name);
            output.push_str(" !== undefined) {\n");
            output.push_str("    out.");
            output.push_str(field_name);
            output.push_str(" = ");
            output.push_str(field_name);
            output.push_str(";\n");
            output.push_str("  }\n");
        }
    }
    output.push_str("  return out;\n");
    output.push_str("}\n");
    Ok(())
}

fn render_model_serializer(output: &mut String, model: &PlannedJsonType) -> Result<()> {
    let schema = decode_schema(model)?;
    output.push_str("export function serialize");
    output.push_str(&model.model_name);
    output.push_str("(value: ");
    output.push_str(&model.model_name);
    output.push_str("): unknown {\n");
    output.push_str("  const out: Record<string, unknown> = {};\n");

    if typed_map_value_schema(&schema)?.is_some() {
        output.push_str(
            "  for (const [key, entry] of Object.entries(value.additionalProperties ?? {})) {\n",
        );
        output.push_str("    out[key] = entry;\n");
        output.push_str("  }\n");
        output.push_str("  return out;\n");
        output.push_str("}\n");
        return Ok(());
    }

    let required = required_fields(&schema);
    if let Some(properties) = &schema.properties {
        for (json_name, property) in properties {
            let field_name = typescript_generated_field_name(json_name);
            let assignment = serialize_expr(property, &format!("value.{field_name}"));
            if required.contains(json_name) {
                output.push_str("  out.");
                output.push_str(json_name);
                output.push_str(" = ");
                output.push_str(&assignment);
                output.push_str(";\n");
            } else {
                output.push_str("  if (value.");
                output.push_str(&field_name);
                output.push_str(" !== undefined) {\n");
                output.push_str("    out.");
                output.push_str(json_name);
                output.push_str(" = ");
                output.push_str(&assignment);
                output.push_str(";\n");
                output.push_str("  }\n");
            }
        }
    }
    if is_open_object(&schema) {
        output.push_str(
            "  for (const [key, entry] of Object.entries(value.additionalProperties ?? {})) {\n",
        );
        output.push_str("    out[key] = entry;\n");
        output.push_str("  }\n");
    }
    output.push_str("  return out;\n");
    output.push_str("}\n");
    Ok(())
}

fn render_typed_map_parser_body(output: &mut String, schema: &Schema, value_schema: &Schema) {
    output.push_str("  const keys = Object.keys(raw);\n");
    if let Some(max_properties) = schema.max_properties {
        output.push_str("  if (keys.length > ");
        output.push_str(&max_properties.to_string());
        output.push_str(") {\n");
        output.push_str("    violations.push({ path: '', reason: 'maxProperties: at most ");
        output.push_str(&max_properties.to_string());
        output.push_str(" entries' });\n");
        output.push_str("  }\n");
    }
    output.push_str("  const additionalProperties: Record<string, ");
    output.push_str(&type_annotation(value_schema).unwrap_or_else(|_| "unknown".to_string()));
    output.push_str("> = {};\n");
    output.push_str("  for (const key of keys) {\n");
    output.push_str("    let entry: ");
    output.push_str(&type_annotation(value_schema).unwrap_or_else(|_| "unknown".to_string()));
    output.push_str(" | undefined = undefined;\n");
    render_value_parser(
        output,
        value_schema,
        "raw[key]",
        "entry",
        "key",
        "    ",
        true,
    );
    output.push_str("    if (entry !== undefined) {\n");
    output.push_str("      additionalProperties[key] = entry;\n");
    output.push_str("    }\n");
    output.push_str("  }\n");
    output.push_str("  if (violations.length) {\n");
    output.push_str("    throw new ValidationError(violations);\n");
    output.push_str("  }\n");
    output.push_str("  return { additionalProperties };\n");
}

fn render_property_parser(
    output: &mut String,
    model: &PlannedJsonType,
    models: &[&PlannedJsonType],
    json_name: &str,
    property: &Schema,
    required: bool,
) -> Result<()> {
    let field_name = typescript_generated_field_name(json_name);
    let annotation = if required {
        type_annotation(property)?
    } else {
        optional_type_annotation(&type_annotation(property)?)
    };
    output.push_str("  let ");
    output.push_str(&field_name);
    output.push_str(": ");
    output.push_str(&annotation);
    output.push_str(" = undefined as unknown as ");
    output.push_str(&annotation);
    output.push_str(";\n");

    if required {
        if allows_null(property) {
            output.push_str("  if (raw.");
            output.push_str(json_name);
            output.push_str(" === undefined) {\n");
        } else {
            output.push_str("  if (raw.");
            output.push_str(json_name);
            output.push_str(" === undefined || raw.");
            output.push_str(json_name);
            output.push_str(" === null) {\n");
        }
        output.push_str("    violations.push({ path: '");
        output.push_str(json_name);
        output.push_str("', reason: 'required' });\n");
        output.push_str("  } else {\n");
        render_property_value_parser(output, model, models, json_name, property, &field_name)?;
        output.push_str("  }\n");
    } else {
        if allows_null(property) {
            output.push_str("  if (raw.");
            output.push_str(json_name);
            output.push_str(" !== undefined) {\n");
        } else {
            output.push_str("  if (raw.");
            output.push_str(json_name);
            output.push_str(" === null) {\n");
            output.push_str("    violations.push({ path: '");
            output.push_str(json_name);
            output.push_str("', reason: 'explicit null not allowed' });\n");
            output.push_str("  } else if (raw.");
            output.push_str(json_name);
            output.push_str(" !== undefined) {\n");
        }
        render_property_value_parser(output, model, models, json_name, property, &field_name)?;
        output.push_str("  }\n");
    }

    Ok(())
}

fn render_property_value_parser(
    output: &mut String,
    model: &PlannedJsonType,
    models: &[&PlannedJsonType],
    json_name: &str,
    property: &Schema,
    field_name: &str,
) -> Result<()> {
    let raw_expr = format!("raw.{json_name}");
    let path_expr = typescript_string_literal(json_name);
    if let Some(const_value) = &property.const_value {
        let const_name = const_const_name(
            &model.model_name,
            json_name,
            models,
            ConstNameCollisionKind::Const,
        )?;
        render_const_parser(
            output,
            const_value,
            &raw_expr,
            field_name,
            &path_expr,
            "    ",
            &const_name,
        );
        return Ok(());
    }

    render_value_parser(
        output, property, &raw_expr, field_name, &path_expr, "    ", false,
    );
    Ok(())
}

fn render_value_parser(
    output: &mut String,
    schema: &Schema,
    raw_expr: &str,
    target: &str,
    path_expr: &str,
    indent: &str,
    target_optional: bool,
) {
    if let Some(reference) = &schema.reference {
        let model_name = reference_model_name(reference);
        output.push_str(indent);
        output.push_str("try {\n");
        output.push_str(indent);
        output.push_str("  ");
        output.push_str(target);
        output.push_str(" = parse");
        output.push_str(&model_name);
        output.push('(');
        output.push_str(raw_expr);
        output.push_str(");\n");
        output.push_str(indent);
        output.push_str("} catch (error) {\n");
        output.push_str(indent);
        output.push_str("  collect(violations, ");
        output.push_str(path_expr);
        output.push_str(", error);\n");
        output.push_str(indent);
        output.push_str("}\n");
        return;
    }

    if let Some(branches) = &schema.one_of {
        let non_null = branches
            .iter()
            .filter(|branch| !schema_type_includes(branch, "null"))
            .collect::<Vec<_>>();
        if branches
            .iter()
            .any(|branch| schema_type_includes(branch, "null"))
        {
            output.push_str(indent);
            output.push_str("if (");
            output.push_str(raw_expr);
            output.push_str(" === null) {\n");
            output.push_str(indent);
            output.push_str("  ");
            output.push_str(target);
            output.push_str(" = null;\n");
            output.push_str(indent);
            output.push_str("} else {\n");
            if let Some(branch) = non_null.first() {
                render_value_parser(
                    output,
                    branch,
                    raw_expr,
                    target,
                    path_expr,
                    &format!("{indent}  "),
                    target_optional,
                );
            }
            output.push_str(indent);
            output.push_str("}\n");
            return;
        }
    }

    if let Some(const_value) = &schema.const_value {
        let const_literal =
            typescript_value_literal(const_value).unwrap_or_else(|_| "undefined".to_string());
        render_const_parser(
            output,
            const_value,
            raw_expr,
            target,
            path_expr,
            indent,
            &const_literal,
        );
        return;
    }

    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => {
            render_typeof_parser(output, raw_expr, target, path_expr, indent, "string")
        }
        Some("number") => {
            render_typeof_parser(output, raw_expr, target, path_expr, indent, "number")
        }
        Some("boolean") => {
            render_typeof_parser(output, raw_expr, target, path_expr, indent, "boolean")
        }
        Some("integer") => {
            output.push_str(indent);
            output.push_str("if (typeof ");
            output.push_str(raw_expr);
            output.push_str(" !== 'number' || !Number.isSafeInteger(");
            output.push_str(raw_expr);
            output.push_str(")) {\n");
            output.push_str(indent);
            output.push_str("  violations.push({ path: ");
            output.push_str(path_expr);
            output.push_str(", reason: 'expected integer' });\n");
            output.push_str(indent);
            output.push_str("} else {\n");
            output.push_str(indent);
            output.push_str("  ");
            output.push_str(target);
            output.push_str(" = ");
            output.push_str(raw_expr);
            output.push_str(";\n");
            output.push_str(indent);
            output.push_str("}\n");
        }
        Some("array") => render_array_parser(output, schema, raw_expr, target, path_expr, indent),
        Some("object") => {
            output.push_str(indent);
            output.push_str(target);
            output.push_str(" = ");
            output.push_str(raw_expr);
            output.push_str(" as ");
            output.push_str(&type_annotation(schema).unwrap_or_else(|_| "unknown".to_string()));
            output.push_str(";\n");
        }
        Some("null") => {
            output.push_str(indent);
            output.push_str(target);
            output.push_str(" = null;\n");
        }
        _ if target_optional => {
            output.push_str(indent);
            output.push_str(target);
            output.push_str(" = ");
            output.push_str(raw_expr);
            output.push_str(" as never;\n");
        }
        _ => {
            output.push_str(indent);
            output.push_str(target);
            output.push_str(" = ");
            output.push_str(raw_expr);
            output.push_str(" as never;\n");
        }
    }
}

fn render_typeof_parser(
    output: &mut String,
    raw_expr: &str,
    target: &str,
    path_expr: &str,
    indent: &str,
    ty: &str,
) {
    output.push_str(indent);
    output.push_str("if (typeof ");
    output.push_str(raw_expr);
    output.push_str(" !== '");
    output.push_str(ty);
    output.push_str("') {\n");
    output.push_str(indent);
    output.push_str("  violations.push({ path: ");
    output.push_str(path_expr);
    output.push_str(", reason: 'expected ");
    output.push_str(ty);
    output.push_str("' });\n");
    output.push_str(indent);
    output.push_str("} else {\n");
    output.push_str(indent);
    output.push_str("  ");
    output.push_str(target);
    output.push_str(" = ");
    output.push_str(raw_expr);
    output.push_str(";\n");
    output.push_str(indent);
    output.push_str("}\n");
}

fn render_const_parser(
    output: &mut String,
    const_value: &Value,
    raw_expr: &str,
    target: &str,
    path_expr: &str,
    indent: &str,
    const_expr: &str,
) {
    if matches!(const_value, Value::String(_)) {
        output.push_str(indent);
        output.push_str("if (typeof ");
        output.push_str(raw_expr);
        output.push_str(" !== 'string') {\n");
        output.push_str(indent);
        output.push_str("  violations.push({ path: ");
        output.push_str(path_expr);
        output.push_str(", reason: 'expected string' });\n");
        output.push_str(indent);
        output.push_str("} else if (");
        output.push_str(raw_expr);
        output.push_str(" !== ");
        output.push_str(const_expr);
        output.push_str(") {\n");
        output.push_str(indent);
        output.push_str("  violations.push({ path: ");
        output.push_str(path_expr);
        output.push_str(", reason: `must equal \"${");
        output.push_str(const_expr);
        output.push_str("}\"` });\n");
        output.push_str(indent);
        output.push_str("} else {\n");
    } else {
        output.push_str(indent);
        output.push_str("if (");
        output.push_str(raw_expr);
        output.push_str(" !== ");
        output.push_str(const_expr);
        output.push_str(") {\n");
        output.push_str(indent);
        output.push_str("  violations.push({ path: ");
        output.push_str(path_expr);
        output.push_str(", reason: 'unexpected const value' });\n");
        output.push_str(indent);
        output.push_str("} else {\n");
    }
    output.push_str(indent);
    output.push_str("  ");
    output.push_str(target);
    output.push_str(" = ");
    output.push_str(raw_expr);
    output.push_str(";\n");
    output.push_str(indent);
    output.push_str("}\n");
}

fn render_array_parser(
    output: &mut String,
    schema: &Schema,
    raw_expr: &str,
    target: &str,
    path_expr: &str,
    indent: &str,
) {
    let item_annotation = schema
        .items
        .as_ref()
        .map(|item| type_annotation(item).unwrap_or_else(|_| "unknown".to_string()))
        .unwrap_or_else(|| "unknown".to_string());
    output.push_str(indent);
    output.push_str("if (!Array.isArray(");
    output.push_str(raw_expr);
    output.push_str(")) {\n");
    output.push_str(indent);
    output.push_str("  violations.push({ path: ");
    output.push_str(path_expr);
    output.push_str(", reason: 'expected array' });\n");
    output.push_str(indent);
    output.push_str("} else {\n");
    output.push_str(indent);
    output.push_str("  ");
    output.push_str(target);
    output.push_str(" = [];\n");
    output.push_str(indent);
    output.push_str("  ");
    output.push_str(raw_expr);
    output.push_str(".forEach((element: unknown, index: number) => {\n");
    output.push_str(indent);
    output.push_str("    let item: ");
    output.push_str(&item_annotation);
    output.push_str(" = undefined as unknown as ");
    output.push_str(&item_annotation);
    output.push_str(";\n");
    if let Some(item) = &schema.items {
        let item_path_expr = if let Some(field_name) = string_literal_value(path_expr) {
            format!("`{field_name}[${{index}}]`")
        } else {
            format!("`${{{path_expr}}}[${{index}}]`")
        };
        if item.ty.as_ref().and_then(Value::as_str) == Some("string") {
            output.push_str(indent);
            output.push_str("    if (typeof element !== 'string') {\n");
            output.push_str(indent);
            output.push_str("      violations.push({ path: ");
            output.push_str(&item_path_expr);
            output.push_str(", reason: 'expected element' });\n");
            output.push_str(indent);
            output.push_str("    } else {\n");
            output.push_str(indent);
            output.push_str("      item = element;\n");
            output.push_str(indent);
            output.push_str("    }\n");
        } else {
            render_value_parser(
                output,
                item,
                "element",
                "item",
                &item_path_expr,
                &format!("{indent}    "),
                false,
            );
        }
    } else {
        output.push_str(indent);
        output.push_str("    item = element as unknown;\n");
    }
    output.push_str(indent);
    output.push_str("    ");
    output.push_str("if (item !== undefined) {\n");
    output.push_str(indent);
    output.push_str("      ");
    output.push_str(target);
    output.push_str("!.push(item);\n");
    output.push_str(indent);
    output.push_str("    }\n");
    output.push_str(indent);
    output.push_str("  });\n");
    output.push_str(indent);
    output.push_str("}\n");
}

fn render_closed_object_unknown_key_check(output: &mut String, fields: &[(String, String)]) {
    output.push_str("  for (const key of Object.keys(raw)) {\n");
    output.push_str("    if (");
    output.push_str(
        &fields
            .iter()
            .map(|(json_name, _)| format!("key !== {}", typescript_string_literal(json_name)))
            .collect::<Vec<_>>()
            .join(" && "),
    );
    output.push_str(") {\n");
    output.push_str("      violations.push({ path: key, reason: 'unknown field' });\n");
    output.push_str("    }\n");
    output.push_str("  }\n\n");
}

fn render_declared_field_set(output: &mut String, model: &PlannedJsonType, schema: &Schema) {
    let fields = schema
        .properties
        .as_ref()
        .map(|properties| {
            properties
                .keys()
                .map(|field| typescript_string_literal(field))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    output.push_str("const ");
    output.push_str(&declared_fields_const_name(&model.model_name));
    output.push_str(" = new Set([");
    output.push_str(&fields.join(", "));
    output.push_str("]);\n");
}

fn render_open_object_collection(output: &mut String, model: &PlannedJsonType) {
    output.push_str("  const additionalProperties: Record<string, unknown> = {};\n");
    output.push_str("  for (const key of Object.keys(raw)) {\n");
    output.push_str("    if (!");
    output.push_str(&declared_fields_const_name(&model.model_name));
    output.push_str(".has(key)) {\n");
    output.push_str("      additionalProperties[key] = raw[key];\n");
    output.push_str("    }\n");
    output.push_str("  }\n\n");
}

fn render_collect_helper(output: &mut String) {
    output.push_str(
        "export function collect(violations: Violation[], path: string, error: unknown): void {\n",
    );
    output.push_str("  if (error instanceof ValidationError) {\n");
    output.push_str("    for (const inner of error.violations) {\n");
    output.push_str(
        "      violations.push({ path: `${path}.${inner.path}`, reason: inner.reason });\n",
    );
    output.push_str("    }\n");
    output.push_str("  } else {\n");
    output.push_str("    violations.push({ path, reason: String(error) });\n");
    output.push_str("  }\n");
    output.push_str("}\n");
}

fn serialize_expr(schema: &Schema, value_expr: &str) -> String {
    if let Some(reference) = &schema.reference {
        return format!("serialize{}({value_expr})", reference_model_name(reference));
    }
    if let Some(branches) = &schema.one_of
        && branches
            .iter()
            .any(|branch| schema_type_includes(branch, "null"))
        && let Some(non_null) = branches
            .iter()
            .find(|branch| !schema_type_includes(branch, "null"))
    {
        if non_null.reference.is_some() {
            return format!(
                "{value_expr} === null ? null : {}",
                serialize_expr(non_null, value_expr)
            );
        }
    }
    value_expr.to_string()
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

fn type_annotation(schema: &Schema) -> Result<String> {
    if let Some(const_value) = &schema.const_value
        && let Some(annotation) = typescript_const_annotation(const_value)
    {
        return Ok(annotation);
    }
    if let Some(reference) = &schema.reference {
        return Ok(reference_model_name(reference));
    }
    if let Some(one_of) = &schema.one_of {
        let values = one_of
            .iter()
            .map(type_annotation)
            .collect::<Result<Vec<_>>>()?;
        return Ok(join_union(values));
    }
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => Ok("string".to_string()),
        Some("integer" | "number") => Ok("number".to_string()),
        Some("boolean") => Ok("boolean".to_string()),
        Some("array") => {
            let item = schema
                .items
                .as_ref()
                .map(|item| type_annotation(item))
                .transpose()?
                .unwrap_or_else(|| "unknown".to_string());
            Ok(format!("{item}[]"))
        }
        Some("object") => object_annotation(schema),
        Some("null") => Ok("null".to_string()),
        _ => Ok("unknown".to_string()),
    }
}

fn object_annotation(schema: &Schema) -> Result<String> {
    if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        return Ok("Record<string, unknown>".to_string());
    }
    Ok(format!(
        "Record<string, {}>",
        additional_properties_annotation(schema)?
    ))
}

fn additional_properties_annotation(schema: &Schema) -> Result<String> {
    match &schema.additional_properties {
        Some(Value::Object(value)) => {
            let additional: Schema =
                serde_json::from_value(Value::Object(value.clone())).map_err(|error| {
                    Error::InvalidJsonSchema {
                        path: PathBuf::from("<json-generator>"),
                        reason: format!("failed to read `additionalProperties`: {error}"),
                    }
                })?;
            type_annotation(&additional)
        }
        _ => Ok("unknown".to_string()),
    }
}

fn optional_type_annotation(annotation: &str) -> String {
    if annotation.contains("undefined") {
        annotation.to_string()
    } else {
        format!("{annotation} | undefined")
    }
}

fn typescript_const_annotation(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(format!(
            "{} | (string & {{}})",
            typescript_string_literal(value)
        )),
        _ => None,
    }
}

fn required_fields(schema: &Schema) -> BTreeSet<String> {
    schema
        .required
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
}

fn allows_null(schema: &Schema) -> bool {
    schema.const_value.as_ref() == Some(&Value::Null)
        || schema_type_includes(schema, "null")
        || schema
            .one_of
            .as_ref()
            .is_some_and(|branches| branches.iter().any(allows_null))
}

fn is_open_object(schema: &Schema) -> bool {
    schema.ty.as_ref().and_then(Value::as_str) == Some("object")
        && schema
            .properties
            .as_ref()
            .is_some_and(|properties| !properties.is_empty())
        && schema.additional_properties.as_ref() != Some(&Value::Bool(false))
}

fn schema_type_includes(schema: &Schema, ty: &str) -> bool {
    match schema.ty.as_ref() {
        Some(Value::String(value)) => value == ty,
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|value| value == ty)),
        _ => false,
    }
}

fn schema_uses_ref(schema: &Option<Schema>) -> bool {
    let Some(schema) = schema else {
        return false;
    };
    schema.reference.is_some()
        || schema.properties.as_ref().is_some_and(|properties| {
            properties
                .values()
                .any(|schema| schema_uses_ref(&Some(schema.clone())))
        })
        || schema
            .items
            .as_ref()
            .is_some_and(|items| schema_uses_ref(&Some((**items).clone())))
        || schema.one_of.as_ref().is_some_and(|branches| {
            branches
                .iter()
                .any(|schema| schema_uses_ref(&Some(schema.clone())))
        })
        || schema
            .additional_properties
            .as_ref()
            .and_then(|additional_properties| {
                serde_json::from_value::<Schema>(additional_properties.clone()).ok()
            })
            .is_some_and(|schema| schema_uses_ref(&Some(schema)))
}

fn join_union(values: Vec<String>) -> String {
    let mut deduped = Vec::<String>::new();
    for value in values {
        if !deduped.contains(&value) {
            deduped.push(value);
        }
    }
    deduped.join(" | ")
}

fn reference_model_name(reference: &str) -> String {
    let name = reference
        .split('#')
        .next_back()
        .unwrap_or(reference)
        .trim_start_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(reference);
    name.rsplit('#')
        .next()
        .unwrap_or(name)
        .to_upper_camel_case()
}

fn declared_fields_const_name(model_name: &str) -> String {
    format!("{}_DECLARED", model_name.to_shouty_snake_case())
}

#[derive(Debug, Clone, Copy)]
enum ConstNameCollisionKind {
    Const,
    Default,
}

fn const_const_name(
    model_name: &str,
    field_name: &str,
    models: &[&PlannedJsonType],
    kind: ConstNameCollisionKind,
) -> Result<String> {
    const_name(model_name, field_name, models, kind, "", "_CONST")
}

fn default_const_name(
    model_name: &str,
    field_name: &str,
    models: &[&PlannedJsonType],
    kind: ConstNameCollisionKind,
) -> Result<String> {
    const_name(model_name, field_name, models, kind, "DEFAULT_", "")
}

fn const_name(
    model_name: &str,
    field_name: &str,
    models: &[&PlannedJsonType],
    kind: ConstNameCollisionKind,
    prefix: &str,
    suffix: &str,
) -> Result<String> {
    let field_count = models
        .iter()
        .map(|model| decode_schema(model))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|schema| {
            schema.properties.as_ref().is_some_and(|properties| {
                properties
                    .get(field_name)
                    .is_some_and(|property| match kind {
                        ConstNameCollisionKind::Const => property.const_value.is_some(),
                        ConstNameCollisionKind::Default => property.default.is_some(),
                    })
            })
        })
        .count();

    let mut name = if field_count == 1 {
        field_name.to_shouty_snake_case()
    } else {
        format!(
            "{}_{}",
            model_name.to_shouty_snake_case(),
            field_name.to_shouty_snake_case()
        )
    };
    name.insert_str(0, prefix);
    name.push_str(suffix);
    Ok(name)
}

fn string_literal_value(value: &str) -> Option<String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(ToOwned::to_owned)
}

fn render_doc_comment(output: &mut String, indent: &str, doc: Option<&str>) {
    let Some(doc) = doc.map(str::trim).filter(|doc| !doc.is_empty()) else {
        return;
    };
    output.push_str(indent);
    output.push_str("/**\n");
    for line in doc.lines() {
        output.push_str(indent);
        output.push_str(" * ");
        output.push_str(line.trim());
        output.push('\n');
    }
    output.push_str(indent);
    output.push_str(" */\n");
}

fn typescript_object_key(name: &str) -> String {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        }
        _ => false,
    };
    if valid {
        name.to_string()
    } else {
        typescript_string_literal(name)
    }
}

fn typescript_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn typescript_value_literal(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(typescript_string_literal(value)),
        Value::Array(values) => {
            let values = values
                .iter()
                .map(typescript_value_literal)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("[{}]", values.join(", ")))
        }
        Value::Object(values) => {
            let values = values
                .iter()
                .map(|(key, value)| {
                    Ok(format!(
                        "{}: {}",
                        typescript_object_key(key),
                        typescript_value_literal(value)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("{{ {} }}", values.join(", ")))
        }
    }
}
