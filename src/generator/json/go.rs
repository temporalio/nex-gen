use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::generator::ExternalModelBackend;
use crate::generator::go::{
    GoPackageContext, PlannedMessageSource, PlannedMessageType, PlannedValueType, go_field_name,
    go_package_name, go_string_literal,
};
use crate::planning::{PlannedJsonType, PlannedSpec, PlannedTypeFamily};
use crate::spec::{ExternalTypeSpec, OperationSpec, RecordSpec, TypeSpec};

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

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelFragments {
    pub(in crate::generator) imports: BTreeSet<String>,
    pub(in crate::generator) body: String,
}

#[derive(Debug)]
pub(in crate::generator) struct ModelBackend {
    package: GoPackageContext,
    include_service_imports: bool,
    shared_runtime: bool,
    json_models: Vec<PlannedJsonType>,
    local_json_models: Vec<PlannedJsonType>,
    model_names: BTreeMap<String, String>,
    imports: BTreeMap<String, String>,
}

impl ModelBackend {
    pub(in crate::generator) fn new(
        package: GoPackageContext,
        include_service_imports: bool,
    ) -> Self {
        Self {
            shared_runtime: package.shared_json_import_path().is_some(),
            package,
            include_service_imports,
            json_models: Vec::new(),
            local_json_models: Vec::new(),
            model_names: BTreeMap::new(),
            imports: BTreeMap::new(),
        }
    }
}

impl ExternalModelBackend<PlannedValueType> for ModelBackend {
    type ModelFragments = ModelFragments;
    type WireConversion = ();

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.json_models = api_plan
            .external_types()
            .map(|(_, binding)| binding)
            .filter_map(|binding| match &binding.external_type {
                ExternalTypeSpec::Json(json_type) => Some(json_type.clone()),
                _ => None,
            })
            .collect();
        self.local_json_models.clear();
        self.model_names.clear();
        self.imports.clear();

        let mut aliases = BTreeMap::<String, String>::new();
        for model in &self.json_models {
            let module_path = model.module_path.as_ref().unwrap_or(&api_plan.module_path);
            if let Some(import_path) = self.package.module_import_path(module_path)? {
                let alias = aliases
                    .entry(import_path.clone())
                    .or_insert_with(|| unique_import_alias(&import_path, &self.imports))
                    .clone();
                self.imports.insert(import_path, alias.clone());
                self.model_names.insert(
                    model.full_name.clone(),
                    format!("{alias}.{}", model.model_name),
                );
            } else {
                self.local_json_models.push(model.clone());
                self.model_names
                    .insert(model.full_name.clone(), model.model_name.clone());
            }
        }
        if self.include_service_imports && !api_plan.services.is_empty() {
            for (module_path, names) in &api_plan.data.module_imports {
                let Some(import_path) = self.package.module_import_path(module_path)? else {
                    continue;
                };
                let alias = imports_alias(&mut self.imports, &import_path);
                for name in names {
                    self.model_names.insert(
                        format!("{}#{name}", module_path.as_module_key()),
                        format!("{alias}.{name}"),
                    );
                    self.model_names.insert(
                        format!("{}#/$defs/{name}", module_path.as_module_key()),
                        format!("{alias}.{name}"),
                    );
                }
            }
        }
        if self.shared_runtime
            && !self.local_json_models.is_empty()
            && let Some(import_path) = self.package.shared_json_import_path()
        {
            self.imports.insert(import_path, "nexgenjson".to_string());
        }
        Ok(())
    }

    fn render_models(&self) -> Result<ModelFragments> {
        render_external_models(
            &self.local_json_models.iter().collect::<Vec<_>>(),
            &self.model_names,
            self.shared_runtime,
        )
    }

    fn render_support_files(&self) -> Result<BTreeMap<PathBuf, String>> {
        Ok(BTreeMap::new())
    }

    fn model_type_annotation(&self, model_type: &PlannedValueType) -> Option<String> {
        let PlannedValueType::Message(message) = model_type else {
            return None;
        };
        if message.source != PlannedMessageSource::Json {
            return None;
        }
        self.model_names
            .get(&message.info.full_name)
            .cloned()
            .or_else(|| Some(message.model_name.clone()))
    }

    fn wire_type_identifier(&self, model_type: &PlannedValueType) -> Option<String> {
        let PlannedValueType::Message(message) = model_type else {
            return None;
        };
        if message.source != PlannedMessageSource::Json {
            return None;
        }
        Some(message.info.full_name.clone())
    }

    fn wire_conversion(
        &self,
        _model_type: &PlannedValueType,
        _planned_record: Option<&RecordSpec<PlannedTypeFamily>>,
    ) -> Option<()> {
        None
    }
}

impl ModelBackend {
    pub(in crate::generator) fn is_active(&self) -> bool {
        !self.json_models.is_empty()
    }

    pub(in crate::generator) fn imports(&self) -> Vec<(String, String)> {
        self.imports
            .iter()
            .map(|(path, alias)| (path.clone(), alias.clone()))
            .collect()
    }

    fn json_model_type_annotation(&self, json_type: &PlannedJsonType) -> String {
        self.model_names
            .get(&json_type.full_name)
            .cloned()
            .unwrap_or_else(|| json_type.model_name.clone())
    }

    pub(in crate::generator) fn has_message_wire_type(&self, message: &PlannedMessageType) -> bool {
        message.source == PlannedMessageSource::Json
            && self.model_names.contains_key(&message.info.full_name)
    }

    pub(in crate::generator) fn render_services(
        &self,
        api_plan: &PlannedSpec,
        package: &GoPackageContext,
    ) -> Result<String> {
        if api_plan.services.is_empty() {
            return Ok(String::new());
        }
        let mut output = String::new();
        for service in &api_plan.services {
            render_go_doc_comment(
                &mut output,
                "",
                service.doc.for_language(crate::language::Language::Go),
            );
            output.push_str("var ");
            output.push_str(&go_field_name(&service.name));
            output.push_str(" = struct {\n");
            output.push_str("\tServiceName string\n");
            for operation in &service.operations {
                render_go_doc_comment(
                    &mut output,
                    "\t",
                    operation.doc.for_language(crate::language::Language::Go),
                );
                output.push('\t');
                output.push_str(&go_field_name(&operation.name));
                output.push(' ');
                render_operation_reference_type(&mut output, operation, api_plan, package, self)?;
                output.push('\n');
            }
            output.push_str("}{\n");
            output.push_str("\tServiceName: ");
            output.push_str(&go_string_literal(&service.wire_name));
            output.push_str(",\n");
            for operation in &service.operations {
                output.push('\t');
                output.push_str(&go_field_name(&operation.name));
                output.push_str(": nexus.NewOperationReference[");
                output.push_str(&operation_io_type(
                    operation.input.as_ref(),
                    api_plan,
                    package,
                    self,
                )?);
                output.push_str(", ");
                output.push_str(&operation_io_type(
                    operation.output.as_ref(),
                    api_plan,
                    package,
                    self,
                )?);
                output.push_str("](");
                output.push_str(&go_string_literal(&operation.wire_name));
                output.push_str("),\n");
            }
            output.push_str("}\n\n");
            render_service_client(&mut output, service, api_plan, package, self)?;
        }
        Ok(output)
    }
}

fn unique_import_alias(import_path: &str, imports: &BTreeMap<String, String>) -> String {
    let base = import_path
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(go_package_name)
        .unwrap_or_else(|| "api".to_string());
    if !imports.values().any(|alias| alias == &base) {
        return base;
    }
    for index in 2.. {
        let alias = format!("{base}{index}");
        if !imports.values().any(|existing| existing == &alias) {
            return alias;
        }
    }
    unreachable!("unbounded alias suffix search should find a free alias")
}

fn imports_alias(imports: &mut BTreeMap<String, String>, import_path: &str) -> String {
    if let Some(alias) = imports.get(import_path) {
        return alias.clone();
    }
    let alias = unique_import_alias(import_path, imports);
    imports.insert(import_path.to_string(), alias.clone());
    alias
}

fn render_service_client(
    output: &mut String,
    service: &crate::spec::ServiceSpec<PlannedTypeFamily>,
    api_plan: &PlannedSpec,
    package: &GoPackageContext,
    backend: &ModelBackend,
) -> Result<()> {
    let service_var = go_field_name(&service.name);
    let client_name = format!("{service_var}Client");
    render_go_doc_comment(
        output,
        "",
        service.doc.for_language(crate::language::Language::Go),
    );
    output.push_str("type ");
    output.push_str(&client_name);
    output.push_str(" struct {\n\tclient workflow.NexusClient\n}\n\n");

    output.push_str("func New");
    output.push_str(&client_name);
    output.push_str("(endpoint string) *");
    output.push_str(&client_name);
    output.push_str(" {\n\treturn &");
    output.push_str(&client_name);
    output.push_str("{client: workflow.NewNexusClient(endpoint, ");
    output.push_str(&service_var);
    output.push_str(".ServiceName)}\n}\n\n");

    for operation in &service.operations {
        render_go_doc_comment(
            output,
            "",
            operation.doc.for_language(crate::language::Language::Go),
        );
        output.push_str("func (c *");
        output.push_str(&client_name);
        output.push_str(") ");
        output.push_str(&go_field_name(&operation.name));
        output.push_str("(ctx workflow.Context");
        let input_expr = if operation.input.is_some() {
            output.push_str(", request ");
            output.push_str(&operation_io_type(
                operation.input.as_ref(),
                api_plan,
                package,
                backend,
            )?);
            "request"
        } else {
            "nil"
        };
        output.push_str(
            ") workflow.NexusOperationFuture {\n\treturn c.client.ExecuteOperation(ctx, ",
        );
        output.push_str(&service_var);
        output.push('.');
        output.push_str(&go_field_name(&operation.name));
        output.push_str(", ");
        output.push_str(input_expr);
        output.push_str(", workflow.NexusOperationOptions{})\n}\n\n");
    }

    Ok(())
}

fn render_operation_reference_type(
    output: &mut String,
    operation: &OperationSpec<PlannedTypeFamily>,
    api_plan: &PlannedSpec,
    package: &GoPackageContext,
    backend: &ModelBackend,
) -> Result<()> {
    output.push_str("nexus.OperationReference[");
    output.push_str(&operation_io_type(
        operation.input.as_ref(),
        api_plan,
        package,
        backend,
    )?);
    output.push_str(", ");
    output.push_str(&operation_io_type(
        operation.output.as_ref(),
        api_plan,
        package,
        backend,
    )?);
    output.push(']');
    Ok(())
}

fn operation_io_type(
    ty: Option<&TypeSpec<PlannedTypeFamily>>,
    api_plan: &PlannedSpec,
    package: &GoPackageContext,
    backend: &ModelBackend,
) -> Result<String> {
    let Some(ty) = ty else {
        return Ok("nexus.NoValue".to_string());
    };
    match ty.without_option() {
        TypeSpec::External(ExternalTypeSpec::Json(json_type)) => {
            Ok(backend.json_model_type_annotation(json_type))
        }
        TypeSpec::Record(record_ref) => Ok(api_plan
            .record(&record_ref.full_name)
            .map(|record| record.name.clone())
            .unwrap_or_else(|| record_ref.model_name.clone())),
        TypeSpec::External(ExternalTypeSpec::Alias { type_name, .. }) => Ok(type_name
            .for_language(crate::language::Language::Go)
            .map(|annotation| package.go_type_expr(annotation))
            .unwrap_or_else(|| "any".to_string())),
        _ => Ok("nexus.NoValue".to_string()),
    }
}

fn render_external_models(
    models: &[&PlannedJsonType],
    model_names: &BTreeMap<String, String>,
    shared_runtime: bool,
) -> Result<ModelFragments> {
    if models.is_empty() {
        return Ok(ModelFragments::default());
    }

    let mut imports = BTreeSet::from(["encoding/json".to_string()]);
    let mut output = String::new();
    if !shared_runtime {
        imports.extend([
            "bytes".to_string(),
            "errors".to_string(),
            "fmt".to_string(),
            "math".to_string(),
            "strings".to_string(),
        ]);
        render_validator_core(&mut output);
    }
    render_const_discriminators(&mut output, models)?;
    for model in models {
        output.push('\n');
        render_model(&mut output, model, models, model_names)?;
    }
    if shared_runtime {
        qualify_shared_runtime_references(&mut output);
    }
    if output.contains("fmt.") {
        imports.insert("fmt".to_string());
    }
    if output.contains("nexus.") {
        imports.insert("github.com/nexus-rpc/sdk-go/nexus".to_string());
    }
    Ok(ModelFragments {
        imports,
        body: output,
    })
}

pub(in crate::generator) fn render_support_file() -> String {
    let mut output = String::new();
    output.push_str(GENERATED_JSON_HEADER);
    output.push_str("package nexgenjson\n\n");
    output.push_str("import (\n");
    output.push_str("\t\"bytes\"\n");
    output.push_str("\t\"encoding/json\"\n");
    output.push_str("\t\"errors\"\n");
    output.push_str("\t\"fmt\"\n");
    output.push_str("\t\"math\"\n");
    output.push_str("\t\"strings\"\n");
    output.push_str(")\n\n");
    render_validator_core(&mut output);
    export_validator_core(&mut output);
    output
}

const GENERATED_JSON_HEADER: &str = "// Code generated by nex-gen. DO NOT EDIT.\n";

fn export_validator_core(output: &mut String) {
    for (from, to) in [
        ("func addViolations", "func AddViolations"),
        ("func mergeNested", "func MergeNested"),
        ("func parseStringField", "func ParseStringField"),
        ("func parseIntegerField", "func ParseIntegerField"),
        ("func parseBoolField", "func ParseBoolField"),
        ("func isNull", "func IsNull"),
        ("func marshalField", "func MarshalField"),
        ("addViolations(", "AddViolations("),
        ("mergeNested(", "MergeNested("),
        ("parseStringField(", "ParseStringField("),
        ("parseIntegerField(", "ParseIntegerField("),
        ("parseBoolField(", "ParseBoolField("),
        ("isNull(", "IsNull("),
        ("marshalField(", "MarshalField("),
    ] {
        *output = output.replace(from, to);
    }
}

fn qualify_shared_runtime_references(output: &mut String) {
    for (from, to) in [
        ("[]Violation", "[]nexgenjson.Violation"),
        ("Violation{", "nexgenjson.Violation{"),
        ("&ValidationError{", "&nexgenjson.ValidationError{"),
        ("IntegerCap", "nexgenjson.IntegerCap"),
        ("addViolations(", "nexgenjson.AddViolations("),
        ("mergeNested(", "nexgenjson.MergeNested("),
        ("parseStringField(", "nexgenjson.ParseStringField("),
        ("parseIntegerField(", "nexgenjson.ParseIntegerField("),
        ("parseBoolField(", "nexgenjson.ParseBoolField("),
        ("isNull(", "nexgenjson.IsNull("),
        ("marshalField(", "nexgenjson.MarshalField("),
    ] {
        *output = output.replace(from, to);
    }
}

fn render_validator_core(output: &mut String) {
    output.push_str("// Violation is a single constraint failure. Path is the JSON member path\n");
    output.push_str("// (dotted for nested members); Reason is a human-readable message.\n");
    output.push_str("type Violation struct {\n\tPath   string\n\tReason string\n}\n\n");
    output.push_str("func (v Violation) String() string {\n");
    output.push_str("\tif v.Path == \"\" {\n\t\treturn v.Reason\n\t}\n");
    output.push_str("\treturn v.Path + \": \" + v.Reason\n}\n\n");
    output
        .push_str("// ValidationError aggregates every Violation found while (de)serializing a\n");
    output.push_str("// value, surfacing them all in one error (never a partial first-failure).\n");
    output.push_str("type ValidationError struct {\n\tViolations []Violation\n}\n\n");
    output.push_str("func (e *ValidationError) Error() string {\n");
    output.push_str("\tparts := make([]string, len(e.Violations))\n");
    output.push_str("\tfor i, v := range e.Violations {\n\t\tparts[i] = v.String()\n\t}\n");
    output.push_str("\treturn fmt.Sprintf(\"%d validation error(s): %s\", len(e.Violations), strings.Join(parts, \"; \"))\n");
    output.push_str("}\n\n");
    output.push_str("func addViolations(errs *[]Violation, err error) {\n");
    output.push_str("\tif err == nil {\n\t\treturn\n\t}\n");
    output.push_str("\tvar ve *ValidationError\n");
    output.push_str("\tif errors.As(err, &ve) {\n\t\t*errs = append(*errs, ve.Violations...)\n\t\treturn\n\t}\n");
    output.push_str("\t*errs = append(*errs, Violation{\"\", err.Error()})\n}\n\n");
    output.push_str("func mergeNested(errs *[]Violation, path string, err error) {\n");
    output.push_str("\tif err == nil {\n\t\treturn\n\t}\n");
    output.push_str("\tvar ve *ValidationError\n");
    output.push_str("\tif errors.As(err, &ve) {\n");
    output.push_str("\t\tfor _, v := range ve.Violations {\n");
    output.push_str("\t\t\tp := v.Path\n\t\t\tif p == \"\" {\n\t\t\t\tp = path\n\t\t\t} else {\n\t\t\t\tp = path + \".\" + v.Path\n\t\t\t}\n");
    output
        .push_str("\t\t\t*errs = append(*errs, Violation{p, v.Reason})\n\t\t}\n\t\treturn\n\t}\n");
    output.push_str("\t*errs = append(*errs, Violation{path, err.Error()})\n}\n\n");
    output.push_str("const IntegerCap = 1<<53 - 1\n\n");
    output.push_str("var (\n\terrFractional = errors.New(\"not an integer\")\n\terrRange      = errors.New(\"exceeds ±(2^53-1) integer cap\")\n)\n\n");
    output.push_str("func parseSpecInteger(n json.Number) (int64, error) {\n");
    output.push_str("\tf, err := n.Float64()\n\tif err != nil {\n\t\treturn 0, err\n\t}\n");
    output.push_str("\tif f != math.Trunc(f) {\n\t\treturn 0, errFractional\n\t}\n");
    output.push_str("\tif f < -IntegerCap || f > IntegerCap {\n\t\treturn 0, errRange\n\t}\n");
    output.push_str(
        "\ti, err := n.Int64()\n\tif err != nil {\n\t\treturn 0, err\n\t}\n\treturn i, nil\n}\n\n",
    );
    output.push_str("func isNull(raw json.RawMessage) bool {\n\treturn bytes.Equal(bytes.TrimSpace(raw), []byte(\"null\"))\n}\n\n");
    output.push_str("func parseStringField(raw *json.RawMessage, path string, required, nullable bool, errs *[]Violation) (string, bool) {\n");
    output.push_str("\tif raw == nil {\n\t\tif required {\n\t\t\t*errs = append(*errs, Violation{path, \"required\"})\n\t\t}\n\t\treturn \"\", false\n\t}\n");
    output.push_str("\tif isNull(*raw) {\n\t\tif !nullable {\n\t\t\t*errs = append(*errs, Violation{path, \"explicit null not allowed\"})\n\t\t}\n\t\treturn \"\", false\n\t}\n");
    output.push_str("\tvar s string\n\tif err := json.Unmarshal(*raw, &s); err != nil {\n\t\t*errs = append(*errs, Violation{path, \"expected string\"})\n\t\treturn \"\", false\n\t}\n\treturn s, true\n}\n\n");
    output.push_str("func parseIntegerField(raw *json.RawMessage, path string, required, nullable bool, errs *[]Violation) (int64, bool) {\n");
    output.push_str("\tif raw == nil {\n\t\tif required {\n\t\t\t*errs = append(*errs, Violation{path, \"required\"})\n\t\t}\n\t\treturn 0, false\n\t}\n");
    output.push_str("\tif isNull(*raw) {\n\t\tif !nullable {\n\t\t\t*errs = append(*errs, Violation{path, \"explicit null not allowed\"})\n\t\t}\n\t\treturn 0, false\n\t}\n");
    output.push_str(
        "\tdec := json.NewDecoder(bytes.NewReader(*raw))\n\tdec.UseNumber()\n\tvar n json.Number\n",
    );
    output.push_str("\tif err := dec.Decode(&n); err != nil {\n\t\t*errs = append(*errs, Violation{path, \"expected integer\"})\n\t\treturn 0, false\n\t}\n");
    output.push_str("\tv, err := parseSpecInteger(n)\n\tif err != nil {\n\t\t*errs = append(*errs, Violation{path, err.Error()})\n\t\treturn 0, false\n\t}\n\treturn v, true\n}\n\n");
    output.push_str("func parseBoolField(raw *json.RawMessage, path string, required, nullable bool, errs *[]Violation) (bool, bool) {\n");
    output.push_str("\tif raw == nil {\n\t\tif required {\n\t\t\t*errs = append(*errs, Violation{path, \"required\"})\n\t\t}\n\t\treturn false, false\n\t}\n");
    output.push_str("\tif isNull(*raw) {\n\t\tif !nullable {\n\t\t\t*errs = append(*errs, Violation{path, \"explicit null not allowed\"})\n\t\t}\n\t\treturn false, false\n\t}\n");
    output.push_str("\tvar b bool\n\tif err := json.Unmarshal(*raw, &b); err != nil {\n\t\t*errs = append(*errs, Violation{path, \"expected boolean\"})\n\t\treturn false, false\n\t}\n\treturn b, true\n}\n\n");
    output.push_str("func marshalField(out map[string]json.RawMessage, key string, v any, errs *[]Violation) {\n");
    output.push_str("\tb, err := json.Marshal(v)\n\tif err != nil {\n\t\tmergeNested(errs, key, err)\n\t\treturn\n\t}\n\tout[key] = b\n}\n\n");
}

fn render_const_discriminators(output: &mut String, models: &[&PlannedJsonType]) -> Result<()> {
    let mut constants = Vec::new();
    for model in models {
        let schema = decode_schema(model)?;
        let Some(properties) = &schema.properties else {
            continue;
        };
        for (field_name, property) in properties {
            let Some(Value::String(value)) = &property.const_value else {
                continue;
            };
            constants.push((
                const_type_name(&model.model_name, field_name),
                const_value_name(&model.model_name, field_name, value),
                value.clone(),
            ));
        }
    }
    if constants.is_empty() {
        return Ok(());
    }
    for (type_name, const_name, value) in constants {
        output.push_str("type ");
        output.push_str(&type_name);
        output.push_str(" = string\n\n");
        output.push_str("const ");
        output.push_str(&const_name);
        output.push_str(" = ");
        output.push_str(&type_name);
        output.push('(');
        output.push_str(&go_string_literal(&value));
        output.push_str(")\n\n");
    }
    Ok(())
}

fn render_model(
    output: &mut String,
    model: &PlannedJsonType,
    _models: &[&PlannedJsonType],
    model_names: &BTreeMap<String, String>,
) -> Result<()> {
    let schema = decode_schema(model)?;
    render_go_doc_comment(output, "", schema.description.as_deref());
    output.push_str("type ");
    output.push_str(&model.model_name);
    output.push_str(" struct {\n");
    if let Some(value_schema) = typed_map_value_schema(&schema)? {
        output.push_str("\tAdditionalProperties map[string]");
        output.push_str(&go_type_annotation(&value_schema, "", model_names)?);
        output.push('\n');
        output.push_str("}\n\n");
        render_typed_map_methods(output, model, &schema, &value_schema, model_names)?;
        return Ok(());
    }

    let required = required_fields(&schema);
    let properties = schema.properties.as_ref();
    if let Some(properties) = properties {
        for (json_name, property) in properties {
            render_go_doc_comment(output, "\t", property.description.as_deref());
            output.push('\t');
            output.push_str(&go_field_name(json_name));
            output.push(' ');
            output.push_str(&go_property_type(
                &model.model_name,
                json_name,
                property,
                required.contains(json_name),
                model_names,
            )?);
            output.push_str(" `json:\"");
            output.push_str(json_name);
            if !required.contains(json_name) {
                output.push_str(",omitempty");
            }
            output.push_str("\"`\n");
        }
    }
    if is_open_object(&schema) {
        output.push_str(
            "\t// AdditionalProperties holds unknown members verbatim (forward compat, P13).\n",
        );
        output.push_str("\tAdditionalProperties map[string]json.RawMessage `json:\"-\"`\n");
    }
    output.push_str("}\n\n");
    render_default_accessors(output, model, &schema)?;
    render_validate(output, model, &schema, model_names)?;
    render_unmarshal_json(output, model, &schema, model_names)?;
    render_marshal_json(output, model, &schema)?;
    Ok(())
}

fn render_default_accessors(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
) -> Result<()> {
    let Some(properties) = &schema.properties else {
        return Ok(());
    };
    for (json_name, property) in properties {
        let Some(default) = &property.default else {
            continue;
        };
        let Value::Number(default) = default else {
            continue;
        };
        output.push_str("// ");
        output.push_str(&go_field_name(json_name));
        output.push_str("OrDefault returns ");
        output.push_str(&go_field_name(json_name));
        output.push_str(" when set, else the schema default.\n");
        output.push_str("func (m ");
        output.push_str(&model.model_name);
        output.push_str(") ");
        output.push_str(&go_field_name(json_name));
        output.push_str("OrDefault() int64 {\n");
        output.push_str("\tif m.");
        output.push_str(&go_field_name(json_name));
        output.push_str(" != nil {\n\t\treturn *m.");
        output.push_str(&go_field_name(json_name));
        output.push_str("\n\t}\n\treturn ");
        output.push_str(&default.to_string());
        output.push_str("\n}\n\n");
    }
    Ok(())
}

fn render_validate(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
    model_names: &BTreeMap<String, String>,
) -> Result<()> {
    output.push_str("func (m ");
    output.push_str(&model.model_name);
    output.push_str(") Validate() error {\n\tvar errs []Violation\n");
    if let Some(value_schema) = typed_map_value_schema(schema)? {
        if let Some(max) = schema.max_properties {
            output.push_str("\tif len(m.AdditionalProperties) > ");
            output.push_str(&max.to_string());
            output.push_str(
                " {\n\t\terrs = append(errs, Violation{\"\", fmt.Sprintf(\"maxProperties: at most ",
            );
            output.push_str(&max.to_string());
            output.push_str(" (got %d)\", len(m.AdditionalProperties))})\n\t}\n");
        }
        let _ = value_schema;
    } else if let Some(properties) = &schema.properties {
        for (json_name, property) in properties {
            let field = format!("m.{}", go_field_name(json_name));
            if let Some(Value::String(value)) = &property.const_value {
                output.push_str("\tif ");
                output.push_str(&field);
                output.push_str(" != ");
                output.push_str(&const_value_name(&model.model_name, json_name, value));
                output.push_str(" {\n\t\terrs = append(errs, Violation{");
                output.push_str(&go_string_literal(json_name));
                output.push_str(", `const: must equal \\\"");
                output.push_str(value);
                output.push_str("\\\"`})\n\t}\n");
            }
            if property.ty.as_ref().and_then(Value::as_str) == Some("integer") {
                let expr = if required_fields(schema).contains(json_name) {
                    field.clone()
                } else {
                    format!("*{field}")
                };
                let guard = if required_fields(schema).contains(json_name) {
                    String::new()
                } else {
                    format!("{field} != nil && ")
                };
                output.push_str("\tif ");
                output.push_str(&guard);
                output.push('(');
                output.push_str(&expr);
                output.push_str(" < -IntegerCap || ");
                output.push_str(&expr);
                output.push_str(" > IntegerCap) {\n\t\terrs = append(errs, Violation{");
                output.push_str(&go_string_literal(json_name));
                output.push_str(", \"exceeds ±(2^53-1) integer cap\"})\n\t}\n");
            }
            if property.reference.is_some() {
                if required_fields(schema).contains(json_name) {
                    output.push_str("\tmergeNested(&errs, ");
                    output.push_str(&go_string_literal(json_name));
                    output.push_str(", ");
                    output.push_str(&field);
                    output.push_str(".Validate())\n");
                } else {
                    output.push_str("\tif ");
                    output.push_str(&field);
                    output.push_str(" != nil {\n\t\tmergeNested(&errs, ");
                    output.push_str(&go_string_literal(json_name));
                    output.push_str(", ");
                    output.push_str(&field);
                    output.push_str(".Validate())\n\t}\n");
                }
            }
            let _ = model_names;
        }
    }
    output.push_str("\tif len(errs) > 0 {\n\t\treturn &ValidationError{Violations: errs}\n\t}\n\treturn nil\n}\n\n");
    Ok(())
}

fn render_unmarshal_json(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
    model_names: &BTreeMap<String, String>,
) -> Result<()> {
    output.push_str("func (m *");
    output.push_str(&model.model_name);
    output.push_str(") UnmarshalJSON(data []byte) error {\n");
    output.push_str("\tvar all map[string]json.RawMessage\n\tif err := json.Unmarshal(data, &all); err != nil {\n\t\treturn err\n\t}\n\tvar errs []Violation\n");
    let required = required_fields(schema);
    if is_open_object(schema) {
        output.push_str("\tm.AdditionalProperties = map[string]json.RawMessage{}\n");
        output.push_str("\tfor k, v := range all {\n\t\tswitch k {\n");
    } else {
        output.push_str("\tfor k := range all {\n\t\tswitch k {\n");
    }
    let property_names = schema
        .properties
        .as_ref()
        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if !property_names.is_empty() {
        output.push_str("\t\tcase ");
        output.push_str(
            &property_names
                .iter()
                .map(|name| go_string_literal(name))
                .collect::<Vec<_>>()
                .join(", "),
        );
        output.push_str(":\n");
    }
    output.push_str("\t\tdefault:\n");
    if is_open_object(schema) {
        output.push_str("\t\t\tm.AdditionalProperties[k] = v\n");
    } else {
        output.push_str("\t\t\terrs = append(errs, Violation{k, \"unknown field\"})\n");
    }
    output.push_str("\t\t}\n\t}\n");
    output.push_str("\tget := func(k string) *json.RawMessage {\n\t\tif v, ok := all[k]; ok {\n\t\t\treturn &v\n\t\t}\n\t\treturn nil\n\t}\n\t_ = get\n");
    if let Some(properties) = &schema.properties {
        for (json_name, property) in properties {
            render_property_unmarshal(
                output,
                model,
                json_name,
                property,
                required.contains(json_name),
                model_names,
            )?;
        }
    }
    output.push_str("\tif len(errs) > 0 {\n\t\treturn &ValidationError{Violations: errs}\n\t}\n\treturn nil\n}\n\n");
    Ok(())
}

fn render_property_unmarshal(
    output: &mut String,
    model: &PlannedJsonType,
    json_name: &str,
    property: &Schema,
    required: bool,
    model_names: &BTreeMap<String, String>,
) -> Result<()> {
    let field = go_field_name(json_name);
    if let Some(non_null) = nullable_non_null_schema(property)
        && non_null.reference.is_some()
    {
        let model_type = go_type_annotation(non_null, json_name, model_names)?;
        render_reference_property_unmarshal(output, json_name, &field, &model_type, required, true);
        return Ok(());
    }
    if property.reference.is_some() {
        let model_type = go_type_annotation(property, json_name, model_names)?;
        render_reference_property_unmarshal(
            output,
            json_name,
            &field,
            &model_type,
            required,
            false,
        );
        return Ok(());
    }
    if property.ty.as_ref().and_then(Value::as_str) == Some("string")
        || property.const_value.is_some()
    {
        output.push_str("\tif v, ok := parseStringField(get(");
        output.push_str(&go_string_literal(json_name));
        output.push_str("), ");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", ");
        output.push_str(if required { "true" } else { "false" });
        output.push_str(", ");
        output.push_str(if allows_null(property) {
            "true"
        } else {
            "false"
        });
        output.push_str(", &errs); ok {\n\t\tm.");
        output.push_str(&field);
        output.push_str(" = ");
        if required {
            output.push_str("v\n");
        } else {
            output.push_str("&v\n");
        }
        if let Some(Value::String(value)) = &property.const_value {
            output.push_str("\t\tif v != ");
            output.push_str(&const_value_name(&model.model_name, json_name, value));
            output.push_str(" {\n\t\t\terrs = append(errs, Violation{");
            output.push_str(&go_string_literal(json_name));
            output.push_str(", `const: must equal \\\"");
            output.push_str(value);
            output.push_str("\\\"`})\n\t\t}\n");
        }
        output.push_str("\t}\n");
        return Ok(());
    }
    if property.ty.as_ref().and_then(Value::as_str) == Some("integer") {
        output.push_str("\tif v, ok := parseIntegerField(get(");
        output.push_str(&go_string_literal(json_name));
        output.push_str("), ");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", ");
        output.push_str(if required { "true" } else { "false" });
        output.push_str(", false, &errs); ok {\n\t\tm.");
        output.push_str(&field);
        output.push_str(" = ");
        if required {
            output.push_str("v\n");
        } else {
            output.push_str("&v\n");
        }
        output.push_str("\t}\n");
        return Ok(());
    }
    if property.ty.as_ref().and_then(Value::as_str) == Some("boolean") {
        output.push_str("\tif v, ok := parseBoolField(get(");
        output.push_str(&go_string_literal(json_name));
        output.push_str("), ");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", ");
        output.push_str(if required { "true" } else { "false" });
        output.push_str(", ");
        output.push_str(if allows_null(property) {
            "true"
        } else {
            "false"
        });
        output.push_str(", &errs); ok {\n\t\tm.");
        output.push_str(&field);
        output.push_str(" = ");
        if required {
            output.push_str("v\n");
        } else {
            output.push_str("&v\n");
        }
        output.push_str("\t}\n");
        return Ok(());
    }
    if property.ty.as_ref().and_then(Value::as_str) == Some("array") {
        output.push_str("\tif raw := get(");
        output.push_str(&go_string_literal(json_name));
        output.push_str("); raw == nil {\n");
        if required {
            output.push_str("\t\terrs = append(errs, Violation{");
            output.push_str(&go_string_literal(json_name));
            output.push_str(", \"required\"})\n");
        }
        output.push_str("\t} else if isNull(*raw) {\n\t\terrs = append(errs, Violation{");
        output.push_str(&go_string_literal(json_name));
        output.push_str(
            ", \"explicit null not allowed\"})\n\t} else if err := json.Unmarshal(*raw, &m.",
        );
        output.push_str(&field);
        output.push_str("); err != nil {\n\t\terrs = append(errs, Violation{");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", \"expected array\"})\n\t}\n");
        return Ok(());
    }
    if allows_null(property) {
        output.push_str("\tif v, ok := parseStringField(get(");
        output.push_str(&go_string_literal(json_name));
        output.push_str("), ");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", ");
        output.push_str(if required { "true" } else { "false" });
        output.push_str(", true, &errs); ok {\n\t\tm.");
        output.push_str(&field);
        output.push_str(" = &v\n\t}\n");
    }
    Ok(())
}

fn render_reference_property_unmarshal(
    output: &mut String,
    json_name: &str,
    field: &str,
    model_type: &str,
    required: bool,
    nullable: bool,
) {
    output.push_str("\tif raw := get(");
    output.push_str(&go_string_literal(json_name));
    output.push_str("); raw == nil {\n");
    if required {
        output.push_str("\t\terrs = append(errs, Violation{");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", \"required\"})\n");
    }
    output.push_str("\t} else if isNull(*raw) {\n");
    if !nullable {
        output.push_str("\t\terrs = append(errs, Violation{");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", \"explicit null not allowed\"})\n");
    }
    output.push_str("\t} else {\n");
    if required && !nullable {
        output.push_str("\t\tmergeNested(&errs, ");
        output.push_str(&go_string_literal(json_name));
        output.push_str(", json.Unmarshal(*raw, &m.");
        output.push_str(field);
        output.push_str("))\n");
    } else {
        output.push_str("\t\tvar tmp ");
        output.push_str(model_type);
        output.push_str(
            "\n\t\tif err := json.Unmarshal(*raw, &tmp); err != nil {\n\t\t\tmergeNested(&errs, ",
        );
        output.push_str(&go_string_literal(json_name));
        output.push_str(", err)\n\t\t} else {\n\t\t\tm.");
        output.push_str(field);
        output.push_str(" = &tmp\n\t\t}\n");
    }
    output.push_str("\t}\n");
}

fn render_marshal_json(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
) -> Result<()> {
    output.push_str("func (m ");
    output.push_str(&model.model_name);
    output.push_str(") MarshalJSON() ([]byte, error) {\n");
    output.push_str("\tvar errs []Violation\n\taddViolations(&errs, m.Validate())\n");
    output.push_str("\tout := map[string]json.RawMessage{}\n");
    if is_open_object(schema) {
        output.push_str("\tfor k, v := range m.AdditionalProperties {\n\t\tout[k] = v\n\t}\n");
    }
    let required = required_fields(schema);
    if let Some(properties) = &schema.properties {
        for (json_name, property) in properties {
            let field = format!("m.{}", go_field_name(json_name));
            if required.contains(json_name) {
                if allows_null(property) {
                    output.push_str("\tif ");
                    output.push_str(&field);
                    output.push_str(" != nil {\n\t\tmarshalField(out, ");
                    output.push_str(&go_string_literal(json_name));
                    output.push_str(", *");
                    output.push_str(&field);
                    output.push_str(", &errs)\n\t} else {\n\t\tout[");
                    output.push_str(&go_string_literal(json_name));
                    output.push_str("] = json.RawMessage(\"null\")\n\t}\n");
                } else {
                    output.push_str("\tmarshalField(out, ");
                    output.push_str(&go_string_literal(json_name));
                    output.push_str(", ");
                    output.push_str(&field);
                    output.push_str(", &errs)\n");
                }
            } else {
                output.push_str("\tif ");
                output.push_str(&field);
                output.push_str(" != nil {\n\t\tmarshalField(out, ");
                output.push_str(&go_string_literal(json_name));
                output.push_str(", ");
                if property.ty.as_ref().and_then(Value::as_str) != Some("array") {
                    output.push('*');
                }
                output.push_str(&field);
                output.push_str(", &errs)\n\t}\n");
            }
        }
    }
    output.push_str("\tif len(errs) > 0 {\n\t\treturn nil, &ValidationError{Violations: errs}\n\t}\n\treturn json.Marshal(out)\n}\n\n");
    Ok(())
}

fn render_typed_map_methods(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
    value_schema: &Schema,
    _model_names: &BTreeMap<String, String>,
) -> Result<()> {
    output.push_str("func (m ");
    output.push_str(&model.model_name);
    output.push_str(") Validate() error {\n\tvar errs []Violation\n");
    if let Some(max) = schema.max_properties {
        output.push_str("\tif len(m.AdditionalProperties) > ");
        output.push_str(&max.to_string());
        output.push_str(
            " {\n\t\terrs = append(errs, Violation{\"\", fmt.Sprintf(\"maxProperties: at most ",
        );
        output.push_str(&max.to_string());
        output.push_str(" (got %d)\", len(m.AdditionalProperties))})\n\t}\n");
    }
    output.push_str("\tif len(errs) > 0 {\n\t\treturn &ValidationError{Violations: errs}\n\t}\n\treturn nil\n}\n\n");
    output.push_str("func (m *");
    output.push_str(&model.model_name);
    output.push_str(") UnmarshalJSON(data []byte) error {\n");
    output.push_str("\tvar raw map[string]json.RawMessage\n\tif err := json.Unmarshal(data, &raw); err != nil {\n\t\treturn err\n\t}\n\tvar errs []Violation\n");
    output.push_str("\tm.AdditionalProperties = make(map[string]string, len(raw))\n");
    output.push_str("\tfor k, v := range raw {\n\t\tif isNull(v) {\n\t\t\terrs = append(errs, Violation{k, \"explicit null not allowed\"})\n\t\t\tcontinue\n\t\t}\n");
    if value_schema.ty.as_ref().and_then(Value::as_str) == Some("string") {
        output.push_str("\t\tvar s string\n\t\tif err := json.Unmarshal(v, &s); err != nil {\n\t\t\terrs = append(errs, Violation{k, \"expected string\"})\n\t\t\tcontinue\n\t\t}\n\t\tm.AdditionalProperties[k] = s\n");
    }
    output.push_str("\t}\n");
    if let Some(max) = schema.max_properties {
        output.push_str("\tif len(m.AdditionalProperties) > ");
        output.push_str(&max.to_string());
        output.push_str(
            " {\n\t\terrs = append(errs, Violation{\"\", fmt.Sprintf(\"maxProperties: at most ",
        );
        output.push_str(&max.to_string());
        output.push_str(" (got %d)\", len(m.AdditionalProperties))})\n\t}\n");
    }
    output.push_str("\tif len(errs) > 0 {\n\t\treturn &ValidationError{Violations: errs}\n\t}\n\treturn nil\n}\n\n");
    output.push_str("func (m ");
    output.push_str(&model.model_name);
    output.push_str(") MarshalJSON() ([]byte, error) {\n\tif err := m.Validate(); err != nil {\n\t\treturn nil, err\n\t}\n\tout := make(map[string]string, len(m.AdditionalProperties))\n\tfor k, v := range m.AdditionalProperties {\n\t\tout[k] = v\n\t}\n\treturn json.Marshal(out)\n}\n\n");
    Ok(())
}

fn decode_schema(model: &PlannedJsonType) -> Result<Schema> {
    serde_json::from_value(model.schema.clone()).map_err(|error| Error::InvalidJsonSchema {
        path: PathBuf::from("<go-json-generator>"),
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
        Some(Value::Object(value)) => serde_json::from_value(Value::Object(value.clone()))
            .map(Some)
            .map_err(|error| Error::InvalidJsonSchema {
                path: PathBuf::from("<go-json-generator>"),
                reason: format!("failed to read `additionalProperties`: {error}"),
            }),
        _ => Ok(None),
    }
}

fn go_property_type(
    model_name: &str,
    json_name: &str,
    schema: &Schema,
    required: bool,
    model_names: &BTreeMap<String, String>,
) -> Result<String> {
    if let Some(Value::String(_)) = &schema.const_value {
        return Ok(const_type_name(model_name, json_name));
    }
    let mut annotation = go_type_annotation(schema, json_name, model_names)?;
    if !required && !annotation.starts_with("[]") {
        annotation = format!("*{annotation}");
    }
    if required && allows_null(schema) && !annotation.starts_with('*') {
        annotation = format!("*{annotation}");
    }
    Ok(annotation)
}

fn go_type_annotation(
    schema: &Schema,
    json_name: &str,
    model_names: &BTreeMap<String, String>,
) -> Result<String> {
    if let Some(reference) = &schema.reference {
        return Ok(reference_model_name(reference, model_names));
    }
    if let Some(branches) = &schema.one_of
        && let Some(non_null) = branches
            .iter()
            .find(|branch| !schema_type_includes(branch, "null"))
    {
        return go_type_annotation(non_null, json_name, model_names);
    }
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => Ok("string".to_string()),
        Some("integer") => Ok("int64".to_string()),
        Some("number") => Ok("float64".to_string()),
        Some("boolean") => Ok("bool".to_string()),
        Some("array") => {
            let item = schema
                .items
                .as_ref()
                .map(|item| go_type_annotation(item, json_name, model_names))
                .transpose()?
                .unwrap_or_else(|| "any".to_string());
            Ok(format!("[]{item}"))
        }
        Some("object") => Ok("map[string]json.RawMessage".to_string()),
        _ => Ok("any".to_string()),
    }
}

fn reference_model_name(reference: &str, model_names: &BTreeMap<String, String>) -> String {
    if let Some(model_name) = model_names.get(reference) {
        return model_name.clone();
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

fn nullable_non_null_schema(schema: &Schema) -> Option<&Schema> {
    schema.one_of.as_ref()?.iter().find(|branch| {
        !schema_type_includes(branch, "null") && branch.const_value.as_ref() != Some(&Value::Null)
    })
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

fn is_open_object(schema: &Schema) -> bool {
    schema.ty.as_ref().and_then(Value::as_str) == Some("object")
        && schema
            .properties
            .as_ref()
            .is_some_and(|properties| !properties.is_empty())
        && schema.additional_properties.as_ref() != Some(&Value::Bool(false))
}

fn const_type_name(model_name: &str, field_name: &str) -> String {
    format!("{model_name}{}", go_field_name(field_name))
}

fn const_value_name(model_name: &str, field_name: &str, value: &str) -> String {
    format!(
        "{}{}",
        const_type_name(model_name, field_name),
        value.to_upper_camel_case()
    )
}

fn render_go_doc_comment(output: &mut String, indent: &str, doc: Option<&str>) {
    let Some(doc) = doc.map(str::trim).filter(|doc| !doc.is_empty()) else {
        return;
    };
    for line in doc.lines() {
        output.push_str(indent);
        output.push_str("// ");
        output.push_str(line.trim());
        output.push('\n');
    }
}
