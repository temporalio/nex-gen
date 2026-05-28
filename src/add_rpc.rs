use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use heck::{ToKebabCase, ToUpperCamelCase};
use prost_types::FieldDescriptorProto;
use prost_types::field_descriptor_proto::{Label, Type};
use wit_parser::{
    FunctionKind, Interface, Resolve, Type as WitType, TypeDefKind, TypeId, WorldItem, WorldKey,
};

use crate::descriptors::{DescriptorIndex, EnumMetadata, MessageMetadata, RpcMetadata};
use crate::error::{Error, Result};
use crate::spec::{
    LinkedWitMetadata, find_proto_name_for_type, find_proto_name_for_type_def,
    load_linked_wit_metadata_from_inputs, parse_wit_with_inputs, select_world,
    wire_operation_name_from_docs,
};

const DEFAULT_PACKAGE_NAME: &str = "temporal:nexus@1.0.0";
const DEFAULT_WORLD_NAME: &str = "system";
const DEFAULT_ENDPOINT_PLACEHOLDER: &str = "__REPLACE_ME__";

pub fn generate_add_rpc_wit(
    descriptors: &DescriptorIndex,
    rpc_name: &str,
    input_paths: &[PathBuf],
) -> Result<String> {
    let rpc = descriptors.resolve_rpc(rpc_name)?;
    let linked_wit = load_linked_wit_metadata_from_inputs(input_paths)?;
    AddRpcBuilder::new(descriptors, rpc, linked_wit)
        .build()
        .map(|rendered| rendered.render_standalone())
}

pub fn generate_add_rpc_wit_with_input(
    descriptors: &DescriptorIndex,
    rpc_name: &str,
    input_path: &Path,
    input: &str,
    linked_input_paths: &[PathBuf],
) -> Result<String> {
    let rpc = descriptors.resolve_rpc(rpc_name)?;
    let linked_wit = load_linked_wit_metadata_from_inputs(linked_input_paths)?;
    let existing = ExistingWitDocument::load(input_path, input, linked_input_paths)?;
    let interface_name = rpc.service_name.to_kebab_case();
    let operation_name = rpc.name.to_kebab_case();

    if let Some(interface) = existing.interfaces.get(&interface_name) {
        if interface.function_names.contains(&operation_name) {
            let update = AddRpcBuilder::new(descriptors, rpc, linked_wit)
                .with_existing_interface(interface)
                .build_existing_operation_update(interface, &operation_name)?;
            return update.apply(input, &interface_name);
        }
        if let Some(existing_operation_name) = interface.function_names_by_wire_name.get(&rpc.name)
        {
            let update = AddRpcBuilder::new(descriptors, rpc, linked_wit)
                .with_existing_interface(interface)
                .build_existing_operation_update(interface, existing_operation_name)?;
            return update.apply(input, &interface_name);
        }

        let rendered = AddRpcBuilder::new(descriptors, rpc, linked_wit)
            .with_existing_interface(interface)
            .build()?;
        let snippet = rendered.render_interface_items();
        return insert_into_named_block(input, "interface", &interface_name, &snippet);
    }

    let rendered = AddRpcBuilder::new(descriptors, rpc, linked_wit).build()?;
    let source = insert_world_export(input, &existing.world_name, &interface_name)?;
    let interface_block = rendered.render_new_interface_block();
    insert_after_named_block(&source, "world", &existing.world_name, &interface_block)
}

#[derive(Debug, Clone)]
struct ExistingWitDocument {
    world_name: String,
    interfaces: BTreeMap<String, ExistingInterface>,
}

impl ExistingWitDocument {
    fn load(path: &Path, input: &str, linked_input_paths: &[PathBuf]) -> Result<Self> {
        let parsed = parse_wit_with_inputs(input, path, linked_input_paths)?;
        let world_id = select_world(&parsed.resolve, parsed.package_id, path)?;
        let world = &parsed.resolve.worlds[world_id];

        let mut interfaces = BTreeMap::new();
        for (key, item) in &world.exports {
            let WorldItem::Interface { id, .. } = item else {
                continue;
            };
            let interface = &parsed.resolve.interfaces[*id];
            let export_name = exported_interface_name(key, interface);
            let interface_source = find_named_block(input, "interface", &export_name)
                .map(|block| &input[block.brace_start + 1..block.end_start]);
            interfaces.insert(
                export_name.clone(),
                ExistingInterface::from_resolve(
                    &parsed.resolve,
                    interface,
                    export_name,
                    path,
                    interface_source,
                )?,
            );
        }

        Ok(Self {
            world_name: world.name.clone(),
            interfaces,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct ExistingInterface {
    function_names: BTreeSet<String>,
    function_names_by_wire_name: BTreeMap<String, String>,
    functions: BTreeMap<String, ExistingFunction>,
    records_by_proto: BTreeMap<String, ExistingRecord>,
    type_names_by_proto: BTreeMap<String, String>,
    type_names_in_scope: BTreeSet<String>,
}

impl ExistingInterface {
    fn from_resolve(
        resolve: &Resolve,
        interface: &Interface,
        export_name: String,
        path: &Path,
        interface_source: Option<&str>,
    ) -> Result<Self> {
        let function_names = interface.functions.keys().cloned().collect();
        let mut function_names_by_wire_name = BTreeMap::new();
        let mut functions = BTreeMap::new();
        let mut type_names_by_proto = BTreeMap::new();
        let mut type_names_in_scope = BTreeSet::new();
        let mut records_by_proto = BTreeMap::new();

        for (function_name, function) in &interface.functions {
            let context = format!("interface `{export_name}` function `{function_name}`");
            let function = ExistingFunction::from_resolve(resolve, function, path, &context)?;
            function_names_by_wire_name.insert(function.wire_name.clone(), function_name.clone());
            functions.insert(function_name.clone(), function);
        }

        for (type_name, type_id) in &interface.types {
            type_names_in_scope.insert(type_name.clone());
            let type_def = &resolve.types[*type_id];
            let context = format!("interface `{export_name}` type `{type_name}`");
            let Some(proto_name) = find_proto_name_for_type_def(type_def, path, &context)? else {
                continue;
            };
            type_names_by_proto.insert(proto_name.clone(), type_name.clone());
            if let TypeDefKind::Record(record) = &type_def.kind {
                records_by_proto.insert(
                    proto_name,
                    ExistingRecord::from_resolve(resolve, type_name, record),
                );
            }
        }
        if let Some(interface_source) = interface_source {
            type_names_in_scope.extend(collect_used_type_names(interface_source));
        }

        Ok(Self {
            function_names,
            function_names_by_wire_name,
            functions,
            records_by_proto,
            type_names_by_proto,
            type_names_in_scope,
        })
    }
}

#[derive(Debug, Clone)]
struct ExistingFunction {
    wire_name: String,
    parameter_name: Option<String>,
    input_type_name: Option<String>,
    input_proto: Option<String>,
    output_type_name: Option<String>,
    output_proto: Option<String>,
    params_len: usize,
    has_result: bool,
    freestanding: bool,
}

impl ExistingFunction {
    fn from_resolve(
        resolve: &Resolve,
        function: &wit_parser::Function,
        path: &Path,
        context: &str,
    ) -> Result<Self> {
        let wire_name = wire_operation_name_from_docs(
            function.docs.contents.as_deref(),
            path,
            context,
            &function.name.to_upper_camel_case(),
        )?;
        let (parameter_name, input_type_name, input_proto) =
            if let Some(parameter) = function.params.first() {
                (
                    Some(parameter.name.clone()),
                    wit_type_name(resolve, &parameter.ty),
                    find_proto_name_for_type(resolve, &parameter.ty, path, context)?,
                )
            } else {
                (None, None, None)
            };
        let (output_type_name, output_proto) = if let Some(output_type) = function.result.as_ref() {
            (
                wit_type_name(resolve, output_type),
                find_proto_name_for_type(resolve, output_type, path, context)?,
            )
        } else {
            (None, None)
        };

        Ok(Self {
            wire_name,
            parameter_name,
            input_type_name,
            input_proto,
            output_type_name,
            output_proto,
            params_len: function.params.len(),
            has_result: function.result.is_some(),
            freestanding: matches!(
                function.kind,
                FunctionKind::Freestanding | FunctionKind::AsyncFreestanding
            ),
        })
    }
}

#[derive(Debug, Clone)]
struct ExistingRecord {
    wit_name: String,
    fields: BTreeMap<String, ExistingField>,
}

impl ExistingRecord {
    fn from_resolve(resolve: &Resolve, wit_name: &str, record: &wit_parser::Record) -> Self {
        let mut fields = BTreeMap::new();
        for field in &record.fields {
            let docs = field.docs.contents.as_deref();
            let explicit_proto_name = proto_field_name_from_docs(docs);
            let proto_name = explicit_proto_name
                .clone()
                .unwrap_or_else(|| field.name.replace('-', "_"));
            fields.insert(
                proto_name,
                ExistingField {
                    wit_name: field.name.clone(),
                    type_expr: render_wit_type(resolve, &field.ty),
                    explicit_proto_field: explicit_proto_name.is_some(),
                    omitted: field_has_omit_directive(docs),
                },
            );
        }

        Self {
            wit_name: wit_name.to_string(),
            fields,
        }
    }
}

#[derive(Debug, Clone)]
struct ExistingField {
    wit_name: String,
    type_expr: String,
    explicit_proto_field: bool,
    omitted: bool,
}

struct AddRpcBuilder<'a> {
    descriptors: &'a DescriptorIndex,
    rpc: &'a RpcMetadata,
    linked_wit: LinkedWitMetadata,
    linked_uses: BTreeMap<String, BTreeSet<String>>,
    available_type_names: BTreeMap<String, String>,
    reserved_type_names: BTreeSet<String>,
    rendered_types: BTreeSet<String>,
    rendered_definitions: Vec<String>,
}

impl<'a> AddRpcBuilder<'a> {
    fn new(
        descriptors: &'a DescriptorIndex,
        rpc: &'a RpcMetadata,
        linked_wit: LinkedWitMetadata,
    ) -> Self {
        Self {
            descriptors,
            rpc,
            linked_wit,
            linked_uses: BTreeMap::new(),
            available_type_names: BTreeMap::new(),
            reserved_type_names: BTreeSet::new(),
            rendered_types: BTreeSet::new(),
            rendered_definitions: Vec::new(),
        }
    }

    fn with_existing_interface(mut self, interface: &ExistingInterface) -> Self {
        self.available_type_names = interface.type_names_by_proto.clone();
        self.reserved_type_names = interface.type_names_in_scope.clone();
        self
    }

    fn build(mut self) -> Result<RenderedAddRpcWit> {
        let input_type = self.render_type_reference(&self.rpc.input_type, &self.rpc.full_name)?;
        let output_type = self.render_type_reference(&self.rpc.output_type, &self.rpc.full_name)?;

        Ok(RenderedAddRpcWit {
            rpc_full_name: self.rpc.full_name.clone(),
            interface_name: self.rpc.service_name.to_kebab_case(),
            linked_uses: self.linked_uses,
            rendered_definitions: self.rendered_definitions,
            operation: format!(
                "  {}: func(\n    request: {},\n  ) -> {};\n",
                self.rpc.name.to_kebab_case(),
                input_type,
                output_type
            ),
        })
    }

    fn build_existing_operation_update(
        mut self,
        interface: &ExistingInterface,
        operation_name: &str,
    ) -> Result<ExistingOperationUpdate> {
        let input_type = self.render_type_reference(&self.rpc.input_type, &self.rpc.full_name)?;
        let output_type = self.render_type_reference(&self.rpc.output_type, &self.rpc.full_name)?;
        let Some(function) = interface.functions.get(operation_name) else {
            return Err(Error::UnsupportedAddRpc {
                context: self.rpc.full_name.clone(),
                reason: format!("existing operation `{operation_name}` was not found"),
            });
        };
        self.validate_existing_function(function, operation_name, &input_type, &output_type)?;

        let mut record_updates = Vec::new();
        for proto_name in [&self.rpc.input_type, &self.rpc.output_type] {
            let proto_name = proto_name.trim_start_matches('.');
            let Some(message) = self.descriptors.message(proto_name) else {
                continue;
            };
            let Some(record) = interface.records_by_proto.get(proto_name) else {
                return Err(Error::UnsupportedAddRpc {
                    context: self.rpc.full_name.clone(),
                    reason: format!(
                        "existing operation `{operation_name}` uses proto `{proto_name}`, but no matching WIT record was found"
                    ),
                });
            };
            let missing_fields = self.missing_record_fields(record, message, proto_name)?;
            if !missing_fields.is_empty() {
                record_updates.push(RecordFieldUpdate {
                    record_name: record.wit_name.clone(),
                    fields: missing_fields,
                });
            }
        }

        Ok(ExistingOperationUpdate {
            linked_uses: self.linked_uses,
            rendered_definitions: self.rendered_definitions,
            record_updates,
        })
    }

    fn validate_existing_function(
        &self,
        function: &ExistingFunction,
        operation_name: &str,
        input_type: &str,
        output_type: &str,
    ) -> Result<()> {
        let mut conflicts = Vec::new();
        if !function.freestanding {
            conflicts.push("operation is not a freestanding function".to_string());
        }
        if function.params_len != 1 {
            conflicts.push(format!(
                "operation has {} parameters instead of 1",
                function.params_len
            ));
        }
        if function.parameter_name.as_deref() != Some("request") {
            conflicts.push(format!(
                "operation parameter is `{}` instead of `request`",
                function.parameter_name.as_deref().unwrap_or("<missing>")
            ));
        }
        if function.input_type_name.as_deref() != Some(input_type) {
            conflicts.push(format!(
                "operation request type is `{}` instead of `{input_type}`",
                function.input_type_name.as_deref().unwrap_or("<missing>")
            ));
        }
        if function.input_proto.as_deref() != Some(self.rpc.input_type.trim_start_matches('.')) {
            conflicts.push(format!(
                "operation request proto is `{}` instead of `{}`",
                function.input_proto.as_deref().unwrap_or("<missing>"),
                self.rpc.input_type.trim_start_matches('.')
            ));
        }
        if !function.has_result {
            conflicts.push("operation does not declare a result type".to_string());
        }
        if function.output_type_name.as_deref() != Some(output_type) {
            conflicts.push(format!(
                "operation result type is `{}` instead of `{output_type}`",
                function.output_type_name.as_deref().unwrap_or("<missing>")
            ));
        }
        if function.output_proto.as_deref() != Some(self.rpc.output_type.trim_start_matches('.')) {
            conflicts.push(format!(
                "operation result proto is `{}` instead of `{}`",
                function.output_proto.as_deref().unwrap_or("<missing>"),
                self.rpc.output_type.trim_start_matches('.')
            ));
        }

        if conflicts.is_empty() {
            return Ok(());
        }

        Err(Error::UnsupportedAddRpc {
            context: self.rpc.full_name.clone(),
            reason: format!(
                "existing operation `{operation_name}` conflicts with descriptor: {}",
                conflicts.join("; ")
            ),
        })
    }

    fn missing_record_fields(
        &mut self,
        record: &ExistingRecord,
        message: &MessageMetadata,
        proto_name: &str,
    ) -> Result<Vec<String>> {
        let expected_by_proto = message
            .descriptor
            .field
            .iter()
            .map(|field| field_name(field, proto_name))
            .collect::<Result<BTreeSet<_>>>()?;

        for existing_proto_name in record.fields.keys() {
            if !expected_by_proto.contains(existing_proto_name.as_str()) {
                return Err(Error::UnsupportedAddRpc {
                    context: proto_name.to_string(),
                    reason: format!(
                        "existing WIT record `{}` contains non-descriptor field `{existing_proto_name}`",
                        record.wit_name
                    ),
                });
            }
        }

        let mut missing = Vec::new();
        for field in &message.descriptor.field {
            let field_name = field_name(field, proto_name)?;
            let Some(existing) = record.fields.get(field_name) else {
                if self.record_has_field_covering_proto(record, field_name) {
                    continue;
                }
                let expected = self.render_message_field(message, field)?;
                missing.push(expected.line.clone());
                continue;
            };
            if existing.omitted {
                continue;
            }
            let expected = self.render_message_field_for_comparison(message, field)?;
            let name_matches =
                existing.wit_name == expected.wit_name || existing.explicit_proto_field;
            let type_matches = self.type_is_compatible(&existing.type_expr, &expected.type_expr);
            if !name_matches || !type_matches {
                return Err(Error::UnsupportedAddRpc {
                    context: format!("{proto_name}.{}", field_name),
                    reason: format!(
                        "existing WIT field is `{}: {}` but descriptor requires `{}: {}`",
                        existing.wit_name,
                        existing.type_expr,
                        expected.wit_name,
                        expected.type_expr
                    ),
                });
            }
        }

        Ok(missing)
    }

    fn type_is_compatible(&self, existing: &str, expected: &str) -> bool {
        if existing == expected {
            return true;
        }

        match (option_inner_type(existing), option_inner_type(expected)) {
            (Some(existing_inner), Some(expected_inner)) => {
                self.type_is_compatible(existing_inner, expected_inner)
            }
            (None, Some(expected_inner)) => self.type_is_compatible(existing, expected_inner),
            (Some(existing_inner), None) => self.type_is_compatible(existing_inner, expected),
            (None, None) => self.base_type_is_compatible(existing, expected),
        }
    }

    fn base_type_is_compatible(&self, existing: &str, expected: &str) -> bool {
        let mut visited = BTreeSet::new();
        let mut pending = vec![existing];
        while let Some(current) = pending.pop() {
            if current == expected {
                return true;
            }
            if !visited.insert(current.to_string()) {
                continue;
            }
            if let Some(targets) = self.linked_wit.type_compatibility.get(current) {
                pending.extend(targets.iter().map(String::as_str));
            }
        }
        false
    }

    fn record_has_field_covering_proto(&self, record: &ExistingRecord, proto_name: &str) -> bool {
        record.fields.values().any(|field| {
            self.linked_wit
                .type_covered_fields
                .get(base_type_expr(&field.type_expr))
                .is_some_and(|covered_fields| covered_fields.contains(proto_name))
        })
    }

    fn render_type_reference(&mut self, proto_name: &str, context: &str) -> Result<String> {
        let proto_name = proto_name.trim_start_matches('.');
        if let Some(existing_name) = self.available_type_names.get(proto_name) {
            return Ok(existing_name.clone());
        }

        if let Some(linked_type) = self.linked_wit.proto_types.get(proto_name).cloned() {
            self.use_linked_type(proto_name, &linked_type.wit_name)?;
            return Ok(linked_type.wit_name);
        }

        if let Some(message) = self.descriptors.message(proto_name) {
            let wit_name = self.reserve_local_type_name(proto_name, context)?;
            if self.rendered_types.insert(proto_name.to_string()) {
                let definition = self.render_message(message, &wit_name)?;
                self.rendered_definitions.push(definition);
            }
            return Ok(wit_name);
        }

        if let Some(enumeration) = self.descriptors.enumeration(proto_name) {
            let wit_name = self.reserve_local_type_name(proto_name, context)?;
            if self.rendered_types.insert(proto_name.to_string()) {
                let definition = self.render_enum(enumeration, &wit_name)?;
                self.rendered_definitions.push(definition);
            }
            return Ok(wit_name);
        }

        Err(Error::UnsupportedAddRpc {
            context: context.to_string(),
            reason: format!("unknown proto type `{proto_name}`"),
        })
    }

    fn render_message(&mut self, message: &MessageMetadata, wit_name: &str) -> Result<String> {
        let rendered_fields = self.render_message_fields(message)?;

        let mut rendered = String::new();
        rendered.push_str(&format!("  /// @nexus.proto \"{}\"\n", message.full_name));
        rendered.push_str(&format!("  record {wit_name} {{\n"));
        for field in rendered_fields {
            rendered.push_str(&field.line);
            rendered.push('\n');
        }
        rendered.push_str("  }\n");
        Ok(rendered)
    }

    fn render_message_fields(
        &mut self,
        message: &MessageMetadata,
    ) -> Result<Vec<RenderedFieldSpec>> {
        let mut rendered_fields = Vec::new();
        for field in &message.descriptor.field {
            rendered_fields.push(self.render_message_field(message, field)?);
        }
        Ok(rendered_fields)
    }

    fn render_message_field(
        &mut self,
        message: &MessageMetadata,
        field: &FieldDescriptorProto,
    ) -> Result<RenderedFieldSpec> {
        let field_name = field_name(field, &message.full_name)?;
        let wit_field_type = self.render_field_type(field, &message.full_name, field_name)?;
        let wit_field_name = field_name.to_kebab_case();
        Ok(RenderedFieldSpec {
            wit_name: wit_field_name.clone(),
            type_expr: wit_field_type.clone(),
            line: format!("    {wit_field_name}: {wit_field_type},"),
        })
    }

    fn render_message_field_for_comparison(
        &mut self,
        message: &MessageMetadata,
        field: &FieldDescriptorProto,
    ) -> Result<RenderedFieldSpec> {
        let linked_uses = self.linked_uses.clone();
        let available_type_names = self.available_type_names.clone();
        let reserved_type_names = self.reserved_type_names.clone();
        let rendered_types = self.rendered_types.clone();
        let rendered_definitions = self.rendered_definitions.clone();

        let rendered = self.render_message_field(message, field);

        self.linked_uses = linked_uses;
        self.available_type_names = available_type_names;
        self.reserved_type_names = reserved_type_names;
        self.rendered_types = rendered_types;
        self.rendered_definitions = rendered_definitions;

        rendered
    }

    fn render_enum(&mut self, enumeration: &EnumMetadata, wit_name: &str) -> Result<String> {
        let mut rendered = String::new();
        rendered.push_str(&format!(
            "  /// @nexus.proto \"{}\"\n",
            enumeration.full_name
        ));
        rendered.push_str(&format!("  enum {wit_name} {{\n"));
        for value in &enumeration.descriptor.value {
            let Some(name) = value.name.as_deref() else {
                return Err(Error::UnsupportedAddRpc {
                    context: enumeration.full_name.clone(),
                    reason: "enum value is missing a name".to_string(),
                });
            };
            rendered.push_str(&format!("    {},\n", name.to_kebab_case()));
        }
        rendered.push_str("  }\n");
        Ok(rendered)
    }

    fn render_field_type(
        &mut self,
        field: &FieldDescriptorProto,
        parent_type: &str,
        field_name: &str,
    ) -> Result<String> {
        let context = format!("{parent_type}.{field_name}");
        let label =
            Label::try_from(field.label.unwrap_or(Label::Optional as i32)).map_err(|_| {
                Error::UnsupportedAddRpc {
                    context: context.clone(),
                    reason: "unknown field label".to_string(),
                }
            })?;
        let field_type = Type::try_from(field.r#type.unwrap_or_default()).map_err(|_| {
            Error::UnsupportedAddRpc {
                context: context.clone(),
                reason: "unknown field type".to_string(),
            }
        })?;

        let base_type = match field_type {
            Type::Double => "f64".to_string(),
            Type::Float => "f32".to_string(),
            Type::Int64 | Type::Sint64 | Type::Sfixed64 => "s64".to_string(),
            Type::Uint64 | Type::Fixed64 => "u64".to_string(),
            Type::Int32 | Type::Sint32 | Type::Sfixed32 => "s32".to_string(),
            Type::Uint32 | Type::Fixed32 => "u32".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "string".to_string(),
            Type::Bytes => "list<u8>".to_string(),
            Type::Message | Type::Group | Type::Enum => {
                let Some(type_name) = field.type_name.as_deref() else {
                    return Err(Error::UnsupportedAddRpc {
                        context,
                        reason: "field is missing a referenced type name".to_string(),
                    });
                };
                self.render_type_reference(type_name, parent_type)?
            }
        };

        if label == Label::Repeated {
            return Ok(format!("list<{base_type}>"));
        }

        if field_has_presence(field, field_type)
            || !field_supports_required_without_presence(field_type)
        {
            return Ok(format!("option<{base_type}>"));
        }

        Ok(base_type)
    }

    fn reserve_local_type_name(&mut self, proto_name: &str, context: &str) -> Result<String> {
        if let Some(existing_name) = self.available_type_names.get(proto_name) {
            return Ok(existing_name.clone());
        }

        let wit_name = self.local_type_name(proto_name);
        if self.reserved_type_names.contains(&wit_name) {
            return Err(Error::UnsupportedAddRpc {
                context: context.to_string(),
                reason: format!(
                    "generated type name `{wit_name}` would collide with an existing WIT type"
                ),
            });
        }

        self.available_type_names
            .insert(proto_name.to_string(), wit_name.clone());
        self.reserved_type_names.insert(wit_name.clone());
        Ok(wit_name)
    }

    fn use_linked_type(&mut self, proto_name: &str, wit_name: &str) -> Result<()> {
        let Some(use_path) = self.linked_wit.type_use_paths.get(wit_name) else {
            return Err(Error::UnsupportedAddRpc {
                context: self.rpc.full_name.clone(),
                reason: format!("linked WIT type `{wit_name}` was not found in linked metadata"),
            });
        };

        let already_in_scope = self.reserved_type_names.contains(wit_name);
        self.available_type_names
            .insert(proto_name.to_string(), wit_name.to_string());
        self.reserved_type_names.insert(wit_name.to_string());
        if !already_in_scope {
            self.linked_uses
                .entry(use_path.clone())
                .or_default()
                .insert(wit_name.to_string());
        }
        Ok(())
    }

    fn local_type_name(&self, proto_name: &str) -> String {
        if let Some(message) = self.descriptors.message(proto_name) {
            return descriptor_relative_name(&message.full_name, &message.package);
        }
        if let Some(enumeration) = self.descriptors.enumeration(proto_name) {
            return descriptor_relative_name(&enumeration.full_name, &enumeration.package);
        }
        proto_name
            .trim_start_matches('.')
            .replace('.', "-")
            .to_kebab_case()
    }
}

#[derive(Debug, Clone)]
struct RenderedFieldSpec {
    wit_name: String,
    type_expr: String,
    line: String,
}

struct ExistingOperationUpdate {
    linked_uses: BTreeMap<String, BTreeSet<String>>,
    rendered_definitions: Vec<String>,
    record_updates: Vec<RecordFieldUpdate>,
}

impl ExistingOperationUpdate {
    fn apply(&self, input: &str, interface_name: &str) -> Result<String> {
        let mut source = input.to_string();
        let interface_additions =
            render_interface_additions(&self.linked_uses, &self.rendered_definitions);
        if !interface_additions.is_empty() {
            source = insert_into_named_block(
                &source,
                "interface",
                interface_name,
                &interface_additions,
            )?;
        }
        for update in &self.record_updates {
            source = insert_into_named_block(
                &source,
                "record",
                &update.record_name,
                &update.fields.join("\n"),
            )?;
        }
        Ok(source)
    }
}

struct RecordFieldUpdate {
    record_name: String,
    fields: Vec<String>,
}

struct RenderedAddRpcWit {
    rpc_full_name: String,
    interface_name: String,
    linked_uses: BTreeMap<String, BTreeSet<String>>,
    rendered_definitions: Vec<String>,
    operation: String,
}

impl RenderedAddRpcWit {
    fn render_standalone(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str(&format!(
            "/// WIT scaffold generated from `{}`.\n",
            self.rpc_full_name
        ));
        rendered.push_str(
            "/// Replace the endpoint and refine any inferred field mappings as needed.\n",
        );
        rendered.push_str(&format!("package {DEFAULT_PACKAGE_NAME};\n\n"));
        rendered.push_str(&format!("world {DEFAULT_WORLD_NAME} {{\n"));
        rendered.push_str(&format!("  export {};\n", self.interface_name));
        rendered.push_str("}\n\n");
        rendered.push_str(&self.render_new_interface_block());
        rendered
    }

    fn render_new_interface_block(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str(&format!(
            "/// @nexus.endpoint \"{DEFAULT_ENDPOINT_PLACEHOLDER}\"\n"
        ));
        rendered.push_str(&format!("interface {} {{\n", self.interface_name));
        rendered.push_str(&self.render_interface_items());
        rendered.push_str("}\n");
        rendered
    }

    fn render_interface_items(&self) -> String {
        let mut rendered =
            render_interface_additions(&self.linked_uses, &self.rendered_definitions);
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(&self.operation);
        rendered
    }
}

fn render_interface_additions(
    linked_uses: &BTreeMap<String, BTreeSet<String>>,
    rendered_definitions: &[String],
) -> String {
    let mut rendered = String::new();

    if !linked_uses.is_empty() {
        rendered.push_str(&render_use_block(linked_uses));
    }

    if !rendered_definitions.is_empty() {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        for (index, definition) in rendered_definitions.iter().enumerate() {
            rendered.push_str(definition);
            if index + 1 != rendered_definitions.len() {
                rendered.push('\n');
            }
        }
    }

    rendered
}

fn render_use_block(linked_uses: &BTreeMap<String, BTreeSet<String>>) -> String {
    let mut rendered = String::new();
    for (use_path, linked_types) in linked_uses {
        rendered.push_str(&format!("  use {use_path}.{{\n"));
        for linked_type in linked_types {
            rendered.push_str(&format!("    {linked_type},\n"));
        }
        rendered.push_str("  };\n");
    }
    rendered
}

#[derive(Debug, Clone, Copy)]
struct NamedBlock {
    brace_start: usize,
    end_start: usize,
    end_exclusive: usize,
}

fn insert_into_named_block(
    source: &str,
    keyword: &str,
    name: &str,
    snippet: &str,
) -> Result<String> {
    let Some(block) = find_named_block(source, keyword, name) else {
        return Err(Error::UnsupportedAddRpc {
            context: format!("{keyword} `{name}`"),
            reason: "existing WIT file does not contain the target block".to_string(),
        });
    };

    let insertion_start = source[..block.end_start]
        .char_indices()
        .rev()
        .find_map(|(index, character)| match character {
            ' ' | '\t' => None,
            '\n' => Some(index + 1),
            _ => Some(block.end_start),
        })
        .unwrap_or(block.end_start);
    let mut rendered = String::with_capacity(source.len() + snippet.len() + 1);
    rendered.push_str(&source[..insertion_start]);
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered.push_str(snippet);
    if !snippet.ends_with('\n') {
        rendered.push('\n');
    }
    rendered.push_str(&source[insertion_start..]);
    Ok(rendered)
}

fn insert_world_export(source: &str, world_name: &str, export_name: &str) -> Result<String> {
    let Some(block) = find_named_block(source, "world", world_name) else {
        return Err(Error::UnsupportedAddRpc {
            context: format!("world `{world_name}`"),
            reason: "existing WIT file does not contain the target world".to_string(),
        });
    };

    let mut rendered = String::with_capacity(source.len() + export_name.len() + 12);
    rendered.push_str(&source[..block.end_start]);
    rendered.push_str(&format!("  export {export_name};\n"));
    rendered.push_str(&source[block.end_start..]);
    Ok(rendered)
}

fn insert_after_named_block(
    source: &str,
    keyword: &str,
    name: &str,
    snippet: &str,
) -> Result<String> {
    let Some(block) = find_named_block(source, keyword, name) else {
        return Err(Error::UnsupportedAddRpc {
            context: format!("{keyword} `{name}`"),
            reason: "existing WIT file does not contain the target block".to_string(),
        });
    };

    let mut rendered = String::with_capacity(source.len() + snippet.len() + 2);
    rendered.push_str(&source[..block.end_exclusive]);
    rendered.push_str("\n\n");
    rendered.push_str(snippet);
    rendered.push_str(&source[block.end_exclusive..]);
    Ok(rendered)
}

fn find_named_block(source: &str, keyword: &str, name: &str) -> Option<NamedBlock> {
    let needle = format!("{keyword} {name}");
    for (index, _) in source.match_indices(&needle) {
        if index > 0 && !source[..index].chars().next_back().unwrap().is_whitespace() {
            continue;
        }

        let after_name = index + needle.len();
        let brace_offset = source[after_name..].find('{')?;
        let brace_start = after_name + brace_offset;
        if !source[after_name..brace_start]
            .chars()
            .all(char::is_whitespace)
        {
            continue;
        }

        let end_start = find_matching_brace(source, brace_start)?;
        return Some(NamedBlock {
            brace_start,
            end_start,
            end_exclusive: end_start + 1,
        });
    }
    None
}

fn find_matching_brace(source: &str, open_brace: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in source[open_brace..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open_brace + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn exported_interface_name(key: &WorldKey, interface: &Interface) -> String {
    match key {
        WorldKey::Name(name) => name.clone(),
        WorldKey::Interface(_) => interface
            .name
            .clone()
            .unwrap_or_else(|| "unnamed-interface".to_string()),
    }
}

fn collect_used_type_names(interface_source: &str) -> BTreeSet<String> {
    let mut used_names = BTreeSet::new();
    let mut offset = 0usize;

    while let Some(use_index) = interface_source[offset..].find("use ") {
        let use_start = offset + use_index;
        let Some(brace_index) = interface_source[use_start..].find('{') else {
            break;
        };
        let brace_start = use_start + brace_index;
        let Some(brace_end_rel) = interface_source[brace_start + 1..].find('}') else {
            break;
        };
        let brace_end = brace_start + 1 + brace_end_rel;

        for name in interface_source[brace_start + 1..brace_end].split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let imported_name = name.split_whitespace().next().unwrap_or(name);
            used_names.insert(imported_name.to_string());
        }

        offset = brace_end + 1;
    }

    used_names
}

fn proto_field_name_from_docs(docs: Option<&str>) -> Option<String> {
    let docs = docs?;
    for line in docs.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("@nexus.proto-field") else {
            continue;
        };
        let rest = rest.trim();
        if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
            return Some(rest[1..rest.len() - 1].to_string());
        }
    }
    None
}

fn field_has_omit_directive(docs: Option<&str>) -> bool {
    docs.into_iter()
        .flat_map(str::lines)
        .map(str::trim)
        .any(|line| line == "@nexus.omit")
}

fn wit_type_name(resolve: &Resolve, ty: &WitType) -> Option<String> {
    let WitType::Id(type_id) = ty else {
        return None;
    };
    resolve.types[*type_id].name.clone()
}

fn render_wit_type(resolve: &Resolve, ty: &WitType) -> String {
    match ty {
        WitType::Bool => "bool".to_string(),
        WitType::U8 => "u8".to_string(),
        WitType::U16 => "u16".to_string(),
        WitType::U32 => "u32".to_string(),
        WitType::U64 => "u64".to_string(),
        WitType::S8 => "s8".to_string(),
        WitType::S16 => "s16".to_string(),
        WitType::S32 => "s32".to_string(),
        WitType::S64 => "s64".to_string(),
        WitType::F32 => "f32".to_string(),
        WitType::F64 => "f64".to_string(),
        WitType::Char => "char".to_string(),
        WitType::String => "string".to_string(),
        WitType::ErrorContext => "error-context".to_string(),
        WitType::Id(type_id) => render_wit_type_id(resolve, *type_id),
    }
}

fn render_wit_type_id(resolve: &Resolve, type_id: TypeId) -> String {
    let type_def = &resolve.types[type_id];
    if let Some(name) = &type_def.name {
        return name.clone();
    }
    match &type_def.kind {
        TypeDefKind::Option(inner) => format!("option<{}>", render_wit_type(resolve, inner)),
        TypeDefKind::List(inner) => format!("list<{}>", render_wit_type(resolve, inner)),
        TypeDefKind::Type(inner) => render_wit_type(resolve, inner),
        TypeDefKind::Tuple(tuple) => format!(
            "tuple<{}>",
            tuple
                .types
                .iter()
                .map(|ty| render_wit_type(resolve, ty))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeDefKind::Result(result) => {
            let ok = result
                .ok
                .as_ref()
                .map(|ty| render_wit_type(resolve, ty))
                .unwrap_or_else(|| "_".to_string());
            let err = result
                .err
                .as_ref()
                .map(|ty| render_wit_type(resolve, ty))
                .unwrap_or_else(|| "_".to_string());
            format!("result<{ok}, {err}>")
        }
        _ => type_def
            .name
            .clone()
            .unwrap_or_else(|| type_def.kind.as_str().to_string()),
    }
}

fn option_inner_type(type_expr: &str) -> Option<&str> {
    type_expr
        .strip_prefix("option<")
        .and_then(|inner| inner.strip_suffix('>'))
}

fn base_type_expr(type_expr: &str) -> &str {
    option_inner_type(type_expr).unwrap_or(type_expr)
}

fn field_name<'a>(field: &'a FieldDescriptorProto, parent_type: &str) -> Result<&'a str> {
    field
        .name
        .as_deref()
        .ok_or_else(|| Error::UnsupportedAddRpc {
            context: parent_type.to_string(),
            reason: "field is missing a name".to_string(),
        })
}

fn field_has_presence(field: &FieldDescriptorProto, field_type: Type) -> bool {
    field.proto3_optional.unwrap_or(false)
        || field.oneof_index.is_some()
        || matches!(field_type, Type::Message | Type::Group)
}

fn field_supports_required_without_presence(field_type: Type) -> bool {
    matches!(field_type, Type::String | Type::Bytes)
}

fn descriptor_relative_name(full_name: &str, package: &str) -> String {
    let relative = full_name
        .trim_start_matches('.')
        .strip_prefix(&format!("{package}."))
        .unwrap_or(full_name.trim_start_matches('.'));
    relative.replace('.', "-").to_kebab_case()
}
