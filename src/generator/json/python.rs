use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::generator::ExternalModelBackend;
use crate::generator::python::{
    PythonImports, PythonModelHoists, RenderedModelFragments, WireValueConversion,
    module_common_prefix_len, python_field_name, python_string_literal,
    render_generated_file_header, render_named_python_import, render_optional_python_imports,
    render_python_docstring,
};
use crate::planning::{PlannedJsonType, PlannedSpec, PlannedTypeFamily};
use crate::spec::{ExternalTypeSpec, ModulePath, RecordSpec};
use crate::workspace::{ApiSpecBranch, ApiSpecNode};

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
}

pub(in crate::generator) fn model_type_ref(json_type: &PlannedJsonType) -> String {
    json_type.model_name.clone()
}

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelBackend {
    json_models: Vec<PlannedJsonType>,
    hoisted_json_models: Vec<PlannedJsonType>,
    tree_leaf: bool,
    runtime_import_module: String,
}

impl ExternalModelBackend<PlannedJsonType> for ModelBackend {
    type ModelFragments = RenderedModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.tree_leaf = !api_plan.module_path.is_root();
        self.runtime_import_module = if self.tree_leaf {
            root_python_runtime_module(&api_plan.module_path)
        } else {
            "._json".to_string()
        };
        let mut json_models = api_plan
            .external_types()
            .map(|(_, binding)| binding)
            .filter_map(|binding| match &binding.external_type {
                ExternalTypeSpec::Json(json_type) => Some(json_type.clone()),
                _ => None,
            })
            .collect();
        self.json_models = std::mem::take(&mut json_models);
        self.hoisted_json_models = Vec::new();
        Ok(())
    }

    fn render_models(&self) -> Result<RenderedModelFragments> {
        let json_models = self.json_models.iter().collect::<Vec<_>>();
        render_external_models(json_models.as_slice(), &self.runtime_import_module)
    }

    fn render_support_files(&self) -> Result<BTreeMap<PathBuf, String>> {
        if self.tree_leaf || self.json_models.is_empty() {
            return Ok(BTreeMap::new());
        }

        Ok(BTreeMap::from([(
            PathBuf::from("_json.py"),
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
            imports: PythonImports::default(),
            supports_unpacked_input: false,
        })
    }
}

impl ModelBackend {
    pub(in crate::generator) fn prepare_with_hoists(
        &mut self,
        api_plan: &PlannedSpec,
        hoists: &PythonModelHoists,
    ) -> Result<()> {
        self.prepare(api_plan)?;
        let mut local_models = Vec::new();
        let mut hoisted_models = Vec::new();
        for model in std::mem::take(&mut self.json_models) {
            if hoists.is_hoisted(&api_plan.module_path, &model.model_name) {
                hoisted_models.push(model);
            } else {
                local_models.push(model);
            }
        }
        self.json_models = local_models;
        self.hoisted_json_models = hoisted_models;
        Ok(())
    }

    pub(in crate::generator) fn is_hoisted(&self, json_type: &PlannedJsonType) -> bool {
        self.hoisted_json_models
            .iter()
            .any(|model| model.full_name == json_type.full_name)
    }
}

#[derive(Debug, Default)]
struct JsonModelHoistPlan {
    hoisted: BTreeMap<ModulePath, BTreeSet<String>>,
    hoisted_models: Vec<PlannedJsonType>,
    dependency_imports: BTreeMap<ModulePath, BTreeSet<String>>,
}

impl JsonModelHoistPlan {
    fn for_tree(branch: &ApiSpecBranch<PlannedTypeFamily>) -> Self {
        let mut models = BTreeMap::<String, (ModulePath, PlannedJsonType)>::new();
        collect_tree_json_models(branch, &mut models);

        let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
        for (full_name, (_, model)) in &models {
            let mut refs = BTreeSet::new();
            collect_json_schema_model_refs(&model.schema, &models, &mut refs);
            graph.insert(full_name.clone(), refs);
        }

        let mut hoisted_full_names = BTreeSet::new();
        for (source_name, (source_module, _)) in &models {
            for target_name in graph.get(source_name).into_iter().flatten() {
                let Some((target_module, _)) = models.get(target_name) else {
                    continue;
                };
                if source_module == target_module {
                    continue;
                }
                if json_model_can_reach(target_name, source_name, &graph, &mut BTreeSet::new()) {
                    hoisted_full_names.insert(source_name.clone());
                    hoisted_full_names.insert(target_name.clone());
                }
            }
        }

        let mut hoisted = BTreeMap::<ModulePath, BTreeSet<String>>::new();
        let mut hoisted_models = Vec::new();
        for full_name in &hoisted_full_names {
            let Some((module_path, model)) = models.get(full_name) else {
                continue;
            };
            hoisted
                .entry(module_path.clone())
                .or_default()
                .insert(model.model_name.clone());
            hoisted_models.push(model.clone());
        }

        let mut dependency_imports = BTreeMap::<ModulePath, BTreeSet<String>>::new();
        for full_name in &hoisted_full_names {
            for target_name in graph.get(full_name).into_iter().flatten() {
                if hoisted_full_names.contains(target_name) {
                    continue;
                }
                let Some((module_path, model)) = models.get(target_name) else {
                    continue;
                };
                dependency_imports
                    .entry(module_path.clone())
                    .or_default()
                    .insert(model.model_name.clone());
            }
        }

        Self {
            hoisted,
            hoisted_models,
            dependency_imports,
        }
    }

    fn is_empty(&self) -> bool {
        self.hoisted_models.is_empty()
    }
}

pub(in crate::generator) fn tree_model_hoists(
    branch: &ApiSpecBranch<PlannedTypeFamily>,
) -> Result<PythonModelHoists> {
    let plan = JsonModelHoistPlan::for_tree(branch);
    let mut hoists = PythonModelHoists::default();
    if plan.is_empty() {
        return Ok(hoists);
    }
    for (module_path, names) in &plan.hoisted {
        hoists.add_module_hoists(module_path.clone(), names.clone());
    }
    hoists.add_file(
        PathBuf::from("_models.py"),
        render_hoisted_models_module(&plan)?,
    );
    Ok(hoists)
}

fn render_hoisted_models_module(hoists: &JsonModelHoistPlan) -> Result<String> {
    let models = hoists.hoisted_models.iter().collect::<Vec<_>>();
    let model_fragments = render_external_models(models.as_slice(), "._json")?;
    let mut body = model_fragments.body.clone();
    if !model_fragments.post_model_statements.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&model_fragments.post_model_statements);
    }

    let mut output = String::new();
    render_generated_file_header(&mut output);
    output.push('\n');
    let wrote_imports =
        render_optional_python_imports(&mut output, &body, &model_fragments.module_imports, &[]);
    let mut wrote_relative_imports = false;
    for (module, names) in &model_fragments.relative_imports {
        if names.is_empty() {
            continue;
        }
        if wrote_imports || wrote_relative_imports {
            output.push('\n');
        }
        render_named_python_import(
            &mut output,
            module,
            &names.iter().cloned().collect::<Vec<_>>(),
        );
        wrote_relative_imports = true;
    }
    let mut wrote_dependency_imports = false;
    for (module_path, names) in &hoists.dependency_imports {
        if names.is_empty() {
            continue;
        }
        if wrote_imports || wrote_relative_imports || wrote_dependency_imports {
            output.push('\n');
        }
        render_named_python_import(
            &mut output,
            &python_relative_models_module(&ModulePath::default(), module_path),
            &names.iter().cloned().collect::<Vec<_>>(),
        );
        wrote_dependency_imports = true;
    }

    if !body.is_empty() {
        output.push('\n');
        output.push('\n');
        output.push_str(&body);
    }
    output.push_str("\n\n__all__ = [\n");
    for name in hoists
        .hoisted_models
        .iter()
        .map(|model| &model.model_name)
        .collect::<BTreeSet<_>>()
    {
        output.push_str("    ");
        output.push_str(&python_string_literal(name));
        output.push_str(",\n");
    }
    output.push_str("]\n");
    Ok(output)
}

fn collect_tree_json_models(
    branch: &ApiSpecBranch<PlannedTypeFamily>,
    models: &mut BTreeMap<String, (ModulePath, PlannedJsonType)>,
) {
    for node in branch.children.values() {
        match node {
            ApiSpecNode::Leaf(leaf) => {
                for binding in leaf.spec.external_types().map(|(_, binding)| binding) {
                    if let ExternalTypeSpec::Json(json_type) = &binding.external_type {
                        models.insert(
                            json_type.full_name.clone(),
                            (leaf.module_path.clone(), json_type.clone()),
                        );
                    }
                }
            }
            ApiSpecNode::Branch(branch) => collect_tree_json_models(branch, models),
        }
    }
}

fn collect_json_schema_model_refs(
    value: &serde_json::Value,
    models: &BTreeMap<String, (ModulePath, PlannedJsonType)>,
    refs: &mut BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(serde_json::Value::as_str)
                && let Some(full_name) = json_schema_ref_full_name(reference)
                && models.contains_key(&full_name)
            {
                refs.insert(full_name);
            }
            for value in object.values() {
                collect_json_schema_model_refs(value, models, refs);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_json_schema_model_refs(value, models, refs);
            }
        }
        _ => {}
    }
}

fn json_schema_ref_full_name(reference: &str) -> Option<String> {
    let fragment = reference
        .split_once('#')
        .map(|(_, fragment)| fragment)
        .unwrap_or(reference);
    let name = fragment.strip_prefix("/$defs/")?;
    Some(name.replace("~1", "/").replace("~0", "~"))
}

fn json_model_can_reach(
    source: &str,
    target: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if source == target {
        return true;
    }
    if !visited.insert(source.to_string()) {
        return false;
    }
    graph.get(source).is_some_and(|next| {
        next.iter()
            .any(|name| json_model_can_reach(name, target, graph, visited))
    })
}

fn python_relative_models_module(from: &ModulePath, to: &ModulePath) -> String {
    let common = module_common_prefix_len(&from.0, &to.0);
    let dot_count = from.0.len().saturating_sub(common) + 1;
    let mut module = ".".repeat(dot_count);
    let rest = to.0[common..]
        .iter()
        .map(|segment| segment.replace('-', "_"))
        .chain(std::iter::once("models".to_string()))
        .collect::<Vec<_>>();
    module.push_str(&rest.join("."));
    module
}

pub(in crate::generator) fn render_support_file() -> String {
    render_json_runtime_module()
}

fn root_python_runtime_module(module_path: &ModulePath) -> String {
    format!("{}{}", ".".repeat(module_path.0.len() + 1), "_json")
}

pub(in crate::generator) fn render_external_models(
    json_models: &[&PlannedJsonType],
    runtime_import_module: &str,
) -> Result<RenderedModelFragments> {
    if json_models.is_empty() {
        return Ok(RenderedModelFragments::default());
    }

    let mut models_body = String::new();
    let mut needs_optional_non_nullable_helper = false;
    let mut needs_set_fields_helper = false;
    let mut needs_pydantic_core = false;
    let mut needs_spec_int_helper = false;
    for (index, model) in json_models.iter().enumerate() {
        render_model(
            &mut models_body,
            model,
            &mut needs_optional_non_nullable_helper,
            &mut needs_set_fields_helper,
            &mut needs_pydantic_core,
            &mut needs_spec_int_helper,
        )?;
        if index + 1 != json_models.len() {
            models_body.push_str("\n\n");
        }
    }
    let mut body = String::new();
    body.push_str(&models_body);

    let mut post_model_statements = String::new();
    render_model_rebuilds(&mut post_model_statements, json_models);
    let mut module_imports = BTreeSet::from(["pydantic".to_string()]);
    if needs_pydantic_core {
        module_imports.insert("pydantic_core".to_string());
    }
    let mut relative_imports = BTreeMap::<String, BTreeSet<String>>::new();
    let mut runtime_imports = BTreeSet::new();
    if needs_spec_int_helper {
        runtime_imports.insert("SpecInt".to_string());
    }
    if needs_optional_non_nullable_helper {
        runtime_imports.insert("reject_explicit_null".to_string());
    }
    if needs_set_fields_helper {
        runtime_imports.insert("emit_set_fields".to_string());
    }
    if !runtime_imports.is_empty() {
        relative_imports.insert(runtime_import_module.to_string(), runtime_imports);
    }

    Ok(RenderedModelFragments {
        body,
        post_model_statements,
        module_imports,
        relative_imports,
        exported_names: json_models
            .iter()
            .map(|model| model.model_name.clone())
            .collect(),
    })
}

fn render_json_runtime_module() -> String {
    let mut output = String::new();
    output.push_str("# Generated by nex-gen. DO NOT EDIT!\n\n");
    output.push_str("from __future__ import annotations\n\n");
    output.push_str("import collections.abc\n");
    output.push_str("import typing\n\n");
    output.push_str("import pydantic\n");
    output.push_str("import pydantic.functional_validators\n");
    output.push_str("import pydantic_core\n\n\n");
    render_spec_int_helper(&mut output);
    output.push_str("\n\n");
    render_optional_non_nullable_helper(&mut output);
    output.push_str("\n\n");
    render_set_fields_helper(&mut output);
    output
}

fn render_model_rebuilds(output: &mut String, models: &[&PlannedJsonType]) {
    for model in models {
        output.push_str("_ = ");
        output.push_str(&model.model_name);
        output.push_str(".model_rebuild()\n");
    }
}

fn render_model(
    output: &mut String,
    model: &PlannedJsonType,
    needs_optional_non_nullable_helper: &mut bool,
    needs_set_fields_helper: &mut bool,
    needs_pydantic_core: &mut bool,
    needs_spec_int_helper: &mut bool,
) -> Result<()> {
    let schema = decode_schema(model)?;
    *needs_spec_int_helper |= schema_uses_integer(&schema);
    let extra = match schema.additional_properties.as_ref() {
        Some(Value::Bool(false)) => "forbid",
        _ => "allow",
    };
    output.push_str("class ");
    output.push_str(&model.model_name);
    output.push_str("(pydantic.BaseModel):\n");
    render_python_docstring(
        output,
        "    ",
        schema.description.as_deref(),
        &[],
        None,
        false,
    );
    output.push_str(
        "    model_config: typing.ClassVar[pydantic.ConfigDict] = pydantic.ConfigDict(strict=True, populate_by_name=True, extra=",
    );
    output.push_str(&python_string_literal(extra));
    output.push_str(")\n");

    if let Some(value_schema) = typed_map_value_schema(&schema)? {
        render_typed_map_model_methods(output, &schema, &value_schema);
        *needs_pydantic_core = true;
        return Ok(());
    }

    let Some(properties) = &schema.properties else {
        return Ok(());
    };
    if properties.is_empty() {
        return Ok(());
    }

    let required = schema
        .required
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut optional_non_nullable_fields = BTreeSet::new();
    let mut const_fields = Vec::new();
    for (json_name, property) in properties {
        output.push('\n');
        let field_name = python_field_name(json_name);
        let annotation = annotation(property)?;
        let required_field = required.contains(json_name);
        output.push_str("    ");
        output.push_str(&field_name);
        output.push_str(": ");
        if let Some(const_value) = &property.const_value {
            const_fields.push((json_name.clone(), field_name.clone(), const_value.clone()));
            output.push_str(&annotation);
            output.push_str(" = ");
            let default = python_value_literal(const_value)?;
            render_field_expr(output, json_name, &field_name, Some(&default));
        } else if required_field {
            output.push_str(&annotation);
            output.push_str(" = ");
            render_field_expr(output, json_name, &field_name, None);
        } else if let Some(default) = &property.default {
            output.push_str(&annotation);
            output.push_str(" = ");
            let default = python_value_literal(default)?;
            render_field_expr(output, json_name, &field_name, Some(&default));
        } else {
            if !allows_null(property) {
                optional_non_nullable_fields.insert(json_name.clone());
                if field_name != *json_name {
                    optional_non_nullable_fields.insert(field_name.clone());
                }
            }
            output.push_str(&optional_annotation(&annotation));
            output.push_str(" = ");
            render_field_expr(output, json_name, &field_name, Some("None"));
        }
        output.push('\n');
        render_python_docstring(
            output,
            "    ",
            property.description.as_deref(),
            &[],
            None,
            false,
        );
    }
    render_const_validators(output, &const_fields)?;
    render_optional_non_nullable_validator(output, &optional_non_nullable_fields);
    *needs_optional_non_nullable_helper |= !optional_non_nullable_fields.is_empty();
    *needs_pydantic_core |= !optional_non_nullable_fields.is_empty() || !const_fields.is_empty();
    render_set_fields_serializer(output);
    *needs_set_fields_helper = true;
    Ok(())
}

fn render_spec_int_helper(output: &mut String) {
    output.push_str("_INTEGER_CAP = (1 << 53) - 1\n\n\n");
    output.push_str("def _parse_spec_integer(value: object) -> int:\n");
    output.push_str("    if isinstance(value, bool):\n");
    output.push_str("        raise ValueError(\"expected integer, got boolean\")\n");
    output.push_str("    if isinstance(value, int):\n");
    output.push_str("        out = value\n");
    output.push_str("    elif isinstance(value, float):\n");
    output.push_str("        if not value.is_integer():\n");
    output.push_str(
        "            raise ValueError(\"number has a fractional part; not an integer\")\n",
    );
    output.push_str("        out = int(value)\n");
    output.push_str("    else:\n");
    output
        .push_str("        raise ValueError(f\"expected integer, got {type(value).__name__}\")\n");
    output.push_str("    if abs(out) > _INTEGER_CAP:\n");
    output.push_str("        raise ValueError(\"integer exceeds +/-(2**53-1) cap\")\n");
    output.push_str("    return out\n\n\n");
    output.push_str(
        "SpecInt: typing.TypeAlias = typing.Annotated[int, pydantic.functional_validators.BeforeValidator(_parse_spec_integer)]\n",
    );
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

fn render_typed_map_model_methods(output: &mut String, schema: &Schema, value_schema: &Schema) {
    if let Some(max_properties) = schema.max_properties {
        output.push_str("\n    _MAX_PROPERTIES: typing.ClassVar[int] = ");
        output.push_str(&max_properties.to_string());
        output.push('\n');
    }

    output.push_str("\n    @pydantic.model_validator(mode=\"after\")\n");
    output.push_str("    def _validate_extras(self) -> typing.Any:\n");
    output.push_str("        extra = typing.cast(dict[str, object], self.model_extra or {})\n");
    output.push_str("        errors: list[pydantic_core.InitErrorDetails] = []\n");
    render_typed_map_value_validator(output, value_schema);
    if schema.max_properties.is_some() {
        output.push_str("        if len(extra) > self._MAX_PROPERTIES:\n");
        output.push_str("            errors.append(\n");
        output.push_str("                pydantic_core.InitErrorDetails(\n");
        output.push_str("                    type=pydantic_core.PydanticCustomError(\n");
        output.push_str(
            "                        \"too_many_properties\", typing.cast(typing.Any, f\"at most {self._MAX_PROPERTIES} properties allowed\")\n",
        );
        output.push_str("                    ),\n");
        output.push_str("                    loc=(),\n");
        output.push_str("                    input=len(extra),\n");
        output.push_str("                )\n");
        output.push_str("            )\n");
    }
    output.push_str("        if errors:\n");
    output.push_str("            raise pydantic.ValidationError.from_exception_data(\n");
    output.push_str("                title=type(self).__name__, line_errors=errors\n");
    output.push_str("            )\n");
    output.push_str("        return self\n\n");
    output.push_str("    @pydantic.model_serializer(mode=\"wrap\")\n");
    output.push_str("    def _serialize(\n");
    output.push_str("        self,\n");
    output.push_str("        _handler: typing.Callable[[pydantic.BaseModel], typing.Any],\n");
    output.push_str("    ) -> dict[str, object]:\n");
    output
        .push_str("        return dict(typing.cast(dict[str, object], self.model_extra or {}))\n");
}

fn render_typed_map_value_validator(output: &mut String, value_schema: &Schema) {
    if schema_type_includes(value_schema, "string") {
        output.push_str("        for key, value in extra.items():\n");
        output.push_str("            if not isinstance(value, str):\n");
        output.push_str("                errors.append(\n");
        output.push_str("                    pydantic_core.InitErrorDetails(\n");
        output.push_str("                        type=pydantic_core.PydanticCustomError(\n");
        output.push_str("                            \"string_type\", \"expected string value\"\n");
        output.push_str("                        ),\n");
        output.push_str("                        loc=(key,),\n");
        output.push_str("                        input=value,\n");
        output.push_str("                    )\n");
        output.push_str("                )\n");
    }
}

fn render_optional_non_nullable_helper(output: &mut String) {
    output.push_str("def reject_explicit_null(\n");
    output.push_str("    cls: type[pydantic.BaseModel],\n");
    output.push_str("    data: object,\n");
    output.push_str("    handler: typing.Callable[[object], typing.Any],\n");
    output.push_str(") -> typing.Any:\n");
    output.push_str(
        "    null_fields = typing.cast(frozenset[str], getattr(cls, \"_OPTIONAL_NON_NULLABLE_FIELDS\"))\n",
    );
    output.push_str("    raw_data = data\n");
    output.push_str("    pre_errors: list[pydantic_core.InitErrorDetails] = []\n");
    output.push_str("    if isinstance(data, dict):\n");
    output.push_str("        values = typing.cast(dict[str, object], data)\n");
    output.push_str("        pre_errors = [\n");
    output.push_str("            pydantic_core.InitErrorDetails(\n");
    output.push_str("                type=pydantic_core.PydanticCustomError(\n");
    output
        .push_str("                    \"null_for_nonnullable\", \"explicit null not allowed\"\n");
    output.push_str("                ),\n");
    output.push_str("                loc=(field,),\n");
    output.push_str("                input=None,\n");
    output.push_str("            )\n");
    output.push_str("            for field in null_fields\n");
    output.push_str("            if field in values and values[field] is None\n");
    output.push_str("        ]\n");
    output.push_str("    try:\n");
    output.push_str("        instance = handler(raw_data)\n");
    output.push_str("    except pydantic.ValidationError as error:\n");
    output.push_str("        field_errors: list[pydantic_core.InitErrorDetails] = []\n");
    output.push_str(
        "        for error_detail in typing.cast(list[dict[str, object]], error.errors()):\n",
    );
    output.push_str("            loc: tuple[str | int, ...] = tuple(\n");
    output.push_str(
        "                typing.cast(collections.abc.Iterable[str | int], error_detail[\"loc\"])\n",
    );
    output.push_str("            )\n");
    output.push_str("            field_errors.append(\n");
    output.push_str("                pydantic_core.InitErrorDetails(\n");
    output.push_str("                    type=pydantic_core.PydanticCustomError(\n");
    output.push_str("                        typing.cast(typing.Any, error_detail[\"type\"]),\n");
    output.push_str("                        typing.cast(typing.Any, error_detail[\"msg\"]),\n");
    output.push_str("                    ),\n");
    output.push_str("                    loc=loc,\n");
    output.push_str("                    input=error_detail.get(\"input\"),\n");
    output.push_str("                )\n");
    output.push_str("            )\n");
    output.push_str("        raise pydantic.ValidationError.from_exception_data(\n");
    output.push_str("            title=cls.__name__, line_errors=pre_errors + field_errors\n");
    output.push_str("        ) from None\n");
    output.push_str("    if pre_errors:\n");
    output.push_str("        raise pydantic.ValidationError.from_exception_data(\n");
    output.push_str("            title=cls.__name__, line_errors=pre_errors\n");
    output.push_str("        )\n");
    output.push_str("    return instance\n");
}

fn render_set_fields_helper(output: &mut String) {
    output.push_str("def emit_set_fields(\n");
    output.push_str("    model: pydantic.BaseModel,\n");
    output.push_str("    handler: typing.Callable[[pydantic.BaseModel], typing.Any],\n");
    output.push_str(") -> dict[str, object]:\n");
    output.push_str("    dumped = typing.cast(dict[str, object], handler(model))\n");
    output.push_str("    alias_of = {\n");
    output.push_str(
        "        name: (field.alias or name) for name, field in type(model).model_fields.items()\n",
    );
    output.push_str("    }\n");
    output.push_str("    keep = {alias_of.get(name, name) for name in model.model_fields_set}\n");
    output.push_str("    out = {key: value for key, value in dumped.items() if key in keep}\n");
    output.push_str("    if model.model_extra:\n");
    output.push_str("        out.update(typing.cast(dict[str, object], model.model_extra))\n");
    output.push_str("    return out\n");
}

fn render_const_validators(output: &mut String, fields: &[(String, String, Value)]) -> Result<()> {
    if fields.is_empty() {
        return Ok(());
    }

    for (json_name, field_name, const_value) in fields {
        let const_literal = python_value_literal(const_value)?;
        let error_message = format!("{json_name} must equal {const_literal}");
        output.push_str("\n    @pydantic.model_validator(mode=\"before\")\n");
        output.push_str("    @classmethod\n");
        output.push_str("    def _inject_const_");
        output.push_str(field_name);
        output.push_str("(\n");
        output.push_str("        cls,\n");
        output.push_str("        data: object,\n");
        output.push_str("    ) -> object:\n");
        output.push_str("        if isinstance(data, dict):\n");
        output.push_str("            values = typing.cast(dict[str, object], data)\n");
        output.push_str("            if ");
        output.push_str(&python_string_literal(json_name));
        output.push_str(" not in values");
        if field_name != json_name {
            output.push_str(" and ");
            output.push_str(&python_string_literal(field_name));
            output.push_str(" not in values");
        }
        output.push_str(":\n");
        output.push_str("                data = {**values, ");
        output.push_str(&python_string_literal(json_name));
        output.push_str(": ");
        output.push_str(&const_literal);
        output.push_str("}\n");
        output.push_str("            elif values.get(");
        output.push_str(&python_string_literal(json_name));
        output.push_str(", values.get(");
        output.push_str(&python_string_literal(field_name));
        output.push_str(")) != ");
        output.push_str(&const_literal);
        output.push_str(":\n");
        output.push_str("                raise pydantic_core.PydanticCustomError(\n");
        output.push_str("                    \"const\", ");
        output.push_str(&python_string_literal(&error_message));
        output.push_str("\n");
        output.push_str("                )\n");
        output.push_str("        return typing.cast(object, data)\n");
    }

    Ok(())
}

fn render_optional_non_nullable_validator(output: &mut String, fields: &BTreeSet<String>) {
    if fields.is_empty() {
        return;
    }

    output.push_str(
        "\n    _OPTIONAL_NON_NULLABLE_FIELDS: typing.ClassVar[frozenset[str]] = frozenset({",
    );
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&python_string_literal(field));
    }
    output.push_str("})\n\n");
    output.push_str("    @pydantic.model_validator(mode=\"wrap\")\n");
    output.push_str("    @classmethod\n");
    output.push_str("    def _reject_null(\n");
    output.push_str("        cls,\n");
    output.push_str("        data: object,\n");
    output.push_str("        handler: typing.Callable[[object], typing.Any],\n");
    output.push_str("    ) -> typing.Any:\n");
    output.push_str("        return reject_explicit_null(cls, data, handler)\n");
}

fn render_set_fields_serializer(output: &mut String) {
    output.push_str("\n    @pydantic.model_serializer(mode=\"wrap\")\n");
    output.push_str("    def _serialize(\n");
    output.push_str("        self,\n");
    output.push_str("        handler: typing.Callable[[pydantic.BaseModel], typing.Any],\n");
    output.push_str("    ) -> dict[str, object]:\n");
    output.push_str("        return emit_set_fields(self, handler)\n");
}

fn render_field_expr(
    output: &mut String,
    json_name: &str,
    field_name: &str,
    default: Option<&str>,
) {
    output.push_str("pydantic.Field(");
    let mut arguments = Vec::new();
    if let Some(default) = default {
        arguments.push(format!("default={default}"));
    }
    if json_name != field_name {
        arguments.push(format!("alias={}", python_string_literal(json_name)));
    }
    output.push_str(&arguments.join(", "));
    output.push(')');
}

fn python_value_literal(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("None".to_string()),
        Value::Bool(value) => Ok(if *value { "True" } else { "False" }.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(python_string_literal(value)),
        Value::Array(values) => {
            let values = values
                .iter()
                .map(python_value_literal)
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("[{}]", values.join(", ")))
        }
        Value::Object(values) => {
            let values = values
                .iter()
                .map(|(key, value)| {
                    Ok(format!(
                        "{}: {}",
                        python_string_literal(key),
                        python_value_literal(value)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("{{{}}}", values.join(", ")))
        }
    }
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

fn annotation(schema: &Schema) -> Result<String> {
    if let Some(const_value) = &schema.const_value
        && let Some(annotation) = python_literal_annotation(const_value)
    {
        return Ok(annotation);
    }
    if let Some(reference) = &schema.reference {
        return Ok(reference_model_name(reference));
    }
    if let Some(one_of) = &schema.one_of {
        let non_null = one_of
            .iter()
            .filter(|branch| branch.ty.as_ref().and_then(Value::as_str) != Some("null"))
            .collect::<Vec<_>>();
        let Some(branch) = non_null.first() else {
            return Ok("None".to_string());
        };
        return Ok(optional_annotation(&annotation(branch)?));
    }
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => Ok("str".to_string()),
        Some("integer") => Ok("SpecInt".to_string()),
        Some("number") => Ok("float".to_string()),
        Some("boolean") => Ok("bool".to_string()),
        Some("array") => {
            let item = schema
                .items
                .as_ref()
                .map(|item| annotation(item))
                .transpose()?
                .unwrap_or_else(|| "typing.Any".to_string());
            Ok(format!("list[{item}]"))
        }
        Some("object") => object_annotation(schema),
        Some("null") => Ok("None".to_string()),
        _ => Ok("typing.Any".to_string()),
    }
}

fn python_literal_annotation(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("typing.Literal[None]".to_string()),
        Value::Bool(value) => Some(format!(
            "typing.Literal[{}]",
            if *value { "True" } else { "False" }
        )),
        Value::Number(value) if value.is_i64() || value.is_u64() => {
            Some(format!("typing.Literal[{value}]"))
        }
        Value::String(value) => Some(format!("typing.Literal[{}]", python_string_literal(value))),
        _ => None,
    }
}

fn allows_null(schema: &Schema) -> bool {
    schema.const_value.as_ref() == Some(&Value::Null)
        || schema_type_includes(schema, "null")
        || schema
            .one_of
            .as_ref()
            .is_some_and(|branches| branches.iter().any(allows_null))
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

fn schema_uses_integer(schema: &Schema) -> bool {
    schema_type_includes(schema, "integer")
        || schema
            .properties
            .as_ref()
            .is_some_and(|properties| properties.values().any(schema_uses_integer))
        || schema
            .items
            .as_ref()
            .is_some_and(|items| schema_uses_integer(items))
        || schema
            .one_of
            .as_ref()
            .is_some_and(|branches| branches.iter().any(schema_uses_integer))
        || schema
            .additional_properties
            .as_ref()
            .and_then(|additional_properties| {
                serde_json::from_value::<Schema>(additional_properties.clone()).ok()
            })
            .is_some_and(|additional_properties| schema_uses_integer(&additional_properties))
}

fn object_annotation(schema: &Schema) -> Result<String> {
    if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        return Ok("dict[str, typing.Any]".to_string());
    }
    match &schema.additional_properties {
        Some(Value::Object(_)) => {
            let additional: Schema = serde_json::from_value(
                schema
                    .additional_properties
                    .clone()
                    .expect("additional properties presence checked"),
            )
            .map_err(|error| Error::InvalidJsonSchema {
                path: PathBuf::from("<json-generator>"),
                reason: format!("failed to read `additionalProperties`: {error}"),
            })?;
            Ok(format!("dict[str, {}]", annotation(&additional)?))
        }
        _ => Ok("dict[str, typing.Any]".to_string()),
    }
}

fn optional_annotation(annotation: &str) -> String {
    if annotation.contains(" | None") || annotation == "None" {
        annotation.to_string()
    } else {
        format!("{annotation} | None")
    }
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
