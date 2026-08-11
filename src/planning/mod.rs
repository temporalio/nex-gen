use std::collections::{BTreeMap, BTreeSet};

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use prost_types::FileOptions;

use crate::descriptors::DescriptorIndex;
use crate::error::{Error, Result};
use crate::generator::ModelWireCapabilities;
use crate::language::Language;
use crate::spec::{
    ApiSpec, ApiSpecTransform, AuthoredResourceType, ExternalTypeSpec, FunctionArgSpec,
    FunctionArgsSpec, FunctionFieldSpec, FunctionResultSpec, JsonModelSpec, LanguageStringSpec,
    ModulePath, OperationSpec, RecordFieldVisibility, RecordSpec, ResourceFieldSpec, SelectedNames,
    SelectedSupportSpec, SelectedTextSpec, ServiceSpec, SupportSpec, Symbol, TypeDeclSpec,
    TypeNameFamily, TypeReplacementSpec, TypeSpec,
};
use crate::spec::{ApiSpecLeaf, ApiSpecNode, ApiSpecTree, CompilerPass};

mod authored_validation;
mod emitted_names;
mod operation_binding;
mod operation_lowering;
mod proto;
mod reachability;
mod resource_binding;
mod selection;
mod type_planning;

pub(crate) use emitted_names::EmittedNameResolutionPass;
pub(crate) use emitted_names::build_json_name_manifest;
pub(crate) use operation_binding::OperationBindingPass;
pub(crate) use operation_lowering::OperationLoweringPass;
pub(crate) use proto::{message_model_name, relative_descriptor_name};
pub(crate) use reachability::ReachabilityPass;
pub(crate) use resource_binding::{
    RequestPlan, RequestPlanSource, ResolvedResourceBindingSource, ResolvedResourceMethodBinding,
    ResolvedResourceReturnSpec, ResolvedResourceSpec, ResolvedServiceResources,
    ResourceResolutionPass,
};
pub(crate) use selection::LanguageSelectionPass;
#[cfg(test)]
pub(crate) use selection::select_spec;
pub(crate) use type_planning::TypePlanningPass;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlannedTypeFamily;

impl TypeNameFamily for PlannedTypeFamily {
    type SpecData = PlannedSpecData;
    type Record = PlannedRecordType;
    type Enum = PlannedEnumType;
    type Flags = PlannedFlagsType;
    type Variant = PlannedVariantType;
    type Resource = PlannedResourceType;
    type Proto = PlannedProtoType;
    type Json = PlannedJsonType;
    type Alias = PlannedAliasType;
    type ServiceData = ();
    type RecordData = PlannedRecordData;
    type ResourceData = PlannedResource;
    type OperationData = PlannedOperationData;
    type FieldData = PlannedFieldData;
    // Planned code still exposes the legacy text container to the generators.
    // `TypePlanningMapper` materializes it from already-selected values with no
    // language override maps, so no later stage performs selection.
    type Text = LanguageStringSpec;
    type Support = SupportSpec;
}

pub(crate) type PlannedSpec = ApiSpec<PlannedTypeFamily>;
pub(crate) type PlannedType = TypeSpec<PlannedTypeFamily>;

/// Selected graph after resource-method and resource-return bindings have
/// been resolved. The graph remains structurally identical; the pass-specific
/// analysis belongs to `SpecData`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceBoundNames;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResourceBoundData {
    bindings: BTreeMap<String, ResolvedServiceResources>,
}

/// Semantic operation and resource relationships after resource resolution.
///
/// Unlike `ResourceBoundData`, this information is attached to the operation
/// and resource nodes that own it, so later passes do not need a service-name
/// keyed side table to understand the API.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OperationBoundOperation {
    pub(crate) output_resource_return: Option<ResolvedResourceReturnSpec>,
    pub(crate) direct_return: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OperationBoundResource {
    pub(crate) resolved: Option<ResolvedResourceSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationBoundNames;

impl TypeNameFamily for OperationBoundNames {
    type SpecData = ();
    type Record = Symbol;
    type Enum = Symbol;
    type Flags = Symbol;
    type Variant = Symbol;
    type Resource = AuthoredResourceType;
    type Proto = Symbol;
    type Json = JsonModelSpec<Symbol>;
    type Alias = Symbol;
    type ServiceData = ();
    type RecordData = ();
    type ResourceData = OperationBoundResource;
    type OperationData = OperationBoundOperation;
    type FieldData = ();
    type Text = SelectedTextSpec;
    type Support = SelectedSupportSpec;
}

/// Operation-bound IR after resource-return structures have been lowered into
/// explicit generated record declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationLoweredNames;

impl TypeNameFamily for OperationLoweredNames {
    type SpecData = ();
    type Record = Symbol;
    type Enum = Symbol;
    type Flags = Symbol;
    type Variant = Symbol;
    type Resource = AuthoredResourceType;
    type Proto = Symbol;
    type Json = JsonModelSpec<Symbol>;
    type Alias = Symbol;
    type ServiceData = ();
    type RecordData = ();
    type ResourceData = OperationBoundResource;
    type OperationData = OperationBoundOperation;
    type FieldData = ();
    type Text = SelectedTextSpec;
    type Support = SelectedSupportSpec;
}

impl TypeNameFamily for ResourceBoundNames {
    type SpecData = ResourceBoundData;
    type Record = Symbol;
    type Enum = Symbol;
    type Flags = Symbol;
    type Variant = Symbol;
    type Resource = AuthoredResourceType;
    type Proto = Symbol;
    type Json = JsonModelSpec<Symbol>;
    type Alias = Symbol;
    type ServiceData = ();
    type RecordData = ();
    type ResourceData = ();
    type OperationData = ();
    type FieldData = ();
    type Text = SelectedTextSpec;
    type Support = SelectedSupportSpec;
}

struct ResourceBindingMapper {
    data: ResourceBoundData,
}

impl ApiSpecTransform<SelectedNames, ResourceBoundNames> for ResourceBindingMapper {
    fn map_spec_data(&mut self, _data: ()) -> ResourceBoundData {
        self.data.clone()
    }
    fn map_record(&mut self, name: Symbol) -> Symbol {
        name
    }
    fn map_enum(&mut self, name: Symbol) -> Symbol {
        name
    }
    fn map_flags(&mut self, name: Symbol) -> Symbol {
        name
    }
    fn map_variant(&mut self, name: Symbol) -> Symbol {
        name
    }
    fn map_resource(&mut self, name: AuthoredResourceType) -> AuthoredResourceType {
        name
    }
    fn map_proto(&mut self, name: Symbol) -> Symbol {
        name
    }
    fn map_json(&mut self, name: JsonModelSpec<Symbol>) -> JsonModelSpec<Symbol> {
        name
    }
    fn map_alias(&mut self, name: Symbol) -> Symbol {
        name
    }
    fn map_service_data(&mut self, _name: &str, _data: ()) {}
    fn map_record_data(&mut self, _name: &str, _data: ()) {}
    fn map_resource_data(&mut self, _name: &str, _data: ()) {}
    fn map_operation_data(&mut self, _name: &str, _data: ()) {}
    fn map_field_data(&mut self, _record: &str, _field: &str, _data: ()) {}
    fn map_text(&mut self, text: SelectedTextSpec) -> SelectedTextSpec {
        text
    }
    fn map_support(&mut self, support: SelectedSupportSpec) -> SelectedSupportSpec {
        support
    }
}

pub(super) fn materialize_selected_text(text: &SelectedTextSpec) -> LanguageStringSpec {
    LanguageStringSpec {
        default: text.value.clone(),
        default_import: text.import.clone(),
        ..Default::default()
    }
}

pub(super) fn materialize_selected_replacement(
    replacement: &TypeReplacementSpec<OperationLoweredNames>,
) -> TypeReplacementSpec {
    TypeReplacementSpec {
        type_name: materialize_selected_text(&replacement.type_name),
        from_proto: materialize_selected_text(&replacement.from_proto),
        to_proto: materialize_selected_text(&replacement.to_proto),
    }
}

pub(crate) fn operation_input_model(
    operation: &OperationSpec<PlannedTypeFamily>,
) -> Option<&PlannedType> {
    match operation.input_type() {
        Some(input) if planned_type_is_model_shaped(input) => Some(input),
        Some(_) => panic!("planned operation inputs are model-shaped when present"),
        None => None,
    }
}

pub(crate) fn planned_type_is_model_shaped(planned_type: &PlannedType) -> bool {
    matches!(
        planned_type,
        TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(_)))
            | TypeSpec::External(ExternalTypeSpec::Json(_))
            | TypeSpec::Record(_)
    )
}

pub(crate) fn operation_output_direct_result(operation: &OperationSpec<PlannedTypeFamily>) -> bool {
    matches!(
        operation.output_type(),
        Some(TypeSpec::Resource(PlannedResourceType {
            wire_type: None,
            ..
        }))
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanningMode {
    NativeApi,
    DefinitionsOnly,
}

fn module_import_index(
    tree: &ApiSpecTree<OperationLoweredNames>,
) -> BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>> {
    let mut imports = BTreeMap::<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>::new();
    collect_tree_module_imports(&tree.root, &mut imports);
    imports
}

fn module_export_names(spec: &ApiSpec<OperationLoweredNames>) -> BTreeSet<String> {
    spec.types
        .iter()
        .filter_map(|(name, decl)| {
            // Only JSON-schema models are treated as an always-exported module
            // surface (root type + every `$def`, including unreferenced ones).
            // WIT-sourced records/enums/protos keep their reachability-based
            // tree-shaking, so they are never force-exported here.
            let TypeDeclSpec::External(binding) = decl else {
                return None;
            };
            if !matches!(binding.external_type, ExternalTypeSpec::Json(_)) {
                return None;
            }
            external_type_module_path(&binding.external_type)
                .is_none_or(|module_path| module_path == &spec.module_path)
                .then(|| name.clone())
        })
        .collect()
}

fn external_type_module_path(
    external: &ExternalTypeSpec<OperationLoweredNames>,
) -> Option<&ModulePath> {
    match external {
        ExternalTypeSpec::Proto(symbol) => symbol.module_path(),
        ExternalTypeSpec::Json(json_type) => json_type.name.module_path(),
        ExternalTypeSpec::Alias { name, .. } => name.module_path(),
    }
}

fn collect_tree_module_imports(
    node: &ApiSpecNode<OperationLoweredNames>,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    match node {
        ApiSpecNode::Leaf(leaf) => collect_spec_module_imports(&leaf.spec, imports),
        ApiSpecNode::Branch(branch) => {
            for child in branch.children.values() {
                collect_tree_module_imports(child, imports);
            }
        }
    }
}

fn collect_spec_module_imports(
    spec: &ApiSpec<OperationLoweredNames>,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    for service in &spec.services {
        for operation in &service.operations {
            collect_type_module_imports(&spec.module_path, operation.input_type(), imports);
            collect_type_module_imports(&spec.module_path, operation.output_type(), imports);
        }
        for resource in &service.resources {
            for field in &resource.fields {
                collect_type_module_imports(&spec.module_path, Some(&field.field_type), imports);
            }
            for method in &resource.methods {
                for field in &method.params {
                    collect_type_module_imports(
                        &spec.module_path,
                        Some(&field.field_type),
                        imports,
                    );
                }
                if let Some(result) = &method.result {
                    collect_type_module_imports(
                        &spec.module_path,
                        Some(&result.result_type),
                        imports,
                    );
                }
            }
        }
    }
    for decl in spec.types.values() {
        match decl {
            TypeDeclSpec::External(binding) => {
                collect_external_type_module_imports(
                    &spec.module_path,
                    &binding.external_type,
                    imports,
                );
                if let Some(authored_type) = &binding.authored_type {
                    collect_type_module_imports(&spec.module_path, Some(authored_type), imports);
                }
            }
            TypeDeclSpec::Record(record) => {
                if let Some(source_type) = &record.source_type {
                    collect_external_type_module_imports(&spec.module_path, source_type, imports);
                }
                for field in record.fields.values() {
                    collect_type_module_imports(
                        &spec.module_path,
                        Some(&field.field_type),
                        imports,
                    );
                    if let Some(function) = &field.function {
                        if let Some(alternate) = &function.alternate_type {
                            collect_type_module_imports(
                                &spec.module_path,
                                Some(alternate),
                                imports,
                            );
                        }
                        collect_function_args_module_imports(
                            &spec.module_path,
                            &function.args,
                            imports,
                        );
                        if let Some(result) = &function.result_type_parameter {
                            let _ = result;
                        }
                    }
                }
            }
            TypeDeclSpec::Variant(variant) => {
                for case in &variant.cases {
                    if let Some(payload) = &case.payload {
                        collect_type_module_imports(&spec.module_path, Some(payload), imports);
                    }
                }
            }
            TypeDeclSpec::Enum(_) | TypeDeclSpec::Flags(_) => {}
        }
    }
}

fn collect_function_args_module_imports(
    source_module: &ModulePath,
    args: &FunctionArgsSpec<OperationLoweredNames>,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    let args = match args {
        FunctionArgsSpec::Varargs { prefix, .. } => prefix.as_slice(),
        FunctionArgsSpec::Fixed(args) => args.as_slice(),
    };
    for arg in args {
        collect_type_module_imports(source_module, Some(&arg.field_type), imports);
    }
}

fn collect_type_module_imports(
    source_module: &ModulePath,
    ty: Option<&TypeSpec<OperationLoweredNames>>,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    let Some(ty) = ty else {
        return;
    };
    match ty {
        TypeSpec::Record(symbol)
        | TypeSpec::Enum(symbol)
        | TypeSpec::Flags(symbol)
        | TypeSpec::Variant(symbol) => {
            collect_symbol_module_import(source_module, symbol, imports);
        }
        TypeSpec::Resource(resource) => {
            collect_resource_symbol_module_import(source_module, resource, imports)
        }
        TypeSpec::External(external) => {
            collect_external_type_module_imports(source_module, external, imports)
        }
        TypeSpec::Option(inner) | TypeSpec::List(inner) => {
            collect_type_module_imports(source_module, Some(inner), imports);
        }
        TypeSpec::Tuple(items) => {
            for item in items {
                collect_type_module_imports(source_module, Some(item), imports);
            }
        }
        TypeSpec::Map(key, value) => {
            collect_type_module_imports(source_module, Some(key), imports);
            collect_type_module_imports(source_module, Some(value), imports);
        }
        TypeSpec::Result { ok, err } => {
            collect_type_module_imports(source_module, ok.as_deref(), imports);
            collect_type_module_imports(source_module, err.as_deref(), imports);
        }
        _ => {}
    }
}

fn collect_external_type_module_imports(
    source_module: &ModulePath,
    external: &ExternalTypeSpec<OperationLoweredNames>,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    match external {
        ExternalTypeSpec::Proto(symbol) => {
            collect_symbol_module_import(source_module, symbol, imports)
        }
        ExternalTypeSpec::Json(json_type) => {
            collect_symbol_module_import(source_module, &json_type.name, imports)
        }
        ExternalTypeSpec::Alias { name, target, .. } => {
            collect_symbol_module_import(source_module, name, imports);
            collect_type_module_imports(source_module, Some(target), imports);
        }
    }
}

fn collect_resource_symbol_module_import(
    source_module: &ModulePath,
    resource: &AuthoredResourceType,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    collect_symbol_module_import(source_module, &resource.name, imports);
    if let Some(wire_type) = &resource.wire_type {
        collect_authored_external_type_module_imports(source_module, wire_type, imports);
    }
}

fn collect_authored_external_type_module_imports(
    source_module: &ModulePath,
    external: &ExternalTypeSpec,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    match external {
        ExternalTypeSpec::Proto(symbol) => {
            collect_symbol_module_import(source_module, symbol, imports)
        }
        ExternalTypeSpec::Json(json_type) => {
            collect_symbol_module_import(source_module, &json_type.name, imports)
        }
        ExternalTypeSpec::Alias { name, target, .. } => {
            collect_symbol_module_import(source_module, name, imports);
            collect_authored_type_module_imports(source_module, target, imports);
        }
    }
}

fn collect_authored_type_module_imports(
    source_module: &ModulePath,
    ty: &TypeSpec,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    match ty {
        TypeSpec::Record(symbol)
        | TypeSpec::Enum(symbol)
        | TypeSpec::Flags(symbol)
        | TypeSpec::Variant(symbol) => collect_symbol_module_import(source_module, symbol, imports),
        TypeSpec::Resource(resource) => {
            collect_resource_symbol_module_import(source_module, resource, imports)
        }
        TypeSpec::External(external) => {
            collect_authored_external_type_module_imports(source_module, external, imports)
        }
        TypeSpec::Option(inner) | TypeSpec::List(inner) => {
            collect_authored_type_module_imports(source_module, inner, imports)
        }
        TypeSpec::Tuple(items) => {
            for item in items {
                collect_authored_type_module_imports(source_module, item, imports);
            }
        }
        TypeSpec::Map(key, value) => {
            collect_authored_type_module_imports(source_module, key, imports);
            collect_authored_type_module_imports(source_module, value, imports);
        }
        TypeSpec::Result { ok, err } => {
            if let Some(ok) = ok {
                collect_authored_type_module_imports(source_module, ok, imports);
            }
            if let Some(err) = err {
                collect_authored_type_module_imports(source_module, err, imports);
            }
        }
        _ => {}
    }
}

fn collect_symbol_module_import(
    source_module: &ModulePath,
    symbol: &Symbol,
    imports: &mut BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
) {
    let Some(target_module) = symbol.module_path() else {
        return;
    };
    if target_module == source_module {
        return;
    }
    imports
        .entry(source_module.clone())
        .or_default()
        .entry(target_module.clone())
        .or_default()
        .insert(symbol.local_name().to_string());
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedOperationResourceReturn {
    pub(crate) resource_type_name: String,
    pub(crate) bindings: Vec<PlannedOperationResourceFieldBinding>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedOperationResourceFieldBinding {
    pub(crate) field_name: String,
    pub(crate) optional: bool,
    pub(crate) source: ResolvedResourceBindingSource,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedResource {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) fields: Vec<PlannedResourceField>,
    pub(crate) methods: Vec<PlannedResourceMethod>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedResourceField {
    pub(crate) name: String,
    pub(crate) optional: bool,
    pub(crate) kind: PlannedType,
    pub(crate) function: Option<FunctionFieldSpec<PlannedTypeFamily>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedResourceMethod {
    pub(crate) name: String,
    pub(crate) params: Vec<PlannedResourceField>,
    pub(crate) result: Option<PlannedResourceMethodResult>,
    pub(crate) binding: PlannedResourceMethodBindingSpec,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedResourceMethodResult {
    pub(crate) kind: PlannedResourceMethodResultKind,
    pub(crate) optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlannedResourceMethodResultKind {
    Resource { type_name: String },
    Value(PlannedType),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlannedResourceMethodBindingSpec {
    Operation {
        operation_name: String,
        request_plan: RequestPlan,
        direct_return: bool,
    },
    Stub,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedProtoTypeInfo {
    pub(crate) full_name: String,
    pub(crate) package: String,
    pub(crate) file_name: Option<String>,
    pub(crate) file_options: Option<FileOptions>,
    pub(crate) reference: LanguageStringSpec,
    pub(crate) type_name: LanguageStringSpec,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlannedRecordData {
    pub(crate) proto: Option<PlannedProtoTypeInfo>,
    pub(crate) capabilities: ModelWireCapabilities,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlannedFieldData {
    pub(crate) has_presence: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlannedOperationData {
    pub(crate) output_resource_return: Option<PlannedOperationResourceReturn>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedEnumType {
    pub(crate) full_name: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedFlagsType {
    pub(crate) full_name: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedVariantType {
    pub(crate) full_name: String,
    pub(crate) name: String,
}

impl AsRef<str> for PlannedVariantType {
    fn as_ref(&self) -> &str {
        &self.full_name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedAliasType {
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedJsonType {
    pub(crate) full_name: String,
    pub(crate) module_path: Option<ModulePath>,
    pub(crate) model_name: String,
    pub(crate) schema: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlannedSpecData {
    pub(crate) module_imports: BTreeMap<ModulePath, BTreeSet<String>>,
    pub(crate) module_exports: BTreeSet<String>,
}

impl AsRef<str> for PlannedJsonType {
    fn as_ref(&self) -> &str {
        &self.full_name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlannedProtoType {
    Message(PlannedProtoMessageType),
    Enum(PlannedProtoEnumType),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedProtoMessageType {
    pub(crate) proto: PlannedProtoTypeInfo,
    pub(crate) model_name: String,
    pub(crate) replacement: Option<TypeReplacementSpec>,
    pub(crate) authored_type: Option<Box<PlannedType>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedProtoEnumType {
    pub(crate) proto: PlannedProtoTypeInfo,
    pub(crate) name: String,
    pub(crate) replacement: Option<TypeReplacementSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedRecordType {
    pub(crate) full_name: String,
    pub(crate) model_name: String,
}

impl AsRef<str> for PlannedRecordType {
    fn as_ref(&self) -> &str {
        &self.full_name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedResourceType {
    pub(crate) type_name: String,
    pub(crate) wire_type: Option<ExternalTypeSpec<PlannedTypeFamily>>,
}

impl AsRef<str> for PlannedResourceType {
    fn as_ref(&self) -> &str {
        &self.type_name
    }
}

impl PlannedProtoType {
    pub(crate) fn full_name(&self) -> &str {
        match self {
            PlannedProtoType::Message(message) => &message.proto.full_name,
            PlannedProtoType::Enum(enumeration) => &enumeration.proto.full_name,
        }
    }
}
pub(crate) use authored_validation::AuthoredValidationPass;
