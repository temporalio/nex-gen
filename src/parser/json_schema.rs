use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::language::Language;
use crate::spec::{
    ApiRef, ApiSpec, ExternalTypeBindingSpec, ExternalTypeSpec, JsonModelSpec, LanguageStringSpec,
    OperationSpec, ServiceSpec, SupportSpec, TypeSpec,
};

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
    model_name: String,
    schema: Schema,
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

#[cfg(test)]
pub(crate) fn parse_api_spec_from_json_schema_for_language(
    language: Language,
    input: &str,
    path: PathBuf,
) -> Result<ApiSpec> {
    api_spec_from_json_schema_sources(language, vec![(path, input.to_string())])
}

fn api_spec_from_json_schema_sources(
    _language: Language,
    sources: Vec<(PathBuf, String)>,
) -> Result<ApiSpec> {
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
                    model_name,
                    schema: doc.root.clone(),
                },
            );
        }
    }

    validate_model_refs(&docs, &models)?;

    let mut external_types = BTreeMap::new();
    for model in models.values() {
        insert_json_external_type(&mut external_types, model)?;
    }

    let mut services = Vec::new();
    for (canonical_path, (path, doc)) in &docs {
        let Some(service_specs) = &doc.services else {
            continue;
        };
        for (service_key, service) in service_specs {
            services.push(build_service(
                path,
                canonical_path,
                service_key,
                service,
                &docs,
                &models,
                &mut external_types,
            )?);
        }
    }

    Ok(ApiSpec {
        version: "0.0.0".to_string(),
        support: SupportSpec::default(),
        services,
        external_types,
        records: BTreeMap::new(),
        enums: BTreeMap::new(),
        flags: BTreeMap::new(),
        variants: BTreeMap::new(),
    })
}

fn validate_document(path: &Path, doc: &Document) -> Result<()> {
    if doc.nexusrpc.as_ref().and_then(Value::as_str) != Some("1.0.0") {
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
    if root_is_schema_shaped(&doc.root) {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: "a Nexus JSON schema document root is an envelope, not a model; move the model into `$defs`"
                .to_string(),
        });
    }
    Ok(())
}

fn validate_model_schema(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    validate_schema_common(path, schema, context)?;
    if schema.reference.is_some() {
        return Ok(());
    }
    if schema.ty.as_ref().and_then(Value::as_str) != Some("object") {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("{context} must be `type: object` or a bare `$ref`"),
        });
    }
    validate_schema_tree(path, schema, context)
}

fn validate_schema_tree(path: &Path, schema: &Schema, context: &str) -> Result<()> {
    validate_schema_common(path, schema, context)?;
    if let Some(properties) = &schema.properties {
        for (name, property) in properties {
            validate_schema_tree(path, property, &format!("{context}.properties.{name}"))?;
        }
    }
    if let Some(items) = &schema.items {
        validate_schema_tree(path, items, &format!("{context}.items"))?;
    }
    if let Some(one_of) = &schema.one_of {
        if one_of.len() != 2 {
            return Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `oneOf` is only supported for nullability"),
            });
        }
        let null_count = one_of
            .iter()
            .filter(|schema| schema.ty.as_ref().and_then(Value::as_str) == Some("null"))
            .count();
        if null_count != 1 {
            return Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `oneOf` is only supported for nullability"),
            });
        }
        for branch in one_of {
            validate_schema_tree(path, branch, &format!("{context}.oneOf"))?;
        }
    }
    if let Some(additional) = &schema.additional_properties
        && additional.is_object()
    {
        let additional_schema =
            serde_json::from_value::<Schema>(additional.clone()).map_err(|error| {
                Error::InvalidJsonSchema {
                    path: path.to_path_buf(),
                    reason: format!("{context}.additionalProperties is invalid: {error}"),
                }
            })?;
        validate_schema_tree(
            path,
            &additional_schema,
            &format!("{context}.additionalProperties"),
        )?;
    }
    Ok(())
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
    for keyword in [
        "allOf",
        "anyOf",
        "not",
        "if",
        "then",
        "else",
        "prefixItems",
        "unevaluatedProperties",
        "unevaluatedItems",
        "dependentSchemas",
        "contains",
        "maxContains",
        "minContains",
        "$anchor",
        "$dynamicRef",
        "$dynamicAnchor",
        "nullable",
    ] {
        if schema.extra.contains_key(keyword) {
            return Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("{context}: `{keyword}` is not supported"),
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

fn build_service(
    path: &Path,
    canonical_path: &Path,
    service_key: &str,
    service: &Service,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &BTreeMap<TypeKey, JsonModel>,
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<ServiceSpec> {
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
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<OperationSpec> {
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
        output_resource_type: None,
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
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
) -> Result<TypeSpec> {
    validate_schema_common(path, schema, &format!("operation {operation_key} {suffix}"))?;
    if let Some(reference) = &schema.reference {
        let model = resolve_ref(path, canonical_path, reference, docs, models)?;
        insert_json_external_type(external_types, model)?;
        return json_model_type(model);
    }

    validate_model_schema(path, schema, &format!("operation {operation_key} {suffix}"))?;
    let model_name = format!("{}{}", operation_key.to_upper_camel_case(), suffix);
    let model = JsonModel {
        full_name: format!("{service_name}.{model_name}"),
        model_name,
        schema: schema.clone(),
    };
    insert_json_external_type(external_types, &model)?;
    json_model_type(&model)
}

fn resolve_ref<'a>(
    path: &Path,
    canonical_path: &Path,
    reference: &str,
    docs: &IndexMap<PathBuf, (PathBuf, Document)>,
    models: &'a BTreeMap<TypeKey, JsonModel>,
) -> Result<&'a JsonModel> {
    let (file_part, pointer) = reference.split_once('#').unwrap_or((reference, ""));
    let target_path = if file_part.is_empty() {
        canonical_path.to_path_buf()
    } else {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let target = canonical(&base.join(file_part));
        if !docs.contains_key(&target) {
            return Err(Error::InvalidJsonSchema {
                path: path.to_path_buf(),
                reason: format!("`$ref` target file `{file_part}` is not in the input set"),
            });
        }
        target
    };

    let key = if pointer.is_empty() || pointer == "/" {
        TypeKey::Root(target_path)
    } else if let Some(name) = pointer.strip_prefix("/$defs/") {
        TypeKey::Def(target_path, name.replace("~1", "/").replace("~0", "~"))
    } else {
        return Err(Error::InvalidJsonSchema {
            path: path.to_path_buf(),
            reason: format!("`$ref` `{reference}` must point at a `$defs` entry or file root"),
        });
    };

    models.get(&key).ok_or_else(|| Error::InvalidJsonSchema {
        path: path.to_path_buf(),
        reason: format!("`$ref` `{reference}` does not resolve to a known JSON model"),
    })
}

fn insert_json_external_type(
    external_types: &mut BTreeMap<String, ExternalTypeBindingSpec>,
    model: &JsonModel,
) -> Result<()> {
    let type_spec = json_model_spec(model)?;
    external_types
        .entry(model.full_name.clone())
        .or_insert_with(|| ExternalTypeBindingSpec {
            external_type: ExternalTypeSpec::Json(type_spec),
            reference: LanguageStringSpec::default(),
            type_name: language_string(Some(model.model_name.clone())),
            replacement: None,
            authored_type: None,
        });
    Ok(())
}

fn json_model_type(model: &JsonModel) -> Result<TypeSpec> {
    Ok(TypeSpec::External(ExternalTypeSpec::Json(json_model_spec(
        model,
    )?)))
}

fn json_model_spec(model: &JsonModel) -> Result<JsonModelSpec<ApiRef>> {
    Ok(JsonModelSpec {
        name: ApiRef::new(model.full_name.clone()),
        model_name: model.model_name.clone(),
        schema: serde_json::to_value(&model.schema).map_err(|error| Error::InvalidJsonSchema {
            path: PathBuf::from("<json-schema>"),
            reason: format!(
                "failed to preserve JSON schema model `{}`: {error}",
                model.full_name
            ),
        })?,
    })
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
    path.file_stem()
        .map(|value| value.to_string_lossy().to_string())
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

        assert!(spec.records.is_empty());
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
        assert!(spec.external_types.contains_key("SendMessageInput"));
        assert!(spec.external_types.contains_key("SendMessageOutput"));
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

        assert!(spec.records.is_empty());
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
    fn rejects_ref_with_sibling_keywords() {
        let error = parse_api_spec_from_json_schema_for_language(
            Language::Python,
            r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    operations:
      getRoom:
        input:
          $ref: "#/$defs/GetRoomInput"
          description: not allowed
$defs:
  GetRoomInput:
    type: object
    properties: {}
"##,
            PathBuf::from("api.yaml"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("must not carry sibling"));
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
}
