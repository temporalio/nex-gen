use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use heck::{ToShoutySnakeCase, ToUpperCamelCase};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::generator::ExternalModelBackend;
use crate::generator::json_schema::build_json_name_manifest;
use crate::generator::python::{
    PythonImports, PythonModelHoists, RenderedModelFragments, WireValueConversion,
    module_common_prefix_len, python_field_name, python_string_literal,
    render_generated_file_header, render_named_python_import, render_optional_python_imports,
    render_python_docstring,
};
use crate::language::Language;
use crate::parser::NameManifest;
use crate::planning::{PlannedFamily, PlannedJsonType, PlannedSpec};
use crate::spec::{ApiSpecBranch, ApiSpecNode};
use crate::spec::{ExternalTypeSpec, ModulePath, RecordSpec};

#[derive(Debug, Deserialize, Default)]
struct Schema {
    #[serde(rename = "$ref")]
    reference: Option<String>,
    #[serde(rename = "type")]
    ty: Option<Value>,
    title: Option<String>,
    description: Option<String>,
    deprecated: Option<bool>,
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
    #[serde(rename = "minProperties")]
    min_properties: Option<usize>,
    #[serde(rename = "propertyNames")]
    property_names: Option<Box<Schema>>,
    #[serde(rename = "dependentRequired")]
    dependent_required: Option<IndexMap<String, Vec<String>>>,
    minimum: Option<serde_json::Number>,
    maximum: Option<serde_json::Number>,
    #[serde(rename = "exclusiveMinimum")]
    exclusive_minimum: Option<serde_json::Number>,
    #[serde(rename = "exclusiveMaximum")]
    exclusive_maximum: Option<serde_json::Number>,
    #[serde(rename = "multipleOf")]
    multiple_of: Option<serde_json::Number>,
    #[serde(rename = "minLength")]
    min_length: Option<u64>,
    #[serde(rename = "maxLength")]
    max_length: Option<u64>,
    pattern: Option<String>,
    format: Option<String>,
    #[serde(rename = "contentEncoding")]
    content_encoding: Option<String>,
    #[serde(rename = "minItems")]
    min_items: Option<u64>,
    #[serde(rename = "maxItems")]
    max_items: Option<u64>,
    #[serde(rename = "uniqueItems")]
    unique_items: Option<bool>,
    contains: Option<Box<Schema>>,
    #[serde(rename = "minContains")]
    min_contains: Option<u64>,
    #[serde(rename = "maxContains")]
    max_contains: Option<u64>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<Value>>,
    #[serde(rename = "x-py-name")]
    x_py_name: Option<String>,
}

impl Schema {
    /// The emitted Python attribute identifier for a property: the `x-py-name`
    /// override if present (used verbatim), otherwise the snake-cased JSON name.
    /// The wire name (`json_name`) is unaffected — the `Field(alias=...)` pin
    /// keeps the contract stable. See specs/json-schema/features/properties.md.
    fn py_member_name(&self, json_name: &str) -> String {
        self.x_py_name
            .clone()
            .unwrap_or_else(|| python_field_name(json_name))
    }
}

impl Schema {
    fn is_integer_field(&self) -> bool {
        self.ty.as_ref().and_then(Value::as_str) == Some("integer")
    }

    fn is_array_field(&self) -> bool {
        self.ty.as_ref().and_then(Value::as_str) == Some("array")
    }

    /// True when the array field needs a custom after-validator: `uniqueItems`
    /// and `contains` (with `minContains`/`maxContains`) have no native Pydantic
    /// equivalent. `minItems`/`maxItems` map to native `min_length`/`max_length`
    /// and are handled in the field expression instead.
    fn needs_array_validator(&self) -> bool {
        self.is_array_field() && (self.unique_items == Some(true) || self.contains.is_some())
    }

    fn is_number_field(&self) -> bool {
        self.ty.as_ref().and_then(Value::as_str) == Some("number")
    }

    fn is_string_field(&self) -> bool {
        self.ty.as_ref().and_then(Value::as_str) == Some("string")
    }

    /// Number-field `multipleOf` uses an explicit `math.fmod` AfterValidator
    /// (not Pydantic's tolerant native `multiple_of`) for bit-identical
    /// divisibility across the four targets — see `multipleOf.md`.
    fn number_multiple_of(&self) -> Option<&serde_json::Number> {
        if self.is_number_field() {
            self.multiple_of.as_ref()
        } else {
            None
        }
    }

    /// True when the declared-property object carries a member-count or
    /// cross-field object constraint that lowers to a custom model validator.
    fn has_object_count_or_dependency(&self) -> bool {
        self.min_properties.is_some()
            || self.max_properties.is_some()
            || self.dependent_required.is_some()
    }
}

fn py_bound_literal(number: &serde_json::Number, is_integer: bool) -> String {
    if is_integer && let Some(value) = number.as_f64() {
        return (value.trunc() as i64).to_string();
    }
    number.to_string()
}

thread_local! {
    /// Resolved type identifiers keyed by both `full_name` and the `#/$defs/<full_name>`
    /// `$ref` form, so `reference_model_name` follows the same name manifest as the
    /// declaration (honoring `x-py-name` overrides) instead of recasing the ref segment.
    /// Generation is single-threaded per file, so a thread-local avoids threading the map
    /// through every recursive `annotation` call that resolves a `$ref`.
    static REF_NAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

fn set_ref_names(ref_names: &BTreeMap<String, String>) {
    REF_NAMES.with(|cell| cell.borrow_mut().clone_from(ref_names));
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
    /// Resolved emitted identifiers (with `x-py-name` overrides applied).
    manifest: NameManifest,
    /// Resolved type names keyed by `full_name` and the `#/$defs/<full_name>` ref form.
    ref_names: BTreeMap<String, String>,
}

impl ExternalModelBackend<PlannedJsonType> for ModelBackend {
    type ModelFragments = RenderedModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.tree_leaf = !api_plan.module_path.is_root();
        self.runtime_import_module = if self.tree_leaf {
            root_python_runtime_module(&api_plan.module_path)
        } else {
            "._definitions".to_string()
        };
        // Resolve every emitted identifier once (overrides applied), then adopt the
        // resolved type name as each model's `model_name` so every downstream derivation
        // (class decl, union `TypeAlias`, `model_type_ref`) follows the same identifier.
        // `$ref` targets are resolved via `ref_names` below.
        self.manifest = build_json_name_manifest(Language::Python, api_plan)?;
        let mut json_models: Vec<PlannedJsonType> = api_plan
            .external_types()
            .map(|(_, binding)| binding)
            .filter_map(|binding| match &binding.external_type {
                ExternalTypeSpec::Json(json_type) => Some(json_type.clone()),
                _ => None,
            })
            .map(|mut json_type| {
                if let Some(resolved) = self.manifest.type_name(&json_type.full_name) {
                    json_type.model_name = resolved.to_string();
                }
                json_type
            })
            .collect();
        self.json_models = std::mem::take(&mut json_models);
        self.hoisted_json_models = Vec::new();
        self.ref_names.clear();
        for model in &self.json_models {
            // A resolved `$ref` is `#/$defs/<full_name>`; register that form (plus the
            // bare `full_name`) so `reference_model_name` resolves through the manifest
            // instead of recasing the ref segment (which would drop a type override).
            self.ref_names
                .insert(model.full_name.clone(), model.model_name.clone());
            self.ref_names.insert(
                format!("#/$defs/{}", model.full_name),
                model.model_name.clone(),
            );
        }
        Ok(())
    }

    fn render_models(&self) -> Result<RenderedModelFragments> {
        set_ref_names(&self.ref_names);
        let json_models = self.json_models.iter().collect::<Vec<_>>();
        render_external_models(json_models.as_slice(), &self.runtime_import_module)
    }

    fn render_support_files(&self) -> Result<BTreeMap<PathBuf, String>> {
        if self.tree_leaf || self.json_models.is_empty() {
            return Ok(BTreeMap::new());
        }

        Ok(BTreeMap::from([(
            PathBuf::from("_definitions.py"),
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
        _planned_record: Option<&RecordSpec<PlannedFamily>>,
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
    fn for_tree(branch: &ApiSpecBranch<PlannedFamily>) -> Self {
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
    branch: &ApiSpecBranch<PlannedFamily>,
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
        PathBuf::from("_recursive.py"),
        render_hoisted_models_module(&plan)?,
    );
    hoists.add_exported_names(
        plan.hoisted_models
            .iter()
            .map(|model| model.model_name.clone())
            .collect(),
    );
    Ok(hoists)
}

fn render_hoisted_models_module(hoists: &JsonModelHoistPlan) -> Result<String> {
    let models = hoists.hoisted_models.iter().collect::<Vec<_>>();
    let model_fragments = render_external_models(models.as_slice(), "._definitions")?;
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
    branch: &ApiSpecBranch<PlannedFamily>,
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
    format!("{}{}", ".".repeat(module_path.0.len() + 1), "_definitions")
}

pub(in crate::generator) fn render_external_models(
    json_models: &[&PlannedJsonType],
    runtime_import_module: &str,
) -> Result<RenderedModelFragments> {
    if json_models.is_empty() {
        return Ok(RenderedModelFragments::default());
    }

    // Partition class models from `oneOf` sum-type union defs. Union defs are
    // emitted as `typing.Union[...]` TypeAliases *after* all classes so their
    // eager `Union[...]` expression sees every member class defined.
    let class_models: Vec<&PlannedJsonType> = json_models
        .iter()
        .copied()
        .filter(|model| !is_python_union_model(model))
        .collect();
    let union_models: Vec<&PlannedJsonType> = json_models
        .iter()
        .copied()
        .filter(|model| is_python_union_model(model))
        .collect();

    let mut models_body = String::new();
    let mut needs_optional_non_nullable_helper = false;
    let mut needs_set_fields_helper = false;
    let mut needs_pydantic_core = false;
    let mut needs_spec_int_helper = false;
    let mut needs_multiple_of_helper = false;
    let mut needs_pattern_helper = false;
    let mut needs_format_helper = false;
    for (index, model) in class_models.iter().enumerate() {
        render_model(
            &mut models_body,
            model,
            &mut needs_optional_non_nullable_helper,
            &mut needs_set_fields_helper,
            &mut needs_pydantic_core,
            &mut needs_spec_int_helper,
            &mut needs_multiple_of_helper,
            &mut needs_pattern_helper,
            &mut needs_format_helper,
        )?;
        if index + 1 != class_models.len() {
            models_body.push_str("\n\n");
        }
    }
    for model in &union_models {
        let schema = decode_schema(model)?;
        needs_spec_int_helper |= schema_uses_integer(&schema);
        models_body.push_str("\n\n\n");
        render_python_docstring(
            &mut models_body,
            "",
            schema.description.as_deref(),
            &[],
            None,
            false,
        );
        models_body.push_str(&model.model_name);
        models_body.push_str(": typing.TypeAlias = ");
        models_body.push_str(&annotation(&schema)?);
        models_body.push('\n');
    }
    let mut body = String::new();
    body.push_str(&models_body);

    let mut post_model_statements = String::new();
    render_cyclic_model_rebuilds(&mut post_model_statements, json_models);
    render_union_ref_rebuilds(&mut post_model_statements, &class_models, &union_models);
    render_map_member_adapters(
        &mut post_model_statements,
        &class_models,
        &union_models
            .iter()
            .map(|model| model.model_name.clone())
            .collect(),
        &mut needs_multiple_of_helper,
        &mut needs_pattern_helper,
        &mut needs_format_helper,
    )?;
    let mut module_imports = BTreeSet::from(["pydantic".to_string()]);
    if needs_pydantic_core {
        module_imports.insert("pydantic_core".to_string());
    }
    let mut relative_imports = BTreeMap::<String, BTreeSet<String>>::new();
    let mut runtime_imports = BTreeSet::new();
    if needs_spec_int_helper || post_model_statements.contains("SpecInt") {
        runtime_imports.insert("SpecInt".to_string());
    }
    if needs_multiple_of_helper {
        runtime_imports.insert("_check_multiple_of".to_string());
    }
    if needs_pattern_helper {
        runtime_imports.insert("_check_pattern".to_string());
    }
    if needs_format_helper {
        runtime_imports.insert("_check_format".to_string());
    }
    // Import the materialized-temporal / bytes field aliases actually referenced
    // by the rendered module (defined once in the runtime module) — by a model's
    // own field, or by a map's member adapter.
    for alias in [
        "DateTimeField",
        "DateField",
        "TimeField",
        "DurationField",
        "Base64Field",
        "Base64UrlField",
    ] {
        if models_body.contains(alias) || post_model_statements.contains(alias) {
            runtime_imports.insert(alias.to_string());
        }
    }
    if needs_optional_non_nullable_helper {
        runtime_imports.insert("_reject_explicit_null".to_string());
    }
    if needs_set_fields_helper {
        runtime_imports.insert("_emit_set_fields".to_string());
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
        allows_private_wire_access: false,
    })
}

/// True when a model's schema is a `oneOf` sum type (two or more non-null
/// branches) — emitted as a `typing.Union` TypeAlias, not a Pydantic class.
fn is_python_union_model(model: &PlannedJsonType) -> bool {
    decode_schema(model).is_ok_and(|schema| {
        schema.one_of.as_ref().is_some_and(|branches| {
            branches
                .iter()
                .filter(|branch| branch.ty.as_ref().and_then(Value::as_str) != Some("null"))
                .count()
                >= 2
        })
    })
}

fn schema_references_union(schema: &Schema, union_names: &BTreeSet<String>) -> bool {
    if let Some(reference) = &schema.reference
        && union_names.contains(&reference_model_name(reference))
    {
        return true;
    }
    if let Some(properties) = &schema.properties
        && properties
            .values()
            .any(|property| schema_references_union(property, union_names))
    {
        return true;
    }
    if let Some(items) = &schema.items
        && schema_references_union(items, union_names)
    {
        return true;
    }
    if let Some(one_of) = &schema.one_of
        && one_of
            .iter()
            .any(|branch| schema_references_union(branch, union_names))
    {
        return true;
    }
    if let Some(additional) = &schema.additional_properties
        && let Ok(additional_schema) = serde_json::from_value::<Schema>(additional.clone())
        && schema_references_union(&additional_schema, union_names)
    {
        return true;
    }
    false
}

/// A class model referencing a named union def carries a deferred (`from
/// __future__ import annotations`) `field: Union` annotation the alias only
/// satisfies once defined (after all classes), so rebuild it here.
fn render_union_ref_rebuilds(
    output: &mut String,
    class_models: &[&PlannedJsonType],
    union_models: &[&PlannedJsonType],
) {
    if union_models.is_empty() {
        return;
    }
    let union_names: BTreeSet<String> = union_models
        .iter()
        .map(|model| model.model_name.clone())
        .collect();
    for model in class_models {
        let Ok(schema) = decode_schema(model) else {
            continue;
        };
        if schema_references_union(&schema, &union_names) {
            output.push_str("_ = ");
            output.push_str(&model.model_name);
            output.push_str(".model_rebuild()\n");
        }
    }
}

fn render_cyclic_model_rebuilds(output: &mut String, models: &[&PlannedJsonType]) {
    let local_models = models
        .iter()
        .map(|model| {
            (
                model.full_name.clone(),
                (ModulePath::default(), (*model).clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let graph = models
        .iter()
        .map(|model| {
            let mut refs = BTreeSet::new();
            collect_json_schema_model_refs(&model.schema, &local_models, &mut refs);
            (model.full_name.clone(), refs)
        })
        .collect::<BTreeMap<_, _>>();

    for model in models {
        let Some(refs) = graph.get(&model.full_name) else {
            continue;
        };
        if refs.iter().any(|reference| {
            json_model_can_reach(reference, &model.full_name, &graph, &mut BTreeSet::new())
        }) {
            output.push_str("_ = ");
            output.push_str(&model.model_name);
            output.push_str(".model_rebuild()\n");
        }
    }
}

fn render_json_runtime_module() -> String {
    let mut output = String::new();
    output.push_str("# Generated by nexgen. DO NOT EDIT!\n\n");
    output.push_str("from __future__ import annotations\n\n");
    output.push_str("import base64\n");
    output.push_str("import collections.abc\n");
    output.push_str("import datetime\n");
    output.push_str("import math\n");
    output.push_str("import re\n");
    output.push_str("import typing\n\n");
    output.push_str("import pydantic\n");
    output.push_str("import pydantic.functional_validators\n");
    output.push_str("import pydantic_core\n\n\n");
    // The underscore-prefixed helpers below are still imported by sibling
    // generated modules (e.g. `models.py`); listing them keeps type checkers
    // from flagging them as unused private symbols.
    output.push_str("__all__ = [\n");
    for name in [
        "SpecInt",
        "DateTimeField",
        "DateField",
        "TimeField",
        "DurationField",
        "Base64Field",
        "Base64UrlField",
        "_check_multiple_of",
        "_check_pattern",
        "_check_format",
        "_reject_explicit_null",
        "_emit_set_fields",
    ] {
        output.push_str("    \"");
        output.push_str(name);
        output.push_str("\",\n");
    }
    output.push_str("]\n\n\n");
    render_spec_int_helper(&mut output);
    output.push_str("\n\n");
    render_multiple_of_helper(&mut output);
    output.push_str("\n\n");
    render_pattern_helper(&mut output);
    output.push_str("\n\n");
    render_format_helper(&mut output);
    output.push_str("\n\n");
    render_temporal_helpers(&mut output);
    output.push_str("\n\n");
    render_content_encoding_helpers(&mut output);
    output.push_str("\n\n");
    render_optional_non_nullable_helper(&mut output);
    output.push_str("\n\n");
    render_set_fields_helper(&mut output);
    output
}

/// Emits the materialized-temporal runtime: the pinned narrowed regexes, the
/// Gregorian calendar predicate, the parse (`BeforeValidator`) + generator-owned
/// serialize (`PlainSerializer`) adapters, and the four `Annotated` field
/// aliases (`DateTimeField` / `DateField` / `TimeField` / `DurationField`). See
/// `specs/json-schema/features/format.md`. We do NOT use Pydantic's native `datetime`
/// coercion (it accepts a missing offset and normalizes differently).
fn render_temporal_helpers(output: &mut String) {
    use crate::json_schema::format::TemporalKind;
    output.push_str(&format!(
        "_TEMPORAL_DATE_TIME_RE = re.compile(r\"{}\")\n",
        TemporalKind::DateTime.pattern()
    ));
    output.push_str(&format!(
        "_TEMPORAL_DATE_RE = re.compile(r\"{}\")\n",
        TemporalKind::Date.pattern()
    ));
    output.push_str(&format!(
        "_TEMPORAL_TIME_RE = re.compile(r\"{}\")\n",
        TemporalKind::Time.pattern()
    ));
    output.push_str(&format!(
        "_TEMPORAL_DURATION_RE = re.compile(r\"{}\")\n",
        TemporalKind::Duration.pattern()
    ));
    output.push_str(TEMPORAL_HELPER_BODY);
}

const TEMPORAL_HELPER_BODY: &str = r#"_TEMPORAL_MAX_DURATION_SECONDS = ((1 << 63) - 1) // 1_000_000_000


def _days_in_month(year: int, month: int) -> int:
    if month in (1, 3, 5, 7, 8, 10, 12):
        return 31
    if month in (4, 6, 9, 11):
        return 30
    if month == 2:
        return 29 if (year % 4 == 0 and year % 100 != 0) or year % 400 == 0 else 28
    return 0


def _valid_temporal_calendar(value: str) -> bool:
    if len(value) < 10:
        return False
    try:
        year, month, day = int(value[0:4]), int(value[5:7]), int(value[8:10])
    except ValueError:
        return False
    maximum = _days_in_month(year, month)
    return maximum > 0 and 1 <= day <= maximum


def _parse_date_time(value: object) -> object:
    if not isinstance(value, str):
        return value
    if _TEMPORAL_DATE_TIME_RE.match(value) is None or not _valid_temporal_calendar(value):
        raise ValueError(f"must be a valid date-time, got {value!r}")
    normalized = value.upper()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    return datetime.datetime.fromisoformat(normalized)


def _parse_date(value: object) -> object:
    if not isinstance(value, str):
        return value
    if _TEMPORAL_DATE_RE.match(value) is None or not _valid_temporal_calendar(value):
        raise ValueError(f"must be a valid date, got {value!r}")
    return datetime.date.fromisoformat(value)


def _parse_time(value: object) -> object:
    if not isinstance(value, str):
        return value
    if _TEMPORAL_TIME_RE.match(value) is None:
        raise ValueError(f"must be a valid time, got {value!r}")
    normalized = value.upper()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    return datetime.time.fromisoformat(normalized)


def _parse_duration(value: object) -> object:
    if not isinstance(value, str):
        return value
    if _TEMPORAL_DURATION_RE.match(value) is None:
        raise ValueError(f"must be a valid duration, got {value!r}")
    total = 0
    number = ""
    for char in value[2:]:
        if char.isdigit():
            number += char
            continue
        total += int(number) * {"H": 3600, "M": 60, "S": 1}[char]
        number = ""
        if total > _TEMPORAL_MAX_DURATION_SECONDS:
            raise ValueError(f"must be a valid duration, got {value!r}")
    return datetime.timedelta(seconds=total)


def _temporal_frac(microsecond: int) -> str:
    if microsecond == 0:
        return ""
    return "." + f"{microsecond:06d}".rstrip("0")


def _temporal_offset(value: datetime.datetime | datetime.time) -> str:
    offset = value.utcoffset()
    if offset is None:
        return ""
    total = int(offset.total_seconds())
    if total == 0:
        return "Z"
    sign = "+" if total > 0 else "-"
    total = abs(total)
    return f"{sign}{total // 3600:02d}:{(total % 3600) // 60:02d}"


def _format_date_time(value: datetime.datetime) -> str:
    return (
        f"{value.year:04d}-{value.month:02d}-{value.day:02d}"
        f"T{value.hour:02d}:{value.minute:02d}:{value.second:02d}"
        f"{_temporal_frac(value.microsecond)}{_temporal_offset(value)}"
    )


def _format_date(value: datetime.date) -> str:
    return f"{value.year:04d}-{value.month:02d}-{value.day:02d}"


def _format_time(value: datetime.time) -> str:
    return (
        f"{value.hour:02d}:{value.minute:02d}:{value.second:02d}"
        f"{_temporal_frac(value.microsecond)}{_temporal_offset(value)}"
    )


def _format_duration(value: datetime.timedelta) -> str:
    total = int(value.total_seconds())
    if total == 0:
        return "PT0S"
    hours, remainder = divmod(total, 3600)
    minutes, seconds = divmod(remainder, 60)
    out = "PT"
    if hours:
        out += f"{hours}H"
    if minutes:
        out += f"{minutes}M"
    if seconds:
        out += f"{seconds}S"
    return out


DateTimeField: typing.TypeAlias = typing.Annotated[
    datetime.datetime,
    pydantic.BeforeValidator(_parse_date_time),
    pydantic.PlainSerializer(_format_date_time, return_type=str),
]
DateField: typing.TypeAlias = typing.Annotated[
    datetime.date,
    pydantic.BeforeValidator(_parse_date),
    pydantic.PlainSerializer(_format_date, return_type=str),
]
TimeField: typing.TypeAlias = typing.Annotated[
    datetime.time,
    pydantic.BeforeValidator(_parse_time),
    pydantic.PlainSerializer(_format_time, return_type=str),
]
DurationField: typing.TypeAlias = typing.Annotated[
    datetime.timedelta,
    pydantic.BeforeValidator(_parse_duration),
    pydantic.PlainSerializer(_format_duration, return_type=str),
]
"#;

/// Emits the materialized-`contentEncoding` runtime: the pinned canonical
/// base64 / base64url regexes (the validity oracle), the parse
/// (`BeforeValidator`) + generator-owned canonical serialize (`PlainSerializer`)
/// adapters, and the two `Annotated` bytes field aliases. We own the codec via
/// the model hooks rather than lean on Pydantic's `Base64Bytes`, for full control
/// of the accept/reject line and the canonical output. See
/// `specs/json-schema/features/contentEncoding.md`.
fn render_content_encoding_helpers(output: &mut String) {
    use crate::json_schema::content_encoding::Encoding;
    output.push_str(&format!(
        "_BASE64_RE = re.compile({}, re.ASCII)\n",
        python_string_literal(&crate::json_schema::pattern::rewrite_end_anchor(
            Encoding::Base64.pattern(),
            r"\Z"
        ))
    ));
    output.push_str(&format!(
        "_BASE64URL_RE = re.compile({}, re.ASCII)\n",
        python_string_literal(&crate::json_schema::pattern::rewrite_end_anchor(
            Encoding::Base64Url.pattern(),
            r"\Z"
        ))
    ));
    output.push_str(CONTENT_ENCODING_HELPER_BODY);
}

const CONTENT_ENCODING_HELPER_BODY: &str = r#"

def _parse_base64(value: typing.Any) -> bytes:
    if isinstance(value, bytes):
        return value
    if not isinstance(value, str) or _BASE64_RE.match(value) is None:
        raise ValueError(f"must be base64-encoded, got {value!r}")
    return base64.b64decode(value, validate=True)


def _format_base64(value: bytes) -> str:
    return base64.b64encode(value).decode("ascii")


def _parse_base64url(value: typing.Any) -> bytes:
    if isinstance(value, bytes):
        return value
    if not isinstance(value, str) or _BASE64URL_RE.match(value) is None:
        raise ValueError(f"must be base64url-encoded, got {value!r}")
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


def _format_base64url(value: bytes) -> str:
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")


Base64Field: typing.TypeAlias = typing.Annotated[
    bytes,
    pydantic.BeforeValidator(_parse_base64),
    pydantic.PlainSerializer(_format_base64, return_type=str),
]
Base64UrlField: typing.TypeAlias = typing.Annotated[
    bytes,
    pydantic.BeforeValidator(_parse_base64url),
    pydantic.PlainSerializer(_format_base64url, return_type=str),
]
"#;

fn render_multiple_of_helper(output: &mut String) {
    output.push_str("def _check_multiple_of(\n");
    output.push_str("    divisor: float,\n");
    output.push_str(") -> typing.Callable[[float], float]:\n");
    output.push_str(
        "    \"\"\"Builds an AfterValidator asserting `math.fmod`-exact divisibility for number fields.\"\"\"\n",
    );
    output.push_str("\n");
    output.push_str("    def validate(value: float) -> float:\n");
    output.push_str("        if math.fmod(value, divisor) != 0:\n");
    output.push_str(
        "            raise ValueError(f\"must be a multiple of {divisor}, got {value}\")\n",
    );
    output.push_str("        return value\n");
    output.push_str("\n");
    output.push_str("    return validate\n");
}

fn render_pattern_helper(output: &mut String) {
    output.push_str("def _check_pattern(\n");
    output.push_str("    pattern: str,\n");
    output.push_str(") -> typing.Callable[[str], str]:\n");
    output.push_str(
        "    \"\"\"Builds an AfterValidator asserting an unanchored, ASCII-class regex match for string fields.\"\"\"\n",
    );
    output.push_str("\n");
    output.push_str("    compiled = re.compile(pattern, re.ASCII)\n");
    output.push_str("\n");
    output.push_str("    def validate(value: str) -> str:\n");
    output.push_str("        if compiled.search(value) is None:\n");
    output.push_str(
        "            raise ValueError(f\"must match pattern {pattern}, got {value!r}\")\n",
    );
    output.push_str("        return value\n");
    output.push_str("\n");
    output.push_str("    return validate\n");
}

/// Emits the `_check_format` runtime helper: an AfterValidator that asserts a
/// string matches a pinned `format` regex, with an optional total-length guard
/// run **first** (short-circuit — the email order neutralizes a matcher-recursion
/// hazard). `len(value)` is the Unicode code-point count. See
/// `specs/json-schema/features/format.md`.
fn render_format_helper(output: &mut String) {
    output.push_str("def _check_format(\n");
    output.push_str("    format_name: str,\n");
    output.push_str("    pattern: str,\n");
    output.push_str("    max_code_points: int | None = None,\n");
    output.push_str(") -> typing.Callable[[str], str]:\n");
    output.push_str(
        "    \"\"\"Builds an AfterValidator asserting a value matches a pinned `format` regex (+ optional length guard).\"\"\"\n",
    );
    output.push_str("\n");
    output.push_str("    compiled = re.compile(pattern, re.ASCII)\n");
    output.push_str("\n");
    output.push_str("    def validate(value: str) -> str:\n");
    output.push_str(
        "        if (max_code_points is not None and len(value) > max_code_points) or compiled.search(value) is None:\n",
    );
    output.push_str(
        "            raise ValueError(f\"must be a valid {format_name}, got {value!r}\")\n",
    );
    output.push_str("        return value\n");
    output.push_str("\n");
    output.push_str("    return validate\n");
}

fn render_model(
    output: &mut String,
    model: &PlannedJsonType,
    needs_optional_non_nullable_helper: &mut bool,
    needs_set_fields_helper: &mut bool,
    needs_pydantic_core: &mut bool,
    needs_spec_int_helper: &mut bool,
    needs_multiple_of_helper: &mut bool,
    needs_pattern_helper: &mut bool,
    needs_format_helper: &mut bool,
) -> Result<()> {
    let schema = decode_schema(model)?;
    // A `oneOf` sum-type union def is emitted as a TypeAlias by the caller.
    if is_python_union_model(model) {
        return Ok(());
    }
    *needs_spec_int_helper |= schema_uses_integer(&schema);
    let extra = match schema.additional_properties.as_ref() {
        Some(Value::Bool(false)) => "forbid",
        _ => "allow",
    };
    // Native deprecation marker (PEP 702) on the type; `category=None` is the
    // no-runtime-warning form. See specs/json-schema/features/deprecated.md.
    if schema.deprecated == Some(true) {
        output.push_str(
            "@typing_extensions.deprecated(\"This type is deprecated.\", category=None)\n",
        );
    }
    output.push_str("class ");
    output.push_str(&model.model_name);
    output.push_str("(pydantic.BaseModel):\n");
    render_python_docstring(
        output,
        "    ",
        compose_python_doc(schema.title.as_deref(), schema.description.as_deref()).as_deref(),
        &[],
        None,
        false,
    );
    output.push_str(
        "    model_config: typing.ClassVar[pydantic.ConfigDict] = pydantic.ConfigDict(strict=True, populate_by_name=True, extra=",
    );
    output.push_str(&python_string_literal(extra));
    output.push_str(")\n");

    if is_python_map_model(&schema) {
        // A map-shaped model has no declared fields: its members live in
        // Pydantic's `model_extra`, validated by the generated model validator.
        let value_schema = typed_map_value_schema(&schema)?;
        if value_schema.is_some()
            || schema.min_properties.is_some()
            || schema.max_properties.is_some()
            || schema.property_names.is_some()
        {
            render_map_model_methods(output, &schema, &model.model_name, value_schema.is_some());
            *needs_pydantic_core = true;
        }
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
    let mut enum_fields: Vec<(String, String, Vec<Value>)> = Vec::new();
    let mut array_validator_fields: Vec<(String, String, &Schema)> = Vec::new();
    for (json_name, property) in properties {
        output.push('\n');
        let field_name = property.py_member_name(json_name);
        let mut annotation = refined_annotation(
            property,
            None,
            needs_multiple_of_helper,
            needs_pattern_helper,
            needs_format_helper,
        )?;
        // Native deprecation marker (PEP 702) on the field; `category=None` is
        // the no-runtime-warning form. See specs/json-schema/features/deprecated.md.
        if property.deprecated == Some(true) {
            annotation = format!(
                "typing.Annotated[{annotation}, typing_extensions.deprecated(\"This field is deprecated.\", category=None)]"
            );
        }
        if property.needs_array_validator() {
            array_validator_fields.push((json_name.clone(), field_name.clone(), property));
        }
        if let Some(values) = &property.enum_values {
            enum_fields.push((json_name.clone(), field_name.clone(), values.clone()));
        }
        let required_field = required.contains(json_name);
        output.push_str("    ");
        output.push_str(&field_name);
        output.push_str(": ");
        if let Some(const_value) = &property.const_value {
            const_fields.push((json_name.clone(), field_name.clone(), const_value.clone()));
            output.push_str(&annotation);
            output.push_str(" = ");
            let default = python_value_literal(const_value)?;
            render_field_expr(output, json_name, &field_name, Some(&default), property);
        } else if required_field {
            output.push_str(&annotation);
            output.push_str(" = ");
            render_field_expr(output, json_name, &field_name, None, property);
        } else if let Some(default) = &property.default {
            output.push_str(&annotation);
            output.push_str(" = ");
            let default = python_value_literal(default)?;
            render_field_expr(output, json_name, &field_name, Some(&default), property);
        } else {
            if !allows_null(property) {
                optional_non_nullable_fields.insert(json_name.clone());
                if field_name != *json_name {
                    optional_non_nullable_fields.insert(field_name.clone());
                }
            }
            output.push_str(&optional_annotation(&annotation));
            output.push_str(" = ");
            render_field_expr(output, json_name, &field_name, Some("None"), property);
        }
        output.push('\n');
        render_python_docstring(
            output,
            "    ",
            compose_python_doc(property.title.as_deref(), property.description.as_deref())
                .as_deref(),
            &[],
            None,
            false,
        );
    }
    render_const_validators(output, &const_fields)?;
    render_enum_validators(output, &enum_fields)?;
    render_array_validators(output, &array_validator_fields)?;
    render_object_constraints_validator(output, &schema);
    render_optional_non_nullable_validator(output, &optional_non_nullable_fields);
    *needs_optional_non_nullable_helper |= !optional_non_nullable_fields.is_empty();
    *needs_pydantic_core |= !optional_non_nullable_fields.is_empty()
        || !const_fields.is_empty()
        || !enum_fields.is_empty()
        || !array_validator_fields.is_empty()
        || schema.has_object_count_or_dependency();
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

/// True when the model is map-shaped: an object with no declared `properties`
/// whose members are open (typed via `additionalProperties`, or free-form for
/// `true`). A closed empty object (`additionalProperties: false`) admits no
/// members and is not map-shaped.
fn is_python_map_model(schema: &Schema) -> bool {
    schema.ty.as_ref().and_then(Value::as_str) == Some("object")
        && schema
            .properties
            .as_ref()
            .is_none_or(|properties| properties.is_empty())
        && schema.additional_properties.as_ref() != Some(&Value::Bool(false))
}

/// Emits a map-shaped model's `_validate_extras` validator: the per-member `T`
/// validation (typed maps only) plus the member-count and key-shape constraints.
fn render_map_model_methods(
    output: &mut String,
    schema: &Schema,
    model_name: &str,
    typed_members: bool,
) {
    // The checks are rendered first so the `extra` binding is only emitted when
    // one of them reads it: an unused local is a type-checker diagnostic, and a
    // map may carry no constraint at all (an unconstrained member type).
    let mut checks = String::new();
    if typed_members {
        render_typed_map_value_validator(&mut checks, model_name);
    }
    // `len(extra)` is the distinct wire-key count for a map (no declared
    // fields), counted as one number (never a declared + extras sum).
    if let Some(min) = schema.min_properties {
        checks.push_str(&format!("        if len(extra) < {min}:\n"));
        render_py_count_violation(
            &mut checks,
            "too_few_properties",
            &format!("must have at least {min} properties, got {{len(extra)}}"),
            "len(extra)",
            "                ",
        );
    }
    if let Some(max) = schema.max_properties {
        checks.push_str(&format!("        if len(extra) > {max}:\n"));
        render_py_count_violation(
            &mut checks,
            "too_many_properties",
            &format!("must have at most {max} properties, got {{len(extra)}}"),
            "len(extra)",
            "                ",
        );
    }
    if let Some(subschema) = &schema.property_names {
        render_py_property_name_validator(&mut checks, subschema);
    }

    output.push_str("\n    @pydantic.model_validator(mode=\"after\")\n");
    output.push_str("    def _validate_extras(self) -> typing.Any:\n");
    if checks.contains("extra") {
        output.push_str("        extra = typing.cast(dict[str, object], self.model_extra or {})\n");
    }
    output.push_str("        errors: list[pydantic_core.InitErrorDetails] = []\n");
    output.push_str(&checks);
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
    if typed_members {
        // Each member re-encodes through the same adapter that validated it, so a
        // materialized member (a referenced model, a native temporal or bytes
        // construct) reaches the wire in its declared form rather than however
        // Pydantic happens to render an untyped value.
        let adapter = map_member_adapter_name(model_name);
        output.push_str("        return {\n");
        output.push_str(&format!(
            "            key: {adapter}.dump_python(value, mode=\"json\", by_alias=True)\n"
        ));
        output.push_str(
            "            for key, value in typing.cast(dict[str, object], self.model_extra or {}).items()\n",
        );
        output.push_str("        }\n");
        return;
    }
    output
        .push_str("        return dict(typing.cast(dict[str, object], self.model_extra or {}))\n");
}

/// Emits the per-member validation loop of a typed map: each member is validated
/// **and materialized** through the model's member `TypeAdapter`, which carries
/// the member type's whole annotation — the spec-strict integer parse, a native
/// temporal/bytes construct, a `Literal` value set, a referenced model, the
/// numeric/length bounds, and the `pattern`/`format`/`multipleOf` validators — so
/// a member is held to exactly what a declared field of that type is held to
/// ([[additionalProperties]] §"Validator mapping": per-member `T` validation).
/// Pydantic's own violations are merged under the member's key, so the reported
/// path threads the member (`labels.env`, `entries.a.street`) per **P11**.
fn render_typed_map_value_validator(output: &mut String, model_name: &str) {
    let adapter = map_member_adapter_name(model_name);
    output.push_str("        for key, value in list(extra.items()):\n");
    output.push_str("            try:\n");
    output.push_str(&format!(
        "                extra[key] = {adapter}.validate_python(value)\n"
    ));
    output.push_str("            except pydantic.ValidationError as error:\n");
    output.push_str("                for detail in error.errors():\n");
    output.push_str("                    errors.append(\n");
    output.push_str("                        pydantic_core.InitErrorDetails(\n");
    output.push_str("                            type=pydantic_core.PydanticCustomError(\n");
    // `PydanticCustomError` types its arguments as `LiteralString`; a nested
    // error's own type and message are ordinary `str`, so both are cast (the
    // count/name validators do the same for their f-strings).
    output.push_str("                                typing.cast(typing.Any, detail[\"type\"]),\n");
    output.push_str("                                typing.cast(typing.Any, detail[\"msg\"]),\n");
    output.push_str("                            ),\n");
    output.push_str("                            loc=(key, *detail[\"loc\"]),\n");
    output.push_str("                            input=detail[\"input\"],\n");
    output.push_str("                        )\n");
    output.push_str("                    )\n");
}

/// The non-null branch of a member schema that is the nullability `oneOf`
/// wrapper, which carries the member's own constraints.
fn nullable_member_schema(schema: &Schema) -> Option<&Schema> {
    let branches = schema.one_of.as_ref()?;
    let non_null: Vec<&Schema> = branches
        .iter()
        .filter(|branch| branch.ty.as_ref().and_then(Value::as_str) != Some("null"))
        .collect();
    match non_null.len() {
        1 => Some(non_null[0]),
        _ => None,
    }
}

/// The module-level `TypeAdapter` a map-shaped model validates its members with.
/// It is defined *after* every class so its eagerly-evaluated annotation sees a
/// referenced model that is declared later in the module — the same reason union
/// aliases and `model_rebuild()` calls sit there.
fn map_member_adapter_name(model_name: &str) -> String {
    format!("_{}_MEMBER", model_name.to_shouty_snake_case())
}

/// Emits the member `TypeAdapter` definitions for every map-shaped model in the
/// module (see [`map_member_adapter_name`]). `strict=True` is passed as the
/// adapter's config so a member is held to the same strict mode a declared field
/// is (PRINCIPLES Python §1) — except when the member type *is* a model or union
/// alias, which carries its own config and rejects an override.
fn render_map_member_adapters(
    output: &mut String,
    models: &[&PlannedJsonType],
    union_names: &BTreeSet<String>,
    needs_multiple_of_helper: &mut bool,
    needs_pattern_helper: &mut bool,
    needs_format_helper: &mut bool,
) -> Result<()> {
    for model in models {
        let schema = decode_schema(model)?;
        if !is_python_map_model(&schema) {
            continue;
        }
        let Some(value_schema) = typed_map_value_schema(&schema)? else {
            continue;
        };
        // A nullable member's constraints sit on the non-null branch of its
        // wrapper, so the refinements are composed over that branch and the
        // wrapper's `| None` is re-added around the result.
        let (member, nullable): (&Schema, bool) = match nullable_member_schema(&value_schema) {
            Some(inner) => (inner, true),
            None => (&value_schema, false),
        };
        // The `Field` bounds sit *innermost*, next to the type they bound — the
        // position a declared field puts them in — so Pydantic reads
        // `min_length` as the string's length and not as the length of whatever
        // an outer validator returned. A nullable member widens the type inside
        // that same `Annotated`, the way a declared field pairs `T | None` with
        // its `Field(...)`; the refinements wrap the result.
        let mut annotation = annotation(member)?;
        if nullable {
            annotation = optional_annotation(&annotation);
        }
        let constraints = field_constraint_args(member);
        if !constraints.is_empty() {
            annotation = format!(
                "typing.Annotated[{annotation}, pydantic.Field({})]",
                constraints.join(", ")
            );
        }
        let annotation = refined_annotation(
            member,
            Some(annotation),
            needs_multiple_of_helper,
            needs_pattern_helper,
            needs_format_helper,
        )?;
        // Pydantic rejects a `config` override on a `BaseModel` — a referenced
        // model carries its own strict config. A union alias is not a model, so
        // it takes the override like any other annotation.
        let is_model_class = member
            .reference
            .as_ref()
            .map(|reference| reference_model_name(reference))
            .is_some_and(|name| !union_names.contains(&name));
        let config = if is_model_class {
            String::new()
        } else {
            ", config=pydantic.ConfigDict(strict=True)".to_string()
        };
        output.push_str(&format!(
            "{}: pydantic.TypeAdapter[typing.Any] = pydantic.TypeAdapter(\n    {annotation}{config}\n)\n",
            map_member_adapter_name(&model.model_name)
        ));
    }
    Ok(())
}

/// Emits an `errors.append(...)` for an object member-count violation (`{indent}`
/// is the body indent of the enclosing `if`). `message` is a Python f-string
/// body (may reference the runtime count) and `count_expr` is the reported
/// input value.
fn render_py_count_violation(
    output: &mut String,
    error_type: &str,
    message: &str,
    count_expr: &str,
    indent: &str,
) {
    output.push_str(indent);
    output.push_str("errors.append(\n");
    output.push_str(indent);
    output.push_str("    pydantic_core.InitErrorDetails(\n");
    output.push_str(indent);
    output.push_str("        type=pydantic_core.PydanticCustomError(\n");
    output.push_str(indent);
    output.push_str(&format!(
        "            {}, typing.cast(typing.Any, f{})\n",
        python_string_literal(error_type),
        python_string_literal(message)
    ));
    output.push_str(indent);
    output.push_str("        ),\n");
    output.push_str(indent);
    output.push_str("        loc=(),\n");
    output.push_str(indent);
    output.push_str(&format!("        input={count_expr},\n"));
    output.push_str(indent);
    output.push_str("    )\n");
    output.push_str(indent);
    output.push_str(")\n");
}

/// Emits the `propertyNames` key-shape loop for a typed map (over `extra`),
/// pushing an `InitErrorDetails` per key whose string length is out of bounds.
/// `len(key)` counts Unicode code points in Python — spec-correct.
fn render_py_property_name_validator(output: &mut String, subschema: &Schema) {
    if subschema.min_length.is_none() && subschema.max_length.is_none() {
        return;
    }
    output.push_str("        for key in extra:\n");
    let mut emit = |condition: &str, reason: &str| {
        output.push_str(&format!("            if {condition}:\n"));
        output.push_str("                errors.append(\n");
        output.push_str("                    pydantic_core.InitErrorDetails(\n");
        output.push_str("                        type=pydantic_core.PydanticCustomError(\n");
        output.push_str(&format!(
            "                            \"invalid_property_name\", typing.cast(typing.Any, f{})\n",
            python_string_literal(&format!("invalid property name \"{{key}}\": {reason}"))
        ));
        output.push_str("                        ),\n");
        output.push_str("                        loc=(key,),\n");
        output.push_str("                        input=key,\n");
        output.push_str("                    )\n");
        output.push_str("                )\n");
    };
    if let Some(min) = subschema.min_length {
        emit(
            &format!("len(key) < {min}"),
            &format!("must have length >= {min}, got {{len(key)}}"),
        );
    }
    if let Some(max) = subschema.max_length {
        emit(
            &format!("len(key) > {max}"),
            &format!("must have length <= {max}, got {{len(key)}}"),
        );
    }
}

/// Emits a `_validate_object` after-validator for a declared-property object
/// covering `minProperties`/`maxProperties` (over the distinct wire-key count,
/// `len(model_fields_set)`, which includes extras and excludes default-filled
/// fields) and `dependentRequired` (cross-field presence over the same set).
fn render_object_constraints_validator(output: &mut String, schema: &Schema) {
    if !schema.has_object_count_or_dependency() {
        return;
    }
    output.push_str("\n    @pydantic.model_validator(mode=\"after\")\n");
    output.push_str("    def _validate_object(self) -> typing.Any:\n");
    output.push_str("        errors: list[pydantic_core.InitErrorDetails] = []\n");
    output.push_str("        present = self.model_fields_set\n");
    let count = "len(present)";
    if let Some(min) = schema.min_properties {
        output.push_str(&format!("        if {count} < {min}:\n"));
        render_py_count_violation(
            output,
            "too_few_properties",
            &format!("must have at least {min} properties, got {{{count}}}"),
            count,
            "            ",
        );
    }
    if let Some(max) = schema.max_properties {
        output.push_str(&format!("        if {count} > {max}:\n"));
        render_py_count_violation(
            output,
            "too_many_properties",
            &format!("must have at most {max} properties, got {{{count}}}"),
            count,
            "            ",
        );
    }
    if let Some(dependent_required) = &schema.dependent_required {
        let member = |name: &str| -> String {
            schema
                .properties
                .as_ref()
                .and_then(|properties| properties.get(name))
                .map(|property| property.py_member_name(name))
                .unwrap_or_else(|| python_field_name(name))
        };
        for (trigger, deps) in dependent_required {
            let trigger_field = member(trigger);
            output.push_str(&format!(
                "        if {} in present:\n",
                python_string_literal(&trigger_field)
            ));
            for dep in deps {
                let dep_field = member(dep);
                output.push_str(&format!(
                    "            if {} not in present:\n",
                    python_string_literal(&dep_field)
                ));
                output.push_str("                errors.append(\n");
                output.push_str("                    pydantic_core.InitErrorDetails(\n");
                output
                    .push_str("                        type=pydantic_core.PydanticCustomError(\n");
                let reason =
                    format!("property \"{dep}\" is required when \"{trigger}\" is present");
                output.push_str(&format!(
                    "                            \"dependent_required\", {}\n",
                    python_string_literal(&reason)
                ));
                output.push_str("                        ),\n");
                output.push_str(&format!(
                    "                        loc=({},),\n",
                    python_string_literal(dep)
                ));
                output.push_str("                        input=None,\n");
                output.push_str("                    )\n");
                output.push_str("                )\n");
            }
        }
    }
    output.push_str("        if errors:\n");
    output.push_str("            raise pydantic.ValidationError.from_exception_data(\n");
    output.push_str("                title=type(self).__name__, line_errors=errors\n");
    output.push_str("            )\n");
    output.push_str("        return self\n");
}

fn render_optional_non_nullable_helper(output: &mut String) {
    output.push_str("def _reject_explicit_null(\n");
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
    output.push_str("def _emit_set_fields(\n");
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

/// Builds the boolean Python sub-conditions that define "match" for a scalar
/// `contains` matcher over `elem`. A type-only matcher matches every element, so
/// an empty condition set renders as the literal `True`.
fn py_matcher_condition(matcher: &Schema, elem: &str) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(value) = &matcher.const_value {
        parts.push(format!("{elem} == {}", python_value_literal(value)?));
    }
    if let Some(values) = &matcher.enum_values {
        let alternatives = values
            .iter()
            .map(python_value_literal)
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        if !alternatives.is_empty() {
            parts.push(format!("{elem} in ({alternatives},)"));
        }
    }
    let is_integer = matcher.ty.as_ref().and_then(Value::as_str) == Some("integer");
    if let Some(min) = &matcher.minimum {
        parts.push(format!("{elem} >= {}", py_bound_literal(min, is_integer)));
    }
    if let Some(max) = &matcher.maximum {
        parts.push(format!("{elem} <= {}", py_bound_literal(max, is_integer)));
    }
    if let Some(min) = &matcher.exclusive_minimum {
        parts.push(format!("{elem} > {}", py_bound_literal(min, is_integer)));
    }
    if let Some(max) = &matcher.exclusive_maximum {
        parts.push(format!("{elem} < {}", py_bound_literal(max, is_integer)));
    }
    if let Some(divisor) = &matcher.multiple_of {
        parts.push(format!(
            "{elem} % {} == 0",
            py_bound_literal(divisor, is_integer)
        ));
    }
    if let Some(min) = matcher.min_length {
        parts.push(format!("len({elem}) >= {min}"));
    }
    if let Some(max) = matcher.max_length {
        parts.push(format!("len({elem}) <= {max}"));
    }
    if parts.is_empty() {
        Ok("True".to_string())
    } else {
        Ok(parts.join(" and "))
    }
}

/// Emits one `_validate_arrays` after-validator covering every array field that
/// needs `uniqueItems` / `contains` enforcement (both lack a native Pydantic
/// equivalent). Violations aggregate into a single `pydantic.ValidationError`.
fn render_array_validators(
    output: &mut String,
    fields: &[(String, String, &Schema)],
) -> Result<()> {
    if fields.is_empty() {
        return Ok(());
    }

    output.push_str("\n    @pydantic.model_validator(mode=\"after\")\n");
    output.push_str("    def _validate_arrays(self) -> typing.Any:\n");
    output.push_str("        errors: list[pydantic_core.InitErrorDetails] = []\n");
    for (json_name, field_name, schema) in fields {
        let loc = python_string_literal(json_name);
        output.push_str("        value = self.");
        output.push_str(field_name);
        output.push('\n');
        output.push_str("        if value is not None:\n");
        if schema.unique_items == Some(true) {
            output.push_str("            seen: dict[object, int] = {}\n");
            output.push_str("            for index, element in enumerate(value):\n");
            output.push_str("                if element in seen:\n");
            output.push_str("                    errors.append(\n");
            output.push_str("                        pydantic_core.InitErrorDetails(\n");
            output
                .push_str("                            type=pydantic_core.PydanticCustomError(\n");
            output.push_str(
                "                                \"unique_items\", typing.cast(typing.Any, f\"duplicate items: element at index {index} equals index {seen[element]}\")\n",
            );
            output.push_str("                            ),\n");
            output.push_str(&format!("                            loc=({loc},),\n"));
            output.push_str("                            input=element,\n");
            output.push_str("                        )\n");
            output.push_str("                    )\n");
            output.push_str("                else:\n");
            output.push_str("                    seen[element] = index\n");
        }
        if let Some(matcher) = &schema.contains {
            let condition = py_matcher_condition(matcher, "element")?;
            let effective_min = schema.min_contains.unwrap_or(1);
            output.push_str(&format!(
                "            match_count = sum(1 for element in value if {condition})\n"
            ));
            if effective_min > 0 {
                output.push_str(&format!("            if match_count < {effective_min}:\n"));
                let message = if schema.min_contains.is_some() {
                    format!(
                        "typing.cast(typing.Any, f\"too few matching items: at least {effective_min}, got {{match_count}}\")"
                    )
                } else {
                    "\"no element matches the required schema\"".to_string()
                };
                let error_type = if schema.min_contains.is_some() {
                    "too_few_matching_items"
                } else {
                    "contains"
                };
                output.push_str("                errors.append(\n");
                output.push_str("                    pydantic_core.InitErrorDetails(\n");
                output.push_str(&format!(
                    "                        type=pydantic_core.PydanticCustomError(\n                            {}, {message}\n                        ),\n",
                    python_string_literal(error_type)
                ));
                output.push_str(&format!("                        loc=({loc},),\n"));
                output.push_str("                        input=value,\n");
                output.push_str("                    )\n");
                output.push_str("                )\n");
            }
            if let Some(max) = schema.max_contains {
                output.push_str(&format!("            if match_count > {max}:\n"));
                output.push_str("                errors.append(\n");
                output.push_str("                    pydantic_core.InitErrorDetails(\n");
                output.push_str(&format!(
                    "                        type=pydantic_core.PydanticCustomError(\n                            \"too_many_matching_items\", typing.cast(typing.Any, f\"too many matching items: at most {max}, got {{match_count}}\")\n                        ),\n"
                ));
                output.push_str(&format!("                        loc=({loc},),\n"));
                output.push_str("                        input=value,\n");
                output.push_str("                    )\n");
                output.push_str("                )\n");
            }
        }
    }
    output.push_str("        if errors:\n");
    output.push_str("            raise pydantic.ValidationError.from_exception_data(\n");
    output.push_str("                title=type(self).__name__, line_errors=errors\n");
    output.push_str("            )\n");
    output.push_str("        return self\n");
    Ok(())
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

/// Emits, per `enum` field, a `model_validator(mode="before")` membership check.
/// Unlike `const` there is no injection (no single value to fill on absence —
/// presence is owned by `required`); a present out-of-set value raises an
/// aggregated `enum` error naming the set and the offending value. String/int/
/// bool enums are additionally closed by their `Literal` annotation; float enums
/// (plain `float`) rest on this check alone. See `specs/json-schema/features/enum.md`.
fn render_enum_validators(
    output: &mut String,
    fields: &[(String, String, Vec<Value>)],
) -> Result<()> {
    if fields.is_empty() {
        return Ok(());
    }

    for (json_name, field_name, values) in fields {
        let literals = values
            .iter()
            .map(python_value_literal)
            .collect::<Result<Vec<_>>>()?;
        let set_literal = format!("[{}]", literals.join(", "));
        let message = format!(
            "{json_name} must be one of [{}], got {{got}}",
            literals.join(", ")
        );
        output.push_str("\n    @pydantic.model_validator(mode=\"before\")\n");
        output.push_str("    @classmethod\n");
        output.push_str("    def _check_enum_");
        output.push_str(field_name);
        output.push_str("(\n");
        output.push_str("        cls,\n");
        output.push_str("        data: object,\n");
        output.push_str("    ) -> object:\n");
        output.push_str("        if isinstance(data, dict):\n");
        output.push_str("            values = typing.cast(dict[str, object], data)\n");
        output.push_str("            if ");
        output.push_str(&python_string_literal(json_name));
        output.push_str(" in values");
        if field_name != json_name {
            output.push_str(" or ");
            output.push_str(&python_string_literal(field_name));
            output.push_str(" in values");
        }
        output.push_str(":\n");
        output.push_str("                got = values.get(");
        output.push_str(&python_string_literal(json_name));
        if field_name != json_name {
            output.push_str(", values.get(");
            output.push_str(&python_string_literal(field_name));
            output.push_str(")");
        }
        output.push_str(")\n");
        output.push_str("                if got not in ");
        output.push_str(&set_literal);
        output.push_str(":\n");
        output.push_str("                    raise pydantic_core.PydanticCustomError(\n");
        output.push_str("                        \"enum\", ");
        output.push_str(&python_string_literal(&message));
        output.push_str(", {\"got\": got}\n");
        output.push_str("                    )\n");
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
    output.push_str("        return _reject_explicit_null(cls, data, handler)\n");
}

fn render_set_fields_serializer(output: &mut String) {
    output.push_str("\n    @pydantic.model_serializer(mode=\"wrap\")\n");
    output.push_str("    def _serialize(\n");
    output.push_str("        self,\n");
    output.push_str("        handler: typing.Callable[[pydantic.BaseModel], typing.Any],\n");
    output.push_str("    ) -> dict[str, object]:\n");
    output.push_str("        return _emit_set_fields(self, handler)\n");
}

fn render_field_expr(
    output: &mut String,
    json_name: &str,
    field_name: &str,
    default: Option<&str>,
    property: &Schema,
) {
    output.push_str("pydantic.Field(");
    let mut arguments = Vec::new();
    if let Some(default) = default {
        arguments.push(format!("default={default}"));
    }
    if json_name != field_name {
        arguments.push(format!("alias={}", python_string_literal(json_name)));
    }
    arguments.extend(field_constraint_args(property));
    output.push_str(&arguments.join(", "));
    output.push(')');
}

/// The `pydantic.Field(...)` arguments for the bounds Pydantic enforces natively.
/// Shared by a declared field and by a typed map's member, which composes them
/// into its own `Annotated[...]` (it has no field to hang them off).
fn field_constraint_args(schema: &Schema) -> Vec<String> {
    let mut arguments = Vec::new();
    // Numeric bounds map to native Pydantic constraints (annotated_types
    // Ge/Le/Gt/Lt). Integer `multipleOf` uses Pydantic's native `multiple_of`
    // (exact for ints); number `multipleOf` is handled by an explicit `fmod`
    // AfterValidator in the annotation instead.
    let is_integer = schema.is_integer_field();
    if is_integer || schema.is_number_field() {
        if let Some(min) = &schema.minimum {
            arguments.push(format!("ge={}", py_bound_literal(min, is_integer)));
        }
        if let Some(max) = &schema.maximum {
            arguments.push(format!("le={}", py_bound_literal(max, is_integer)));
        }
        if let Some(min) = &schema.exclusive_minimum {
            arguments.push(format!("gt={}", py_bound_literal(min, is_integer)));
        }
        if let Some(max) = &schema.exclusive_maximum {
            arguments.push(format!("lt={}", py_bound_literal(max, is_integer)));
        }
        if is_integer && let Some(divisor) = &schema.multiple_of {
            arguments.push(format!("multiple_of={}", py_bound_literal(divisor, true)));
        }
    }
    // String-length bounds map to Pydantic's native `min_length`/`max_length`,
    // which count Unicode code points (verified in `maxLength.md`) — spec-correct
    // without a custom validator.
    if schema.is_string_field() {
        if let Some(min) = schema.min_length {
            arguments.push(format!("min_length={min}"));
        }
        if let Some(max) = schema.max_length {
            arguments.push(format!("max_length={max}"));
        }
    }
    // Array `minItems`/`maxItems` map to Pydantic's native `min_length`/
    // `max_length`, which bound the element count for sequences — spec-correct
    // without a custom validator (see minItems.md / maxItems.md).
    if schema.is_array_field() {
        if let Some(min) = schema.min_items {
            arguments.push(format!("min_length={min}"));
        }
        if let Some(max) = schema.max_items {
            arguments.push(format!("max_length={max}"));
        }
    }
    arguments
}

/// Composes a docstring from a `title` (summary line) and `description` (body);
/// returns `None` when both are empty. See specs/json-schema/features/{title,description}.md.
fn compose_python_doc(title: Option<&str>, description: Option<&str>) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
        lines.push(title.to_string());
    }
    if let Some(description) = description.map(str::trim).filter(|d| !d.is_empty()) {
        for line in description.lines() {
            lines.push(line.trim().to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
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

/// The materialized `TemporalKind` of a schema that is directly a temporal
/// string (not looking through `oneOf`, which `annotation` handles by recursion).
fn temporal_kind_direct(schema: &Schema) -> Option<crate::json_schema::format::TemporalKind> {
    if schema.ty.as_ref().and_then(Value::as_str) != Some("string") {
        return None;
    }
    schema
        .format
        .as_deref()
        .and_then(crate::json_schema::format::TemporalKind::from_name)
}

/// The runtime-module `Annotated` alias name for a materialized `contentEncoding`.
fn content_encoding_field_alias(
    encoding: crate::json_schema::content_encoding::Encoding,
) -> &'static str {
    match encoding {
        crate::json_schema::content_encoding::Encoding::Base64 => "Base64Field",
        crate::json_schema::content_encoding::Encoding::Base64Url => "Base64UrlField",
    }
}

/// The materialized `contentEncoding` of a schema that is directly a bytes string
/// (the `oneOf[…, null]` wrapper is handled by `annotation` recursion).
fn content_encoding_direct(
    schema: &Schema,
) -> Option<crate::json_schema::content_encoding::Encoding> {
    if schema.ty.as_ref().and_then(Value::as_str) != Some("string") {
        return None;
    }
    schema
        .content_encoding
        .as_deref()
        .and_then(crate::json_schema::content_encoding::Encoding::from_name)
}

/// The runtime-module `Annotated` alias name for a materialized temporal kind.
fn temporal_field_alias(kind: crate::json_schema::format::TemporalKind) -> &'static str {
    match kind {
        crate::json_schema::format::TemporalKind::DateTime => "DateTimeField",
        crate::json_schema::format::TemporalKind::Date => "DateField",
        crate::json_schema::format::TemporalKind::Time => "TimeField",
        crate::json_schema::format::TemporalKind::Duration => "DurationField",
    }
}

/// The emitted annotation for a value, with the refinements Pydantic expresses as
/// `Annotated` validators layered on: a number's `multipleOf` (`math.fmod`-exact),
/// a string's `pattern`, and a string's `format`. The bounds Pydantic takes as
/// native `Field` arguments are added separately (see [`field_constraint_args`]),
/// so a value position that has no `Field` — a typed map's member — composes the
/// two itself.
fn refined_annotation(
    schema: &Schema,
    base: Option<String>,
    needs_multiple_of_helper: &mut bool,
    needs_pattern_helper: &mut bool,
    needs_format_helper: &mut bool,
) -> Result<String> {
    let mut annotation = match base {
        Some(base) => base,
        None => annotation(schema)?,
    };
    if let Some(divisor) = schema.number_multiple_of() {
        *needs_multiple_of_helper = true;
        annotation = format!(
            "typing.Annotated[{annotation}, pydantic.AfterValidator(_check_multiple_of({}))]",
            py_bound_literal(divisor, false)
        );
    }
    if let Some(pattern) = &schema.pattern
        && schema.ty.as_ref().and_then(Value::as_str) == Some("string")
    {
        *needs_pattern_helper = true;
        // Per-target `$`→`\Z` rewrite: `re`'s `\Z` is the strict
        // end-of-string anchor (no trailing-`\n` exception). See
        // `specs/json-schema/features/pattern.md`.
        let rewritten = crate::json_schema::pattern::rewrite_end_anchor(pattern, r"\Z");
        annotation = format!(
            "typing.Annotated[{annotation}, pydantic.AfterValidator(_check_pattern({}))]",
            python_string_literal(&rewritten)
        );
    }
    if let Some(format) = &schema.format
        && schema.ty.as_ref().and_then(Value::as_str) == Some("string")
        && let Some(check) = crate::json_schema::format::check_for(format)
    {
        *needs_format_helper = true;
        // Per-target `$`→`\Z` rewrite (strict end-of-string, no trailing-`\n`
        // exception), matching `_check_pattern`.
        let rewritten = crate::json_schema::pattern::rewrite_end_anchor(&check.pattern, r"\Z");
        let max_arg = match check.max_code_points {
            Some(max) => format!(", {max}"),
            None => String::new(),
        };
        annotation = format!(
            "typing.Annotated[{annotation}, pydantic.AfterValidator(_check_format({}, {}{max_arg}))]",
            python_string_literal(check.name),
            python_string_literal(&rewritten)
        );
    }
    Ok(annotation)
}

fn annotation(schema: &Schema) -> Result<String> {
    if let Some(const_value) = &schema.const_value
        && let Some(annotation) = python_literal_annotation(const_value)
    {
        return Ok(annotation);
    }
    // A scalar `enum` is a closed `Literal` union. Number(float) members are the
    // exception (PEP 586 forbids float literals): they fall through to the plain
    // `float` type and rest on the membership validator (see enum.md).
    if let Some(values) = &schema.enum_values
        && !values.is_empty()
    {
        let tokens = values
            .iter()
            .filter_map(python_literal_token)
            .collect::<Vec<_>>();
        if tokens.len() == values.len() {
            return Ok(format!("typing.Literal[{}]", tokens.join(", ")));
        }
    }
    if let Some(reference) = &schema.reference {
        return Ok(reference_model_name(reference));
    }
    if let Some(one_of) = &schema.one_of {
        let non_null = one_of
            .iter()
            .filter(|branch| branch.ty.as_ref().and_then(Value::as_str) != Some("null"))
            .collect::<Vec<_>>();
        let nullable = one_of
            .iter()
            .any(|branch| branch.ty.as_ref().and_then(Value::as_str) == Some("null"));
        // Two or more non-null branches form a closed sum type — a
        // `typing.Union[...]` (Pydantic v2 smart mode selects the branch by
        // token / `Literal` discriminant). One non-null branch is the
        // degenerate nullability pattern.
        if non_null.len() >= 2 {
            let mut members = non_null
                .iter()
                .map(|branch| annotation(branch))
                .collect::<Result<Vec<_>>>()?;
            if nullable {
                members.push("None".to_string());
            }
            return Ok(members.join(" | "));
        }
        let Some(branch) = non_null.first() else {
            return Ok("None".to_string());
        };
        return Ok(optional_annotation(&annotation(branch)?));
    }
    // A materialized temporal `format` replaces `str` with a native typed field,
    // carried by a runtime-module `Annotated` alias (BeforeValidator parse +
    // PlainSerializer generator-owned serialize). The `oneOf[…, null]` nullable
    // wrapper is handled above by recursing into the non-null branch.
    if let Some(kind) = temporal_kind_direct(schema) {
        return Ok(temporal_field_alias(kind).to_string());
    }
    // A materialized `contentEncoding` replaces `str` with `bytes`, carried by a
    // runtime-module `Annotated` alias (BeforeValidator parse + PlainSerializer
    // generator-owned canonical serialize).
    if let Some(encoding) = content_encoding_direct(schema) {
        return Ok(content_encoding_field_alias(encoding).to_string());
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
    python_literal_token(value).map(|token| format!("typing.Literal[{token}]"))
}

/// The inner `Literal[...]` member token for a scalar value, or `None` when the
/// value cannot be a `Literal` member (a float — PEP 586 — or a composite).
fn python_literal_token(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("None".to_string()),
        Value::Bool(value) => Some(if *value { "True" } else { "False" }.to_string()),
        Value::Number(value) if value.is_i64() || value.is_u64() => Some(value.to_string()),
        Value::String(value) => Some(python_string_literal(value)),
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
    if admits_none(annotation) {
        annotation.to_string()
    } else {
        format!("{annotation} | None")
    }
}

/// True when the annotation itself already admits `None` — a `None` member of
/// the *top-level* union. A nested one does not count: in `list[str | None]`
/// the elements are nullable while the list is not, so an optional field of
/// that type still needs its own `| None` ([[items]] §"Element nullability is
/// the element's own concern").
fn admits_none(annotation: &str) -> bool {
    split_top_level_union(annotation).contains(&"None")
}

/// Splits a type annotation on its top-level `|`, ignoring any inside a
/// subscript (`list[str | None]` is one member, not two).
fn split_top_level_union(annotation: &str) -> Vec<&str> {
    let mut members = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in annotation.char_indices() {
        match character {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                members.push(annotation[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    members.push(annotation[start..].trim());
    members
}

fn reference_model_name(reference: &str) -> String {
    if let Some(resolved) = REF_NAMES.with(|cell| cell.borrow().get(reference).cloned()) {
        return resolved;
    }
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
