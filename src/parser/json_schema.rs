use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use heck::{ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase, ToUpperCamelCase};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::language::Language;
use crate::spec::{
    ApiSpec, ExternalTypeBindingSpec, ExternalTypeSpec, JsonModelSpec, LanguageStringSpec,
    ModulePath, OperationSpec, ServiceSpec, SupportSpec, Symbol, TypeDeclSpec, TypeSpec,
};
use crate::workspace::{ApiSpecBranch, ApiSpecLeaf, ApiSpecNode, ApiSpecTree};

#[derive(Debug, Clone, Deserialize, Default)]
struct Document {
    nexusrpc: Option<Value>,
    #[serde(rename = "$schema")]
    schema: Option<Value>,
    services: Option<IndexMap<String, Service>>,
    #[serde(rename = "$defs")]
    defs: Option<IndexMap<String, Schema>>,
    #[serde(flatten)]
    root: Schema,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Service {
    fqn: Option<String>,
    description: Option<String>,
    endpoint: Option<String>,
    #[serde(default)]
    operations: IndexMap<String, Operation>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Operation {
    fqn: Option<String>,
    description: Option<String>,
    input: Option<Schema>,
    output: Option<Schema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct Schema {
    #[serde(rename = "$ref")]
    reference: Option<String>,
    #[serde(rename = "$id")]
    id: Option<Value>,
    #[serde(rename = "type")]
    ty: Option<Value>,
    title: Option<String>,
    description: Option<String>,
    properties: Option<IndexMap<String, Schema>>,
    required: Option<Value>,
    #[serde(rename = "additionalProperties")]
    additional_properties: Option<Value>,
    items: Option<Box<Schema>>,
    #[serde(rename = "oneOf")]
    one_of: Option<Vec<Schema>>,
    #[serde(flatten)]
    extra: IndexMap<String, Value>,
}

impl Schema {
    fn is_bare_ref(&self) -> bool {
        self.reference.is_some()
            && Schema {
                reference: None,
                ..self.clone()
            } == Schema::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum TypeKey {
    Root(PathBuf),
    Def(PathBuf, String),
}

#[derive(Clone, Debug)]
struct JsonModel {
    full_name: String,
    canonical_path: PathBuf,
    model_name: String,
    schema: Schema,
}

#[derive(Clone, Debug)]
struct JsonSource {
    path: PathBuf,
    source_root: PathBuf,
    relative_path: PathBuf,
    input: String,
}

struct ParsedJsonDocuments {
    docs: IndexMap<PathBuf, (PathBuf, Document)>,
    models: BTreeMap<TypeKey, JsonModel>,
}

pub fn load_api_spec_from_json_schema_for_language_with_inputs(
    language: Language,
    input_paths: &[PathBuf],
) -> Result<ApiSpec> {
    let sources = input_paths
        .iter()
        .map(|path| {
            let input = fs::read_to_string(path).map_err(|source| Error::ReadFile {
                path: path.clone(),
                source,
            })?;
            Ok((path.clone(), input))
        })
        .collect::<Result<Vec<_>>>()?;
    api_spec_from_json_schema_sources(language, sources)
}

pub fn load_api_spec_tree_from_json_schema_for_language_with_inputs(
    language: Language,
    input_paths: &[PathBuf],
) -> Result<ApiSpecTree> {
    let sources = expand_json_schema_sources(input_paths)?;
    api_spec_tree_from_json_schema_sources(language, sources)
}

fn expand_json_schema_sources(input_paths: &[PathBuf]) -> Result<Vec<JsonSource>> {
    if input_paths.is_empty() {
        return Err(Error::InvalidJsonSchema {
            path: PathBuf::from("<input>"),
            reason: "at least one JSON schema input path is required".to_string(),
        });
    }
    let mut sources = Vec::new();
    for input_path in input_paths {
        if input_path.is_dir() {
            let source_root = canonical(input_path);
            let mut files = Vec::new();
            collect_json_schema_files(input_path, &mut files)?;
            files.sort();
            for path in files {
                let relative_path =
                    normalize(path.strip_prefix(input_path).unwrap_or(path.as_path()));
                let input = fs::read_to_string(&path).map_err(|source| Error::ReadFile {
                    path: path.clone(),
                    source,
                })?;
                sources.push(JsonSource {
                    path,
                    source_root: source_root.clone(),
                    relative_path,
                    input,
                });
            }
        } else {
            let input = fs::read_to_string(input_path).map_err(|source| Error::ReadFile {
                path: input_path.clone(),
                source,
            })?;
            let source_root = input_path
                .parent()
                .map(canonical)
                .unwrap_or_else(|| PathBuf::from("."));
            let relative_path = input_path
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| input_path.clone());
            sources.push(JsonSource {
                path: input_path.clone(),
                source_root,
                relative_path,
                input,
            });
        }
    }
    sources.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut seen = BTreeMap::<PathBuf, PathBuf>::new();
    for source in &sources {
        if let Some(existing) = seen.insert(source.relative_path.clone(), source.path.clone()) {
            return Err(Error::InvalidJsonSchema {
                path: source.path.clone(),
                reason: format!(
                    "duplicate JSON schema module path `{}` also provided by `{}`",
                    source.relative_path.display(),
                    existing.display()
                ),
            });
        }
    }
    Ok(sources)
}

fn collect_json_schema_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|source| Error::ReadFile {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::ReadFile {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_schema_files(&path, files)?;
        } else if supported_json_schema_extension(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn supported_json_schema_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("json" | "yaml" | "yml")
    )
}

fn api_spec_tree_from_json_schema_sources(
    language: Language,
    sources: Vec<JsonSource>,
) -> Result<ApiSpecTree> {
    let module_paths = sources
        .iter()
        .map(|source| {
            (
                canonical(&source.path),
                module_path_from_relative_source(&source.relative_path),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let parsed = parse_json_documents(
        sources
            .iter()
            .map(|source| (source.path.clone(), source.input.clone()))
            .collect(),
    )?;
    if sources.len() == 1 {
        let source = sources
            .into_iter()
            .next()
            .expect("single JSON source should be present");
        let mut spec = api_spec_from_parsed_json_documents(
            language,
            &parsed,
            &[canonical(&source.path)],
            None,
        )?;
        spec.module_path = ModulePath::default();
        return Ok(ApiSpecTree {
            root: ApiSpecNode::Leaf(ApiSpecLeaf {
                module_path: spec.module_path.clone(),
                source_root: source.source_root,
                source_path: source.relative_path,
                spec,
            }),
        });
    }

    for source in &sources {
        let module_path = module_path_from_relative_source(&source.relative_path);
        if let Some(segment) = module_path
            .0
            .iter()
            .find(|segment| is_reserved_module_name(segment))
        {
            return Err(Error::InvalidJsonSchema {
                path: source.path.clone(),
                reason: format!(
                    "input `{}` maps to the reserved module name `{segment}`, which collides with a generated file (models/services/definitions/index/_recursive); rename the input file or directory",
                    source.relative_path.display()
                ),
            });
        }
    }

    let mut root = ApiSpecBranch {
        module_path: ModulePath::default(),
        children: BTreeMap::new(),
    };
    for source in sources {
        let module_path = module_path_from_relative_source(&source.relative_path);
        let mut spec = api_spec_from_parsed_json_documents(
            language,
            &parsed,
            &[canonical(&source.path)],
            Some(&module_paths),
        )?;
        spec.module_path = module_path.clone();
        insert_leaf(
            &mut root,
            ApiSpecLeaf {
                module_path,
                source_root: source.source_root,
                source_path: source.relative_path,
                spec,
            },
        )?;
    }
    Ok(ApiSpecTree {
        root: ApiSpecNode::Branch(root),
    })
}

fn insert_leaf(branch: &mut ApiSpecBranch, leaf: ApiSpecLeaf) -> Result<()> {
    let segments = leaf.module_path.0.clone();
    let Some((segment, rest)) = segments.split_first() else {
        return Err(Error::InvalidJsonSchema {
            path: leaf.source_path,
            reason: "JSON schema module path must not be empty".to_string(),
        });
    };
    insert_leaf_at(branch, segment, rest, leaf)
}

fn insert_leaf_at(
    branch: &mut ApiSpecBranch,
    segment: &str,
    rest: &[String],
    leaf: ApiSpecLeaf,
) -> Result<()> {
    if rest.is_empty() {
        if branch
            .children
            .insert(segment.to_string(), ApiSpecNode::Leaf(leaf))
            .is_some()
        {
            return Err(Error::InvalidJsonSchema {
                path: PathBuf::from(segment),
                reason: "duplicate JSON schema module path".to_string(),
            });
        }
        return Ok(());
    }

    let child_path = branch.module_path.child(segment);
    let child = branch
        .children
        .entry(segment.to_string())
        .or_insert_with(|| {
            ApiSpecNode::Branch(ApiSpecBranch {
                module_path: child_path,
                children: BTreeMap::new(),
            })
        });
    let ApiSpecNode::Branch(child_branch) = child else {
        return Err(Error::InvalidJsonSchema {
            path: leaf.source_path,
            reason: "JSON schema module path conflicts with another module".to_string(),
        });
    };
    insert_leaf_at(child_branch, &rest[0], &rest[1..], leaf)
}

/// Whether a module-path segment collides with a name the generators reserve
/// for their own emitted files (the union across languages — see
/// `specs/json-schema/generated-file-layout.md`). Reserving the union means a name
/// reserved in *any* target is rejected for *all*, keeping the flat package
/// coherent everywhere.
fn is_reserved_module_name(segment: &str) -> bool {
    matches!(
        segment,
        "definitions" | "_recursive" | "models" | "services" | "index" | "__init__"
    )
}

fn module_path_from_relative_source(path: &Path) -> ModulePath {
    let mut segments = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if let Some(last) = segments.last_mut() {
        *last = strip_json_schema_extension(last).to_string();
    }
    ModulePath(segments)
}

/// Strips a JSON-Schema input file's extension and, if present, the
/// `.nexusrpc` naming-convention infix that marks the file as carrying a
/// Nexus service/operation envelope (e.g. `chat.nexusrpc.yaml` -> `chat`).
pub(crate) fn strip_json_schema_extension(name: &str) -> &str {
    let without_extension = name
        .strip_suffix(".json")
        .or_else(|| name.strip_suffix(".yaml"))
        .or_else(|| name.strip_suffix(".yml"))
        .unwrap_or(name);
    without_extension
        .strip_suffix(".nexusrpc")
        .unwrap_or(without_extension)
}

#[cfg(test)]
pub(crate) fn parse_api_spec_from_json_schema_for_language(
    language: Language,
    input: &str,
    path: PathBuf,
) -> Result<ApiSpec> {
    api_spec_from_json_schema_sources(language, vec![(path, input.to_string())])
}

fn api_spec_from_json_schema_sources(
    language: Language,
    sources: Vec<(PathBuf, String)>,
) -> Result<ApiSpec> {
    let parsed = parse_json_documents(sources)?;
    let paths = parsed.docs.keys().cloned().collect::<Vec<_>>();
    api_spec_from_parsed_json_documents(language, &parsed, &paths, None)
}

fn parse_json_documents(sources: Vec<(PathBuf, String)>) -> Result<ParsedJsonDocuments> {
    if sources.is_empty() {
        return Err(Error::InvalidJsonSchema {
            path: PathBuf::from("<input>"),
            reason: "at least one JSON schema input path is required".to_string(),
        });
    }

    let mut docs = IndexMap::new();
    for (path, input) in sources {
        let doc =
            serde_yaml::from_str::<Document>(&input).map_err(|error| Error::JsonSchemaParse {
                path: path.clone(),
                message: error.to_string(),
            })?;
        docs.insert(canonical(&path), (path, doc));
    }

    // Snapshot the raw (pre-merge) schemas so that `allOf` / `$ref`-with-siblings
    // folds can resolve and inline a `$ref` branch's target, then normalize every
    // schema in place (merging/flattening `allOf` into a single materialized
    // schema) so the rest of the pipeline — validation, ref collection, and every
    // backend — sees a plain merged schema with no combinator residue.
    let raw_models = collect_raw_models(&docs);
    let doc_paths: BTreeSet<PathBuf> = docs.keys().cloned().collect();
    let merge_ctx = MergeCtx {
        doc_paths: &doc_paths,
        raw_models: &raw_models,
    };
    let canonical_paths: Vec<PathBuf> = docs.keys().cloned().collect();
    for canonical_path in &canonical_paths {
        let (path, doc) = docs
            .get_mut(canonical_path)
            .expect("document present for canonical path");
        let path = path.clone();
        normalize_document(&path, canonical_path, doc, &merge_ctx)?;
    }

    let mut models = BTreeMap::<TypeKey, JsonModel>::new();
    for (canonical_path, (path, doc)) in &docs {
        validate_document(path, doc)?;
        if let Some(defs) = &doc.defs {
            for (name, schema) in defs {
                validate_model_schema(path, schema, &format!("$defs.{name}"))?;
                models.insert(
                    TypeKey::Def(canonical_path.clone(), name.clone()),
                    JsonModel {
                        full_name: name.clone(),
                        canonical_path: canonical_path.clone(),
                        model_name: name.to_upper_camel_case(),
                        schema: schema.clone(),
                    },
                );
            }
        }
        if root_is_schema_shaped(&doc.root) && !doc.root.is_bare_ref() {
            validate_model_schema(path, &doc.root, "root schema")?;
            let model_name = root_type_name(path).to_upper_camel_case();
            models.insert(
                TypeKey::Root(canonical_path.clone()),
                JsonModel {
                    full_name: model_name.clone(),
                    canonical_path: canonical_path.clone(),
                    model_name,
                    schema: doc.root.clone(),
                },
            );
        }
    }

    validate_model_refs(&docs, &models)?;
    validate_all_unions(&docs, &models)?;
    validate_reference_satisfiability(&docs, &models)?;

    Ok(ParsedJsonDocuments { docs, models })
}

fn api_spec_from_parsed_json_documents(
    language: Language,
    parsed: &ParsedJsonDocuments,
    canonical_paths: &[PathBuf],
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> Result<ApiSpec> {
    let mut external_types = BTreeMap::new();
    for (key, model) in &parsed.models {
        if model_key_path(key).is_some_and(|path| canonical_paths.contains(path)) {
            insert_json_external_type(
                &mut external_types,
                model,
                &parsed.docs,
                &parsed.models,
                module_paths,
            )?;
            collect_schema_model_refs(
                &model.canonical_path,
                &model.canonical_path,
                &model.schema,
                &parsed.docs,
                &parsed.models,
                module_paths,
                &mut external_types,
            )?;
        }
    }

    let mut services = Vec::new();
    for canonical_path in canonical_paths {
        let Some((path, doc)) = parsed.docs.get(canonical_path) else {
            continue;
        };
        let Some(service_specs) = &doc.services else {
            continue;
        };
        for (service_key, service) in service_specs {
            services.push(build_service(
                path,
                canonical_path,
                service_key,
                service,
                &parsed.docs,
                &parsed.models,
                module_paths,
                &mut external_types,
            )?);
        }
    }

    let spec = ApiSpec {
        module_path: ModulePath::default(),
        data: (),
        version: "0.0.0".to_string(),
        support: SupportSpec::default(),
        services,
        types: external_types
            .into_iter()
            .map(|(name, binding)| (name, TypeDeclSpec::External(binding)))
            .collect(),
    };
    validate_identifier_namespace(language, &spec)?;
    Ok(spec)
}

fn model_key_path(key: &TypeKey) -> Option<&PathBuf> {
    match key {
        TypeKey::Root(path) | TypeKey::Def(path, _) => Some(path),
    }
}

fn collect_schema_model_refs(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<()> {
    if let Some(reference) = &schema.reference {
        let model = resolve_ref(path, canonical_path, reference, docs, models)?;
        let model_key = json_model_key(model, module_paths);
        if !external_types.contains_key(&model_key) {
            insert_json_external_type(external_types, model, docs, models, module_paths)?;
            collect_schema_model_refs(
                &model.canonical_path,
                &model.canonical_path,
                &model.schema,
                docs,
                models,
                module_paths,
                external_types,
            )?;
        }
        return Ok(());
    }
    if let Some(properties) = &schema.properties {
        for property in properties.values() {
            collect_schema_model_refs(
                path,
                canonical_path,
                property,
                docs,
                models,
                module_paths,
                external_types,
            )?;
        }
    }
    if let Some(items) = &schema.items {
        collect_schema_model_refs(
            path,
            canonical_path,
            items,
            docs,
            models,
            module_paths,
            external_types,
        )?;
    }
    if let Some(one_of) = &schema.one_of {
        for branch in one_of {
            collect_schema_model_refs(
                path,
                canonical_path,
                branch,
                docs,
                models,
                module_paths,
                external_types,
            )?;
        }
    }
    if let Some(additional) = &schema.additional_properties
        && additional.is_object()
    {
        let additional_schema =
            serde_json::from_value::<Schema>(additional.clone()).map_err(|error| {
                Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!("additionalProperties is invalid: {error}"),
                }
            })?;
        collect_schema_model_refs(
            path,
            canonical_path,
            &additional_schema,
            docs,
            models,
            module_paths,
            external_types,
        )?;
    }
    Ok(())
}

fn validate_document(path: &Path, doc: &Document) -> Result<()> {
    let has_nexus_envelope = doc.nexusrpc.is_some();
    if has_nexus_envelope && doc.nexusrpc.as_ref().and_then(Value::as_str) != Some("1.0.0") {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: "`nexusrpc` must be exactly \"1.0.0\"".to_string(),
        });
    }
    if let Some(schema) = &doc.schema
        && schema.as_str() != Some("https://json-schema.org/draft/2020-12/schema")
    {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: "`$schema` must be `https://json-schema.org/draft/2020-12/schema`".to_string(),
        });
    }
    if has_nexus_envelope && root_is_schema_shaped(&doc.root) {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: "a Nexus JSON schema document root is an envelope, not a model; move the model into `$defs`"
                .to_string(),
        });
    }
    if !has_nexus_envelope && doc.services.is_some() {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: "`services` require a Nexus JSON schema document with `nexusrpc: \"1.0.0\"`"
                .to_string(),
        });
    }
    // A definitions-only pure file (only `$defs`, plus optional `description` /
    // `$schema`, and no `nexusrpc`) is a definitions bucket, not a type: it has
    // no file-root type and contributes its `$defs` alone. See
    // `specs/json-schema/input-files.md` (Definitions-only exception). We reject only
    // a plain file that carries neither a root schema nor any `$defs`.
    let has_defs = doc.defs.as_ref().is_some_and(|defs| !defs.is_empty());
    if !has_nexus_envelope && !root_is_schema_shaped(&doc.root) && !has_defs {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: "plain JSON schema files must define a root schema or `$defs`".to_string(),
        });
    }
    Ok(())
}

fn validate_model_schema(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    validate_schema_common(path, schema, context)?;
    if schema.reference.is_some() {
        return Ok(());
    }
    // A named `oneOf` union (a `$def` whose body is a supported sum type) is a
    // model in its own right; the structural sum-type checks (decidable
    // selector, disjoint kinds, discriminator) run in the ref-resolving union
    // pass. See `specs/json-schema/features/oneOf.md`.
    if schema.one_of.is_some() && schema.ty.is_none() {
        return validate_schema_tree(path, schema, context);
    }
    if schema.ty.as_ref().and_then(Value::as_str) != Some("object") {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("{context} must be `type: object`, a `oneOf` union, or a bare `$ref`"),
        });
    }
    validate_schema_tree(path, schema, context)
}

fn validate_schema_tree(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    validate_schema_node(path, schema, context, false)
}

fn validate_schema_node(
    path: &Path,
    schema: &Schema,
    context: &str,
    is_union_branch: bool,
) -> Result<()> {
    validate_schema_common(path, schema, context)?;
    // A `type: "null"` is legal only as a nullability `oneOf` branch; the array
    // `type` form is never legal. Skip the standalone-null reject for branches.
    if !is_union_branch {
        validate_type_form(path, schema, context)?;
    } else if matches!(&schema.ty, Some(Value::Array(_))) {
        validate_type_form(path, schema, context)?;
    }
    validate_numeric_constraints(path, schema, context)?;
    validate_string_constraints(path, schema, context)?;
    validate_format(path, schema, context)?;
    validate_content_encoding(path, schema, context)?;
    validate_array_constraints(path, schema, context)?;
    validate_object_constraints(path, schema, context)?;
    validate_const_enum(path, schema, context)?;
    validate_default(path, schema, context)?;
    // Runs after the value keywords so a composite `const`/`enum`/`default` on a
    // shapeless `type: object` reports the more specific value diagnostic first.
    if !is_union_branch {
        validate_type_presence(path, schema, context)?;
    }
    validate_annotations(path, schema, context)?;
    validate_required(path, schema, context)?;
    if let Some(properties) = &schema.properties {
        let required: Vec<&str> = schema
            .required
            .as_ref()
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for (name, property) in properties {
            // `default` on a `required` member is dead metadata (a required
            // member is never absent, so its default never applies) → reject
            // (P7.1). See `specs/json-schema/features/default.md`.
            if property.extra.contains_key("default") && required.contains(&name.as_str()) {
                return Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!(
                        "{context}.properties.{name}: `default` on a required member never applies (a required member is always present); make the member optional, or drop the `default`"
                    ),
                });
            }
            validate_schema_node(
                path,
                property,
                &format!("{context}.properties.{name}"),
                false,
            )?;
        }
    }
    if let Some(items) = &schema.items {
        validate_schema_node(path, items, &format!("{context}.items"), false)?;
    }
    if let Some(one_of) = &schema.one_of {
        // Structural sum-type acceptance/rejection (decidable selector,
        // disjoint kinds, discriminator, `integer`+`number` overlap, …) needs
        // `$ref` resolution and runs in `validate_all_unions`. Here we only
        // recurse into each branch's own subtree.
        for branch in one_of {
            validate_schema_node(path, branch, &format!("{context}.oneOf"), true)?;
        }
    }
    if let Some(additional) = &schema.additional_properties {
        match additional {
            // `true` (open map) / `false` (closed object) are the accepted flags.
            Value::Bool(_) => {}
            Value::Object(_) => {
                let additional_schema = serde_json::from_value::<Schema>(additional.clone())
                    .map_err(|error| Error::InvalidJsonSchema {
                        path: path.to_path_buf(),
                        reason: format!("{context}.additionalProperties is invalid: {error}"),
                    })?;
                // `additionalProperties: {}` — the empty schema — means "any value",
                // exactly what `true` means; require the unambiguous spelling. The
                // pre-validation normalize pass re-serializes an empty schema into a
                // null-filled object, so compare against the default rather than an
                // empty map.
                if additional_schema == Schema::default() {
                    return Err(Error::InvalidJsonSchema {
                        path: path.to_path_buf(),
                        reason: format!(
                            "{context}.additionalProperties: an empty schema `{{}}` means any value; write `additionalProperties: true` instead"
                        ),
                    });
                }
                validate_schema_tree(
                    path,
                    &additional_schema,
                    &format!("{context}.additionalProperties"),
                )?;
            }
            _ => {
                return Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!(
                        "{context}.additionalProperties must be `true`, `false`, or a schema object"
                    ),
                });
            }
        }
    }
    Ok(())
}

/// The fix-it reason for an unsupported keyword: points at the coherent
/// in-subset alternative rather than a bare "not supported" (see each keyword's
/// feature spec).
fn unsupported_keyword_reason(keyword: &str) -> &'static str {
    match keyword {
        "anyOf" => {
            "`anyOf` is not supported; a value-level union is expressed with a `oneOf` of pairwise-disjoint kinds"
        }
        "if" | "then" | "else" => {
            "conditional schemas (`if`/`then`/`else`) are not supported; model the alternatives as a `oneOf`"
        }
        "prefixItems" => {
            "tuple arrays (`prefixItems`) are not supported; use a single uniform `items` element type"
        }
        "unevaluatedProperties" => {
            "`unevaluatedProperties` is not supported; bound extra members with `additionalProperties` (`true`, `false`, or a value schema)"
        }
        "unevaluatedItems" => {
            "`unevaluatedItems` is not supported; bound the element type with `items`"
        }
        "dependentSchemas" => {
            "`dependentSchemas` is not supported; a conditional subschema has no static shape — split the variants into explicit types"
        }
        "patternProperties" => {
            "`patternProperties` is not supported; use a typed map (`additionalProperties: {type: ...}`) or enumerate the keys under `properties`"
        }
        "nullable" => {
            "OAS 3.0 `nullable` is not supported; model a nullable field with `oneOf: [{type: T}, {type: \"null\"}]`"
        }
        "$anchor" | "$dynamicRef" | "$dynamicAnchor" => {
            "`$anchor`/`$dynamicRef`/`$dynamicAnchor` are not supported; use a plain `$ref`"
        }
        "$vocabulary" => {
            "`$vocabulary` is not supported; it is a meta-schema keyword with no place in a type schema (the dialect is pinned to 2020-12)"
        }
        other => panic!("unsupported-keyword reason requested for unhandled keyword `{other}`"),
    }
}

fn validate_schema_common(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    if schema.id.is_some() {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("{context}: `$id` is not supported"),
        });
    }
    if schema.reference.is_some() && !schema.is_bare_ref() {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("{context}: a `$ref` must not carry sibling keywords"),
        });
    }
    if let Some(reference) = &schema.reference
        && (reference.starts_with("http://") || reference.starts_with("https://"))
    {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("{context}: remote `$ref` `{reference}` is not supported"),
        });
    }
    // `not` has degenerate forms the spec calls out with distinct diagnostics.
    if let Some(negated) = schema.extra.get("not") {
        let reason = match negated {
            Value::Object(map) if map.is_empty() => {
                "`not: {}` is unsatisfiable — it accepts no instance (a dead type)"
            }
            Value::Bool(true) => {
                "`not: true` is unsatisfiable — it accepts no instance (a dead type)"
            }
            Value::Bool(false) => {
                "`not: false` is a no-op — it constrains nothing (a dead keyword); remove it"
            }
            _ => {
                "`not` is not supported; state the positive `type`/constraints, or enumerate the admissible values with `enum`, rather than what is disallowed"
            }
        };
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("{context}: {reason}"),
        });
    }
    for keyword in [
        "anyOf",
        "if",
        "then",
        "else",
        "prefixItems",
        "unevaluatedProperties",
        "unevaluatedItems",
        "dependentSchemas",
        "patternProperties",
        "$anchor",
        "$dynamicRef",
        "$dynamicAnchor",
        "$vocabulary",
        "nullable",
    ] {
        if schema.extra.contains_key(keyword) {
            return Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: {}", unsupported_keyword_reason(keyword)),
            });
        }
    }
    // The directional / content metadata keywords reject with a fix-it (they
    // have no single-type lowering — see the reject specs). `deprecated` is the
    // supported sibling; `examples`/`$comment` are accepted-and-ignored.
    if schema.extra.contains_key("readOnly") || schema.extra.contains_key("writeOnly") {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "{context}: `readOnly`/`writeOnly` is not supported; a directional annotation has no single-type lowering (drop it, or split the type into request/response shapes)"
            ),
        });
    }
    if schema.extra.contains_key("contentMediaType") {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "{context}: `contentMediaType` is not supported; the string is carried verbatim (drop it, or validate the media type in application code)"
            ),
        });
    }
    if schema.extra.contains_key("contentSchema") {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "{context}: `contentSchema` is not supported; a schema over encoded string content has no native lowering (drop it, or model the decoded value as its own typed member)"
            ),
        });
    }
    Ok(())
}

/// Requires every leaf schema to name an explicit, known `type`, and requires
/// `type: object` / `type: array` to carry a concrete shape (see
/// `specs/json-schema/features/type.md`). A `oneOf` / `$ref` schema is exempt — its
/// shape comes from the branches or the referenced target; `allOf` is merged
/// away before validation runs. Not called for union branches (their kind is
/// checked by the sum-type pass).
fn validate_type_presence(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    if schema.reference.is_some() || schema.one_of.is_some() {
        return Ok(());
    }
    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };
    // The array `type` form and standalone `type: "null"` are rejected earlier
    // by `validate_type_form`, so here an unreadable `type` means it is absent.
    let Some(name) = schema.ty.as_ref().and_then(Value::as_str) else {
        return reject(format!(
            "{context}: a leaf schema requires an explicit `type`; add one (e.g. `type: object`), or supply the shape via `oneOf`, `allOf`, or `$ref`"
        ));
    };
    const KNOWN: [&str; 7] = [
        "null", "boolean", "object", "array", "number", "string", "integer",
    ];
    if !KNOWN.contains(&name) {
        return reject(format!(
            "{context}: unknown `type` `{name}`; use one of `null`, `boolean`, `object`, `array`, `number`, `string`, `integer`"
        ));
    }
    match name {
        "object" => {
            if schema.properties.is_none() && schema.additional_properties.is_none() {
                return reject(format!(
                    "{context}: `type: object` needs an explicit shape; add `properties: {{...}}` (typed struct), `additionalProperties: true` (open map), or `additionalProperties: false` (closed empty object)"
                ));
            }
        }
        "array" => {
            if schema.items.is_none() {
                return reject(format!(
                    "{context}: `type: array` needs an explicit element type; add `items: {{...}}`"
                ));
            }
        }
        _ => {
            if schema.properties.is_some() || schema.additional_properties.is_some() {
                return reject(format!(
                    "{context}: `properties`/`additionalProperties` require `type: object`"
                ));
            }
            if schema.items.is_some() {
                return reject(format!("{context}: `items` requires `type: array`"));
            }
        }
    }
    Ok(())
}

/// Load-time validation of `required` (see `specs/json-schema/features/required.md`):
/// the value must be an array of unique property-name strings, and every name
/// must be declared in `properties` (P7.1 — a mandatory member with no declared
/// shape is undecidable).
fn validate_required(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    let Some(value) = &schema.required else {
        return Ok(());
    };
    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };
    let Some(entries) = value.as_array() else {
        return reject(format!(
            "{context}: `required` must be an array of property-name strings"
        ));
    };
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for entry in entries {
        let Some(name) = entry.as_str() else {
            return reject(format!(
                "{context}: `required` may contain only property-name strings; `{entry}` is not a string"
            ));
        };
        if !names.insert(name) {
            return reject(format!(
                "{context}: `required` lists `{name}` more than once; entries must be unique"
            ));
        }
    }
    let declared: BTreeSet<&str> = schema
        .properties
        .as_ref()
        .map(|properties| properties.keys().map(String::as_str).collect())
        .unwrap_or_default();
    for &name in &names {
        if !declared.contains(name) {
            return reject(format!(
                "{context}: `required` names `{name}`, which is not declared in `properties`; add it to `properties` or remove it from `required`"
            ));
        }
    }
    Ok(())
}

/// Rejects the two unsupported spellings of `type`: the array form
/// (`["string","null"]`) and a standalone `type: "null"`. Both are degenerate
/// or ambiguous here — nullability is modeled with the dedicated
/// `oneOf:[{type:T},{type:"null"}]` convention (see `specs/json-schema/features/type.md`
/// and `nullability.md`). A `type: "null"` is legal *only* as one branch of that
/// `oneOf`, so this check is skipped for union branches by the caller.
fn validate_type_form(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    match &schema.ty {
        Some(Value::Array(_)) => Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "{context}: an array `type` (e.g. `[\"string\",\"null\"]`) is not supported; it is structurally ambiguous — model a nullable field with `oneOf: [{{type: T}}, {{type: \"null\"}}]` instead"
            ),
        }),
        Some(Value::String(name)) if name == "null" => Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "{context}: a standalone `type: \"null\"` is not supported; a field that is always null carries no information — model a nullable field with `oneOf: [{{type: T}}, {{type: \"null\"}}]` instead"
            ),
        }),
        _ => Ok(()),
    }
}

/// Load-time validation of the numeric-constraint keywords (`minimum`,
/// `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`). See
/// `specs/json-schema/features/maximum.md` and `multipleOf.md` for the authoritative
/// rules. The keywords remain in the schema `extra` map for the backends; this
/// only rejects statically unsatisfiable / unsupported forms with fix-its.
fn validate_numeric_constraints(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    const KEYWORDS: [&str; 5] = [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
    ];
    if KEYWORDS.iter().all(|key| !schema.extra.contains_key(*key)) {
        return Ok(());
    }

    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    let ty = schema.ty.as_ref().and_then(Value::as_str);
    let is_integer = ty == Some("integer");
    let is_number = ty == Some("number");
    if !is_integer && !is_number {
        // P7.1: a numeric bound is statically meaningless on any non-numeric
        // type (including a bare/typeless or nullable `oneOf` node).
        return reject(format!(
            "{context}: numeric constraint keywords (`minimum`/`maximum`/`exclusiveMinimum`/`exclusiveMaximum`/`multipleOf`) require `type: integer` or `type: number`"
        ));
    }

    // Extract each keyword as a finite f64, rejecting non-numbers and the
    // draft-4/OAS-3.0 boolean form of the exclusive keywords.
    let bound = |key: &str| -> Result<Option<f64>> {
        match schema.extra.get(key) {
            None => Ok(None),
            Some(Value::Number(number)) => match number.as_f64() {
                Some(value) if value.is_finite() => Ok(Some(value)),
                _ => Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!("{context}: `{key}` must be a finite number"),
                }),
            },
            Some(Value::Bool(_)) if key == "exclusiveMaximum" || key == "exclusiveMinimum" => {
                let inclusive = if key == "exclusiveMaximum" {
                    "maximum"
                } else {
                    "minimum"
                };
                Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!(
                        "{context}: the draft-4/OpenAPI-3.0 boolean form `{key}: true` is not supported; write `{key}: <number>` for a strict bound (or `{inclusive}: <number>` for an inclusive one)"
                    ),
                })
            }
            Some(_) => Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `{key}` must be a number"),
            }),
        }
    };

    let minimum = bound("minimum")?;
    let maximum = bound("maximum")?;
    let exclusive_minimum = bound("exclusiveMinimum")?;
    let exclusive_maximum = bound("exclusiveMaximum")?;
    let multiple_of = bound("multipleOf")?;

    // Integer fields require integer-valued bounds (Pydantic cannot build a
    // fractional `le`/`ge` on an int field; keeps cross-language comparison
    // exact).
    if is_integer {
        for (key, value) in [
            ("minimum", minimum),
            ("maximum", maximum),
            ("exclusiveMinimum", exclusive_minimum),
            ("exclusiveMaximum", exclusive_maximum),
        ] {
            if let Some(value) = value
                && value.fract() != 0.0
            {
                return reject(format!(
                    "{context}: `{key}` must be an integer bound on an `integer` field (got {value}); use an integer bound, or make the field `type: number`"
                ));
            }
        }
    }

    // `multipleOf` must be a positive integer (fractional divisors deferred).
    if let Some(divisor) = multiple_of {
        if divisor <= 0.0 {
            return reject(format!(
                "{context}: `multipleOf` must be greater than 0 (got {divisor})"
            ));
        }
        if divisor.fract() != 0.0 {
            return reject(format!(
                "{context}: `multipleOf: {divisor}` is not yet supported; fractional divisors are deferred, use a positive integer divisor"
            ));
        }
    }

    // Same-axis redundant pairs (P7.1): one bound always dominates.
    if maximum.is_some() && exclusive_maximum.is_some() {
        return reject(format!(
            "{context}: specify exactly one of `maximum` or `exclusiveMaximum`, not both"
        ));
    }
    if minimum.is_some() && exclusive_minimum.is_some() {
        return reject(format!(
            "{context}: specify exactly one of `minimum` or `exclusiveMinimum`, not both"
        ));
    }

    // Satisfiability of the combined bounds (empty accepted set → reject).
    let lower = minimum
        .map(|value| (value, false))
        .or(exclusive_minimum.map(|value| (value, true)));
    let upper = maximum
        .map(|value| (value, false))
        .or(exclusive_maximum.map(|value| (value, true)));
    if let (Some((lo, lo_exclusive)), Some((hi, hi_exclusive))) = (lower, upper) {
        let empty = if is_integer {
            let smallest = if lo_exclusive { lo + 1.0 } else { lo };
            let largest = if hi_exclusive { hi - 1.0 } else { hi };
            smallest > largest
        } else {
            lo > hi || (lo == hi && (lo_exclusive || hi_exclusive))
        };
        if empty {
            return reject(format!(
                "{context}: the numeric bounds describe an empty range (no value can satisfy them)"
            ));
        }
    }

    // Integer range + `multipleOf`: reject when no multiple lies in the range.
    if is_integer
        && let Some(divisor) = multiple_of
        && let (Some((lo, lo_exclusive)), Some((hi, hi_exclusive))) = (lower, upper)
    {
        let smallest = if lo_exclusive { lo + 1.0 } else { lo };
        let largest = if hi_exclusive { hi - 1.0 } else { hi };
        let greatest_multiple = (largest / divisor).floor() * divisor;
        if greatest_multiple < smallest {
            return reject(format!(
                "{context}: no multiple of {divisor} lies within the accepted range"
            ));
        }
    }

    // A pinned literal (`const`/`default`) or any closed-set `enum` member on the
    // same node must satisfy the bounds — a value the field can never legally
    // hold is a schema bug (P13.1).
    let bound_violation = |value: f64| -> Option<String> {
        if let Some(max) = maximum
            && value > max
        {
            Some(format!("must be <= {max}"))
        } else if let Some(min) = minimum
            && value < min
        {
            Some(format!("must be >= {min}"))
        } else if let Some(excl) = exclusive_maximum
            && value >= excl
        {
            Some(format!("must be < {excl}"))
        } else if let Some(excl) = exclusive_minimum
            && value <= excl
        {
            Some(format!("must be > {excl}"))
        } else if let Some(divisor) = multiple_of
            && (value / divisor).fract() != 0.0
        {
            Some(format!("must be a multiple of {divisor}"))
        } else {
            None
        }
    };
    for literal_key in ["const", "default"] {
        let Some(Value::Number(number)) = schema.extra.get(literal_key) else {
            continue;
        };
        let Some(value) = number.as_f64() else {
            continue;
        };
        if let Some(reason) = bound_violation(value) {
            return reject(format!(
                "{context}: `{literal_key}` value {value} violates the numeric bounds ({reason})"
            ));
        }
    }
    if let Some(Value::Array(members)) = schema.extra.get("enum") {
        for member in members {
            let Some(value) = member.as_f64() else {
                continue;
            };
            if let Some(reason) = bound_violation(value) {
                return reject(format!(
                    "{context}: `enum` value {value} violates the numeric bounds ({reason})"
                ));
            }
        }
    }

    Ok(())
}

/// Load-time validation of the string-length keywords (`minLength`,
/// `maxLength`). See `specs/json-schema/features/maxLength.md` for the authoritative
/// rules. Length is counted in Unicode code points. The keywords remain in the
/// schema `extra` map for the backends; this only rejects statically
/// unsatisfiable / unsupported forms with fix-its.
fn validate_string_constraints(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    const KEYWORDS: [&str; 2] = ["minLength", "maxLength"];
    if KEYWORDS.iter().all(|key| !schema.extra.contains_key(*key)) {
        return Ok(());
    }

    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    // P7.1: a string-length bound is statically meaningless on a non-string
    // type (the array-length analog is `maxItems`, the member-count analog is
    // `maxProperties`).
    if schema.ty.as_ref().and_then(Value::as_str) != Some("string") {
        return reject(format!(
            "{context}: string-length keywords (`minLength`/`maxLength`) require `type: string`"
        ));
    }

    // Each bound must be a non-negative integer. A `.0`-valued float is accepted
    // as its integer value (honoring the `1.0`-as-integer rule from `type`).
    let bound = |key: &str| -> Result<Option<u64>> {
        match schema.extra.get(key) {
            None => Ok(None),
            Some(Value::Number(number)) => match number.as_f64() {
                Some(value) if value.is_finite() && value >= 0.0 && value.fract() == 0.0 => {
                    Ok(Some(value as u64))
                }
                _ => Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!("{context}: `{key}` must be a non-negative integer"),
                }),
            },
            Some(_) => Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `{key}` must be a non-negative integer"),
            }),
        }
    };

    let min_length = bound("minLength")?;
    let max_length = bound("maxLength")?;

    // `minLength > maxLength` is unsatisfiable; `minLength == maxLength` pins an
    // exact length (accepted — a fixed-width string).
    if let (Some(min), Some(max)) = (min_length, max_length)
        && min > max
    {
        return reject(format!(
            "{context}: `minLength` ({min}) exceeds `maxLength` ({max}); the bounds describe an empty range (no string can satisfy them)"
        ));
    }

    // A `const`/`default`/`enum` string literal on the same node must satisfy
    // the bounds (code-point length) at load.
    let check_literal = |literal: &str, source: &str| -> Result<()> {
        let length = literal.chars().count() as u64;
        if let Some(max) = max_length
            && length > max
        {
            return reject(format!(
                "{context}: `{source}` value {literal:?} has length {length}, exceeding `maxLength` {max}"
            ));
        }
        if let Some(min) = min_length
            && length < min
        {
            return reject(format!(
                "{context}: `{source}` value {literal:?} has length {length}, below `minLength` {min}"
            ));
        }
        Ok(())
    };
    for literal_key in ["const", "default"] {
        if let Some(Value::String(literal)) = schema.extra.get(literal_key) {
            check_literal(literal, literal_key)?;
        }
    }
    if let Some(Value::Array(values)) = schema.extra.get("enum") {
        for value in values {
            if let Some(literal) = value.as_str() {
                check_literal(literal, "enum")?;
            }
        }
    }

    Ok(())
}

/// Load-time gate for the `format` keyword (JSON Schema 2020-12 §7). We opt into
/// `format-assertion` semantics for a curated portable subset and reject
/// everything else at load, so no `format` silently no-ops (P10). See
/// `specs/json-schema/features/format.md` and `crate::json_schema::format`.
///
/// Rejects (P7 / P7.1): a non-string `format` value, a `format` on a
/// non-`string` node, an unknown/non-standard name (with a fix-it), a
/// deferred standard format, the temporal formats (materialization pending), and
/// a `const`/`default`/`enum` string literal on the same node that fails its
/// format. The `format` value stays in the schema `extra` map for the backends.
fn validate_format(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    let Some(value) = schema.extra.get("format") else {
        return Ok(());
    };

    let reject = |reason: String| -> Result<()> {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    let Some(format) = value.as_str() else {
        return reject(format!("{context}: `format` must be a string"));
    };

    // P7.1: `format` names a semantic shape of a string; it is statically
    // meaningless on any other type (a vacuous no-op the spec would allow, a load
    // reject here — as [[pattern]] treats a type mismatch).
    if schema.ty.as_ref().and_then(Value::as_str) != Some("string") {
        return reject(format!("{context}: `format` requires `type: string`"));
    }

    let format_name = match crate::json_schema::format::classify(format) {
        crate::json_schema::format::FormatClass::Supported(check) => check.name,
        // The temporal formats are materialized into native typed fields with a
        // narrowed grammar (leap `:60` rejected; `duration` time-only). Any
        // supplied literal is validated against that materialized grammar below.
        crate::json_schema::format::FormatClass::Temporal(kind) => kind.name(),
        crate::json_schema::format::FormatClass::Deferred => {
            return reject(format!(
                "{context}: `format: {format}` is not yet supported (deferred); \
                 it needs IDNA/Unicode or templating handling that is not yet portable"
            ));
        }
        crate::json_schema::format::FormatClass::Unknown => {
            return reject(format!(
                "{context}: unknown `format: {format}`; supported formats are {}",
                crate::json_schema::format::SUPPORTED_FORMATS.join(", ")
            ));
        }
    };

    // A supplied `const`/`default`/`enum` string literal on the same node must
    // satisfy the format at load (the literal-vs-constraint obligation).
    let check_literal = |literal: &str, source: &str| -> Result<()> {
        if !crate::json_schema::format::is_valid(format, literal) {
            return reject(format!(
                "{context}: `{source}` value {literal:?} is not a valid {format_name}"
            ));
        }
        Ok(())
    };
    for literal_key in ["const", "default"] {
        if let Some(Value::String(literal)) = schema.extra.get(literal_key) {
            check_literal(literal, literal_key)?;
        }
    }
    if let Some(Value::Array(values)) = schema.extra.get("enum") {
        for value in values {
            if let Some(literal) = value.as_str() {
                check_literal(literal, "enum")?;
            }
        }
    }

    Ok(())
}

/// Load-time gate for the `contentEncoding` keyword (JSON Schema 2020-12 §8.3).
/// We opt into assertion + materialization for the two byte-transform encodings
/// (`base64` / `base64url`, materialized to a native bytes type) and reject every
/// other encoding at load, so no `contentEncoding` silently no-ops (P10). See
/// `specs/json-schema/features/contentEncoding.md` and `crate::json_schema::content_encoding`.
///
/// Rejects (P7 / P7.1): a non-string `contentEncoding` value, a
/// `contentEncoding` on a non-`string` node, an unsupported encoding (with a
/// fix-it), a co-occurring `contentMediaType` / `contentSchema` (owned by those
/// features, which have nowhere to emit the label in the model), and a
/// `const`/`default`/`enum` string literal that is not well-formed for the
/// declared encoding. The value stays in the schema `extra` map for the backends.
fn validate_content_encoding(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    let Some(value) = schema.extra.get("contentEncoding") else {
        return Ok(());
    };

    let reject = |reason: String| -> Result<()> {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    // The spec's own MUST: the value MUST be a string.
    let Some(encoding_name) = value.as_str() else {
        return reject(format!("{context}: `contentEncoding` must be a string"));
    };

    // P7.1: `contentEncoding` describes a string that is really encoded binary;
    // it is statically meaningless on any other type (a vacuous no-op the spec
    // would allow, a load reject here — as [[format]] / [[pattern]] treat a type
    // mismatch).
    if schema.ty.as_ref().and_then(Value::as_str) != Some("string") {
        return reject(format!(
            "{context}: `contentEncoding` requires `type: string`"
        ));
    }

    let encoding = match crate::json_schema::content_encoding::classify(encoding_name) {
        crate::json_schema::content_encoding::EncodingClass::Supported(encoding) => encoding,
        crate::json_schema::content_encoding::EncodingClass::Unsupported => {
            return reject(format!(
                "{context}: `contentEncoding: {encoding_name}` is not supported; \
                 supported encodings are {}",
                crate::json_schema::content_encoding::SUPPORTED_ENCODINGS.join(", ")
            ));
        }
    };

    // `contentMediaType` / `contentSchema` on the same node are owned by their
    // own features (a base64 blob labeled with a media type has nowhere to emit
    // the label in the model); the reject there wins over materialization here.
    for labeled in ["contentMediaType", "contentSchema"] {
        if schema.extra.contains_key(labeled) {
            return reject(format!(
                "{context}: `{labeled}` alongside `contentEncoding` is not supported \
                 (materialized bytes are unlabeled binary; drop `{labeled}`)"
            ));
        }
    }

    // A supplied `const`/`default`/`enum` string literal on the same node must be
    // well-formed for the declared encoding at load (the literal-vs-constraint
    // obligation), and is thereby stored / echoed in its canonical form.
    let check_literal = |literal: &str, source: &str| -> Result<()> {
        if !crate::json_schema::content_encoding::is_valid(encoding, literal) {
            return reject(format!(
                "{context}: `{source}` value {literal:?} is not valid {}-encoded data",
                encoding.name()
            ));
        }
        Ok(())
    };
    for literal_key in ["const", "default"] {
        if let Some(Value::String(literal)) = schema.extra.get(literal_key) {
            check_literal(literal, literal_key)?;
        }
    }
    if let Some(Value::Array(values)) = schema.extra.get("enum") {
        for value in values {
            if let Some(literal) = value.as_str() {
                check_literal(literal, "enum")?;
            }
        }
    }

    Ok(())
}

/// The scalar JSON kind of a schema `type` string, if it names a scalar. Object
/// and array (and unknown) yield `None` — the composite line the array
/// assertions draw their support envelope at.
fn scalar_type(ty: Option<&str>) -> Option<&'static str> {
    match ty {
        Some("string") => Some("string"),
        Some("boolean") => Some("boolean"),
        Some("integer") => Some("integer"),
        Some("number") => Some("number"),
        _ => None,
    }
}

/// The scalar JSON kind of a literal value (for `const`/`enum` matcher
/// compatibility). Composite values (arrays/objects) yield `None`.
fn scalar_value_kind(value: &Value) -> Option<&'static str> {
    match value {
        Value::String(_) => Some("string"),
        Value::Bool(_) => Some("boolean"),
        Value::Number(number) => {
            if number.as_f64().is_some_and(|value| value.fract() == 0.0) {
                Some("integer")
            } else {
                Some("number")
            }
        }
        _ => None,
    }
}

/// True when the two scalar kinds can name the same element (string/string,
/// boolean/boolean, or any numeric pair — an integer-valued number normalizes
/// to an integer per `type`).
fn scalar_kinds_compatible(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    matches!((a, b), ("integer", "number") | ("number", "integer"))
}

/// Load-time validation of the `default` annotation's own shape. `default` is a
/// pure annotation (no validator); the only load obligations are shape checks
/// (see `specs/json-schema/features/default.md`). The `const`+`default` /
/// `enum`+`default` interactions live in `validate_const_enum`, and the
/// default-against-constraint checks live in the numeric/string/content
/// validators; the `default` on a `required` member is caught at the parent
/// object level. This function enforces: no `null` default (degenerate), no
/// object/array default (deferred to composite-value materialization), and a
/// scalar default that is type-compatible with the declared `type`.
fn validate_default(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    let Some(default) = schema.extra.get("default") else {
        return Ok(());
    };
    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };
    // `default: null` → reject as degenerate (mirrors `const: null`).
    if default.is_null() {
        return reject(format!(
            "{context}: `default: null` is not supported; on a non-nullable member it is invalid, and on a nullable one it is a no-op (absence already surfaces as null) — drop it"
        ));
    }
    // Object/array default → reject (deferred; composite-value materialization is
    // not yet specified — the member's *type* may still be object/array).
    let Some(value_kind) = scalar_value_kind(default) else {
        return reject(format!(
            "{context}: an object/array `default` value is not yet supported; only scalar (string/number/integer/boolean) defaults are materialized on read (scalar values only)"
        ));
    };
    // Scalar default: it must be assignable to the declared `type`. A typeless
    // node (a bare/nullable `oneOf`) carries no scalar type to clash with, so a
    // scalar default is accepted there.
    let ty = schema.ty.as_ref().and_then(Value::as_str);
    match scalar_type(ty) {
        Some(declared) if !scalar_kinds_compatible(declared, value_kind) => reject(format!(
            "{context}: `default` value {default} (of kind `{value_kind}`) is incompatible with `type: {}`",
            ty.unwrap_or("")
        )),
        None if ty.is_some() => reject(format!(
            "{context}: `default` value {default} (of kind `{value_kind}`) is incompatible with `type: {}`",
            ty.unwrap_or("")
        )),
        _ => Ok(()),
    }
}

/// Load-time validation of the metadata annotations: `title` (§9.1),
/// `deprecated` (§9.3), `$comment` (core §8.3), and `examples` (§9.5). None
/// contribute a validator; these are pure shape checks (see the feature specs).
/// `title` becomes the doc-comment summary line, `deprecated` a native marker;
/// `examples` and `$comment` are accepted and dropped (never leak into output).
fn validate_annotations(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };
    // `title` — a short label; empty/whitespace-only or multi-line is degenerate.
    if let Some(title) = &schema.title {
        if title.trim().is_empty() {
            return reject(format!(
                "{context}: `title` must not be empty or whitespace-only (it would render a dead doc summary); drop it, or give it text"
            ));
        }
        if title.contains('\n') {
            return reject(format!(
                "{context}: `title` must be a single line (it is the doc-comment summary); move the prose to `description`"
            ));
        }
    }
    // `description` — the doc body; may span paragraphs, but an empty or
    // whitespace-only string renders a dead doc body (see
    // `specs/json-schema/features/description.md`).
    if let Some(description) = &schema.description
        && description.trim().is_empty()
    {
        return reject(format!(
            "{context}: `description` must not be empty or whitespace-only; drop it, or give it text"
        ));
    }
    // `deprecated` — the spec's own MUST: boolean. `false` is accepted and inert.
    if let Some(value) = schema.extra.get("deprecated")
        && !value.is_boolean()
    {
        return reject(format!(
            "{context}: `deprecated` must be a boolean, got {value}"
        ));
    }
    // `$comment` — the spec's own MUST: string (any content, incl. empty).
    if let Some(value) = schema.extra.get("$comment")
        && !value.is_string()
    {
        return reject(format!(
            "{context}: `$comment` must be a string, got {value}"
        ));
    }
    // `examples` — accepted and ignored (inert); its array-MUST is not enforced
    // while dropped (see specs/json-schema/features/examples.md). No check.
    Ok(())
}

/// Load-time validation of the array-constraint keywords (`minItems`,
/// `maxItems`, `uniqueItems`, `contains`, `minContains`, `maxContains`). See
/// `specs/json-schema/features/{minItems,maxItems,uniqueItems,contains,minContains,maxContains}.md`
/// for the authoritative rules. The keywords remain in the schema `extra` map
/// for the backends; this only rejects statically unsatisfiable / unsupported
/// (deferred) forms with fix-its.
fn validate_array_constraints(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    const KEYWORDS: [&str; 6] = [
        "minItems",
        "maxItems",
        "uniqueItems",
        "contains",
        "minContains",
        "maxContains",
    ];
    if KEYWORDS.iter().all(|key| !schema.extra.contains_key(*key)) {
        return Ok(());
    }

    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    // P7.1: array-constraint keywords are statically meaningless on a non-array
    // type (the string-length analog is `maxLength`, the object member-count
    // analog is `maxProperties`).
    if schema.ty.as_ref().and_then(Value::as_str) != Some("array") {
        return reject(format!(
            "{context}: array-constraint keywords (`minItems`/`maxItems`/`uniqueItems`/`contains`/`minContains`/`maxContains`) require `type: array`"
        ));
    }

    // Each count bound must be a non-negative integer. A `.0`-valued float is
    // accepted as its integer value (honoring the `1.0`-as-integer rule).
    let bound = |key: &str| -> Result<Option<u64>> {
        match schema.extra.get(key) {
            None => Ok(None),
            Some(Value::Number(number)) => match number.as_f64() {
                Some(value) if value.is_finite() && value >= 0.0 && value.fract() == 0.0 => {
                    Ok(Some(value as u64))
                }
                _ => Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!("{context}: `{key}` must be a non-negative integer"),
                }),
            },
            Some(_) => Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `{key}` must be a non-negative integer"),
            }),
        }
    };

    let min_items = bound("minItems")?;
    let max_items = bound("maxItems")?;
    // `minItems > maxItems` is unsatisfiable; `minItems == maxItems` pins an
    // exact size (accepted — a fixed-size array).
    if let (Some(min), Some(max)) = (min_items, max_items)
        && min > max
    {
        return reject(format!(
            "{context}: `minItems` ({min}) exceeds `maxItems` ({max}); the bounds describe an empty range (no array can satisfy them)"
        ));
    }

    let items_kind = scalar_type(
        schema
            .items
            .as_ref()
            .and_then(|item| item.ty.as_ref().and_then(Value::as_str)),
    );
    let items_is_scalar = items_kind.is_some();

    // `uniqueItems` must be a boolean; `true` over a composite element type is
    // deferred (composite deep-equality is correct in principle, just costly).
    match schema.extra.get("uniqueItems") {
        None => {}
        Some(Value::Bool(unique)) => {
            if *unique && !items_is_scalar {
                return reject(format!(
                    "{context}: `uniqueItems: true` over a composite element type is not yet supported; deep structural equality is deferred (scalar `items` only)"
                ));
            }
        }
        Some(_) => {
            return reject(format!("{context}: `uniqueItems` must be a boolean"));
        }
    }

    // `contains` (with `minContains`/`maxContains`).
    let has_contains = schema.extra.contains_key("contains");
    let min_contains = bound("minContains")?;
    let max_contains = bound("maxContains")?;

    if !has_contains && (min_contains.is_some() || max_contains.is_some()) {
        return reject(format!(
            "{context}: `minContains`/`maxContains` require a sibling `contains` matcher (add a `contains` schema or remove them)"
        ));
    }

    if has_contains {
        let contains_value = &schema.extra["contains"];
        // Shapeless matcher (`{}` / `true` / `false`) — no element shape, so no
        // matcher. `{}`/`true` degenerate to "non-empty" (use `minItems: 1`);
        // `false` matches nothing.
        let matcher: Schema = match contains_value {
            Value::Object(_) => {
                serde_json::from_value(contains_value.clone()).map_err(|error| {
                    Error::InvalidJsonSchema {
                        path: path.to_path_buf(),
                        reason: format!("{context}: `contains` is not a valid schema: {error}"),
                    }
                })?
            }
            _ => {
                return reject(format!(
                    "{context}: `contains` must be a schema object with a scalar matcher (a bare `{{}}`/`true`/`false` is not a matcher — use `minItems`)"
                ));
            }
        };

        let matcher_ty = matcher.ty.as_ref().and_then(Value::as_str);
        let matcher_const_kind = matcher.extra.get("const").and_then(scalar_value_kind);
        let matcher_const_is_composite = matcher
            .extra
            .get("const")
            .is_some_and(|value| scalar_value_kind(value).is_none());
        let matcher_enum_kind = matcher
            .extra
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(scalar_value_kind);
        let matcher_enum_is_composite = matcher
            .extra
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .any(|value| scalar_value_kind(value).is_none())
            });

        // Composite matcher — an object/array-typed matcher, a `$ref`, or a
        // composite `const`/`enum` value — is deferred.
        if matches!(matcher_ty, Some("object" | "array"))
            || matcher.reference.is_some()
            || matcher_const_is_composite
            || matcher_enum_is_composite
        {
            return reject(format!(
                "{context}: a composite `contains` matcher is not yet supported; deep matching is deferred (scalar matcher only)"
            ));
        }

        // Composite element type — `contains` over a composite `items` is
        // deferred, exactly as `uniqueItems` defers composite elements.
        if !items_is_scalar {
            return reject(format!(
                "{context}: `contains` over a composite element type is not yet supported; deep matching is deferred (scalar `items` only)"
            ));
        }

        // The matcher must carry at least one recognized scalar assertion — a
        // scalar `type`, a `const`/`enum`, or a scalar constraint.
        let matcher_has_assertion = scalar_type(matcher_ty).is_some()
            || matcher_const_kind.is_some()
            || matcher_enum_kind.is_some()
            || matcher.extra.contains_key("minimum")
            || matcher.extra.contains_key("maximum")
            || matcher.extra.contains_key("exclusiveMinimum")
            || matcher.extra.contains_key("exclusiveMaximum")
            || matcher.extra.contains_key("multipleOf")
            || matcher.extra.contains_key("minLength")
            || matcher.extra.contains_key("maxLength");
        if !matcher_has_assertion {
            return reject(format!(
                "{context}: `contains` must be a schema object with a scalar matcher (a bare `{{}}`/`true`/`false` is not a matcher — use `minItems`)"
            ));
        }

        // The matcher kind must be compatible with the (scalar) element kind, or
        // no element could ever match (statically unsatisfiable).
        let matcher_kind = scalar_type(matcher_ty)
            .or(matcher_const_kind)
            .or(matcher_enum_kind)
            .or_else(|| {
                if matcher.extra.contains_key("minimum")
                    || matcher.extra.contains_key("maximum")
                    || matcher.extra.contains_key("exclusiveMinimum")
                    || matcher.extra.contains_key("exclusiveMaximum")
                    || matcher.extra.contains_key("multipleOf")
                {
                    Some("number")
                } else if matcher.extra.contains_key("minLength")
                    || matcher.extra.contains_key("maxLength")
                {
                    Some("string")
                } else {
                    None
                }
            });
        if let (Some(element), Some(matcher_kind)) = (items_kind, matcher_kind)
            && !scalar_kinds_compatible(element, matcher_kind)
        {
            return reject(format!(
                "{context}: the `contains` matcher type (`{matcher_kind}`) is incompatible with the element type (`{element}`); no element can ever match"
            ));
        }
    }

    // Match-count satisfiability. The `contains` default is `minContains: 1`.
    if let (Some(min), Some(max)) = (min_contains, max_contains)
        && min > max
    {
        return reject(format!(
            "{context}: `minContains` ({min}) exceeds `maxContains` ({max}); the bounds describe an empty range (no match count can satisfy them)"
        ));
    }
    if let Some(max) = max_contains {
        let effective_min = min_contains.unwrap_or(1);
        if effective_min > max {
            return reject(format!(
                "{context}: `maxContains` ({max}) is below the effective `minContains` ({effective_min}); the bounds describe an empty range (set `minContains: 0` to allow zero matches)"
            ));
        }
    }
    // `minContains: 0` alone (no `maxContains`) makes `contains` always pass with
    // no ceiling, so the whole block asserts nothing — reject as vacuous.
    if min_contains == Some(0) && max_contains.is_none() {
        return reject(format!(
            "{context}: `minContains: 0` without a `maxContains` makes `contains` assert nothing; add a `maxContains` or remove the `contains` block"
        ));
    }

    Ok(())
}

/// Load-time validation of the object-constraint keywords (`minProperties`,
/// `maxProperties`, `propertyNames`, `dependentRequired`). See
/// `specs/json-schema/features/{minProperties,maxProperties,propertyNames,dependentRequired}.md`
/// for the authoritative rules. The keywords remain in the schema `extra` map
/// (or the typed fields) for the backends; this only rejects statically
/// unsatisfiable / unsupported (deferred) forms with fix-its.
fn validate_object_constraints(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    const KEYWORDS: [&str; 4] = [
        "minProperties",
        "maxProperties",
        "propertyNames",
        "dependentRequired",
    ];
    if KEYWORDS.iter().all(|key| !schema.extra.contains_key(*key)) {
        return Ok(());
    }

    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    // P7.1: object-constraint keywords are statically meaningless on a
    // non-object type (the string-length analog is `maxLength`, the array-length
    // analog is `maxItems`).
    if schema.ty.as_ref().and_then(Value::as_str) != Some("object") {
        return reject(format!(
            "{context}: object-constraint keywords (`minProperties`/`maxProperties`/`propertyNames`/`dependentRequired`) require `type: object`"
        ));
    }

    let declared: Vec<&String> = schema
        .properties
        .as_ref()
        .map(|properties| properties.keys().collect())
        .unwrap_or_default();
    let has_properties = !declared.is_empty();
    let closed = schema.additional_properties.as_ref() == Some(&Value::Bool(false));
    let is_map = matches!(&schema.additional_properties, Some(value) if value.is_object())
        || schema.additional_properties.as_ref() == Some(&Value::Bool(true));
    let required: Vec<String> = schema
        .required
        .as_ref()
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Each count bound must be a non-negative integer. A `.0`-valued float is
    // accepted as its integer value (honoring the `1.0`-as-integer rule).
    let bound = |key: &str| -> Result<Option<u64>> {
        match schema.extra.get(key) {
            None => Ok(None),
            Some(Value::Number(number)) => match number.as_f64() {
                Some(value) if value.is_finite() && value >= 0.0 && value.fract() == 0.0 => {
                    Ok(Some(value as u64))
                }
                _ => Err(Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!("{context}: `{key}` must be a non-negative integer"),
                }),
            },
            Some(_) => Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `{key}` must be a non-negative integer"),
            }),
        }
    };

    let min_properties = bound("minProperties")?;
    let max_properties = bound("maxProperties")?;

    // `minProperties > maxProperties` is unsatisfiable; equal pins an exact size.
    if let (Some(min), Some(max)) = (min_properties, max_properties)
        && min > max
    {
        return reject(format!(
            "{context}: `minProperties` ({min}) exceeds `maxProperties` ({max}); the bounds describe an empty range (no object can satisfy them)"
        ));
    }
    // A closed object caps the member count at the declared count; a
    // `minProperties` above that is unsatisfiable.
    if let Some(min) = min_properties
        && closed
        && !is_map
        && (declared.len() as u64) < min
    {
        return reject(format!(
            "{context}: `minProperties` ({min}) exceeds the {} declared propert{} of this closed object (no extras are allowed, so it can never be satisfied)",
            declared.len(),
            if declared.len() == 1 { "y" } else { "ies" }
        ));
    }
    // `maxProperties` below the count of required members is unsatisfiable.
    if let Some(max) = max_properties
        && (required.len() as u64) > max
    {
        return reject(format!(
            "{context}: `maxProperties` ({max}) is below the {} required member(s); the object can never satisfy the cap",
            required.len()
        ));
    }

    // `propertyNames` — partial: map-shaped objects only (an object with
    // `additionalProperties` and NO `properties`).
    if let Some(property_names) = schema.extra.get("propertyNames") {
        if has_properties {
            return reject(format!(
                "{context}: `propertyNames` is only supported on a map-shaped object (`additionalProperties` with no `properties`); alongside `properties` it is ambiguous and deferred — encode the key shape on the map form instead"
            ));
        }
        if !is_map {
            return reject(format!(
                "{context}: `propertyNames` requires a map host (`additionalProperties` with a value schema or `true`)"
            ));
        }
        let subschema: Schema = match property_names {
            Value::Object(_) => {
                serde_json::from_value(property_names.clone()).map_err(|error| {
                    Error::InvalidJsonSchema {
                        path: path.to_path_buf(),
                        reason: format!(
                            "{context}: `propertyNames` is not a valid schema: {error}"
                        ),
                    }
                })?
            }
            _ => {
                return reject(format!(
                    "{context}: `propertyNames` must be a string schema constraining the keys (a bare `{{}}`/`true` asserts nothing — property names are always strings)"
                ));
            }
        };
        // Keys are always strings; a non-string subschema can never match.
        if subschema.ty.as_ref().and_then(Value::as_str) != Some("string") {
            return reject(format!(
                "{context}: `propertyNames` must be `type: string` (property names are always strings, so any other type can never match)"
            ));
        }
        // The subschema must carry a supported string assertion. Only the
        // string-length keywords (`minLength`/`maxLength`) lower today; a bare
        // `type: string` asserts nothing (keys are already strings) → reject.
        let has_length =
            subschema.extra.contains_key("minLength") || subschema.extra.contains_key("maxLength");
        if !has_length {
            for unsupported in ["pattern", "enum", "const", "format"] {
                if subschema.extra.contains_key(unsupported) {
                    return reject(format!(
                        "{context}: `propertyNames` with `{unsupported}` is not yet supported; use `minLength`/`maxLength` to constrain key length"
                    ));
                }
            }
            return reject(format!(
                "{context}: `propertyNames` asserts nothing (property names are already strings); add a `minLength`/`maxLength` key-length constraint or drop the keyword"
            ));
        }
        // Reuse the string-length validation for the key subschema.
        validate_string_constraints(path, &subschema, &format!("{context}.propertyNames"))?;
    }

    // `dependentRequired` — map of trigger → dependents that must also be present.
    if let Some(dependent_required) = schema.extra.get("dependentRequired") {
        let Value::Object(map) = dependent_required else {
            return reject(format!(
                "{context}: `dependentRequired` must be an object mapping a property to the properties required alongside it"
            ));
        };
        for (trigger, deps) in map {
            let Value::Array(dep_values) = deps else {
                return reject(format!(
                    "{context}: `dependentRequired.{trigger}` must be an array of property-name strings"
                ));
            };
            let mut seen = BTreeSet::new();
            let mut dep_names = Vec::new();
            for dep in dep_values {
                let Some(dep) = dep.as_str() else {
                    return reject(format!(
                        "{context}: `dependentRequired.{trigger}` must contain only property-name strings"
                    ));
                };
                if !seen.insert(dep.to_string()) {
                    return reject(format!(
                        "{context}: `dependentRequired.{trigger}` lists `{dep}` more than once; entries must be unique"
                    ));
                }
                dep_names.push(dep.to_string());
            }
            // Trigger must be a declared property (presence check on an
            // undeclared member is undecidable, P7.1).
            if !declared.iter().any(|name| name.as_str() == trigger) {
                return reject(format!(
                    "{context}: `dependentRequired` trigger `{trigger}` is not declared in `properties`"
                ));
            }
            // Trigger in `required` → always present, so its dependents are
            // unconditionally required; move them to `required`.
            if required.iter().any(|name| name == trigger) {
                return reject(format!(
                    "{context}: `dependentRequired` trigger `{trigger}` is also in `required`; its dependents are then unconditionally required — move them to `required`"
                ));
            }
            for dep in &dep_names {
                if !declared.iter().any(|name| name.as_str() == dep) {
                    return reject(format!(
                        "{context}: `dependentRequired.{trigger}` dependent `{dep}` is not declared in `properties`"
                    ));
                }
                if required.iter().any(|name| name == dep) {
                    return reject(format!(
                        "{context}: `dependentRequired.{trigger}` dependent `{dep}` is already in `required` (the dependency is vacuous); remove it from `dependentRequired`"
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Load-time validation of the `const` and `enum` keywords (the closed
/// value-set primitives). See `specs/json-schema/features/const.md` and
/// `specs/json-schema/features/enum.md` for the authoritative rules. Both keep their
/// values in the schema `extra` map for the backends; this rejects statically
/// unsatisfiable / unsupported / degenerate forms with fix-its and enforces the
/// P15 synthesized-name (value → identifier) collision rule.
fn validate_const_enum(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    let has_const = schema.extra.contains_key("const");
    let has_enum = schema.extra.contains_key("enum");
    if !has_const && !has_enum {
        return Ok(());
    }

    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    // `const` and `enum` are mutually exclusive (a `const` is a single-value
    // `enum`; pick one spelling).
    if has_const && has_enum {
        return reject(format!(
            "{context}: `const` and `enum` are mutually exclusive (a `const` is a single-value `enum`); use one spelling"
        ));
    }

    let ty = schema.ty.as_ref().and_then(Value::as_str);
    let declared_scalar = scalar_type(ty);

    // Validate one scalar member value against the declared type + identifier
    // encoding. `source` names the keyword for diagnostics.
    let check_member = |value: &Value, source: &str| -> Result<()> {
        // `null` member/value → reject (degenerate; use the nullability pattern).
        if value.is_null() {
            return reject(format!(
                "{context}: `{source}: null` is not supported; a field that is always null carries no information (use the nullability pattern for a nullable field, or omit it)"
            ));
        }
        // Composite (object/array) member/value → deferred.
        let Some(value_kind) = scalar_value_kind(value) else {
            return reject(format!(
                "{context}: a composite (object/array) `{source}` value is not yet supported; deep structural equality is deferred (scalar values only)"
            ));
        };
        // Type compatibility (P7.1): the value must be assignable to the
        // declared type. A non-scalar declared type can never hold a scalar.
        match declared_scalar {
            Some(declared) if !scalar_kinds_compatible(declared, value_kind) => {
                return reject(format!(
                    "{context}: `{source}` value {value} (of kind `{value_kind}`) is incompatible with `type: {}`",
                    ty.unwrap_or("")
                ));
            }
            None if ty.is_some() => {
                return reject(format!(
                    "{context}: `{source}` value {value} (of kind `{value_kind}`) is incompatible with `type: {}`",
                    ty.unwrap_or("")
                ));
            }
            _ => {}
        }
        // String values are restricted to ASCII without whitespace (keeps the
        // identifier front-end to the Stage 1 word-splitter).
        if let Value::String(text) = value {
            if !text.is_ascii() {
                return reject(format!(
                    "{context}: `{source}` string value {text:?} must be ASCII (non-ASCII values are not supported)"
                ));
            }
            if text.chars().any(|c| c.is_whitespace()) {
                return reject(format!(
                    "{context}: `{source}` string value {text:?} must not contain whitespace"
                ));
            }
        }
        // Stage 3: the value must encode to a non-empty legal identifier token
        // (Go defined-type const / Java value-class constant). An empty token
        // (e.g. the string `"-"`) is rejected.
        if encode_value_identifier(value).is_none() {
            return reject(format!(
                "{context}: `{source}` value {value} does not encode to a legal identifier (its token is empty); this value cannot name a Go/Java constant"
            ));
        }
        Ok(())
    };

    if has_const {
        // `const` and `default` are mutually exclusive (redundant or
        // contradictory — a const already fixes the value).
        if schema.extra.contains_key("default") {
            return reject(format!(
                "{context}: `const` and `default` are mutually exclusive; a `const` already fixes the value — drop the `default`"
            ));
        }
        let value = &schema.extra["const"];
        check_member(value, "const")?;
    }

    if has_enum {
        let Some(members) = schema.extra["enum"].as_array() else {
            return reject(format!("{context}: `enum` must be an array of values"));
        };
        // An empty `enum` is statically unsatisfiable.
        if members.is_empty() {
            return reject(format!(
                "{context}: `enum` must not be empty (an empty value set can never be satisfied)"
            ));
        }
        // Every member: scalar, type-compatible, ASCII/no-whitespace, encodable.
        for value in members {
            check_member(value, "enum")?;
        }
        // Duplicate members (wire-distinct but redundant) → reject.
        for (index, value) in members.iter().enumerate() {
            if members[..index].contains(value) {
                return reject(format!(
                    "{context}: `enum` lists {value} more than once; members must be unique"
                ));
            }
        }
        // P15: two members whose identifier encodings collide (wire-distinct but
        // fold to the same Go/Java constant name) → reject.
        let mut seen: BTreeMap<String, Value> = BTreeMap::new();
        for value in members {
            if let Some(token) = encode_value_identifier(value)
                && let Some(previous) = seen.insert(token.clone(), value.clone())
                && &previous != value
            {
                return reject(format!(
                    "{context}: `enum` members {previous} and {value} both encode to the identifier `{token}` (a name collision); they cannot each name a distinct Go/Java constant"
                ));
            }
        }
        // A `default` alongside `enum` must itself be a member of the set.
        if let Some(default) = schema.extra.get("default")
            && !members.contains(default)
        {
            return reject(format!(
                "{context}: the `default` value {default} is not a member of the `enum` set"
            ));
        }
    }

    Ok(())
}

/// Encodes a scalar `const`/`enum` value to its readable identifier token
/// (the shared Go/Java value-constant front-end, in a case-normalized form used
/// for the P15 collision pass). Returns `None` when the value has no legal token
/// (e.g. a string of only separators, such as `"-"`). Numbers keep their
/// decimal point as `_` (so `3_14` stays distinct from `314`) and encode a
/// leading sign as `Neg`; booleans encode as `True`/`False`.
fn encode_value_identifier(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let token = text.to_upper_camel_case();
            if token.is_empty() { None } else { Some(token) }
        }
        Value::Bool(flag) => Some(if *flag { "True" } else { "False" }.to_string()),
        Value::Number(number) => {
            let decimal = number.to_string();
            let token = decimal.replace('-', "Neg").replace('.', "_");
            if token.is_empty() { None } else { Some(token) }
        }
        _ => None,
    }
}

/// Accumulates the named-model targets that an instance of `schema` is *forced*
/// to contain: a required, non-nullable, single-valued (non-collection) `$ref`,
/// descending through required inline objects. Collection-wrapped, optional, or
/// nullable edges terminate the chain and contribute nothing. Ref-resolution
/// errors are ignored here — they surface in `validate_model_refs`.
fn collect_mandatory_targets(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    doc_paths: &BTreeSet<PathBuf>,
    out: &mut Vec<TypeKey>,
) {
    if schema.is_bare_ref() {
        if let Some(reference) = &schema.reference
            && let Ok(key) = resolve_ref_key(path, canonical_path, reference, doc_paths)
        {
            out.push(key);
        }
        return;
    }
    if schema.ty.as_ref().and_then(Value::as_str) != Some("object") {
        return;
    }
    let Some(properties) = &schema.properties else {
        return;
    };
    let required: BTreeSet<&str> = schema
        .required
        .as_ref()
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for (name, property) in properties {
        if !required.contains(name.as_str()) || property.one_of.is_some() {
            // Optional, or a nullable/union `oneOf` edge — the chain can terminate.
            continue;
        }
        if property.is_bare_ref() {
            if let Some(reference) = &property.reference
                && let Ok(key) = resolve_ref_key(path, canonical_path, reference, doc_paths)
            {
                out.push(key);
            }
        } else if property.ty.as_ref().and_then(Value::as_str) == Some("object") {
            collect_mandatory_targets(path, canonical_path, property, doc_paths, out);
        }
        // `array` / `additionalProperties` / scalar members terminate the chain.
    }
}

/// Depth-first search for a cycle in the mandatory-edge graph, returning the
/// cycle path (`A → B → A`) if one exists. `state`: 0 = unvisited, 1 = on the
/// current stack, 2 = fully explored.
fn find_mandatory_cycle(
    node: &TypeKey,
    edges: &BTreeMap<TypeKey, Vec<TypeKey>>,
    state: &mut BTreeMap<TypeKey, u8>,
    stack: &mut Vec<TypeKey>,
) -> Option<Vec<TypeKey>> {
    state.insert(node.clone(), 1);
    stack.push(node.clone());
    if let Some(targets) = edges.get(node) {
        for target in targets {
            match state.get(target).copied().unwrap_or(0) {
                1 => {
                    let start = stack
                        .iter()
                        .position(|entry| entry == target)
                        .expect("a node on the stack is present in the stack");
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(target.clone());
                    return Some(cycle);
                }
                0 => {
                    if let Some(cycle) = find_mandatory_cycle(target, edges, state, stack) {
                        return Some(cycle);
                    }
                }
                _ => {}
            }
        }
    }
    stack.pop();
    state.insert(node.clone(), 2);
    None
}

/// Rejects an unsatisfiable recursion cycle — one whose every edge is
/// mandatory-and-single-valued (required + non-nullable + non-collection), so no
/// finite instance exists. See `specs/json-schema/features/ref.md` (Recursion &
/// satisfiability). Conservative: it only builds edges it can prove mandatory,
/// so any cycle it finds is genuinely unsatisfiable.
fn validate_reference_satisfiability(
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    let doc_paths: BTreeSet<PathBuf> = docs.keys().cloned().collect();
    let mut edges: BTreeMap<TypeKey, Vec<TypeKey>> = BTreeMap::new();
    for (key, model) in models {
        let source_path = docs
            .get(&model.canonical_path)
            .map(|(path, _)| path.clone())
            .unwrap_or_else(|| model.canonical_path.clone());
        let mut targets = Vec::new();
        collect_mandatory_targets(
            &source_path,
            &model.canonical_path,
            &model.schema,
            &doc_paths,
            &mut targets,
        );
        let mut seen = BTreeSet::new();
        targets.retain(|target| models.contains_key(target) && seen.insert(target.clone()));
        edges.insert(key.clone(), targets);
    }

    let mut state: BTreeMap<TypeKey, u8> = models.keys().map(|key| (key.clone(), 0)).collect();
    for key in models.keys() {
        if state.get(key).copied() != Some(0) {
            continue;
        }
        let mut stack = Vec::new();
        if let Some(cycle) = find_mandatory_cycle(key, &edges, &mut state, &mut stack) {
            let display = |type_key: &TypeKey| {
                models
                    .get(type_key)
                    .map(|model| model.full_name.clone())
                    .unwrap_or_else(|| match type_key {
                        TypeKey::Root(path) => root_type_name(path),
                        TypeKey::Def(_, name) => name.clone(),
                    })
            };
            let path = cycle.iter().map(display).collect::<Vec<_>>().join(" → ");
            let report_path = cycle
                .first()
                .and_then(model_key_path)
                .cloned()
                .unwrap_or_else(|| PathBuf::from("<json-schema>"));
            return Err(Error::InvalidJsonSchema {
                path: report_path,
                reason: format!(
                    "unsatisfiable recursion cycle `{path}`: every edge is a required, non-nullable, single-valued `$ref`, so no finite value can satisfy it — break the cycle by making an edge optional, nullable (`oneOf: [{{...}}, {{type: \"null\"}}]`), or wrapping it in an array"
                ),
            });
        }
    }
    Ok(())
}

fn validate_model_refs(
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    for (canonical_path, (path, doc)) in docs {
        if let Some(defs) = &doc.defs {
            for (name, schema) in defs {
                validate_schema_refs(
                    path,
                    canonical_path,
                    schema,
                    &format!("$defs.{name}"),
                    docs,
                    models,
                )?;
            }
        }
        if let Some(services) = &doc.services {
            for (service_name, service) in services {
                for (operation_name, operation) in &service.operations {
                    for (label, schema) in
                        [("input", &operation.input), ("output", &operation.output)]
                    {
                        if let Some(schema) = schema {
                            validate_schema_refs(
                                path,
                                canonical_path,
                                schema,
                                &format!(
                                    "services.{service_name}.operations.{operation_name}.{label}"
                                ),
                                docs,
                                models,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_schema_refs(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    context: &str,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    validate_schema_common(path, schema, context)?;
    if let Some(reference) = &schema.reference {
        let _ = resolve_ref(path, canonical_path, reference, docs, models)?;
        return Ok(());
    }
    if let Some(properties) = &schema.properties {
        for (name, property) in properties {
            validate_schema_refs(
                path,
                canonical_path,
                property,
                &format!("{context}.properties.{name}"),
                docs,
                models,
            )?;
        }
    }
    if let Some(items) = &schema.items {
        validate_schema_refs(
            path,
            canonical_path,
            items,
            &format!("{context}.items"),
            docs,
            models,
        )?;
    }
    if let Some(one_of) = &schema.one_of {
        for branch in one_of {
            validate_schema_refs(
                path,
                canonical_path,
                branch,
                &format!("{context}.oneOf"),
                docs,
                models,
            )?;
        }
    }
    Ok(())
}

/// The JSON kind that acts as the outer selector for a `oneOf` branch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BranchKind {
    Null,
    Boolean,
    String,
    Integer,
    Number,
    Array,
    Object,
}

impl BranchKind {
    fn label(self) -> &'static str {
        match self {
            BranchKind::Null => "null",
            BranchKind::Boolean => "boolean",
            BranchKind::String => "string",
            BranchKind::Integer => "integer",
            BranchKind::Number => "number",
            BranchKind::Array => "array",
            BranchKind::Object => "object",
        }
    }
}

/// Walks every model (root + `$defs`) and operation input/output, classifying
/// each `oneOf` node and rejecting the forms with no decidable selector. Runs
/// after `validate_model_refs` so `$ref` branches resolve to their target kind.
fn validate_all_unions(
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    for (canonical_path, (path, doc)) in docs {
        if let Some(defs) = &doc.defs {
            for (name, schema) in defs {
                validate_schema_unions(
                    path,
                    canonical_path,
                    schema,
                    &format!("$defs.{name}"),
                    docs,
                    models,
                )?;
            }
        }
        if root_is_schema_shaped(&doc.root) && !doc.root.is_bare_ref() {
            validate_schema_unions(path, canonical_path, &doc.root, "root schema", docs, models)?;
        }
        if let Some(services) = &doc.services {
            for (service_name, service) in services {
                for (operation_name, operation) in &service.operations {
                    for (label, schema) in
                        [("input", &operation.input), ("output", &operation.output)]
                    {
                        if let Some(schema) = schema {
                            validate_schema_unions(
                                path,
                                canonical_path,
                                schema,
                                &format!(
                                    "services.{service_name}.operations.{operation_name}.{label}"
                                ),
                                docs,
                                models,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Recursively validates every `oneOf` node in a schema tree as a sum type.
fn validate_schema_unions(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    context: &str,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    if let Some(one_of) = &schema.one_of {
        validate_one_of(path, canonical_path, one_of, context, docs, models)?;
        for branch in one_of {
            validate_schema_unions(
                path,
                canonical_path,
                branch,
                &format!("{context}.oneOf"),
                docs,
                models,
            )?;
        }
    }
    if let Some(properties) = &schema.properties {
        for (name, property) in properties {
            validate_schema_unions(
                path,
                canonical_path,
                property,
                &format!("{context}.properties.{name}"),
                docs,
                models,
            )?;
        }
    }
    if let Some(items) = &schema.items {
        validate_schema_unions(
            path,
            canonical_path,
            items,
            &format!("{context}.items"),
            docs,
            models,
        )?;
    }
    if let Some(additional) = &schema.additional_properties
        && additional.is_object()
        && let Ok(additional_schema) = serde_json::from_value::<Schema>(additional.clone())
    {
        validate_schema_unions(
            path,
            canonical_path,
            &additional_schema,
            &format!("{context}.additionalProperties"),
            docs,
            models,
        )?;
    }
    Ok(())
}

/// The effective schema a `oneOf` branch selects on: a `$ref` branch resolves to
/// its target model schema, any other branch is itself.
fn resolve_branch_schema(
    branch: &Schema,
    path: &Path,
    canonical_path: &Path,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<Schema> {
    if let Some(reference) = &branch.reference {
        let target = resolve_ref(path, canonical_path, reference, docs, models)?;
        Ok(target.schema.clone())
    } else {
        Ok(branch.clone())
    }
}

/// Classifies a single `oneOf` branch into its JSON kind, rejecting a branch
/// with no classifiable kind (typeless / boolean schema / nested combinator).
fn one_of_branch_kind(
    branch: &Schema,
    resolved: &Schema,
    path: &Path,
    context: &str,
) -> Result<BranchKind> {
    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };
    if resolved.one_of.is_some() {
        return reject(format!(
            "{context}: a `oneOf` branch cannot itself be a `oneOf` union (a branch must declare a single recognized `type`)"
        ));
    }
    match resolved.ty.as_ref() {
        Some(Value::String(ty)) => match ty.as_str() {
            "null" => Ok(BranchKind::Null),
            "boolean" => Ok(BranchKind::Boolean),
            "string" => Ok(BranchKind::String),
            "integer" => Ok(BranchKind::Integer),
            "number" => Ok(BranchKind::Number),
            "array" => Ok(BranchKind::Array),
            "object" => Ok(BranchKind::Object),
            other => reject(format!(
                "{context}: a `oneOf` branch has unrecognized `type: {other}`"
            )),
        },
        Some(_) => reject(format!(
            "{context}: a `oneOf` branch must declare a single string `type` (an array `type` has no single selector kind)"
        )),
        None => {
            let hint = if branch.reference.is_some() {
                " (its `$ref` target declares no single `type`)"
            } else {
                ""
            };
            reject(format!(
                "{context}: a `oneOf` branch has no classifiable kind{hint}; every branch must declare a single recognized `type` (or `$ref` a typed definition)"
            ))
        }
    }
}

/// The scalar `const` value of a property (a bare `const`, or a single-member
/// `enum`), used as a discriminator tag value. `None` when the property carries
/// no single fixed scalar value.
fn discriminator_const(property: &Schema) -> Option<Value> {
    if let Some(value) = property.extra.get("const") {
        return scalar_value_kind(value).map(|_| value.clone());
    }
    if let Some(Value::Array(members)) = property.extra.get("enum")
        && members.len() == 1
        && scalar_value_kind(&members[0]).is_some()
    {
        return Some(members[0].clone());
    }
    None
}

/// The set of property names that qualify as a discriminator tag for an object
/// branch: present in the branch's `required` array and carrying a scalar
/// `const`. Maps each qualifying name to its `const` value.
fn branch_discriminator_tags(object: &Schema) -> BTreeMap<String, Value> {
    let required: BTreeSet<String> = object
        .required
        .as_ref()
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut tags = BTreeMap::new();
    if let Some(properties) = &object.properties {
        for (name, property) in properties {
            if required.contains(name)
                && let Some(value) = discriminator_const(property)
            {
                tags.insert(name.clone(), value);
            }
        }
    }
    tags
}

/// Validates a `oneOf` as a supported closed sum type (or the degenerate
/// nullability pattern, which [[nullability]] owns). See
/// `specs/json-schema/features/oneOf.md` for the full acceptance rules.
fn validate_one_of(
    path: &Path,
    canonical_path: &Path,
    branches: &[Schema],
    context: &str,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    let reject = |reason: String| {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason,
        })
    };

    if branches.is_empty() {
        return reject(format!(
            "{context}: `oneOf` must be a non-empty array of branches"
        ));
    }
    if branches.len() == 1 {
        return reject(format!(
            "{context}: a single-branch `oneOf` is a pointless wrapper; use the branch directly"
        ));
    }

    // Classify every branch by kind, resolving `$ref` branches to their target.
    let mut kinds: Vec<BranchKind> = Vec::with_capacity(branches.len());
    let mut object_schemas: Vec<Schema> = Vec::new();
    for branch in branches {
        let resolved = resolve_branch_schema(branch, path, canonical_path, docs, models)?;
        let kind = one_of_branch_kind(branch, &resolved, path, context)?;
        if kind == BranchKind::Object {
            object_schemas.push(resolved);
        }
        kinds.push(kind);
    }

    // At most one branch per non-object kind (a same-kind scalar choice is an
    // `enum`, not a `oneOf`; duplicate `null` is a tautology).
    for kind in [
        BranchKind::Null,
        BranchKind::Boolean,
        BranchKind::String,
        BranchKind::Integer,
        BranchKind::Number,
        BranchKind::Array,
    ] {
        let count = kinds.iter().filter(|value| **value == kind).count();
        if count > 1 {
            if matches!(
                kind,
                BranchKind::String | BranchKind::Integer | BranchKind::Number
            ) {
                return reject(format!(
                    "{context}: two `oneOf` branches share the `{}` kind; a same-kind scalar choice is an `enum` (or `const` union), not a `oneOf`",
                    kind.label()
                ));
            }
            return reject(format!(
                "{context}: two `oneOf` branches share the `{}` kind, which has no decidable selector",
                kind.label()
            ));
        }
    }

    // `integer` + `number` overlap: any integer satisfies both, so exactly-one
    // is unsatisfiable (no discriminator can fix a numeric-token overlap).
    if kinds.contains(&BranchKind::Integer) && kinds.contains(&BranchKind::Number) {
        return reject(format!(
            "{context}: a `oneOf` cannot mix `integer` and `number` branches (both are the JSON number token and every integer is a number, so exactly-one is unsatisfiable)"
        ));
    }

    // Two or more object branches require a shared required-`const` discriminator.
    if object_schemas.len() >= 2 {
        let mut shared: Option<BTreeMap<String, Value>> = None;
        for object in &object_schemas {
            let tags = branch_discriminator_tags(object);
            shared = Some(match shared {
                None => tags,
                Some(existing) => existing
                    .into_iter()
                    .filter(|(name, _)| tags.contains_key(name))
                    .collect(),
            });
        }
        let shared = shared.unwrap_or_default();
        // Keep only names whose `const` values are pairwise-distinct across all
        // object branches.
        let mut qualifying: Vec<&String> = Vec::new();
        for name in shared.keys() {
            let values: Vec<Value> = object_schemas
                .iter()
                .filter_map(|object| branch_discriminator_tags(object).get(name).cloned())
                .collect();
            let distinct = values
                .iter()
                .enumerate()
                .all(|(index, value)| !values[..index].iter().any(|existing| existing == value));
            if distinct {
                qualifying.push(name);
            }
        }
        match qualifying.len() {
            0 => {
                return reject(format!(
                    "{context}: two or more object `oneOf` branches share no required `const` discriminator property with pairwise-distinct values; add a shared required `const`-tagged property (e.g. `kind`) to each branch"
                ));
            }
            1 => {}
            _ => {
                let names = qualifying
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return reject(format!(
                    "{context}: the object `oneOf` branches have more than one qualifying `const` discriminator ({names}); the intended tag is ambiguous"
                ));
            }
        }
    }

    // Presence/nullable bookkeeping: a lone non-null branch paired with `null`
    // is the degenerate nullability pattern ([[nullability]] owns it); a lone
    // non-null branch with no `null` is a single-branch wrapper (already
    // rejected above). Two or more non-null branches form the sum type.
    Ok(())
}

/// Whether a service/operation key matches its identifier regex (see
/// `specs/json-schema/services.md`): `^[A-Z][a-zA-Z\d]+$` for services (`first_upper`)
/// and `^[a-z][a-zA-Z\d]+$` for operations — a leading letter of the required
/// case followed by one or more ASCII alphanumerics.
fn name_matches(name: &str, first_upper: bool) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let first_ok = if first_upper {
        first.is_ascii_uppercase()
    } else {
        first.is_ascii_lowercase()
    };
    let rest: Vec<char> = chars.collect();
    first_ok && !rest.is_empty() && rest.iter().all(char::is_ascii_alphanumeric)
}

fn build_service(
    path: &Path,
    canonical_path: &Path,
    service_key: &str,
    service: &Service,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<ServiceSpec> {
    if !name_matches(service_key, true) {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "service name `{service_key}` must match `^[A-Z][a-zA-Z\\d]+$` (start uppercase, then letters/digits); set the wire name via `fqn` if it must differ"
            ),
        });
    }
    if service.operations.is_empty() {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("service `{service_key}` must declare at least one operation"),
        });
    }
    let service_name = service_key.to_upper_camel_case();
    let operations = service
        .operations
        .iter()
        .map(|(operation_key, operation)| {
            build_operation(
                path,
                canonical_path,
                &service_name,
                operation_key,
                operation,
                docs,
                models,
                module_paths,
                external_types,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ServiceSpec {
        name: service_name.clone(),
        wire_name: service.fqn.clone().unwrap_or(service_name),
        doc: language_string(service.description.clone()),
        namespace: LanguageStringSpec::default(),
        operations_class: LanguageStringSpec::default(),
        endpoint: service.endpoint.clone(),
        experimental: false,
        delay_load_temporalio_workflow: false,
        operations,
        resources: Vec::new(),
        data: (),
    })
}

fn build_operation(
    path: &Path,
    canonical_path: &Path,
    service_name: &str,
    operation_key: &str,
    operation: &Operation,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<OperationSpec> {
    if !name_matches(operation_key, false) {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!(
                "operation name `{operation_key}` must match `^[a-z][a-zA-Z\\d]+$` (start lowercase, then letters/digits); set the wire name via `fqn` if it must differ"
            ),
        });
    }
    let operation_name = operation_key.to_upper_camel_case();
    let input = operation
        .input
        .as_ref()
        .map(|schema| {
            operation_model_type(
                path,
                canonical_path,
                service_name,
                operation_key,
                "Input",
                schema,
                docs,
                models,
                module_paths,
                external_types,
            )
        })
        .transpose()?;
    let output = operation
        .output
        .as_ref()
        .map(|schema| {
            operation_model_type(
                path,
                canonical_path,
                service_name,
                operation_key,
                "Output",
                schema,
                docs,
                models,
                module_paths,
                external_types,
            )
        })
        .transpose()?;

    Ok(OperationSpec {
        name: operation_name.clone(),
        wire_name: operation.fqn.clone().unwrap_or(operation_name),
        experimental: false,
        doc: language_string(operation.description.clone()),
        return_doc: LanguageStringSpec::default(),
        input,
        output,
        output_transform: None,
        data: (),
    })
}

fn operation_model_type(
    path: &Path,
    canonical_path: &Path,
    service_name: &str,
    operation_key: &str,
    suffix: &str,
    schema: &Schema,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<TypeSpec> {
    validate_schema_common(path, schema, &format!("operation {operation_key} {suffix}"))?;
    if let Some(reference) = &schema.reference {
        let model = resolve_ref(path, canonical_path, reference, docs, models)?;
        require_object_io(
            path,
            canonical_path,
            &model.schema,
            operation_key,
            suffix,
            docs,
            models,
        )?;
        insert_json_external_type(external_types, model, docs, models, module_paths)?;
        collect_schema_model_refs(
            &model.canonical_path,
            &model.canonical_path,
            &model.schema,
            docs,
            models,
            module_paths,
            external_types,
        )?;
        return json_model_type(model, docs, models, module_paths);
    }

    validate_model_schema(path, schema, &format!("operation {operation_key} {suffix}"))?;
    // Inline I/O must be an object (see `specs/json-schema/services.md`). After
    // `validate_model_schema` a non-`$ref` inline schema is either `type: object`
    // or a `oneOf` union; a union is not a valid operation input/output.
    require_object_io(
        path,
        canonical_path,
        schema,
        operation_key,
        suffix,
        docs,
        models,
    )?;
    let model_name = format!("{}{}", operation_key.to_upper_camel_case(), suffix);
    let model = JsonModel {
        full_name: format!("{service_name}.{model_name}"),
        canonical_path: canonical_path.to_path_buf(),
        model_name,
        schema: schema.clone(),
    };
    insert_json_external_type(external_types, &model, docs, models, module_paths)?;
    collect_schema_model_refs(
        canonical_path,
        canonical_path,
        &model.schema,
        docs,
        models,
        module_paths,
        external_types,
    )?;
    json_model_type(&model, docs, models, module_paths)
}

/// Requires an operation `input`/`output` to resolve to an object type: an
/// inline `type: object`, a `$ref` to one, or an `allOf` that merged to one
/// (merges run before this). Following bare-`$ref` chains, a target that lands
/// on a `oneOf` union or a scalar/array is a load reject — a union has no single
/// extensible shape. See `specs/json-schema/services.md`.
fn require_object_io(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    operation_key: &str,
    suffix: &str,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
) -> Result<()> {
    let mut current = schema.clone();
    let mut current_canonical = canonical_path.to_path_buf();
    let mut guard = 0usize;
    loop {
        if current.ty.as_ref().and_then(Value::as_str) == Some("object") {
            return Ok(());
        }
        if current.is_bare_ref() {
            let reference = current
                .reference
                .clone()
                .expect("a bare `$ref` carries a reference");
            let model = resolve_ref(path, &current_canonical, &reference, docs, models)?;
            current_canonical = model.canonical_path.clone();
            current = model.schema.clone();
            guard += 1;
            if guard > models.len() + 1 {
                break;
            }
            continue;
        }
        break;
    }
    Err(Error::InvalidJsonSchema {
        path: path.to_path_buf(),
        reason: format!(
            "operation `{operation_key}` {suffix} must resolve to an object; a `oneOf` union or a scalar/array type is not a valid operation input/output — reference an object type, or wrap the value in a single-field object"
        ),
    })
}

/// Builds an `InvalidJsonSchema` error for the merge/normalization pass.
fn merge_reject(path: &Path, reason: String) -> Error {
    Error::InvalidJsonSchema {
        path: path.to_path_buf(),
        reason,
    }
}

/// Shared context threaded through the `allOf` merge: the set of input document
/// paths (for `$ref` target-file resolution) and a snapshot of the raw
/// (pre-merge) schemas keyed by [`TypeKey`] (for folding a `$ref` branch's
/// target into the merged result).
struct MergeCtx<'a> {
    doc_paths: &'a BTreeSet<PathBuf>,
    raw_models: &'a BTreeMap<TypeKey, Schema>,
}

/// Snapshots every named model schema (each `$defs` entry and each schema-shaped
/// document root) as a raw, pre-merge [`Schema`], keyed by [`TypeKey`]. This is
/// the map the `allOf`/`$ref`-sibling fold resolves a branch `$ref` against so it
/// can inline (flatten) the target's schema.
fn collect_raw_models(docs: &IndexMap<PathBuf, (PathBuf, Document)>) -> BTreeMap<TypeKey, Schema> {
    let mut raw = BTreeMap::new();
    for (canonical_path, (_path, doc)) in docs {
        if let Some(defs) = &doc.defs {
            for (name, schema) in defs {
                raw.insert(
                    TypeKey::Def(canonical_path.clone(), name.clone()),
                    schema.clone(),
                );
            }
        }
        if root_is_schema_shaped(&doc.root) && !doc.root.is_bare_ref() {
            raw.insert(TypeKey::Root(canonical_path.clone()), doc.root.clone());
        }
    }
    raw
}

/// The canonical file path a [`TypeKey`] lives in.
fn type_key_path(key: &TypeKey) -> &PathBuf {
    match key {
        TypeKey::Root(path) | TypeKey::Def(path, _) => path,
    }
}

/// Normalizes every schema in a parsed document in place: each `$defs` entry, the
/// schema-shaped root, and each service operation's input/output. Normalization
/// merges/flattens any `allOf` (and rewrites `$ref`-with-siblings to the same
/// merge) into a single materialized schema.
fn normalize_document(
    path: &Path,
    canonical_path: &Path,
    doc: &mut Document,
    ctx: &MergeCtx,
) -> Result<()> {
    if let Some(defs) = &mut doc.defs {
        for (name, schema) in defs.iter_mut() {
            let mut cycle = Vec::new();
            *schema = normalize_schema(
                path,
                canonical_path,
                schema,
                ctx,
                &mut cycle,
                &format!("$defs.{name}"),
            )?;
        }
    }
    if root_is_schema_shaped(&doc.root) && !doc.root.is_bare_ref() {
        let mut cycle = Vec::new();
        doc.root = normalize_schema(
            path,
            canonical_path,
            &doc.root,
            ctx,
            &mut cycle,
            "root schema",
        )?;
    }
    if let Some(services) = &mut doc.services {
        for (service_name, service) in services.iter_mut() {
            for (operation_name, operation) in service.operations.iter_mut() {
                if let Some(input) = &mut operation.input {
                    let mut cycle = Vec::new();
                    *input = normalize_schema(
                        path,
                        canonical_path,
                        input,
                        ctx,
                        &mut cycle,
                        &format!("services.{service_name}.operations.{operation_name}.input"),
                    )?;
                }
                if let Some(output) = &mut operation.output {
                    let mut cycle = Vec::new();
                    *output = normalize_schema(
                        path,
                        canonical_path,
                        output,
                        ctx,
                        &mut cycle,
                        &format!("services.{service_name}.operations.{operation_name}.output"),
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Normalizes a single schema. A node carrying `allOf` (or `$ref` with sibling
/// keywords — the implicit-`allOf` sugar) is flattened into its conjunct branches
/// and merged into one materialized schema; any other node is left as-is. In both
/// cases the schema's children (`properties`, `items`, `oneOf` branches, and a
/// schema-valued `additionalProperties`) are normalized recursively so nested
/// `allOf` deeper in the tree is merged too.
fn normalize_schema(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    ctx: &MergeCtx,
    cycle: &mut Vec<TypeKey>,
    context: &str,
) -> Result<Schema> {
    let has_all_of = schema.extra.contains_key("allOf");
    let ref_with_siblings = schema.reference.is_some() && !schema.is_bare_ref();

    if has_all_of || ref_with_siblings {
        if has_all_of {
            let Some(entries) = schema.extra["allOf"].as_array() else {
                return Err(merge_reject(
                    path,
                    format!("{context}: `allOf` must be a non-empty array of schemas"),
                ));
            };
            if entries.is_empty() {
                return Err(merge_reject(
                    path,
                    format!(
                        "{context}: `allOf` must not be empty (an empty `allOf` asserts nothing; remove it)"
                    ),
                ));
            }
            if entries.len() == 1 && !ref_with_siblings && own_conjunct(schema) == Schema::default()
            {
                return Err(merge_reject(
                    path,
                    format!(
                        "{context}: a single-branch `allOf` is a pointless wrapper; inline the branch directly"
                    ),
                ));
            }
        }
        let branches = expand_branches(path, canonical_path, schema, ctx, cycle, context)?;
        if branches.is_empty() {
            return Err(merge_reject(
                path,
                format!("{context}: `allOf` merges to an empty schema (it asserts nothing)"),
            ));
        }
        let merged = merge_branch_list(path, branches, context)?;
        return normalize_children(path, canonical_path, merged, ctx, cycle, context);
    }

    normalize_children(path, canonical_path, schema.clone(), ctx, cycle, context)
}

/// Recursively normalizes a schema's child schemas (leaving its own keywords
/// untouched).
fn normalize_children(
    path: &Path,
    canonical_path: &Path,
    mut schema: Schema,
    ctx: &MergeCtx,
    cycle: &mut Vec<TypeKey>,
    context: &str,
) -> Result<Schema> {
    if let Some(properties) = schema.properties.take() {
        let mut normalized = IndexMap::new();
        for (name, property) in properties {
            let normalized_property = normalize_schema(
                path,
                canonical_path,
                &property,
                ctx,
                cycle,
                &format!("{context}.properties.{name}"),
            )?;
            normalized.insert(name, normalized_property);
        }
        schema.properties = Some(normalized);
    }
    if let Some(items) = schema.items.take() {
        schema.items = Some(Box::new(normalize_schema(
            path,
            canonical_path,
            &items,
            ctx,
            cycle,
            &format!("{context}.items"),
        )?));
    }
    if let Some(one_of) = schema.one_of.take() {
        let mut normalized = Vec::new();
        for branch in one_of {
            normalized.push(normalize_schema(
                path,
                canonical_path,
                &branch,
                ctx,
                cycle,
                &format!("{context}.oneOf"),
            )?);
        }
        schema.one_of = Some(normalized);
    }
    if let Some(additional) = &schema.additional_properties
        && additional.is_object()
    {
        let additional_schema: Schema =
            serde_json::from_value(additional.clone()).map_err(|error| {
                merge_reject(
                    path,
                    format!("{context}.additionalProperties is invalid: {error}"),
                )
            })?;
        let normalized = normalize_schema(
            path,
            canonical_path,
            &additional_schema,
            ctx,
            cycle,
            &format!("{context}.additionalProperties"),
        )?;
        schema.additional_properties =
            Some(serde_json::to_value(&normalized).map_err(|error| {
                merge_reject(
                    path,
                    format!("{context}: failed to preserve additionalProperties: {error}"),
                )
            })?);
    }
    normalize_pattern(path, &mut schema, context)?;
    Ok(schema)
}

/// Load-time gate + normalization for the `pattern` keyword, applied to the
/// node's own `pattern` during the normalize pass so the value flows to the
/// backends already normalized (`\s`/`\S` expanded to the explicit ASCII class;
/// `$` kept canonical for the per-target backend rewrite). See
/// `specs/json-schema/features/pattern.md`.
///
/// Rejects (P7 / P7.1): a non-string `pattern` value, a `pattern` on a
/// non-`string` node, a non-portable regex (backtracking / inline flags /
/// open-complement `\S`-in-class), and a `const`/`default`/`enum` string literal
/// on the same node that the pattern does not match.
fn normalize_pattern(path: &Path, schema: &mut Schema, context: &str) -> Result<()> {
    let Some(value) = schema.extra.get("pattern") else {
        return Ok(());
    };
    let Some(pattern) = value.as_str() else {
        return Err(merge_reject(
            path,
            format!("{context}: `pattern` must be a string"),
        ));
    };
    let pattern = pattern.to_string();

    // P7.1: `pattern` is a string assertion — meaningless on a non-string node.
    if schema.ty.as_ref().and_then(Value::as_str) != Some("string") {
        return Err(merge_reject(
            path,
            format!("{context}: `pattern` requires `type: string`"),
        ));
    }

    let normalized = crate::json_schema::pattern::gate_and_normalize(&pattern)
        .map_err(|error| merge_reject(path, format!("{context}: {}", error.0)))?;

    // A supplied string literal on the same node must match the pattern at load
    // (the regex half of the deferred literal-vs-constraint obligation). Matched
    // unanchored, as at runtime.
    let matcher = regex::Regex::new(&normalized).map_err(|error| {
        merge_reject(
            path,
            format!("{context}: `pattern` failed to compile after normalization: {error}"),
        )
    })?;
    let check = |literal: &str, source: &str| -> Result<()> {
        if !matcher.is_match(literal) {
            return Err(merge_reject(
                path,
                format!(
                    "{context}: `{source}` value {literal:?} does not match `pattern` {normalized:?}"
                ),
            ));
        }
        Ok(())
    };
    for literal_key in ["const", "default"] {
        if let Some(Value::String(literal)) = schema.extra.get(literal_key) {
            check(literal, literal_key)?;
        }
    }
    if let Some(Value::Array(values)) = schema.extra.get("enum") {
        for value in values {
            if let Some(literal) = value.as_str() {
                check(literal, "enum")?;
            }
        }
    }

    schema
        .extra
        .insert("pattern".to_string(), Value::String(normalized));
    Ok(())
}

/// A schema's "own" conjunct: a clone with `$ref` and `allOf` stripped, i.e. the
/// keywords declared directly on the node (which fold in as the final,
/// last-wins branch).
fn own_conjunct(schema: &Schema) -> Schema {
    let mut own = schema.clone();
    own.reference = None;
    own.extra.shift_remove("allOf");
    own
}

/// Rejects a schema used as an `allOf` conjunct that is itself a boolean-logic
/// combinator (`oneOf`/`anyOf`/`not`/`if`) — an intersection with a union,
/// negation, or runtime fork does not collapse to a single type.
fn reject_combinator_branch(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    if schema.one_of.is_some() {
        return Err(merge_reject(
            path,
            format!(
                "{context}: an `allOf` branch cannot be a `oneOf` (an intersection with a union does not collapse to a single type)"
            ),
        ));
    }
    for keyword in ["anyOf", "not", "if"] {
        if schema.extra.contains_key(keyword) {
            return Err(merge_reject(
                path,
                format!(
                    "{context}: an `allOf` branch cannot be `{keyword}` (this combinator does not collapse to a single type)"
                ),
            ));
        }
    }
    Ok(())
}

/// Flattens a schema into the ordered list of leaf conjunct schemas that must all
/// hold: a `$ref` branch is resolved and its target folded in (recursively
/// flattened, with cycle detection), nested `allOf` is inlined, `true`/`{}`
/// identity branches are dropped, and the node's own keywords fold in as the
/// final branch (so a use-site annotation wins under last-wins).
fn expand_branches(
    path: &Path,
    canonical_path: &Path,
    schema: &Schema,
    ctx: &MergeCtx,
    cycle: &mut Vec<TypeKey>,
    context: &str,
) -> Result<Vec<Schema>> {
    reject_combinator_branch(path, schema, context)?;

    let mut branches = Vec::new();

    if let Some(reference) = &schema.reference {
        let key = resolve_ref_key(path, canonical_path, reference, ctx.doc_paths)?;
        if cycle.contains(&key) {
            return Err(merge_reject(
                path,
                format!(
                    "{context}: `$ref` `{reference}` forms an `allOf` merge cycle (a type cannot be flattened into itself)"
                ),
            ));
        }
        let target = ctx
            .raw_models
            .get(&key)
            .ok_or_else(|| {
                merge_reject(
                    path,
                    format!(
                        "{context}: `$ref` `{reference}` does not resolve to a known JSON model"
                    ),
                )
            })?
            .clone();
        let target_path = type_key_path(&key).clone();
        cycle.push(key);
        let sub = expand_branches(&target_path, &target_path, &target, ctx, cycle, context)?;
        cycle.pop();
        branches.extend(sub);
    }

    if let Some(all_of) = schema.extra.get("allOf") {
        let Some(entries) = all_of.as_array() else {
            return Err(merge_reject(
                path,
                format!("{context}: `allOf` must be an array of schemas"),
            ));
        };
        for (index, entry) in entries.iter().enumerate() {
            match entry {
                Value::Bool(true) => continue,
                Value::Bool(false) => {
                    return Err(merge_reject(
                        path,
                        format!(
                            "{context}: `allOf[{index}]` is `false` (nothing can validate); remove the branch or the whole `allOf`"
                        ),
                    ));
                }
                Value::Object(_) => {
                    let entry_schema: Schema =
                        serde_json::from_value(entry.clone()).map_err(|error| {
                            merge_reject(
                                path,
                                format!(
                                    "{context}: `allOf[{index}]` is not a valid schema: {error}"
                                ),
                            )
                        })?;
                    if entry_schema == Schema::default() {
                        continue;
                    }
                    let sub =
                        expand_branches(path, canonical_path, &entry_schema, ctx, cycle, context)?;
                    branches.extend(sub);
                }
                _ => {
                    return Err(merge_reject(
                        path,
                        format!("{context}: `allOf[{index}]` must be a schema object"),
                    ));
                }
            }
        }
    }

    let own = own_conjunct(schema);
    if own != Schema::default() {
        branches.push(own);
    }

    Ok(branches)
}

/// Folds an ordered list of conjunct branches into one merged schema.
fn merge_branch_list(path: &Path, branches: Vec<Schema>, context: &str) -> Result<Schema> {
    let mut iter = branches.into_iter();
    let mut acc = iter.next().expect("branch list is non-empty");
    for branch in iter {
        acc = merge_schema_pair(path, acc, &branch, context)?;
    }
    finalize_merged(path, &mut acc, context)?;
    Ok(acc)
}

/// Merges two schemas that both constrain the same value, then finalizes the
/// result (collapses cross-keyword numeric-bound pairs, resolves `const`+`enum`).
fn merge_schema_pair(path: &Path, acc: Schema, branch: &Schema, context: &str) -> Result<Schema> {
    let mut merged = merge_two(path, acc, branch, context)?;
    finalize_merged(path, &mut merged, context)?;
    Ok(merged)
}

/// The core pairwise merge of two conjunct schemas.
fn merge_two(path: &Path, mut acc: Schema, branch: &Schema, context: &str) -> Result<Schema> {
    // `$ref` branches are already flattened away; the merged node is standalone.
    acc.reference = None;
    acc.ty = merge_type(path, acc.ty.take(), branch.ty.clone(), context)?;
    // Metadata annotations are last-wins.
    if branch.title.is_some() {
        acc.title = branch.title.clone();
    }
    if branch.description.is_some() {
        acc.description = branch.description.clone();
    }
    acc.properties = merge_properties(
        path,
        acc.properties.take(),
        branch.properties.clone(),
        context,
    )?;
    acc.required = merge_required(acc.required.take(), branch.required.clone());
    acc.additional_properties = merge_additional_properties(
        path,
        acc.additional_properties.take(),
        branch.additional_properties.clone(),
        context,
    )?;
    acc.items = merge_items(path, acc.items.take(), branch.items.clone(), context)?;
    for (key, branch_value) in &branch.extra {
        match acc.extra.get(key) {
            None => {
                acc.extra.insert(key.clone(), branch_value.clone());
            }
            Some(acc_value) => {
                let merged = merge_extra_value(path, key, acc_value, branch_value, context)?;
                acc.extra.insert(key.clone(), merged);
            }
        }
    }
    Ok(acc)
}

/// Merges two `type` values: identical dedupes, `integer`/`number` collapses to
/// `integer`, and any other differing pair is a disjoint-type reject.
fn merge_type(
    path: &Path,
    acc: Option<Value>,
    branch: Option<Value>,
    context: &str,
) -> Result<Option<Value>> {
    match (acc, branch) {
        (None, other) | (other, None) => Ok(other),
        (Some(a), Some(b)) => {
            if a == b {
                return Ok(Some(a));
            }
            if let (Some(sa), Some(sb)) = (a.as_str(), b.as_str()) {
                if matches!((sa, sb), ("integer", "number") | ("number", "integer")) {
                    return Ok(Some(Value::String("integer".to_string())));
                }
                return Err(merge_reject(
                    path,
                    format!(
                        "{context}: `allOf` branches declare disjoint types `{sa}` and `{sb}` (no value can be both)"
                    ),
                ));
            }
            Err(merge_reject(
                path,
                format!("{context}: `allOf` branches declare incompatible `type` values"),
            ))
        }
    }
}

/// Merges two `properties` maps: the union of names, with a name present in both
/// branches merged recursively.
fn merge_properties(
    path: &Path,
    acc: Option<IndexMap<String, Schema>>,
    branch: Option<IndexMap<String, Schema>>,
    context: &str,
) -> Result<Option<IndexMap<String, Schema>>> {
    match (acc, branch) {
        (None, other) | (other, None) => Ok(other),
        (Some(mut acc), Some(branch)) => {
            for (name, branch_schema) in branch {
                if let Some(existing) = acc.get(&name).cloned() {
                    let merged = merge_schema_pair(
                        path,
                        existing,
                        &branch_schema,
                        &format!("{context}.properties.{name}"),
                    )?;
                    acc.insert(name, merged);
                } else {
                    acc.insert(name, branch_schema);
                }
            }
            Ok(Some(acc))
        }
    }
}

/// Merges two `required` arrays into their union, preserving first-seen order.
fn merge_required(acc: Option<Value>, branch: Option<Value>) -> Option<Value> {
    let mut names: Vec<Value> = Vec::new();
    for source in [acc, branch] {
        if let Some(Value::Array(items)) = source {
            for item in items {
                if !names.contains(&item) {
                    names.push(item);
                }
            }
        }
    }
    if names.is_empty() {
        None
    } else {
        Some(Value::Array(names))
    }
}

/// Merges two `additionalProperties` values. Closed (`false`) in either branch
/// closes the merged object against the union of declared properties (the
/// closed-object footgun-fix); `true` yields to a value schema; two value
/// schemas merge recursively.
fn merge_additional_properties(
    path: &Path,
    acc: Option<Value>,
    branch: Option<Value>,
    context: &str,
) -> Result<Option<Value>> {
    match (acc, branch) {
        (None, None) => Ok(None),
        (Some(Value::Bool(false)), _) | (_, Some(Value::Bool(false))) => {
            Ok(Some(Value::Bool(false)))
        }
        (None, Some(other)) | (Some(other), None) => Ok(Some(other)),
        (Some(a), Some(b)) => {
            if a == b {
                return Ok(Some(a));
            }
            match (&a, &b) {
                (Value::Bool(true), other) | (other, Value::Bool(true)) => Ok(Some(other.clone())),
                (Value::Object(_), Value::Object(_)) => {
                    let acc_schema: Schema = serde_json::from_value(a).map_err(|error| {
                        merge_reject(
                            path,
                            format!("{context}.additionalProperties is invalid: {error}"),
                        )
                    })?;
                    let branch_schema: Schema = serde_json::from_value(b).map_err(|error| {
                        merge_reject(
                            path,
                            format!("{context}.additionalProperties is invalid: {error}"),
                        )
                    })?;
                    let merged = merge_schema_pair(
                        path,
                        acc_schema,
                        &branch_schema,
                        &format!("{context}.additionalProperties"),
                    )?;
                    Ok(Some(serde_json::to_value(&merged).map_err(|error| {
                        merge_reject(
                            path,
                            format!("{context}: failed to preserve additionalProperties: {error}"),
                        )
                    })?))
                }
                _ => Ok(Some(b)),
            }
        }
    }
}

/// Merges two `items` schemas recursively.
fn merge_items(
    path: &Path,
    acc: Option<Box<Schema>>,
    branch: Option<Box<Schema>>,
    context: &str,
) -> Result<Option<Box<Schema>>> {
    match (acc, branch) {
        (None, other) | (other, None) => Ok(other),
        (Some(acc), Some(branch)) => Ok(Some(Box::new(merge_schema_pair(
            path,
            *acc,
            &branch,
            &format!("{context}.items"),
        )?))),
    }
}

/// Merges two values for the same `extra`-map keyword per the per-keyword rules.
fn merge_extra_value(
    path: &Path,
    key: &str,
    acc: &Value,
    branch: &Value,
    context: &str,
) -> Result<Value> {
    if acc == branch {
        return Ok(acc.clone());
    }
    match key {
        "minimum" | "exclusiveMinimum" | "minLength" | "minItems" | "minProperties"
        | "minContains" => numeric_extreme(path, key, acc, branch, true, context),
        "maximum" | "exclusiveMaximum" | "maxLength" | "maxItems" | "maxProperties"
        | "maxContains" => numeric_extreme(path, key, acc, branch, false, context),
        "multipleOf" => merge_multiple_of(path, acc, branch, context),
        "uniqueItems" => Ok(Value::Bool(
            acc.as_bool().unwrap_or(false) || branch.as_bool().unwrap_or(false),
        )),
        "enum" => intersect_enum(path, acc, branch, context),
        "const" => Err(merge_reject(
            path,
            format!(
                "{context}: `allOf` branches declare conflicting `const` values ({acc} vs {branch})"
            ),
        )),
        "format" => Err(merge_reject(
            path,
            format!(
                "{context}: `allOf` branches declare different `format`s ({acc} vs {branch}); no single value is two formats"
            ),
        )),
        "pattern" => Err(merge_reject(
            path,
            format!(
                "{context}: `allOf` branches declare different `pattern`s ({acc} vs {branch}); two regexes are not one regex"
            ),
        )),
        "contains" => Err(merge_reject(
            path,
            format!(
                "{context}: `allOf` branches declare different `contains` matchers; two existential constraints do not merge into one"
            ),
        )),
        "default" | "title" | "description" => Ok(branch.clone()),
        "dependentRequired" => merge_dependent_required(acc, branch),
        "patternProperties" | "propertyNames" => {
            if let (Value::Object(_), Value::Object(_)) = (acc, branch) {
                let acc_schema: Schema = serde_json::from_value(acc.clone()).map_err(|error| {
                    merge_reject(
                        path,
                        format!("{context}: `{key}` is not a valid schema: {error}"),
                    )
                })?;
                let branch_schema: Schema =
                    serde_json::from_value(branch.clone()).map_err(|error| {
                        merge_reject(
                            path,
                            format!("{context}: `{key}` is not a valid schema: {error}"),
                        )
                    })?;
                let merged = merge_schema_pair(
                    path,
                    acc_schema,
                    &branch_schema,
                    &format!("{context}.{key}"),
                )?;
                Ok(serde_json::to_value(&merged).map_err(|error| {
                    merge_reject(
                        path,
                        format!("{context}: failed to preserve `{key}`: {error}"),
                    )
                })?)
            } else {
                Ok(branch.clone())
            }
        }
        _ => Err(merge_reject(
            path,
            format!(
                "{context}: cannot merge differing `{key}` values ({acc} vs {branch}) across `allOf` branches"
            ),
        )),
    }
}

/// Keeps the tighter of two numeric bounds: the greater when `keep_max` (a lower
/// bound), else the smaller (an upper bound). Preserves the original JSON number
/// form (integer vs float).
fn numeric_extreme(
    path: &Path,
    key: &str,
    acc: &Value,
    branch: &Value,
    keep_max: bool,
    context: &str,
) -> Result<Value> {
    let parse = |value: &Value| -> Result<f64> {
        value
            .as_f64()
            .ok_or_else(|| merge_reject(path, format!("{context}: `{key}` must be a number")))
    };
    let a = parse(acc)?;
    let b = parse(branch)?;
    let keep_acc = if keep_max { a >= b } else { a <= b };
    Ok(if keep_acc {
        acc.clone()
    } else {
        branch.clone()
    })
}

/// Merges two `multipleOf` divisors to their least common multiple. Both are
/// positive integers (enforced downstream by the numeric validator), so the LCM
/// is a positive integer.
fn merge_multiple_of(path: &Path, acc: &Value, branch: &Value, context: &str) -> Result<Value> {
    let parse = |value: &Value| -> Result<i64> {
        value
            .as_f64()
            .filter(|number| number.is_finite() && number.fract() == 0.0 && *number > 0.0)
            .map(|number| number as i64)
            .ok_or_else(|| {
                merge_reject(
                    path,
                    format!("{context}: `multipleOf` must be a positive integer to merge"),
                )
            })
    };
    let a = parse(acc)?;
    let b = parse(branch)?;
    let gcd = {
        let (mut x, mut y) = (a, b);
        while y != 0 {
            let t = y;
            y = x % y;
            x = t;
        }
        x
    };
    let lcm = a / gcd * b;
    Ok(Value::Number(serde_json::Number::from(lcm)))
}

/// Intersects two `enum` value sets (first-seen order); an empty intersection is
/// a reject.
fn intersect_enum(path: &Path, acc: &Value, branch: &Value, context: &str) -> Result<Value> {
    let (Value::Array(acc_members), Value::Array(branch_members)) = (acc, branch) else {
        return Err(merge_reject(
            path,
            format!("{context}: `enum` must be an array of values"),
        ));
    };
    let mut out = Vec::new();
    for member in acc_members {
        if branch_members.contains(member) && !out.contains(member) {
            out.push(member.clone());
        }
    }
    if out.is_empty() {
        return Err(merge_reject(
            path,
            format!(
                "{context}: `allOf` branches have an empty `enum` intersection (no value is in every branch)"
            ),
        ));
    }
    Ok(Value::Array(out))
}

/// Merges two `dependentRequired` maps: per trigger key, the union of the
/// dependent-name lists.
fn merge_dependent_required(acc: &Value, branch: &Value) -> Result<Value> {
    let mut out = acc.as_object().cloned().unwrap_or_default();
    if let Some(branch_map) = branch.as_object() {
        for (trigger, deps) in branch_map {
            let entry = out
                .entry(trigger.clone())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let (Some(existing), Some(new_deps)) = (entry.as_array_mut(), deps.as_array()) {
                for dep in new_deps {
                    if !existing.contains(dep) {
                        existing.push(dep.clone());
                    }
                }
            }
        }
    }
    Ok(Value::Object(out))
}

/// Finalizes a merged schema: collapses a cross-keyword numeric-bound pair
/// (`minimum`+`exclusiveMinimum` or `maximum`+`exclusiveMaximum`) that arrived
/// from different branches to the single tighter bound, and resolves a
/// `const`+`enum` combination (the `const` must be a member; the `enum` is
/// dropped).
fn finalize_merged(path: &Path, schema: &mut Schema, context: &str) -> Result<()> {
    collapse_numeric_pair(schema, "minimum", "exclusiveMinimum", true);
    collapse_numeric_pair(schema, "maximum", "exclusiveMaximum", false);

    if schema.extra.contains_key("const") && schema.extra.contains_key("enum") {
        let const_value = schema.extra["const"].clone();
        let in_enum = schema.extra["enum"]
            .as_array()
            .is_some_and(|members| members.contains(&const_value));
        if !in_enum {
            return Err(merge_reject(
                path,
                format!(
                    "{context}: the merged `const` {const_value} is not a member of the merged `enum` set"
                ),
            ));
        }
        schema.extra.shift_remove("enum");
    }
    Ok(())
}

/// Collapses a same-axis inclusive/exclusive bound pair to the single bound that
/// admits the smaller set. For a lower bound (`is_lower`) keep `exclusiveMinimum`
/// iff its floor is `>=` the inclusive one; for an upper bound keep
/// `exclusiveMaximum` iff its ceiling is `<=` the inclusive one.
fn collapse_numeric_pair(schema: &mut Schema, inclusive: &str, exclusive: &str, is_lower: bool) {
    let inclusive_value = schema.extra.get(inclusive).and_then(Value::as_f64);
    let exclusive_value = schema.extra.get(exclusive).and_then(Value::as_f64);
    if let (Some(incl), Some(excl)) = (inclusive_value, exclusive_value) {
        let keep_exclusive = if is_lower { excl >= incl } else { excl <= incl };
        if keep_exclusive {
            schema.extra.shift_remove(inclusive);
        } else {
            schema.extra.shift_remove(exclusive);
        }
    }
}

/// Resolves a `$ref` string to the [`TypeKey`] it names (named-target / local-file
/// rules), independent of whether a model has been built for it yet.
fn resolve_ref_key(
    path: &Path,
    canonical_path: &Path,
    reference: &str,
    doc_paths: &BTreeSet<PathBuf>,
) -> Result<TypeKey> {
    let (file_part, pointer) = reference.split_once('#').unwrap_or((reference, ""));
    let target_path = if file_part.is_empty() {
        canonical_path.to_path_buf()
    } else {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let target = canonical(&base.join(file_part));
        if !doc_paths.contains(&target) {
            return Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("`$ref` target file `{file_part}` is not in the input set"),
            });
        }
        target
    };

    if pointer.is_empty() || pointer == "/" {
        Ok(TypeKey::Root(target_path))
    } else if let Some(name) = pointer.strip_prefix("/$defs/") {
        Ok(TypeKey::Def(
            target_path,
            name.replace("~1", "/").replace("~0", "~"),
        ))
    } else {
        Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("`$ref` `{reference}` must point at a `$defs` entry or file root"),
        })
    }
}

fn resolve_ref<'a>(
    path: &Path,
    canonical_path: &Path,
    reference: &str,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &'a BTreeMap<TypeKey, JsonModel>,
) -> Result<&'a JsonModel> {
    let doc_paths: BTreeSet<PathBuf> = docs.keys().cloned().collect();
    let key = resolve_ref_key(path, canonical_path, reference, &doc_paths)?;
    models.get(&key).ok_or_else(|| Error::InvalidJsonSchema {
        path: path.to_path_buf(),
        reason: format!("`$ref` `{reference}` does not resolve to a known JSON model"),
    })
}

fn insert_json_external_type(
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
    model: &JsonModel,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> Result<()> {
    let type_spec = json_model_spec(model, docs, models, module_paths)?;
    external_types
        .entry(type_spec.name.as_str().to_string())
        .or_insert_with(|| ExternalTypeBindingSpec {
            external_type: ExternalTypeSpec::Json(type_spec),
            reference: LanguageStringSpec::default(),
            type_name: language_string(Some(model.model_name.clone())),
            replacement: None,
            authored_type: None,
        });
    Ok(())
}

fn json_model_type(
    model: &JsonModel,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> Result<TypeSpec> {
    Ok(TypeSpec::External(ExternalTypeSpec::Json(json_model_spec(
        model,
        docs,
        models,
        module_paths,
    )?)))
}

fn json_model_spec(
    model: &JsonModel,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> Result<JsonModelSpec<Symbol>> {
    let schema =
        resolve_schema_refs_for_generation(model, &model.schema, docs, models, module_paths)?;
    Ok(JsonModelSpec {
        name: json_model_symbol(model, module_paths),
        model_name: model.model_name.clone(),
        schema: serde_json::to_value(&schema).map_err(|error| Error::InvalidJsonSchema {
            path: PathBuf::from("<json-schema>"),
            reason: format!(
                "failed to preserve JSON schema model `{}`: {error}",
                model.full_name
            ),
        })?,
    })
}

fn json_model_key(
    model: &JsonModel,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> String {
    json_model_symbol(model, module_paths).as_str().to_string()
}

fn json_model_symbol(
    model: &JsonModel,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> Symbol {
    let Some(module_path) = module_paths.and_then(|paths| paths.get(&model.canonical_path)) else {
        return Symbol::new(model.full_name.clone());
    };
    let full_name = if module_path.is_root() {
        model.full_name.clone()
    } else {
        format!("{}#{}", module_path.as_module_key(), model.full_name)
    };
    Symbol::qualified(module_path.clone(), full_name, model.model_name.clone())
}

fn resolve_schema_refs_for_generation(
    owner: &JsonModel,
    schema: &Schema,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    module_paths: Option<&BTreeMap<PathBuf, ModulePath>>,
) -> Result<Schema> {
    let mut resolved = schema.clone();
    if let Some(reference) = &schema.reference {
        let target = resolve_ref(
            &owner.canonical_path,
            &owner.canonical_path,
            reference,
            docs,
            models,
        )?;
        let target = json_model_symbol(target, module_paths);
        resolved.reference = Some(format!("#/$defs/{}", target.as_str()));
        return Ok(resolved);
    }
    if let Some(properties) = &schema.properties {
        resolved.properties = Some(
            properties
                .iter()
                .map(|(name, property)| {
                    Ok((
                        name.clone(),
                        resolve_schema_refs_for_generation(
                            owner,
                            property,
                            docs,
                            models,
                            module_paths,
                        )?,
                    ))
                })
                .collect::<Result<IndexMap<_, _>>>()?,
        );
    }
    if let Some(items) = &schema.items {
        resolved.items = Some(Box::new(resolve_schema_refs_for_generation(
            owner,
            items,
            docs,
            models,
            module_paths,
        )?));
    }
    if let Some(one_of) = &schema.one_of {
        resolved.one_of = Some(
            one_of
                .iter()
                .map(|branch| {
                    resolve_schema_refs_for_generation(owner, branch, docs, models, module_paths)
                })
                .collect::<Result<Vec<_>>>()?,
        );
    }
    if let Some(additional) = &schema.additional_properties
        && additional.is_object()
    {
        let additional_schema =
            serde_json::from_value::<Schema>(additional.clone()).map_err(|error| {
                Error::InvalidJsonSchema {
                    path: owner.canonical_path.clone(),
                    reason: format!("additionalProperties is invalid: {error}"),
                }
            })?;
        resolved.additional_properties = Some(
            serde_json::to_value(resolve_schema_refs_for_generation(
                owner,
                &additional_schema,
                docs,
                models,
                module_paths,
            )?)
            .map_err(|error| Error::InvalidJsonSchema {
                path: owner.canonical_path.clone(),
                reason: format!("failed to preserve additionalProperties: {error}"),
            })?,
        );
    }
    Ok(resolved)
}

fn language_string(default: Option<String>) -> LanguageStringSpec {
    LanguageStringSpec {
        default,
        ..LanguageStringSpec::default()
    }
}

fn root_is_schema_shaped(root: &Schema) -> bool {
    root.reference.is_some()
        || root.ty.is_some()
        || root.properties.is_some()
        || root.additional_properties.is_some()
        || root.one_of.is_some()
        || root.items.is_some()
}

fn root_type_name(path: &Path) -> String {
    path.file_name()
        .map(|value| strip_json_schema_extension(&value.to_string_lossy()).to_string())
        .unwrap_or_else(|| "Root".to_string())
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize(path))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// P15 identifier namespace + `x-<lang>-name` override escape hatch.
//
// See specs/json-schema/features/properties.md (Stage 1-4 + the override), and
// PRINCIPLES.md P15 (one identifier namespace per scope; synthesized-name
// collisions reject at load, never mangle; the escape hatch is the override).
//
// The load is per emitted target (`language`), so a coincidence that collides
// in one language may be fine in another; the pass runs once per language.
// ---------------------------------------------------------------------------

/// The `x-<lang>-name` extension keyword for a target, or `None` for a language
/// with no JSON identifier policy (Dotnet/Ruby are not part of the P15 subset).
fn lang_name_keyword(language: Language) -> Option<&'static str> {
    match language {
        Language::Go => Some("x-go-name"),
        Language::TypeScript => Some("x-ts-name"),
        Language::Python => Some("x-py-name"),
        Language::Java => Some("x-java-name"),
        _ => None,
    }
}

fn lang_const_name_keyword(language: Language) -> Option<&'static str> {
    match language {
        Language::Go => Some("x-go-const-name"),
        Language::Java => Some("x-java-const-name"),
        _ => None,
    }
}

fn lang_enum_names_keyword(language: Language) -> Option<&'static str> {
    match language {
        Language::Go => Some("x-go-enum-names"),
        Language::Java => Some("x-java-enum-names"),
        _ => None,
    }
}

/// The `x-<lang>-name` override on a schema node for the given target, if any.
fn override_name<'a>(language: Language, schema: &'a Schema) -> Option<&'a str> {
    let keyword = lang_name_keyword(language)?;
    schema.extra.get(keyword).and_then(Value::as_str)
}

/// A syntactically legal identifier for every supported target: non-empty, first
/// char an ASCII letter or `_`, remaining chars ASCII alphanumeric or `_`.
fn ident_is_syntactically_valid(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Whether `name` is a reserved word in the target language.
fn ident_is_reserved(language: Language, name: &str) -> bool {
    match language {
        Language::Go => matches!(
            name,
            "break"
                | "case"
                | "chan"
                | "const"
                | "continue"
                | "default"
                | "defer"
                | "else"
                | "fallthrough"
                | "for"
                | "func"
                | "go"
                | "goto"
                | "if"
                | "import"
                | "interface"
                | "map"
                | "package"
                | "range"
                | "return"
                | "select"
                | "struct"
                | "switch"
                | "type"
                | "var"
        ),
        Language::TypeScript => matches!(
            name,
            "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "debugger"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "false"
                | "finally"
                | "for"
                | "function"
                | "if"
                | "import"
                | "in"
                | "instanceof"
                | "new"
                | "null"
                | "return"
                | "super"
                | "switch"
                | "this"
                | "throw"
                | "true"
                | "try"
                | "typeof"
                | "var"
                | "void"
                | "while"
                | "with"
                | "yield"
                | "as"
                | "implements"
                | "interface"
                | "let"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "static"
        ),
        Language::Python => matches!(
            name,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
                | "match"
                | "case"
        ),
        Language::Java => matches!(
            name,
            "abstract"
                | "assert"
                | "boolean"
                | "break"
                | "byte"
                | "case"
                | "catch"
                | "char"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "do"
                | "double"
                | "else"
                | "enum"
                | "extends"
                | "final"
                | "finally"
                | "float"
                | "for"
                | "goto"
                | "if"
                | "implements"
                | "import"
                | "instanceof"
                | "int"
                | "interface"
                | "long"
                | "native"
                | "new"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "return"
                | "short"
                | "static"
                | "strictfp"
                | "super"
                | "switch"
                | "synchronized"
                | "this"
                | "throw"
                | "throws"
                | "transient"
                | "try"
                | "void"
                | "volatile"
                | "while"
                | "true"
                | "false"
                | "null"
        ),
        _ => false,
    }
}

/// Recases a JSON member name to the target's idiomatic identifier (Stage 1-2),
/// mirroring each backend's field-name derivation.
fn recase_member(language: Language, json_name: &str) -> String {
    match language {
        Language::Go => {
            let camel = json_name.to_upper_camel_case();
            if ident_is_reserved(Language::Go, &camel) {
                format!("{camel}_")
            } else {
                camel
            }
        }
        Language::TypeScript => {
            let camel = json_name.to_lower_camel_case();
            if ident_is_reserved(Language::TypeScript, &camel) {
                format!("{camel}_")
            } else {
                camel
            }
        }
        Language::Python => {
            let snake = json_name.to_snake_case();
            if ident_is_reserved(Language::Python, &snake) {
                format!("{snake}_")
            } else {
                snake
            }
        }
        Language::Java => json_name.to_lower_camel_case(),
        _ => json_name.to_string(),
    }
}

/// If a JSON member name recases to an identifier that cannot be emitted as-is
/// in `language` — syntactically invalid (e.g. a leading digit) or a reserved
/// word — returns the offending recased identifier and a short reason. P15
/// forbids auto-mangling, so such a member must carry an `x-<lang>-name`
/// override; returns `None` when the recased name is directly usable.
fn member_identifier_defect(language: Language, json_name: &str) -> Option<(String, &'static str)> {
    let base = match language {
        Language::Go => json_name.to_upper_camel_case(),
        Language::TypeScript | Language::Java => json_name.to_lower_camel_case(),
        Language::Python => json_name.to_snake_case(),
        _ => return None,
    };
    if !ident_is_syntactically_valid(&base) {
        return Some((base, "is not a valid identifier"));
    }
    if ident_is_reserved(language, &base) {
        return Some((base, "is a reserved word"));
    }
    None
}

/// The emitted member identifier for a property: the `x-<lang>-name` override if
/// present, otherwise the recased JSON name.
fn member_identifier(language: Language, json_name: &str, property: &Schema) -> String {
    override_name(language, property)
        .map(str::to_string)
        .unwrap_or_else(|| recase_member(language, json_name))
}

/// The emitted type identifier for a model. Type names are emitted from the
/// derived `model_name` across all four targets (a type-level `x-<lang>-name`
/// is validated at load but does not yet rewire type-name emission — only
/// member overrides change the emitted identifier), so the collision key stays
/// `model_name` to match generated output.
fn type_identifier(_language: Language, model_name: &str, _schema: &Schema) -> String {
    model_name.to_string()
}

/// Whether a property schema is a scalar closed value set (`const`/`enum`) that
/// synthesizes a Go defined type + value constants / Java value constants.
fn schema_closed_values(schema: &Schema) -> Vec<Value> {
    if let Some(value) = schema.extra.get("const") {
        vec![value.clone()]
    } else if let Some(Value::Array(values)) = schema.extra.get("enum") {
        values.clone()
    } else {
        Vec::new()
    }
}

/// The Go value-constant suffix for a scalar value (mirrors `go_value_suffix`).
fn go_value_suffix_for(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_upper_camel_case(),
        Value::Bool(flag) => if *flag { "True" } else { "False" }.to_string(),
        Value::Number(number) => number.to_string().replace('-', "Neg").replace('.', "_"),
        _ => String::new(),
    }
}

/// Validates an `x-<lang>-name` / `x-<lang>-const-name` override string: it must
/// be a legal, non-reserved identifier in the target language.
fn validate_override(
    language: Language,
    keyword: &str,
    value: &Value,
    context: &str,
) -> Result<()> {
    let Some(text) = value.as_str() else {
        return Err(Error::InvalidJsonSchema {
            path: PathBuf::from("<json-schema>"),
            reason: format!("{context}: `{keyword}` must be a string identifier"),
        });
    };
    if !ident_is_syntactically_valid(text) || ident_is_reserved(language, text) {
        return Err(Error::InvalidJsonSchema {
            path: PathBuf::from("<json-schema>"),
            reason: format!(
                "{context}: `{keyword}` value {text:?} is not a legal, non-reserved {} identifier",
                language.as_str()
            ),
        });
    }
    Ok(())
}

/// Validates every `x-<lang>-*` override reachable in a schema subtree (for the
/// active target only): `x-<lang>-name` on any node, and
/// `x-<lang>-const-name` / `x-<lang>-enum-names` on a `const`/`enum` node.
fn validate_overrides_in_schema(language: Language, schema: &Schema, context: &str) -> Result<()> {
    if let Some(keyword) = lang_name_keyword(language)
        && let Some(value) = schema.extra.get(keyword)
    {
        validate_override(language, keyword, value, context)?;
    }
    if let Some(keyword) = lang_const_name_keyword(language)
        && let Some(value) = schema.extra.get(keyword)
    {
        validate_override(language, keyword, value, context)?;
    }
    if let Some(keyword) = lang_enum_names_keyword(language)
        && let Some(value) = schema.extra.get(keyword)
    {
        let Some(map) = value.as_object() else {
            return Err(Error::InvalidJsonSchema {
                path: PathBuf::from("<json-schema>"),
                reason: format!("{context}: `{keyword}` must be a map of value to identifier"),
            });
        };
        for entry in map.values() {
            validate_override(language, keyword, entry, context)?;
        }
    }
    if let Some(properties) = &schema.properties {
        for (name, property) in properties {
            validate_overrides_in_schema(
                language,
                property,
                &format!("{context}.properties.{name}"),
            )?;
        }
    }
    if let Some(items) = &schema.items {
        validate_overrides_in_schema(language, items, &format!("{context}.items"))?;
    }
    if let Some(one_of) = &schema.one_of {
        for (index, branch) in one_of.iter().enumerate() {
            validate_overrides_in_schema(language, branch, &format!("{context}.oneOf[{index}]"))?;
        }
    }
    Ok(())
}

/// A single per-scope identifier namespace: inserting a name that is already
/// held (by a different origin) is a P15 collision → load reject.
#[derive(Default)]
struct Namespace {
    entries: BTreeMap<String, String>,
}

impl Namespace {
    fn insert(&mut self, language: Language, ident: String, origin: String) -> Result<()> {
        if let Some(previous) = self.entries.get(&ident)
            && previous != &origin
        {
            return Err(Error::InvalidJsonSchema {
                path: PathBuf::from("<json-schema>"),
                reason: format!(
                    "identifier collision in {} output: {previous} and {origin} both map to `{ident}`; disambiguate with an `{}` override (P15 — the generator never auto-mangles)",
                    language.as_str(),
                    lang_name_keyword(language).unwrap_or("x-<lang>-name"),
                ),
            });
        }
        self.entries.insert(ident, origin);
        Ok(())
    }
}

/// A JSON model with its emitted type identifier + decoded schema, grouped by
/// module (a module is one scope for the nesting languages; the single-input
/// path groups everything together).
struct NsModel {
    module_key: String,
    full_name: String,
    type_ident: String,
    schema: Schema,
}

/// The P15 per-target identifier collision pass + override validation over a
/// fully-built spec. Runs once per emitted target (`language`).
fn validate_identifier_namespace(language: Language, spec: &ApiSpec) -> Result<()> {
    if lang_name_keyword(language).is_none() {
        return Ok(());
    }

    let mut models: Vec<NsModel> = Vec::new();
    for (full_name, binding) in spec.external_types() {
        let ExternalTypeSpec::Json(json) = &binding.external_type else {
            continue;
        };
        let schema: Schema = serde_json::from_value(json.schema.clone()).map_err(|error| {
            Error::InvalidJsonSchema {
                path: PathBuf::from("<json-schema>"),
                reason: format!(
                    "failed to decode JSON model `{full_name}` for the P15 pass: {error}"
                ),
            }
        })?;
        let context = format!("`{}`", json.name.local_name());
        validate_overrides_in_schema(language, &schema, &context)?;
        let module_key = json
            .name
            .module_path()
            .map(ModulePath::as_module_key)
            .unwrap_or_default();
        models.push(NsModel {
            module_key,
            full_name: json.name.local_name().to_string(),
            type_ident: type_identifier(language, &json.model_name, &schema),
            schema,
        });
    }

    // Group modules → their own top-level namespace.
    let module_keys: BTreeSet<String> = models
        .iter()
        .map(|model| model.module_key.clone())
        .collect();
    for module_key in &module_keys {
        let mut top = Namespace::default();
        for model in models
            .iter()
            .filter(|model| &model.module_key == module_key)
        {
            top.insert(
                language,
                model.type_ident.clone(),
                format!("type `{}`", model.full_name),
            )?;
            collect_synthesized_top_level(
                language,
                model.full_name.as_str(),
                &model.type_ident,
                &model.schema,
                &mut top,
            )?;
            validate_member_scope(language, model.full_name.as_str(), &model.schema)?;
        }
        // Services and their derived bindings live in the root module scope of
        // the file that declares them (the single-input scope).
        if module_key.is_empty() {
            for service in &spec.services {
                top.insert(
                    language,
                    recase_type_name(language, &service.name),
                    format!("service `{}`", service.name),
                )?;
            }
        }
        // TypeScript `DEFAULT_<FIELD>` constants share the module scope; make
        // them participate rather than silently coexist (P15).
        if language == Language::TypeScript {
            collect_ts_default_constants(module_key, &models, &mut top)?;
        }
    }

    Ok(())
}

/// A service/type name is already `UpperCamelCase`; a target that lowercases
/// (none of the four for a type) would recase here. Type names are used
/// verbatim across all four targets.
fn recase_type_name(_language: Language, name: &str) -> String {
    name.to_string()
}

/// Adds the package/module-scoped identifiers a model synthesizes to the
/// top-level namespace: Go const/enum defined types + value constants (Go is
/// flat and has no nested types, so these live at package scope).
fn collect_synthesized_top_level(
    language: Language,
    model_full_name: &str,
    type_ident: &str,
    schema: &Schema,
    top: &mut Namespace,
) -> Result<()> {
    if language != Language::Go {
        return Ok(());
    }
    let Some(properties) = &schema.properties else {
        return Ok(());
    };
    for (json_name, property) in properties {
        let values = schema_closed_values(property);
        if values.is_empty() {
            continue;
        }
        // The Go closed-value defined type is `<Type><Field>` and each value
        // constant is `<definedType><valueSuffix>` — derived from the recased
        // field name (the generator names these off `go_field_name`, so the
        // collision pass matches the emitted identifiers).
        let defined_type = format!("{type_ident}{}", recase_member(Language::Go, json_name));
        top.insert(
            language,
            defined_type.clone(),
            format!("`{model_full_name}.{json_name}` closed-value type"),
        )?;
        for value in &values {
            let const_ident = format!("{defined_type}{}", go_value_suffix_for(value));
            top.insert(
                language,
                const_ident,
                format!("`{model_full_name}.{json_name}` value constant for {value}"),
            )?;
        }
    }
    Ok(())
}

/// Per-model member-scope collision checks (one scope per aggregate): two
/// members that recase/override to the same identifier collide; and in Go the
/// synthesized `<Field>OrDefault()` accessor shares the struct method-set, so a
/// field/accessor clash is a hard compile error.
fn validate_member_scope(language: Language, model_full_name: &str, schema: &Schema) -> Result<()> {
    let Some(properties) = &schema.properties else {
        return Ok(());
    };
    let mut scope = Namespace::default();
    for (json_name, property) in properties {
        // P15: a member whose recased name is invalid/reserved is rejected, not
        // silently mangled — the `x-<lang>-name` override is the escape hatch.
        if override_name(language, property).is_none()
            && let Some((ident, reason)) = member_identifier_defect(language, json_name)
        {
            return Err(Error::InvalidJsonSchema {
                path: PathBuf::from("<json-schema>"),
                reason: format!(
                    "member `{model_full_name}.{json_name}` recases to `{ident}`, which {reason} in {} output; add an `{}` override with a valid identifier (P15 — the generator never auto-mangles)",
                    language.as_str(),
                    lang_name_keyword(language).unwrap_or("x-<lang>-name"),
                ),
            });
        }
        scope.insert(
            language,
            member_identifier(language, json_name, property),
            format!("member `{model_full_name}.{json_name}`"),
        )?;
    }
    // An open struct (anything but `additionalProperties: false`) emits a
    // synthesized catch-all member holding unknown keys; its identifier shares
    // the member scope, so a declared member colliding with it rejects (P15)
    // rather than silently overwriting the catch-all.
    let is_open = !matches!(&schema.additional_properties, Some(Value::Bool(false)));
    if is_open {
        scope.insert(
            language,
            recase_member(language, "additionalProperties"),
            format!("`{model_full_name}` additional-properties catch-all"),
        )?;
    }
    // Go `<Field>OrDefault()` accessor (scalar `default` on an optional member).
    if language == Language::Go {
        let required: BTreeSet<&str> = schema
            .required
            .as_ref()
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for (json_name, property) in properties {
            let Some(default) = property.extra.get("default") else {
                continue;
            };
            if default.is_null() || default.is_object() || default.is_array() {
                continue;
            }
            if required.contains(json_name.as_str()) {
                continue;
            }
            let accessor = format!(
                "{}OrDefault",
                member_identifier(Language::Go, json_name, property)
            );
            scope.insert(
                language,
                accessor,
                format!("`{model_full_name}.{json_name}` OrDefault accessor"),
            )?;
        }
    }
    Ok(())
}

/// TypeScript `DEFAULT_<FIELD>` constants (module scope). The generator names a
/// default constant `DEFAULT_<FIELD>` when the field name is unique across the
/// module's models, else `DEFAULT_<MODEL>_<FIELD>`. Replicate that name and
/// enter it into the shared module namespace so a genuine clash rejects (P15)
/// rather than silently coexisting behind the model-name prefix.
fn collect_ts_default_constants(
    module_key: &str,
    models: &[NsModel],
    top: &mut Namespace,
) -> Result<()> {
    let group: Vec<&NsModel> = models
        .iter()
        .filter(|model| model.module_key == module_key)
        .collect();
    // How many models declare a scalar-default field with this JSON name.
    let field_count = |json_name: &str| -> usize {
        group
            .iter()
            .filter(|model| {
                model
                    .schema
                    .properties
                    .as_ref()
                    .and_then(|properties| properties.get(json_name))
                    .and_then(|property| property.extra.get("default"))
                    .is_some_and(|default| {
                        !default.is_null() && !default.is_object() && !default.is_array()
                    })
            })
            .count()
    };
    for model in &group {
        let Some(properties) = &model.schema.properties else {
            continue;
        };
        for (json_name, property) in properties {
            let Some(default) = property.extra.get("default") else {
                continue;
            };
            if default.is_null() || default.is_object() || default.is_array() {
                continue;
            }
            let field_shouty = json_name.to_shouty_snake_case();
            let ident = if field_count(json_name) == 1 {
                format!("DEFAULT_{field_shouty}")
            } else {
                format!(
                    "DEFAULT_{}_{field_shouty}",
                    model.type_ident.to_shouty_snake_case()
                )
            };
            top.insert(
                Language::TypeScript,
                ident,
                format!("`{}.{json_name}` DEFAULT_ constant", model.full_name),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::language::Language;

    fn parse(input: &str) -> ApiSpec {
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .unwrap()
    }

    #[test]
    fn parses_operation_refs_as_json_external_models() {
        let spec = parse(
            r##"
nexusrpc: "1.0.0"
$schema: https://json-schema.org/draft/2020-12/schema
services:
  ChatService:
    fqn: example.chat.v1.ChatService
    endpoint: __chat_service
    operations:
      sendMessage:
        fqn: SendMessage
        description: Send a message.
        input: { $ref: "#/$defs/SendMessageInput" }
        output: { $ref: "#/$defs/SendMessageOutput" }
$defs:
  SendMessageInput:
    type: object
    properties:
      roomId: { type: string }
    required: [roomId]
  SendMessageOutput:
    type: object
    properties:
      messageId: { type: string }
    required: [messageId]
"##,
        );

        assert!(spec.records().next().is_none());
        assert_eq!(spec.services[0].name, "ChatService");
        assert_eq!(spec.services[0].endpoint.as_deref(), Some("__chat_service"));
        let operation = &spec.services[0].operations[0];
        assert_eq!(operation.name, "SendMessage");
        assert_eq!(operation.wire_name, "SendMessage");
        assert_eq!(operation.doc.default.as_deref(), Some("Send a message."));
        let Some(TypeSpec::External(ExternalTypeSpec::Json(input))) = &operation.input else {
            panic!("input should be a JSON external model");
        };
        assert_eq!(input.name.as_str(), "SendMessageInput");
        assert_eq!(input.model_name, "SendMessageInput");
        let Some(TypeSpec::External(ExternalTypeSpec::Json(output))) = &operation.output else {
            panic!("output should be a JSON external model");
        };
        assert_eq!(output.name.as_str(), "SendMessageOutput");
        assert!(spec.external_type_binding("SendMessageInput").is_some());
        assert!(spec.external_type_binding("SendMessageOutput").is_some());
    }

    #[test]
    fn inline_operation_io_is_json_external_not_record() {
        let spec = parse(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      getRoom:
        input:
          type: object
          properties:
            roomId: { type: string }
          required: [roomId]
        output:
          type: object
          properties:
            displayName: { type: string }
          required: [displayName]
"##,
        );

        assert!(spec.records().next().is_none());
        let operation = &spec.services[0].operations[0];
        let Some(TypeSpec::External(ExternalTypeSpec::Json(input))) = &operation.input else {
            panic!("input should be a JSON external model");
        };
        assert_eq!(input.name.as_str(), "ChatService.GetRoomInput");
        assert_eq!(input.schema["properties"]["roomId"]["type"], "string");
        let Some(TypeSpec::External(ExternalTypeSpec::Json(output))) = &operation.output else {
            panic!("output should be a JSON external model");
        };
        assert_eq!(output.name.as_str(), "ChatService.GetRoomOutput");
    }

    #[test]
    fn missing_endpoint_is_allowed_in_parsed_spec() {
        let spec = parse(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      getRoom:
        input:
          type: object
          properties: {}
"##,
        );

        assert_eq!(spec.services[0].endpoint, None);
    }

    #[test]
    fn omitted_operation_io_is_void() {
        let spec = parse(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      ping:
        description: Liveness probe.
"##,
        );

        let operation = &spec.services[0].operations[0];
        assert_eq!(operation.name, "Ping");
        assert!(operation.input.is_none());
        assert!(operation.output.is_none());
    }

    #[test]
    fn ref_with_sibling_keywords_merges() {
        // `$ref`-with-siblings is the implicit-`allOf` sugar: the referenced
        // target is folded in and the use-site siblings extend it (see
        // specs/json-schema/features/allOf.md). No longer a reject.
        let spec = parse(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      fetchRoom:
        input:
          $ref: "#/$defs/GetRoomInput"
          properties:
            extra: { type: string }
$defs:
  GetRoomInput:
    type: object
    properties:
      roomId: { type: string }
    required: [roomId]
"##,
        );
        let Some(TypeSpec::External(ExternalTypeSpec::Json(input))) =
            &spec.services[0].operations[0].input
        else {
            panic!("input should be a JSON external model");
        };
        // The merged input carries both the folded `roomId` and the use-site
        // `extra` property, with no `$ref` residue.
        assert_eq!(input.schema["properties"]["roomId"]["type"], "string");
        assert_eq!(input.schema["properties"]["extra"]["type"], "string");
        assert_eq!(input.schema["required"], serde_json::json!(["roomId"]));
        assert!(input.schema.get("allOf").is_none());
        assert!(input.schema["$ref"].is_null());
    }

    fn doc_reject(input: &str) -> String {
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn rejects_wrong_nexusrpc_version() {
        let error = doc_reject(
            r##"
nexusrpc: "1.1.0"
services:
  ChatService:
    operations:
      ping: {}
"##,
        );
        assert!(error.contains("`nexusrpc` must be exactly"), "{error}");
        assert!(error.contains("1.0.0"), "{error}");
    }

    #[test]
    fn rejects_non_string_nexusrpc() {
        let error = doc_reject(
            r##"
nexusrpc: 1
services:
  ChatService:
    operations:
      ping: {}
"##,
        );
        assert!(error.contains("`nexusrpc` must be exactly"), "{error}");
    }

    #[test]
    fn rejects_wrong_schema_dialect() {
        let error = doc_reject(
            r##"
$schema: "http://json-schema.org/draft-07/schema#"
type: object
properties:
  a: { type: string }
"##,
        );
        assert!(error.contains("`$schema` must be"), "{error}");
        assert!(error.contains("2020-12"), "{error}");
    }

    #[test]
    fn rejects_schema_shaped_root_in_nexus_doc() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
type: object
properties:
  a: { type: string }
"##,
        );
        assert!(error.contains("envelope"), "{error}");
    }

    #[test]
    fn rejects_services_without_nexusrpc() {
        let error = doc_reject(
            r##"
services:
  ChatService:
    operations:
      ping: {}
"##,
        );
        assert!(error.contains("`services` require"), "{error}");
    }

    #[test]
    fn rejects_service_without_operations() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  Chat:
    operations: {}
"##,
        );
        assert!(error.contains("at least one operation"), "{error}");
    }

    #[test]
    fn rejects_empty_inline_operation_io() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  Chat:
    operations:
      getRoom:
        input: {}
"##,
        );
        assert!(error.contains("must be `type: object`"), "{error}");
    }

    #[test]
    fn rejects_non_object_inline_operation_io() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  Chat:
    operations:
      getRoom:
        input: { type: string }
"##,
        );
        assert!(error.contains("must be `type: object`"), "{error}");
    }

    #[test]
    fn rejects_plain_file_without_root_schema() {
        let error = doc_reject(
            r##"
description: just a description with no schema keywords
"##,
        );
        assert!(error.contains("must define a root schema"), "{error}");
    }

    #[test]
    fn rejects_empty_input_set() {
        let error = api_spec_from_json_schema_sources(Language::Python, vec![])
            .unwrap_err()
            .to_string();
        assert!(error.contains("at least one JSON schema input"), "{error}");
    }

    #[test]
    fn rejects_malformed_yaml() {
        let error = doc_reject("type: object\n  bad: : indentation: [");
        assert!(error.contains("failed to parse JSON schema"), "{error}");
    }

    fn numeric_reject(field_schema: &str) -> String {
        let input = format!(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
{}
"#,
            field_schema
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            &input,
            PathBuf::from("api.yaml"),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn rejects_empty_numeric_interval() {
        let error = numeric_reject("type: integer\nminimum: 10\nmaximum: 2");
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_fractional_bound_on_integer_field() {
        let error = numeric_reject("type: integer\nmaximum: 5.5");
        assert!(error.contains("integer bound"), "{error}");
    }

    #[test]
    fn rejects_boolean_exclusive_maximum_form() {
        let error = numeric_reject("type: integer\nmaximum: 5\nexclusiveMaximum: true");
        assert!(error.contains("boolean form"), "{error}");
    }

    #[test]
    fn rejects_zero_multiple_of() {
        let error = numeric_reject("type: integer\nmultipleOf: 0");
        assert!(error.contains("greater than 0"), "{error}");
    }

    #[test]
    fn rejects_fractional_multiple_of() {
        let error = numeric_reject("type: number\nmultipleOf: 0.1");
        assert!(error.contains("not yet supported"), "{error}");
    }

    #[test]
    fn rejects_redundant_same_axis_bounds() {
        let error = numeric_reject("type: integer\nmaximum: 10\nexclusiveMaximum: 12");
        assert!(error.contains("exactly one"), "{error}");
    }

    #[test]
    fn rejects_numeric_bound_on_string_field() {
        let error = numeric_reject("type: string\nmaximum: 5");
        assert!(error.contains("require `type: integer`"), "{error}");
    }

    #[test]
    fn rejects_const_violating_bound() {
        let error = numeric_reject("type: integer\nmaximum: 5\nconst: 7");
        assert!(error.contains("violates the numeric bounds"), "{error}");
    }

    #[test]
    fn rejects_non_number_numeric_bound() {
        let error = numeric_reject("type: integer\nminimum: \"0\"");
        assert!(error.contains("`minimum` must be a number"), "{error}");
    }

    #[test]
    fn rejects_non_number_multiple_of() {
        let error = numeric_reject("type: integer\nmultipleOf: \"2\"");
        assert!(error.contains("`multipleOf` must be a number"), "{error}");
    }

    #[test]
    fn rejects_redundant_minimum_exclusive_minimum() {
        let error = numeric_reject("type: integer\nminimum: 0\nexclusiveMinimum: 2");
        assert!(error.contains("exactly one"), "{error}");
    }

    #[test]
    fn rejects_boolean_exclusive_minimum_form() {
        let error = numeric_reject("type: integer\nminimum: 0\nexclusiveMinimum: true");
        assert!(error.contains("boolean form"), "{error}");
    }

    #[test]
    fn rejects_default_violating_bound() {
        let error = numeric_reject("type: integer\nmaximum: 5\ndefault: 9");
        assert!(error.contains("violates the numeric bounds"), "{error}");
    }

    #[test]
    fn rejects_unsatisfiable_integer_range_with_multiple_of() {
        let error = numeric_reject("type: integer\nminimum: 3\nmaximum: 3\nmultipleOf: 2");
        assert!(error.contains("no multiple of"), "{error}");
    }

    #[test]
    fn rejects_string_length_on_non_string_field() {
        let error = numeric_reject("type: integer\nminLength: 3");
        assert!(error.contains("require `type: string`"), "{error}");
    }

    #[test]
    fn rejects_empty_string_length_interval() {
        let error = numeric_reject("type: string\nminLength: 10\nmaxLength: 2");
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_const_string_violating_max_length() {
        let error = numeric_reject("type: string\nmaxLength: 2\nconst: abc");
        assert!(error.contains("exceeding `maxLength`"), "{error}");
    }

    #[test]
    fn rejects_negative_max_length() {
        let error = numeric_reject("type: string\nmaxLength: -1");
        assert!(error.contains("non-negative integer"), "{error}");
    }

    #[test]
    fn rejects_const_below_min_length() {
        let error = numeric_reject("type: string\nminLength: 5\nconst: ab");
        assert!(error.contains("below `minLength`"), "{error}");
    }

    #[test]
    fn rejects_enum_string_violating_max_length() {
        let error = numeric_reject("type: string\nmaxLength: 2\nenum: [ok, toolong]");
        assert!(error.contains("exceeding `maxLength`"), "{error}");
    }

    #[test]
    fn accepts_zero_min_length() {
        numeric_accept("type: string\nminLength: 0");
    }

    #[test]
    fn accepts_valid_string_bounds() {
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  code: { type: string, minLength: 2, maxLength: 5 }
  fixed: { type: string, minLength: 3, maxLength: 3 }
  slug: { type: string, maxLength: 12 }
"#;
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .expect("valid string bounds should load");
    }

    #[test]
    fn rejects_pattern_backreference() {
        let error = numeric_reject("type: string\npattern: (a)\\1");
        assert!(error.contains("not portable"), "{error}");
    }

    #[test]
    fn rejects_pattern_lookahead() {
        let error = numeric_reject("type: string\npattern: (?=.*[A-Z]).+");
        assert!(error.contains("not portable"), "{error}");
    }

    #[test]
    fn rejects_pattern_inline_flag() {
        let error = numeric_reject("type: string\npattern: (?i)^cat$");
        assert!(error.contains("inline flag"), "{error}");
    }

    #[test]
    fn rejects_pattern_open_complement_class() {
        let error = numeric_reject("type: string\npattern: \"[\\\\S.]\"");
        assert!(error.contains("multi-member"), "{error}");
    }

    #[test]
    fn rejects_pattern_on_non_string_field() {
        let error = numeric_reject("type: integer\npattern: ^\\d+$");
        assert!(error.contains("requires `type: string`"), "{error}");
    }

    #[test]
    fn rejects_const_violating_pattern() {
        let error = numeric_reject("type: string\npattern: ^[a-z]+$\nconst: AB");
        assert!(error.contains("does not match `pattern`"), "{error}");
    }

    #[test]
    fn rejects_non_string_pattern_value() {
        let error = numeric_reject("type: string\npattern: 5");
        assert!(error.contains("`pattern` must be a string"), "{error}");
    }

    #[test]
    fn rejects_enum_violating_pattern() {
        let error = numeric_reject("type: string\npattern: \"^[a-z]+$\"\nenum: [ok, AB]");
        assert!(error.contains("does not match `pattern`"), "{error}");
    }

    #[test]
    fn accepts_empty_pattern() {
        numeric_accept("type: string\npattern: \"\"");
    }

    #[test]
    fn rejects_pattern_lookbehind() {
        let error = numeric_reject("type: string\npattern: \"(?<=x)y\"");
        assert!(error.contains("not portable"), "{error}");
    }

    #[test]
    fn accepts_supported_format() {
        let schema = model_schema(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  id: { type: string, format: uuid }
  site: { type: string, format: uri }
"#,
            "Api",
        );
        assert_eq!(schema["properties"]["id"]["format"], "uuid");
        assert_eq!(schema["properties"]["site"]["format"], "uri");
    }

    #[test]
    fn rejects_format_on_non_string_field() {
        let error = numeric_reject("type: integer\nformat: uuid");
        assert!(error.contains("requires `type: string`"), "{error}");
    }

    #[test]
    fn rejects_non_string_format_value() {
        let error = numeric_reject("type: string\nformat: 5");
        assert!(error.contains("`format` must be a string"), "{error}");
    }

    #[test]
    fn rejects_unknown_format() {
        let error = numeric_reject("type: string\nformat: phone");
        assert!(error.contains("unknown `format: phone`"), "{error}");
        assert!(error.contains("uuid"), "fix-it lists supported: {error}");
    }

    #[test]
    fn rejects_typo_format_as_unknown() {
        let error = numeric_reject("type: string\nformat: datetime");
        assert!(error.contains("unknown `format: datetime`"), "{error}");
    }

    #[test]
    fn rejects_deferred_format() {
        let error = numeric_reject("type: string\nformat: iri");
        assert!(error.contains("not yet supported (deferred)"), "{error}");
    }

    fn numeric_accept(field_schema: &str) {
        let input = format!(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
{}
"#,
            field_schema
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            &input,
            PathBuf::from("api.yaml"),
        )
        .expect("schema should load");
    }

    #[test]
    fn accepts_materialized_temporal_formats() {
        for format in ["date-time", "date", "time", "duration"] {
            numeric_accept(&format!("type: string\nformat: {format}"));
        }
    }

    #[test]
    fn accepts_materializable_temporal_const_literals() {
        numeric_accept("type: string\nformat: date-time\nconst: \"2021-06-15T12:30:45Z\"");
        numeric_accept("type: string\nformat: duration\nconst: \"PT1H30M\"");
    }

    #[test]
    fn rejects_materialized_leap_second_literal() {
        // Materialized narrowing: `:60` cannot be held by a native type.
        let error =
            numeric_reject("type: string\nformat: date-time\nconst: \"2021-12-31T23:59:60Z\"");
        assert!(error.contains("is not a valid date-time"), "{error}");
        let error = numeric_reject("type: string\nformat: time\nconst: \"23:59:60Z\"");
        assert!(error.contains("is not a valid time"), "{error}");
    }

    #[test]
    fn rejects_materialized_calendar_duration_literal() {
        // Materialized narrowing: `duration` is time-only (no Y/M/W/D).
        for literal in ["P1Y", "P4W", "P1D"] {
            let error = numeric_reject(&format!(
                "type: string\nformat: duration\nconst: \"{literal}\""
            ));
            assert!(
                error.contains("is not a valid duration"),
                "{literal}: {error}"
            );
        }
    }

    #[test]
    fn rejects_missing_offset_date_time_literal() {
        // Materialized `date-time` requires an offset.
        let error =
            numeric_reject("type: string\nformat: date-time\nconst: \"2021-06-15T12:30:45\"");
        assert!(error.contains("is not a valid date-time"), "{error}");
    }

    #[test]
    fn rejects_const_violating_format() {
        let error = numeric_reject("type: string\nformat: uuid\nconst: not-a-uuid");
        assert!(error.contains("is not a valid uuid"), "{error}");
    }

    #[test]
    fn rejects_default_violating_format() {
        let error = numeric_reject("type: string\nformat: ipv4\ndefault: 256.0.0.1");
        assert!(error.contains("is not a valid ipv4"), "{error}");
    }

    #[test]
    fn rejects_enum_violating_format() {
        let error = numeric_reject("type: string\nformat: uuid\nenum: [not-a-uuid]");
        assert!(error.contains("is not a valid uuid"), "{error}");
    }

    #[test]
    fn accepts_materialized_content_encodings() {
        for encoding in ["base64", "base64url"] {
            numeric_accept(&format!("type: string\ncontentEncoding: {encoding}"));
        }
    }

    #[test]
    fn accepts_valid_content_encoding_const_literals() {
        // ">>>" canonical padded standard / unpadded URL-safe.
        numeric_accept("type: string\ncontentEncoding: base64\nconst: \"Pj4+\"");
        numeric_accept("type: string\ncontentEncoding: base64url\nconst: \"Pj4-\"");
    }

    #[test]
    fn rejects_content_encoding_on_non_string_field() {
        let error = numeric_reject("type: integer\ncontentEncoding: base64");
        assert!(error.contains("requires `type: string`"), "{error}");
    }

    #[test]
    fn rejects_non_string_content_encoding_value() {
        let error = numeric_reject("type: string\ncontentEncoding: 5");
        assert!(
            error.contains("`contentEncoding` must be a string"),
            "{error}"
        );
    }

    #[test]
    fn rejects_unsupported_content_encoding() {
        for encoding in [
            "base32",
            "base16",
            "quoted-printable",
            "7bit",
            "8bit",
            "binary",
        ] {
            let error = numeric_reject(&format!("type: string\ncontentEncoding: {encoding}"));
            assert!(
                error.contains(&format!("`contentEncoding: {encoding}` is not supported")),
                "{error}"
            );
            assert!(error.contains("base64"), "fix-it lists supported: {error}");
        }
    }

    #[test]
    fn rejects_content_media_type_alongside_content_encoding() {
        let error =
            numeric_reject("type: string\ncontentEncoding: base64\ncontentMediaType: image/png");
        assert!(error.contains("contentMediaType"), "{error}");
        assert!(error.contains("not supported"), "{error}");
    }

    #[test]
    fn rejects_const_violating_content_encoding() {
        // URL-safe chars under `base64`.
        let error = numeric_reject("type: string\ncontentEncoding: base64\nconst: \"a-b_\"");
        assert!(
            error.contains("is not valid base64-encoded data"),
            "{error}"
        );
        // Padding under `base64url`.
        let error = numeric_reject("type: string\ncontentEncoding: base64url\nconst: \"aGk=\"");
        assert!(
            error.contains("is not valid base64url-encoded data"),
            "{error}"
        );
    }

    #[test]
    fn rejects_enum_violating_content_encoding() {
        let error = numeric_reject("type: string\ncontentEncoding: base64\nenum: [\"a-b_\"]");
        assert!(
            error.contains("is not valid base64-encoded data"),
            "{error}"
        );
    }

    #[test]
    fn accepts_and_normalizes_perl_space_pattern() {
        let schema = model_schema(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  phrase: { type: string, pattern: "^\\S+\\s\\S+$" }
"#,
            "Api",
        );
        // `\s`/`\S` are expanded to the explicit ASCII class in the loader; `$`
        // stays canonical for the per-target backend rewrite.
        assert_eq!(
            schema["properties"]["phrase"]["pattern"],
            "^[^\\t\\n\\x0B\\f\\r ]+[\\t\\n\\x0B\\f\\r ][^\\t\\n\\x0B\\f\\r ]+$"
        );
    }

    #[test]
    fn rejects_array_keyword_on_non_array_field() {
        let error = numeric_reject("type: string\nminItems: 1");
        assert!(error.contains("require `type: array`"), "{error}");
    }

    #[test]
    fn rejects_empty_items_interval() {
        let error =
            numeric_reject("type: array\nitems: { type: string }\nminItems: 5\nmaxItems: 2");
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_unique_items_on_object_element_array() {
        let error = numeric_reject(
            "type: array\nitems: { type: object, properties: {} }\nuniqueItems: true",
        );
        assert!(error.contains("not yet supported"), "{error}");
    }

    #[test]
    fn rejects_contains_with_composite_matcher() {
        let error = numeric_reject(
            "type: array\nitems: { type: object, properties: {} }\ncontains: { type: object }",
        );
        assert!(error.contains("not yet supported"), "{error}");
    }

    #[test]
    fn rejects_min_contains_without_contains() {
        let error = numeric_reject("type: array\nitems: { type: string }\nminContains: 2");
        assert!(error.contains("require a sibling `contains`"), "{error}");
    }

    #[test]
    fn rejects_shapeless_contains_matcher() {
        let error = numeric_reject("type: array\nitems: { type: string }\ncontains: {}");
        assert!(error.contains("not a matcher"), "{error}");
    }

    #[test]
    fn rejects_type_incompatible_contains_matcher() {
        let error =
            numeric_reject("type: array\nitems: { type: string }\ncontains: { type: integer }");
        assert!(
            error.contains("incompatible with the element type"),
            "{error}"
        );
    }

    #[test]
    fn rejects_vacuous_min_contains_zero() {
        let error = numeric_reject(
            "type: array\nitems: { type: string }\ncontains: { const: x }\nminContains: 0",
        );
        assert!(error.contains("assert nothing"), "{error}");
    }

    #[test]
    fn rejects_max_contains_zero_at_default_min() {
        let error = numeric_reject(
            "type: array\nitems: { type: string }\ncontains: { const: x }\nmaxContains: 0",
        );
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_non_integer_min_items() {
        let error = numeric_reject("type: array\nitems: { type: string }\nminItems: -1");
        assert!(error.contains("non-negative integer"), "{error}");
    }

    #[test]
    fn rejects_non_integer_max_contains() {
        let error = numeric_reject(
            "type: array\nitems: { type: string }\ncontains: { const: x }\nmaxContains: -1",
        );
        assert!(error.contains("non-negative integer"), "{error}");
    }

    #[test]
    fn rejects_non_boolean_unique_items() {
        let error = numeric_reject("type: array\nitems: { type: string }\nuniqueItems: \"true\"");
        assert!(error.contains("`uniqueItems` must be a boolean"), "{error}");
    }

    #[test]
    fn rejects_min_contains_above_max_contains() {
        let error = numeric_reject(
            "type: array\nitems: { type: string }\ncontains: { const: x }\nminContains: 3\nmaxContains: 1",
        );
        assert!(error.contains("exceeds `maxContains`"), "{error}");
    }

    #[test]
    fn rejects_max_contains_without_contains() {
        let error = numeric_reject("type: array\nitems: { type: string }\nmaxContains: 2");
        assert!(error.contains("require a sibling `contains`"), "{error}");
    }

    #[test]
    fn rejects_non_schema_contains_value() {
        let error = numeric_reject("type: array\nitems: { type: string }\ncontains: 5");
        assert!(error.contains("must be a schema object"), "{error}");
    }

    #[test]
    fn accepts_valid_array_constraints() {
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  tags: { type: array, items: { type: string }, minItems: 1, maxItems: 5 }
  aliases: { type: array, items: { type: string }, uniqueItems: true }
  roles:
    type: array
    items: { type: string }
    contains: { const: admin }
    minContains: 1
    maxContains: 2
"#;
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .expect("valid array constraints should load");
    }

    #[test]
    fn accepts_valid_numeric_bounds() {
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  bounded: { type: integer, minimum: 1, maximum: 10 }
  strict: { type: integer, exclusiveMinimum: 0 }
  ratio: { type: number, minimum: 5, multipleOf: 5 }
  stepped: { type: integer, multipleOf: 3 }
"#;
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .expect("valid numeric bounds should load");
    }

    #[test]
    fn rejects_object_keyword_on_non_object_field() {
        let error = numeric_reject("type: string\nminProperties: 1");
        assert!(error.contains("require `type: object`"), "{error}");
    }

    #[test]
    fn rejects_empty_property_interval() {
        let error = numeric_reject(
            "type: object\nadditionalProperties: true\nminProperties: 5\nmaxProperties: 2",
        );
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_min_properties_above_closed_object_capacity() {
        let error = numeric_reject(
            "type: object\nadditionalProperties: false\nproperties: { a: { type: string } }\nminProperties: 2",
        );
        assert!(error.contains("closed object"), "{error}");
    }

    #[test]
    fn rejects_property_names_alongside_properties() {
        let error = numeric_reject(
            "type: object\nproperties: { id: { type: string } }\npropertyNames: { type: string, maxLength: 8 }",
        );
        assert!(error.contains("map-shaped object"), "{error}");
    }

    #[test]
    fn rejects_non_string_property_names() {
        let error = numeric_reject(
            "type: object\nadditionalProperties: true\npropertyNames: { type: integer }",
        );
        assert!(error.contains("must be `type: string`"), "{error}");
    }

    #[test]
    fn rejects_shapeless_property_names() {
        let error = numeric_reject(
            "type: object\nadditionalProperties: true\npropertyNames: { type: string }",
        );
        assert!(error.contains("asserts nothing"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_undeclared_reference() {
        let error = numeric_reject(
            "type: object\nproperties: { a: { type: string } }\ndependentRequired: { a: [b] }",
        );
        assert!(error.contains("not declared in `properties`"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_trigger_in_required() {
        let error = numeric_reject(
            "type: object\nproperties: { a: { type: string }, b: { type: string } }\nrequired: [a]\ndependentRequired: { a: [b] }",
        );
        assert!(error.contains("also in `required`"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_dependent_in_required() {
        let error = numeric_reject(
            "type: object\nproperties: { a: { type: string }, b: { type: string } }\nrequired: [b]\ndependentRequired: { a: [b] }",
        );
        assert!(error.contains("already in `required`"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_non_unique_dependents() {
        let error = numeric_reject(
            "type: object\nproperties: { a: { type: string }, b: { type: string } }\ndependentRequired: { a: [b, b] }",
        );
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn rejects_non_integer_min_properties() {
        let error = numeric_reject("type: object\nadditionalProperties: true\nminProperties: -1");
        assert!(error.contains("non-negative integer"), "{error}");
    }

    #[test]
    fn rejects_max_properties_below_required_count() {
        let error = numeric_reject(
            "type: object\nproperties: { a: {type: string}, b: {type: string}, c: {type: string} }\nrequired: [a, b, c]\nmaxProperties: 2",
        );
        assert!(error.contains("is below the"), "{error}");
    }

    #[test]
    fn rejects_property_names_without_map_host() {
        let error = numeric_reject(
            "type: object\nadditionalProperties: false\npropertyNames: { type: string, maxLength: 8 }",
        );
        assert!(error.contains("requires a map host"), "{error}");
    }

    #[test]
    fn rejects_bare_true_property_names() {
        let error = numeric_reject("type: object\nadditionalProperties: true\npropertyNames: true");
        assert!(error.contains("string schema constraining"), "{error}");
    }

    #[test]
    fn rejects_unsupported_property_names_assertion() {
        let error = numeric_reject(
            "type: object\nadditionalProperties: true\npropertyNames: { type: string, pattern: \"^x\" }",
        );
        assert!(error.contains("not yet supported"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_value_not_object() {
        let error = numeric_reject(
            "type: object\nproperties: { a: {type: string} }\ndependentRequired: []",
        );
        assert!(error.contains("object mapping"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_value_not_array() {
        let error = numeric_reject(
            "type: object\nproperties: { a: {type: string} }\ndependentRequired: { a: b }",
        );
        assert!(
            error.contains("must be an array of property-name strings"),
            "{error}"
        );
    }

    #[test]
    fn rejects_dependent_required_non_string_element() {
        let error = numeric_reject(
            "type: object\nproperties: { a: {type: string} }\ndependentRequired: { a: [1] }",
        );
        assert!(error.contains("property-name strings"), "{error}");
    }

    #[test]
    fn rejects_dependent_required_undeclared_trigger() {
        let error = numeric_reject(
            "type: object\nproperties: { b: {type: string} }\ndependentRequired: { a: [b] }",
        );
        assert!(error.contains("trigger `a`"), "{error}");
        assert!(error.contains("not declared"), "{error}");
    }

    #[test]
    fn accepts_valid_object_constraints() {
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
minProperties: 1
maxProperties: 6
properties:
  a: { type: string }
  b: { type: string }
  c: { type: string }
dependentRequired:
  a: [b]
"#;
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .expect("valid object constraints should load");
    }

    #[test]
    fn accepts_valid_property_names_map() {
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
additionalProperties: { type: string }
propertyNames: { type: string, maxLength: 8 }
"#;
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .expect("valid propertyNames map should load");
    }

    #[test]
    fn resolves_refs_across_input_files() {
        let spec = api_spec_from_json_schema_sources(
            Language::Python,
            vec![
                (
                    PathBuf::from("main.yaml"),
                    r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      getRoom:
        input: { $ref: "types.yaml#/$defs/GetRoomInput" }
"##
                    .to_string(),
                ),
                (
                    PathBuf::from("types.yaml"),
                    r##"
nexusrpc: "1.0.0"
$defs:
  GetRoomInput:
    type: object
    properties:
      roomId: { type: string }
"##
                    .to_string(),
                ),
            ],
        )
        .unwrap();

        let Some(TypeSpec::External(ExternalTypeSpec::Json(input))) =
            &spec.services[0].operations[0].input
        else {
            panic!("input should be a JSON external model");
        };
        assert_eq!(input.name.as_str(), "GetRoomInput");
    }

    /// A minimal pure JSON Schema document whose root type is named `title`.
    fn module_collision_source(path: &str, title: &str) -> JsonSource {
        JsonSource {
            path: PathBuf::from(path),
            source_root: PathBuf::from("."),
            relative_path: PathBuf::from(path),
            input: format!("title: {title}\ntype: object\nproperties:\n  id: {{ type: string }}\n"),
        }
    }

    #[test]
    fn rejects_two_sources_with_the_same_module_path() {
        // `foo.yaml` and `foo.json` are distinct input files but both strip to
        // module path `foo`, so the second leaf collides with the first.
        let sources = vec![
            module_collision_source("foo.yaml", "FooYaml"),
            module_collision_source("foo.json", "FooJson"),
        ];
        let error = api_spec_tree_from_json_schema_sources(Language::Python, sources)
            .expect_err("two sources mapping to the same module path should be rejected")
            .to_string();
        assert!(
            error.contains("duplicate JSON schema module path"),
            "{error}"
        );
    }

    #[test]
    fn rejects_source_module_path_conflicting_with_a_branch() {
        // `foo.yaml` occupies leaf `foo`; `foo/bar.yaml` then needs `foo` to be a
        // branch, so its insertion conflicts with the existing module.
        let sources = vec![
            module_collision_source("foo.yaml", "Foo"),
            module_collision_source("foo/bar.yaml", "Bar"),
        ];
        let error = api_spec_tree_from_json_schema_sources(Language::Python, sources)
            .expect_err("a source colliding with an existing module branch should be rejected")
            .to_string();
        assert!(error.contains("conflicts with another module"), "{error}");
    }

    #[test]
    fn rejects_remote_http_ref() {
        let error = numeric_reject("$ref: \"https://example.com/s.json\"");
        assert!(error.contains("remote `$ref`"), "{error}");
    }

    #[test]
    fn rejects_ref_into_non_defs() {
        let error = numeric_reject("$ref: \"#/properties/x/items\"");
        assert!(
            error.contains("must point at a `$defs` entry or file root"),
            "{error}"
        );
    }

    #[test]
    fn rejects_unresolvable_defs_ref() {
        let error = numeric_reject("$ref: \"#/$defs/Missing\"");
        assert!(
            error.contains("does not resolve to a known JSON model"),
            "{error}"
        );
    }

    /// Parses a single object property `value` carrying `field_schema` and
    /// returns the load error string (for the `const`/`enum` reject cases).
    fn const_enum_reject(field_schema: &str) -> String {
        numeric_reject(field_schema)
    }

    /// Loads a schema with the given field schema under `properties.value` and
    /// returns the load error message (panicking if it unexpectedly succeeds).
    fn structural_reject(field_schema: &str) -> String {
        numeric_reject(field_schema)
    }

    #[test]
    fn rejects_structural_keywords_with_fixits() {
        // Combinator / applicator keywords with no supported lowering, plus the
        // metadata keywords that reject (directional / content). Each must fail
        // to load and name the offending keyword (P6/P7.1). See the reject specs.
        for (keyword, field_schema) in [
            ("anyOf", "anyOf: [{ type: string }]"),
            ("not", "type: string\nnot: { type: integer }"),
            ("if", "type: string\nif: { type: string }"),
            ("then", "type: string\nthen: { type: string }"),
            ("else", "type: string\nelse: { type: string }"),
            (
                "prefixItems",
                "type: array\nprefixItems: [{ type: string }]",
            ),
            ("unevaluatedItems", "type: array\nunevaluatedItems: false"),
            (
                "unevaluatedProperties",
                "type: object\nunevaluatedProperties: false",
            ),
            (
                "dependentSchemas",
                "type: object\ndependentSchemas: { a: { type: object } }",
            ),
            (
                "patternProperties",
                "type: object\npatternProperties: { \"^x\": { type: string } }",
            ),
            ("readOnly", "type: string\nreadOnly: true"),
            ("writeOnly", "type: string\nwriteOnly: true"),
            (
                "contentMediaType",
                "type: string\ncontentMediaType: image/png",
            ),
            (
                "contentSchema",
                "type: string\ncontentSchema: { type: object }",
            ),
            ("$id", "type: string\n$id: \"http://x\""),
            ("$anchor", "type: string\n$anchor: foo"),
            ("$dynamicRef", "type: string\n$dynamicRef: \"#foo\""),
            ("$dynamicAnchor", "type: string\n$dynamicAnchor: foo"),
            ("$vocabulary", "type: string\n$vocabulary: { \"x\": true }"),
        ] {
            let error = structural_reject(field_schema);
            assert!(
                error.contains(keyword) && error.contains("not supported"),
                "expected `{keyword}` reject, got: {error}"
            );
        }
    }

    #[test]
    fn rejects_read_only_false() {
        let error = structural_reject("type: string\nreadOnly: false");
        assert!(
            error.contains("`readOnly`/`writeOnly` is not supported"),
            "{error}"
        );
    }

    #[test]
    fn rejects_nullable_keyword() {
        let error = structural_reject("type: string\nnullable: true");
        assert!(error.contains("`nullable` is not supported"), "{error}");
    }

    #[test]
    fn rejects_array_type_form() {
        let error = structural_reject("type: [string, \"null\"]");
        assert!(error.contains("array `type`"), "{error}");
        assert!(error.contains("oneOf"), "{error}");
    }

    #[test]
    fn rejects_standalone_null_type() {
        let error = structural_reject("type: \"null\"");
        assert!(error.contains("standalone `type: \"null\"`"), "{error}");
        assert!(error.contains("oneOf"), "{error}");
    }

    #[test]
    fn accepts_null_type_in_nullability_one_of() {
        // The one legal home for `type: "null"`: a nullability `oneOf` branch.
        parse(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  middleName:
    oneOf:
      - { type: string }
      - { type: "null" }
"#,
        );
    }

    #[test]
    fn accepts_scalar_const_and_enum() {
        parse(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  a: { type: integer, const: 3 }
  b: { type: boolean, const: true }
  c: { type: number, const: 3.14 }
  d: { type: string, enum: [active, inactive, pending] }
  e: { type: integer, enum: [1, 2, 3] }
  f: { type: number, enum: [1.5, 2.5] }
"#,
        );
    }

    #[test]
    fn rejects_const_and_enum_together() {
        let error = const_enum_reject("type: string\nconst: a\nenum: [a, b]");
        assert!(error.contains("mutually exclusive"), "{error}");
    }

    #[test]
    fn rejects_default_on_required_member() {
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
required: [value]
properties:
  value: { type: string, default: "x" }
"#;
        let error = parse_api_spec_from_json_schema_for_language(
            Language::Python,
            input,
            PathBuf::from("api.yaml"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("required member"), "{error}");
    }

    #[test]
    fn rejects_empty_title() {
        let error = structural_reject("type: string\ntitle: \"   \"");
        assert!(error.contains("`title` must not be empty"), "{error}");
    }

    #[test]
    fn rejects_multiline_title() {
        let error = structural_reject("type: string\ntitle: \"a\\nb\"");
        assert!(error.contains("single line"), "{error}");
    }

    #[test]
    fn rejects_non_boolean_deprecated() {
        let error = structural_reject("type: string\ndeprecated: \"true\"");
        assert!(error.contains("`deprecated` must be a boolean"), "{error}");
    }

    #[test]
    fn rejects_non_string_comment() {
        let error = structural_reject("type: string\n$comment: 42");
        assert!(error.contains("`$comment` must be a string"), "{error}");
    }

    #[test]
    fn accepts_annotations() {
        // title/description/deprecated:true, deprecated:false, examples, $comment
        // all load; examples/$comment are inert; deprecated:false is inert.
        parse(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  a:
    type: string
    title: Label
    description: A described member.
    deprecated: true
    examples: ["x", "y"]
    $comment: internal note
  b:
    type: string
    deprecated: false
"#,
        );
    }

    #[test]
    fn rejects_null_default() {
        let error = structural_reject("type: string\ndefault: null");
        assert!(error.contains("`default: null`"), "{error}");
    }

    #[test]
    fn rejects_object_default() {
        let error = structural_reject("type: object\ndefault: { a: 1 }");
        assert!(error.contains("object/array"), "{error}");
    }

    #[test]
    fn rejects_array_default() {
        let error = structural_reject("type: array\nitems: { type: string }\ndefault: [a]");
        assert!(error.contains("object/array"), "{error}");
    }

    #[test]
    fn rejects_type_incompatible_default() {
        let error = structural_reject("type: string\ndefault: 42");
        assert!(error.contains("incompatible"), "{error}");
    }

    #[test]
    fn accepts_scalar_defaults_of_each_kind() {
        parse(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  s: { type: string, default: "anon" }
  i: { type: integer, default: 0 }
  n: { type: number, default: 1.5 }
  b: { type: boolean, default: false }
"#,
        );
    }

    #[test]
    fn rejects_const_null() {
        let error = const_enum_reject("type: \"null\"\nconst: null");
        assert!(error.contains("null"), "{error}");
    }

    #[test]
    fn rejects_composite_const() {
        let error = const_enum_reject("type: object\nconst: { a: 1 }");
        assert!(error.contains("composite"), "{error}");
    }

    #[test]
    fn rejects_empty_enum() {
        let error = const_enum_reject("type: string\nenum: []");
        assert!(error.contains("must not be empty"), "{error}");
    }

    #[test]
    fn rejects_mixed_type_enum() {
        let error = const_enum_reject("type: string\nenum: [a, 1, true]");
        assert!(error.contains("incompatible"), "{error}");
    }

    #[test]
    fn rejects_type_incompatible_const() {
        let error = const_enum_reject("type: integer\nconst: x");
        assert!(error.contains("incompatible"), "{error}");
    }

    #[test]
    fn rejects_encoded_name_collision_enum() {
        // `user-admin` and `user_admin` are distinct on the wire but both encode
        // to the identifier `UserAdmin` (P15 collision).
        let error = const_enum_reject("type: string\nenum: [user-admin, user_admin]");
        assert!(error.contains("collision"), "{error}");
    }

    #[test]
    fn rejects_unencodable_const_value() {
        let error = const_enum_reject("type: string\nconst: \"-\"");
        assert!(error.contains("legal identifier"), "{error}");
    }

    #[test]
    fn rejects_const_with_default() {
        let error = const_enum_reject("type: string\nconst: a\ndefault: a");
        assert!(error.contains("mutually exclusive"), "{error}");
    }

    #[test]
    fn rejects_duplicate_enum_members() {
        let error = const_enum_reject("type: string\nenum: [a, a]");
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn rejects_enum_default_not_in_set() {
        let error = const_enum_reject("type: string\nenum: [a, b]\ndefault: c");
        assert!(error.contains("not a member"), "{error}");
    }

    #[test]
    fn rejects_non_ascii_const() {
        let error = const_enum_reject("type: string\nconst: \"café\"");
        assert!(error.contains("must be ASCII"), "{error}");
    }

    #[test]
    fn rejects_whitespace_const() {
        let error = const_enum_reject("type: string\nconst: \"user admin\"");
        assert!(error.contains("must not contain whitespace"), "{error}");
    }

    #[test]
    fn rejects_null_enum_member() {
        let error = const_enum_reject("type: string\nenum: [a, null]");
        assert!(error.contains("`enum: null`"), "{error}");
    }

    #[test]
    fn rejects_composite_enum_member() {
        let error = const_enum_reject("type: object\nenum: [{ a: 1 }]");
        assert!(error.contains("composite"), "{error}");
    }

    #[test]
    fn rejects_unencodable_enum_member() {
        let error = const_enum_reject("type: string\nenum: [\"-\", x]");
        assert!(error.contains("legal identifier"), "{error}");
    }

    // ---- `allOf` load-time merge (specs/json-schema/features/allOf.md) ----

    /// Parses `input` and returns the merged JSON schema value of the named
    /// generated model (a `$defs` entry or the document root).
    fn model_schema(input: &str, name: &str) -> Value {
        let spec = parse(input);
        let binding = spec
            .external_type_binding(name)
            .unwrap_or_else(|| panic!("no external type binding `{name}`"));
        match &binding.external_type {
            ExternalTypeSpec::Json(model) => model.schema.clone(),
            other => panic!("binding `{name}` is not a JSON model: {other:?}"),
        }
    }

    #[test]
    fn all_of_object_base_extension_merges_union() {
        let schema = model_schema(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  widget: { $ref: "#/$defs/Widget" }
$defs:
  Base:
    type: object
    required: [id]
    properties:
      id: { type: string }
  Widget:
    allOf:
      - { $ref: "#/$defs/Base" }
      - type: object
        required: [name]
        properties:
          name: { type: string }
"##,
            "Widget",
        );
        // Union of properties (Base.id + own.name), copied — not referenced.
        assert_eq!(schema["properties"]["id"]["type"], "string");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        // Union of required.
        assert_eq!(schema["required"], serde_json::json!(["id", "name"]));
        // No combinator / ref residue.
        assert!(schema.get("allOf").is_none());
        assert!(schema["$ref"].is_null());
    }

    #[test]
    fn all_of_tightens_same_axis_numeric_bound() {
        let schema = model_schema(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  n:
    allOf:
      - { type: integer, minimum: 3 }
      - { type: integer, minimum: 4 }
"##,
            "Api",
        );
        // The greater floor wins.
        assert_eq!(schema["properties"]["n"]["minimum"], 4);
    }

    #[test]
    fn all_of_tightens_across_inclusive_exclusive() {
        let schema = model_schema(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  n:
    allOf:
      - { type: integer, maximum: 10 }
      - { type: integer, exclusiveMaximum: 8 }
"##,
            "Api",
        );
        // `exclusiveMaximum: 8` dominates `maximum: 10`; the inclusive bound is
        // dropped so only one upper bound survives.
        assert_eq!(schema["properties"]["n"]["exclusiveMaximum"], 8);
        assert!(schema["properties"]["n"].get("maximum").is_none());
    }

    #[test]
    fn all_of_multiple_of_merges_to_lcm() {
        let schema = model_schema(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  n:
    allOf:
      - { type: integer, multipleOf: 2 }
      - { type: integer, multipleOf: 3 }
"##,
            "Api",
        );
        assert_eq!(schema["properties"]["n"]["multipleOf"], 6);
    }

    #[test]
    fn all_of_enum_intersects() {
        let schema = model_schema(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  s:
    allOf:
      - { type: string, enum: [a, b, c] }
      - { type: string, enum: [b, c, d] }
"##,
            "Api",
        );
        assert_eq!(
            schema["properties"]["s"]["enum"],
            serde_json::json!(["b", "c"])
        );
    }

    #[test]
    fn all_of_closed_base_closes_to_union() {
        let schema = model_schema(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  merged:
    allOf:
      - type: object
        additionalProperties: false
        properties:
          a: { type: string }
      - type: object
        properties:
          b: { type: string }
"##,
            "Api",
        );
        let merged = &schema["properties"]["merged"];
        // Closed against the union of declared properties (footgun-fix).
        assert_eq!(merged["additionalProperties"], false);
        assert_eq!(merged["properties"]["a"]["type"], "string");
        assert_eq!(merged["properties"]["b"]["type"], "string");
    }

    #[test]
    fn all_of_rejects_disjoint_type() {
        let error = numeric_reject("allOf:\n  - { type: string }\n  - { type: number }");
        assert!(error.contains("disjoint types"), "{error}");
    }

    #[test]
    fn all_of_rejects_disagreeing_const() {
        let error = numeric_reject(
            "allOf:\n  - { type: integer, const: 1 }\n  - { type: integer, const: 2 }",
        );
        assert!(error.contains("conflicting `const`"), "{error}");
    }

    #[test]
    fn all_of_rejects_empty_enum_intersection() {
        let error = numeric_reject(
            "allOf:\n  - { type: string, enum: [a, b] }\n  - { type: string, enum: [c, d] }",
        );
        assert!(error.contains("empty `enum` intersection"), "{error}");
    }

    #[test]
    fn all_of_rejects_false_branch() {
        let error = numeric_reject("allOf:\n  - { type: object }\n  - false");
        assert!(error.contains("`false`"), "{error}");
    }

    #[test]
    fn all_of_rejects_combinator_branch() {
        let error = numeric_reject(
            "allOf:\n  - { type: object }\n  - oneOf: [ { type: string }, { type: \"null\" } ]",
        );
        assert!(error.contains("cannot be a `oneOf`"), "{error}");
    }

    #[test]
    fn all_of_rejects_empty_array() {
        let error = numeric_reject("allOf: []");
        assert!(error.contains("must not be empty"), "{error}");
    }

    #[test]
    fn all_of_rejects_single_branch_wrapper() {
        let error = numeric_reject("allOf:\n  - { type: string }");
        assert!(error.contains("single-branch"), "{error}");
    }

    #[test]
    fn all_of_rejects_empty_numeric_interval_after_merge() {
        // The merged interval is empty; the reject is delegated to the numeric
        // validator on the merged schema.
        let error = numeric_reject(
            "allOf:\n  - { type: integer, minimum: 10 }\n  - { type: integer, maximum: 5 }",
        );
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_all_of_combinator_branch_not() {
        let error = numeric_reject("allOf:\n  - { type: object }\n  - { not: { type: integer } }");
        assert!(error.contains("cannot be `not`"), "{error}");
    }

    #[test]
    fn rejects_all_of_differing_format() {
        let error = numeric_reject(
            "allOf:\n  - { type: string, format: email }\n  - { type: string, format: uri }",
        );
        assert!(error.contains("different `format`s"), "{error}");
    }

    #[test]
    fn rejects_all_of_distinct_patterns() {
        let error = numeric_reject(
            "allOf:\n  - { type: string, pattern: \"^a\" }\n  - { type: string, pattern: \"z$\" }",
        );
        assert!(error.contains("different `pattern`s"), "{error}");
    }

    #[test]
    fn rejects_all_of_conflicting_const_enum() {
        let error = numeric_reject(
            "allOf:\n  - { type: integer, const: 5 }\n  - { type: integer, enum: [1, 2] }",
        );
        assert!(
            error.contains("not a member of the merged `enum`"),
            "{error}"
        );
    }

    #[test]
    fn rejects_all_of_unresolvable_ref_branch() {
        let error = numeric_reject(
            "allOf:\n  - { $ref: \"#/$defs/Missing\" }\n  - { type: object, properties: {} }",
        );
        assert!(
            error.contains("does not resolve to a known JSON model"),
            "{error}"
        );
    }

    #[test]
    fn all_of_rejects_cyclic_ref() {
        let error = parse_api_spec_from_json_schema_for_language(
            Language::Python,
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  loop: { $ref: "#/$defs/Loop" }
$defs:
  Loop:
    allOf:
      - { $ref: "#/$defs/Loop" }
      - type: object
        properties:
          x: { type: string }
"##,
            PathBuf::from("api.yaml"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cycle"), "{error}");
    }

    // --- `oneOf` sum types (specs/json-schema/features/oneOf.md) ---

    fn union_doc_result(doc: &str) -> Result<ApiSpec> {
        parse_api_spec_from_json_schema_for_language(
            Language::Python,
            doc,
            PathBuf::from("api.yaml"),
        )
    }

    fn union_reject(doc: &str) -> String {
        union_doc_result(doc).unwrap_err().to_string()
    }

    #[test]
    fn accepts_disjoint_kind_union_field() {
        let spec = union_doc_result(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string }
      - { type: integer }
"#,
        )
        .expect("disjoint-kind union should load");
        let root = spec.external_type_binding("Api").expect("root model");
        let ExternalTypeSpec::Json(json) = &root.external_type else {
            panic!("root should be a JSON model");
        };
        assert!(json.schema["properties"]["value"]["oneOf"].is_array());
    }

    #[test]
    fn accepts_discriminated_object_union_def() {
        union_doc_result(
            r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  shape: { $ref: "#/$defs/Shape" }
$defs:
  Circle:
    type: object
    required: [kind, radius]
    properties:
      kind: { type: string, const: circle }
      radius: { type: number }
  Square:
    type: object
    required: [kind, side]
    properties:
      kind: { type: string, const: square }
      side: { type: number }
  Shape:
    oneOf:
      - { $ref: "#/$defs/Circle" }
      - { $ref: "#/$defs/Square" }
"##,
        )
        .expect("discriminated object union should load");
    }

    #[test]
    fn two_branch_nullable_stays_a_plain_nullable_field() {
        let spec = union_doc_result(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  middleName:
    oneOf:
      - { type: string }
      - { type: "null" }
"#,
        )
        .expect("nullable oneOf should load");
        let root = spec.external_type_binding("Api").expect("root model");
        let ExternalTypeSpec::Json(json) = &root.external_type else {
            panic!("root should be a JSON model");
        };
        // The degenerate two-branch pattern is preserved as-is (owned by
        // nullability), not rewritten into a sum type.
        let branches = json.schema["properties"]["middleName"]["oneOf"]
            .as_array()
            .expect("nullable oneOf branches");
        assert_eq!(branches.len(), 2);
    }

    #[test]
    fn rejects_single_branch_one_of() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string }
"#,
        );
        assert!(error.contains("single-branch"), "{error}");
    }

    #[test]
    fn rejects_integer_number_overlap_union() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: integer }
      - { type: number }
"#,
        );
        assert!(
            error.contains("integer") && error.contains("number"),
            "{error}"
        );
    }

    #[test]
    fn rejects_non_separable_overlapping_object_union() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: object, properties: { a: { type: string } } }
      - { type: object, properties: { b: { type: string } } }
"#,
        );
        assert!(error.contains("discriminator"), "{error}");
    }

    #[test]
    fn rejects_non_const_discriminator_union() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - type: object
        required: [kind]
        properties: { kind: { type: string, const: a } }
      - type: object
        required: [kind]
        properties: { kind: { type: string } }
"#,
        );
        assert!(error.contains("discriminator"), "{error}");
    }

    #[test]
    fn rejects_non_unique_discriminator_union() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - type: object
        required: [kind]
        properties: { kind: { type: string, const: same }, a: { type: string } }
      - type: object
        required: [kind]
        properties: { kind: { type: string, const: same }, b: { type: string } }
"#,
        );
        assert!(error.contains("discriminator"), "{error}");
    }

    #[test]
    fn rejects_two_string_branch_union() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string, minLength: 1 }
      - { type: string, maxLength: 5 }
"#,
        );
        assert!(error.contains("enum"), "{error}");
    }

    #[test]
    fn rejects_empty_one_of() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf: []
"#,
        );
        assert!(
            error.contains("non-empty") || error.contains("single-branch"),
            "{error}"
        );
    }

    #[test]
    fn rejects_typeless_branch_union() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string }
      - { description: "no type" }
"#,
        );
        assert!(error.contains("classifiable"), "{error}");
    }

    #[test]
    fn rejects_one_of_nested_one_of() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string }
      - oneOf:
          - { type: integer }
          - { type: boolean }
"#,
        );
        assert!(error.contains("cannot itself be a `oneOf`"), "{error}");
    }

    #[test]
    fn rejects_one_of_two_array_branches() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: array, items: { type: string } }
      - { type: array, items: { type: integer } }
"#,
        );
        assert!(error.contains("no decidable selector"), "{error}");
    }

    #[test]
    fn rejects_one_of_duplicate_null_branches() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string }
      - { type: "null" }
      - { type: "null" }
"#,
        );
        assert!(error.contains("`null` kind"), "{error}");
    }

    #[test]
    fn rejects_one_of_ambiguous_discriminator() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - type: object
        required: [kind, variant]
        properties:
          kind: { type: string, const: a }
          variant: { type: string, const: x }
      - type: object
        required: [kind, variant]
        properties:
          kind: { type: string, const: b }
          variant: { type: string, const: y }
"#,
        );
        assert!(error.contains("more than one qualifying"), "{error}");
    }

    #[test]
    fn accepts_nullable_multi_kind_union() {
        union_doc_result(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string }
      - { type: array, items: { type: number } }
      - { type: "null" }
"#,
        )
        .expect("nullable multi-kind union should load");
    }

    #[test]
    fn rejects_null_only_two_branch_one_of() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: "null" }
      - { type: "null" }
"#,
        );
        assert!(error.contains("`null` kind"), "{error}");
    }

    // ----- P15 identifier namespace + `x-<lang>-name` override -----

    fn parse_for(language: Language, input: &str) -> Result<ApiSpec> {
        parse_api_spec_from_json_schema_for_language(language, input, PathBuf::from("api.yaml"))
    }

    fn reject_for(language: Language, input: &str) -> String {
        parse_for(language, input).unwrap_err().to_string()
    }

    #[test]
    fn member_override_accepts_and_is_recognized_as_extension() {
        // `x-<lang>-name` on a member is a recognized generator extension: the
        // loader accepts it (not rejected as unknown) for every target.
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  legacyId:
    type: string
    x-go-name: LegacyID
    x-ts-name: legacyID
    x-py-name: legacy_ident
    x-java-name: legacyID
"#;
        for language in [
            Language::Go,
            Language::TypeScript,
            Language::Python,
            Language::Java,
        ] {
            parse_for(language, input)
                .unwrap_or_else(|error| panic!("{language:?} should accept override: {error}"));
        }
    }

    #[test]
    fn rejects_member_name_collision_after_recasing() {
        // `user_id` and `userId` both recase to Go `UserId` — a member collision.
        let error = reject_for(
            Language::Go,
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  user_id: { type: string }
  userId: { type: string }
"#,
        );
        assert!(
            error.contains("collision") && error.contains("UserId"),
            "{error}"
        );
    }

    #[test]
    fn member_collision_resolved_by_override() {
        // The same clash is admitted once one member carries an `x-go-name`
        // override — and the check is per target (Python would still collide, so
        // this is asserted for Go alone).
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  user_id: { type: string }
  userId: { type: string, x-go-name: UserIdent }
"#;
        parse_for(Language::Go, input).expect("override resolves the Go collision");
    }

    #[test]
    fn rejects_invalid_and_reserved_overrides() {
        // A leading-digit override is not a legal identifier.
        let error = reject_for(
            Language::Go,
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  code: { type: string, x-go-name: "2fa" }
"#,
        );
        assert!(
            error.contains("x-go-name") && error.contains("legal"),
            "{error}"
        );

        // A reserved-word override is rejected.
        let error = reject_for(
            Language::Python,
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  class_field: { type: string, x-py-name: "class" }
"#,
        );
        assert!(error.contains("x-py-name"), "{error}");
    }

    #[test]
    fn rejects_synthesized_closed_type_colliding_with_declared_type_go() {
        // `Palette.color` (enum) synthesizes the Go defined type `PaletteColor`,
        // which collides with the declared `$defs/PaletteColor` — a package-scope
        // clash caught only for Go (Python closes the enum inline).
        let input = r##"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  palette: { $ref: "#/$defs/Palette" }
  pc: { $ref: "#/$defs/PaletteColor" }
$defs:
  Palette:
    type: object
    properties:
      color: { type: string, enum: [red, green] }
  PaletteColor:
    type: object
    properties:
      label: { type: string }
"##;
        let error = reject_for(Language::Go, input);
        assert!(
            error.contains("collision") && error.contains("PaletteColor"),
            "{error}"
        );
        // Python synthesizes no defined type, so the same schema is accepted.
        parse_for(Language::Python, input).expect("Python has no such synthesized type");
    }

    #[test]
    fn rejects_or_default_accessor_colliding_with_member_go() {
        // The Go `<Field>OrDefault()` accessor shares the struct method-set: a
        // sibling member that recases to `FooOrDefault` is a field/method clash.
        let input = r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  foo: { type: string, default: "x" }
  fooOrDefault: { type: string }
"#;
        let error = reject_for(Language::Go, input);
        assert!(
            error.contains("collision") && error.contains("FooOrDefault"),
            "{error}"
        );
        // Python surfaces the default natively (no accessor), so no clash.
        parse_for(Language::Python, input).expect("Python has no OrDefault accessor");
    }

    #[test]
    fn rejects_synthesized_operation_input_colliding_with_defs_type() {
        // The synthesized `<Op>Input` type collides with a declared `$defs` type
        // of the same name (top-level module scope, every target).
        let error = reject_for(
            Language::Python,
            r#"
nexusrpc: "1.0.0"
services:
  Chat:
    operations:
      create:
        input:
          type: object
          properties: { a: { type: string } }
$defs:
  CreateInput:
    type: object
    properties: { b: { type: string } }
"#,
        );
        assert!(
            error.contains("collision") && error.contains("CreateInput"),
            "{error}"
        );
    }

    #[test]
    fn rejects_service_colliding_with_model() {
        // A service binding name collides with a declared model type.
        let error = reject_for(
            Language::Python,
            r#"
nexusrpc: "1.0.0"
services:
  Widget:
    operations:
      ping:
        input:
          type: object
          properties: { a: { type: string } }
$defs:
  Widget:
    type: object
    properties: { b: { type: string } }
"#,
        );
        assert!(
            error.contains("collision") && error.contains("Widget"),
            "{error}"
        );
    }

    // --- `required` load-time validation (specs/json-schema/features/required.md) ---

    #[test]
    fn rejects_required_not_array() {
        let error =
            numeric_reject("type: object\nproperties:\n  a: { type: string }\nrequired: id");
        assert!(error.contains("must be an array"), "{error}");
    }

    #[test]
    fn rejects_required_non_string_element() {
        let error =
            numeric_reject("type: object\nproperties:\n  id: { type: string }\nrequired: [1]");
        assert!(error.contains("only property-name strings"), "{error}");
    }

    #[test]
    fn rejects_required_duplicate() {
        let error =
            numeric_reject("type: object\nproperties:\n  id: { type: string }\nrequired: [id, id]");
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn rejects_required_name_not_in_properties() {
        let error =
            numeric_reject("type: object\nproperties:\n  id: { type: string }\nrequired: [name]");
        assert!(error.contains("not declared in `properties`"), "{error}");
    }

    // --- `type` presence / shape (validate_type_presence) ---

    #[test]
    fn rejects_missing_type_on_leaf() {
        let error = numeric_reject("description: hi");
        assert!(
            error.contains("a leaf schema requires an explicit `type`"),
            "{error}"
        );
    }

    #[test]
    fn rejects_unknown_type_name() {
        let error = numeric_reject("type: foobar");
        assert!(error.contains("unknown `type`"), "{error}");
    }

    #[test]
    fn rejects_object_without_shape() {
        let error = numeric_reject("type: object");
        assert!(error.contains("needs an explicit shape"), "{error}");
    }

    #[test]
    fn rejects_array_without_items() {
        let error = numeric_reject("type: array");
        assert!(error.contains("needs an explicit element type"), "{error}");
    }

    // --- coverage: loader-time rejects found reachable-but-untested ---

    #[test]
    fn rejects_contains_scalar_matcher_over_composite_element() {
        // The element type is a valid (empty) object, so the array itself loads;
        // it is the `contains` matcher over a composite element that is deferred.
        let error = numeric_reject(
            "type: array\nitems: { type: object, properties: {} }\ncontains: { const: x }",
        );
        assert!(
            error.contains("`contains` over a composite element type"),
            "{error}"
        );
    }

    #[test]
    fn rejects_unknown_branch_type() {
        let error = union_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: object, properties: { k: { type: string } }, required: [k] }
      - { type: qux }
"#,
        );
        assert!(error.contains("unrecognized `type: qux`"), "{error}");
    }

    #[test]
    fn rejects_non_array_enum() {
        let error = numeric_reject("type: string\nenum: 5");
        assert!(error.contains("`enum` must be an array"), "{error}");
    }

    #[test]
    fn rejects_all_of_differing_contains() {
        let error = numeric_reject(
            "allOf:\n  - { type: array, contains: { const: 1 } }\n  - { type: array, contains: { const: 2 } }",
        );
        assert!(error.contains("different `contains` matchers"), "{error}");
    }

    #[test]
    fn rejects_all_of_entry_not_a_schema() {
        let error = numeric_reject(
            "allOf:\n  - { type: object, properties: { a: { type: string } } }\n  - 5",
        );
        assert!(error.contains("must be a schema object"), "{error}");
    }

    #[test]
    fn rejects_all_of_merges_to_empty() {
        let error = numeric_reject("allOf: [true, true]");
        assert!(error.contains("empty schema"), "{error}");
    }

    #[test]
    fn rejects_exclusive_empty_integer_interval() {
        let error = numeric_reject("type: integer\nexclusiveMinimum: 1\nexclusiveMaximum: 2");
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_exclusive_boundary_empty_interval() {
        let error = numeric_reject("type: number\nminimum: 5\nexclusiveMaximum: 5");
        assert!(error.contains("empty range"), "{error}");
    }

    #[test]
    fn rejects_shapeless_array_element() {
        let error = numeric_reject("type: array\nitems: {}");
        assert!(
            error.contains("a leaf schema requires an explicit `type`"),
            "{error}"
        );
    }

    #[test]
    fn rejects_out_of_subset_array_element() {
        let error = numeric_reject("type: array\nitems: { type: object }");
        assert!(error.contains("needs an explicit shape"), "{error}");
    }

    #[test]
    fn rejects_tuple_items() {
        // A tuple-form `items` (an array of schemas) does not deserialize into the
        // single-schema `items` slot, so it fails at parse time.
        let error = numeric_reject("type: array\nitems: [ { type: string } ]");
        assert!(error.contains("failed to parse JSON schema"), "{error}");
    }

    #[test]
    fn rejects_non_schema_items() {
        let error = numeric_reject("type: array\nitems: 5");
        assert!(error.contains("failed to parse JSON schema"), "{error}");
    }

    #[test]
    fn rejects_non_string_title() {
        let error = numeric_reject("type: string\ntitle: 42");
        assert!(error.contains("failed to parse JSON schema"), "{error}");
    }

    #[test]
    fn rejects_non_string_description() {
        let error = numeric_reject("type: string\ndescription: 42");
        assert!(error.contains("failed to parse JSON schema"), "{error}");
    }

    #[test]
    fn rejects_non_object_properties() {
        let error = numeric_reject("type: object\nproperties: []");
        assert!(error.contains("failed to parse JSON schema"), "{error}");
    }

    #[test]
    fn rejects_non_string_x_lang_name_override() {
        let error = reject_for(
            Language::Go,
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  code: { type: string, x-go-name: 42 }
"#,
        );
        assert!(
            error.contains("`x-go-name` must be a string identifier"),
            "{error}"
        );
    }

    #[test]
    fn rejects_reserved_module_name() {
        // A source file whose relative path strips to a reserved module segment
        // (`models`) collides with a generated file name.
        let sources = vec![
            module_collision_source("models.yaml", "Models"),
            module_collision_source("other.yaml", "Other"),
        ];
        let error = api_spec_tree_from_json_schema_sources(Language::Python, sources)
            .expect_err("a source mapping to a reserved module name should be rejected")
            .to_string();
        assert!(error.contains("reserved module name"), "{error}");
    }

    #[test]
    fn rejects_object_keyword_on_scalar() {
        let error = numeric_reject("type: string\nproperties:\n  a: { type: string }");
        assert!(error.contains("require `type: object`"), "{error}");
    }

    #[test]
    fn rejects_items_on_scalar() {
        let error = numeric_reject("type: string\nitems: { type: string }");
        assert!(error.contains("`items` requires `type: array`"), "{error}");
    }

    #[test]
    fn accepts_empty_properties_object() {
        numeric_accept("type: object\nproperties: {}");
    }

    // --- `additionalProperties` value shape (validate_schema_node) ---

    #[test]
    fn rejects_non_schema_additional_properties() {
        let error = numeric_reject("type: object\nadditionalProperties: \"yes\"");
        assert!(
            error.contains("must be `true`, `false`, or a schema object"),
            "{error}"
        );
    }

    #[test]
    fn rejects_empty_object_additional_properties() {
        let error = numeric_reject("type: object\nadditionalProperties: {}");
        assert!(
            error.contains("write `additionalProperties: true` instead"),
            "{error}"
        );
    }

    // --- `enum` vs numeric bound (numeric literal loop) ---

    #[test]
    fn rejects_enum_violating_numeric_bound() {
        let error = numeric_reject("type: integer\nmaximum: 5\nenum: [1, 7]");
        assert!(
            error.contains("`enum` value 7 violates the numeric bounds"),
            "{error}"
        );
    }

    // --- `description` annotation (validate_annotations) ---

    #[test]
    fn rejects_empty_description() {
        let error = numeric_reject("type: string\ndescription: \"\"");
        assert!(error.contains("`description` must not be empty"), "{error}");
    }

    #[test]
    fn rejects_whitespace_description() {
        let error = numeric_reject("type: string\ndescription: \"   \"");
        assert!(error.contains("`description` must not be empty"), "{error}");
    }

    // --- operation I/O must resolve to an object (require_object_io) ---

    #[test]
    fn rejects_ref_union_operation_io() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      pick:
        input: { $ref: "#/$defs/Thing" }
$defs:
  Thing:
    oneOf:
      - type: object
        properties: { kind: { type: string, const: a } }
        required: [kind]
      - type: object
        properties: { kind: { type: string, const: b } }
        required: [kind]
"##,
        );
        assert!(error.contains("must resolve to an object"), "{error}");
    }

    #[test]
    fn rejects_inline_union_operation_io() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      pick:
        input:
          oneOf:
            - { type: string }
            - { type: integer }
"##,
        );
        assert!(error.contains("must resolve to an object"), "{error}");
    }

    // --- service / operation names (name_matches) ---

    #[test]
    fn rejects_invalid_service_name() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  chatService:
    operations:
      ping: {}
"##,
        );
        assert!(error.contains("must match `^[A-Z]"), "{error}");
    }

    #[test]
    fn rejects_invalid_operation_name() {
        let error = doc_reject(
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      PollMessages: {}
"##,
        );
        assert!(error.contains("must match `^[a-z]"), "{error}");
    }

    // --- reserved / invalid member identifiers (validate_member_scope) ---

    #[test]
    fn rejects_reserved_member_without_override() {
        let error = doc_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  class: { type: string }
"#,
        );
        assert!(error.contains("is a reserved word"), "{error}");
    }

    #[test]
    fn rejects_invalid_member_identifier() {
        let error = doc_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  "2fa": { type: string }
"#,
        );
        assert!(error.contains("is not a valid identifier"), "{error}");
    }

    // --- `not` per-form diagnostics (validate_schema_common) ---

    #[test]
    fn rejects_not_empty_unsatisfiable() {
        let error = numeric_reject("not: {}");
        assert!(error.contains("unsatisfiable"), "{error}");
    }

    #[test]
    fn rejects_not_true_unsatisfiable() {
        let error = numeric_reject("not: true");
        assert!(error.contains("unsatisfiable"), "{error}");
    }

    #[test]
    fn rejects_not_false_noop() {
        let error = numeric_reject("not: false");
        assert!(error.contains("no-op"), "{error}");
    }

    #[test]
    fn rejects_not_double_negation() {
        let error = numeric_reject("not: { not: { type: string } }");
        assert!(error.contains("not supported"), "{error}");
    }

    // --- unsatisfiable recursion cycles (validate_reference_satisfiability) ---

    #[test]
    fn rejects_unsatisfiable_self_reference() {
        let error = doc_reject(
            r##"
$defs:
  Node:
    type: object
    properties:
      next: { $ref: "#/$defs/Node" }
    required: [next]
"##,
        );
        assert!(error.contains("unsatisfiable recursion cycle"), "{error}");
    }

    #[test]
    fn rejects_unsatisfiable_mutual_recursion() {
        let error = doc_reject(
            r##"
$defs:
  A:
    type: object
    properties:
      b: { $ref: "#/$defs/B" }
    required: [b]
  B:
    type: object
    properties:
      a: { $ref: "#/$defs/A" }
    required: [a]
"##,
        );
        assert!(error.contains("unsatisfiable recursion cycle"), "{error}");
    }

    #[test]
    fn accepts_array_wrapped_recursion() {
        parse(
            r##"
$defs:
  Tree:
    type: object
    properties:
      children:
        type: array
        items: { $ref: "#/$defs/Tree" }
    required: [children]
"##,
        );
    }

    #[test]
    fn accepts_optional_recursion() {
        parse(
            r##"
$defs:
  Node:
    type: object
    properties:
      next: { $ref: "#/$defs/Node" }
"##,
        );
    }

    // --- catch-all collision (validate_member_scope) ---

    #[test]
    fn rejects_member_colliding_with_catch_all() {
        let error = doc_reject(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  additionalProperties: { type: string }
"#,
        );
        assert!(error.contains("catch-all"), "{error}");
        assert!(error.contains("collision"), "{error}");
    }

    #[test]
    fn accepts_additional_properties_member_when_closed() {
        parse(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
type: object
additionalProperties: false
properties:
  additionalProperties: { type: string }
"#,
        );
    }

    // --- definitions-only file (validate_document) ---

    #[test]
    fn accepts_definitions_only_file() {
        let spec = parse(
            r#"
$schema: https://json-schema.org/draft/2020-12/schema
description: A definitions bucket.
$defs:
  Thing:
    type: object
    properties:
      id: { type: string }
"#,
        );
        assert!(spec.external_type_binding("Thing").is_some());
    }

    // --- cross-file `$ref` target must be in the input set (resolve_ref_key) ---

    #[test]
    fn rejects_ref_target_file_not_in_input_set() {
        let error = numeric_reject("$ref: \"missing.yaml#/$defs/X\"");
        assert!(error.contains("not in the input set"), "{error}");
    }
}
