use std::collections::{BTreeMap, BTreeSet};

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use prost_types::FileOptions;

use crate::descriptors::DescriptorIndex;
use crate::error::{Error, Result};
use crate::generator::ModelWireCapabilities;
use crate::language::Language;
use crate::resources::{
    RequestPlan, ResolvedResourceBindingSource, ResolvedResourceMethodBinding,
    ResolvedResourceReturnSpec, ResolvedResourceSpec, resolve_service_resources,
};
use crate::spec::{
    ApiSpec, AuthoredNames, AuthoredResourceType, ExternalTypeSpec, FunctionArgSpec,
    FunctionArgsSpec, FunctionFieldSpec, FunctionResultSpec, JsonModelSpec, LanguageStringSpec,
    ModulePath, OperationSpec, RecordFieldSpec, RecordFieldVisibility, RecordSpec,
    ResourceFieldSpec, ServiceSpec, Symbol, TypeDeclSpec, TypeNameFamily, TypeNameMapper,
    TypeReplacementSpec, TypeSpec, VariantSpec,
};
use crate::workspace::{ApiSpecBranch, ApiSpecLeaf, ApiSpecNode, ApiSpecTree};

mod json;
mod proto;

pub(crate) use proto::{message_model_name, relative_descriptor_name};

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
}

pub(crate) type PlannedSpec = ApiSpec<PlannedTypeFamily>;
pub(crate) type PlannedType = TypeSpec<PlannedTypeFamily>;

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

pub(super) struct ApiPlanner<'a> {
    spec: ApiSpec,
    spec_data: PlannedSpecData,
    mode: PlanningMode,
    language: Language,
    descriptors: &'a DescriptorIndex,
    root_model_capabilities: BTreeMap<String, ModelWireCapabilities>,
    resource_data: IndexMap<String, PlannedResource>,
    record_plans: IndexMap<String, PlannedRecordData>,
    used_json_models: BTreeSet<String>,
    used_enums: BTreeSet<String>,
    used_flags: BTreeSet<String>,
    used_variants: BTreeSet<String>,
}

struct PlannedServiceBuild {
    operations: Vec<OperationSpec<PlannedTypeFamily>>,
}

#[derive(Debug, Clone, Copy)]
struct OperationBindingInfo<'a> {
    name: &'a str,
    direct_return: bool,
}

struct PlannedTypeMapper<'a, 'descriptors> {
    source_spec: &'a ApiSpec,
    planner: &'a mut ApiPlanner<'descriptors>,
}

impl TypeNameMapper<AuthoredNames, PlannedTypeFamily> for PlannedTypeMapper<'_, '_> {
    fn map_spec_data(&mut self, _data: ()) -> PlannedSpecData {
        self.planner.spec_data.clone()
    }

    fn map_record(&mut self, name: crate::spec::Symbol) -> PlannedRecordType {
        let record = self
            .source_spec
            .record(name.as_str())
            .unwrap_or_else(|| panic!("record `{name}` should resolve during planning"));
        self.planner
            .plan_record_type(record, ModelWireCapabilities::default())
    }

    fn map_enum(&mut self, name: crate::spec::Symbol) -> PlannedEnumType {
        let enumeration = self
            .source_spec
            .enum_decl(name.as_str())
            .unwrap_or_else(|| panic!("enum `{name}` should resolve during planning"));
        self.planner.insert_enum(PlannedEnum {
            full_name: enumeration.full_name.clone(),
            proto: None,
            name: enumeration.name.clone(),
            values: enumeration
                .values
                .iter()
                .map(|value| PlannedEnumValue {
                    name: value.name.clone(),
                    number: value.number,
                })
                .collect(),
        });
        PlannedEnumType {
            full_name: enumeration.full_name.clone(),
            name: enumeration.name.clone(),
        }
    }

    fn map_flags(&mut self, name: crate::spec::Symbol) -> PlannedFlagsType {
        let flags = self
            .source_spec
            .flags_decl(name.as_str())
            .unwrap_or_else(|| panic!("flags `{name}` should resolve during planning"));
        self.planner.mark_flags_used(&flags.full_name);
        PlannedFlagsType {
            full_name: flags.full_name.clone(),
            name: flags.name.clone(),
        }
    }

    fn map_variant(&mut self, name: crate::spec::Symbol) -> PlannedVariantType {
        let variant = self
            .source_spec
            .variant(name.as_str())
            .unwrap_or_else(|| panic!("variant `{name}` should resolve during planning"));
        self.planner.mark_variant_used(variant);
        PlannedVariantType {
            full_name: variant.full_name.clone(),
            name: variant.name.clone(),
        }
    }

    fn map_resource(&mut self, resource: AuthoredResourceType) -> PlannedResourceType {
        self.planner.planned_resource_type_from_authored(&resource)
    }

    fn map_proto(&mut self, name: crate::spec::Symbol) -> PlannedProtoType {
        if let Some(message) = self.planner.descriptors.message(name.as_str()) {
            PlannedProtoType::Message(proto::planned_message_reference(message, self.planner))
        } else if let Some(enumeration) = self.planner.descriptors.enumeration(name.as_str()) {
            PlannedProtoType::Enum(proto::planned_enum_reference(
                enumeration,
                &self.planner.spec,
            ))
        } else {
            panic!("proto `{name}` should resolve during planning");
        }
    }

    fn map_json(
        &mut self,
        name: crate::spec::JsonModelSpec<crate::spec::Symbol>,
    ) -> PlannedJsonType {
        PlannedJsonType {
            full_name: name.name.as_str().to_string(),
            module_path: name.name.module_path().cloned(),
            model_name: name.model_name,
            schema: name.schema,
        }
    }

    fn map_alias(&mut self, name: crate::spec::Symbol) -> PlannedAliasType {
        PlannedAliasType {
            name: name.as_str().to_string(),
        }
    }

    fn map_service_data(&mut self, _name: &str, _data: ()) {}

    fn map_record_data(&mut self, full_name: &str, _data: ()) -> PlannedRecordData {
        self.planner
            .record_plans
            .get(full_name)
            .cloned()
            .unwrap_or_default()
    }

    fn map_resource_data(&mut self, name: &str, _data: ()) -> PlannedResource {
        self.planner
            .resource_data
            .get(name)
            .cloned()
            .unwrap_or_else(|| PlannedResource {
                name: name.to_string(),
                type_name: name.to_upper_camel_case(),
                fields: Vec::new(),
                methods: Vec::new(),
            })
    }

    fn map_operation_data(&mut self, _name: &str, _data: ()) -> PlannedOperationData {
        PlannedOperationData::default()
    }

    fn map_field_data(
        &mut self,
        record_full_name: &str,
        field_name: &str,
        _data: (),
    ) -> PlannedFieldData {
        let has_presence = self
            .source_spec
            .record(record_full_name)
            .and_then(|record| {
                proto::record_field_has_presence(record, field_name, self.planner.descriptors)
            });
        PlannedFieldData { has_presence }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanningMode {
    NativeApi,
    DefinitionsOnly,
}

pub(crate) fn build_api_plan_with_mode(
    spec: ApiSpec,
    descriptors: &DescriptorIndex,
    mode: PlanningMode,
    language: Language,
) -> Result<PlannedSpec> {
    ApiPlanner::new(
        spec,
        PlannedSpecData::default(),
        descriptors,
        mode,
        language,
    )?
    .build()
}

pub(crate) fn build_api_plans_for_tree_with_mode(
    tree: ApiSpecTree,
    descriptors: &DescriptorIndex,
    mode: PlanningMode,
    language: Language,
) -> Result<ApiSpecTree<PlannedTypeFamily>> {
    let import_index = module_import_index(&tree);
    plan_tree_node(tree.root, &import_index, descriptors, mode, language)
        .map(|root| ApiSpecTree { root })
}

fn plan_tree_node(
    node: ApiSpecNode,
    import_index: &BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>,
    descriptors: &DescriptorIndex,
    mode: PlanningMode,
    language: Language,
) -> Result<ApiSpecNode<PlannedTypeFamily>> {
    match node {
        ApiSpecNode::Leaf(leaf) => {
            let spec_data = PlannedSpecData {
                module_imports: import_index
                    .get(&leaf.module_path)
                    .cloned()
                    .unwrap_or_default(),
                module_exports: module_export_names(&leaf.spec),
            };
            let planned =
                ApiPlanner::new(leaf.spec, spec_data, descriptors, mode, language)?.build()?;
            Ok(ApiSpecNode::Leaf(ApiSpecLeaf {
                module_path: leaf.module_path,
                source_root: leaf.source_root,
                source_path: leaf.source_path,
                spec: planned,
            }))
        }
        ApiSpecNode::Branch(branch) => {
            let children = branch
                .children
                .into_iter()
                .map(|(name, child)| {
                    Ok((
                        name,
                        plan_tree_node(child, import_index, descriptors, mode, language)?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            Ok(ApiSpecNode::Branch(ApiSpecBranch {
                module_path: branch.module_path,
                children,
            }))
        }
    }
}

fn module_import_index(
    tree: &ApiSpecTree,
) -> BTreeMap<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>> {
    let mut imports = BTreeMap::<ModulePath, BTreeMap<ModulePath, BTreeSet<String>>>::new();
    collect_tree_module_imports(&tree.root, &mut imports);
    imports
}

fn module_export_names(spec: &ApiSpec) -> BTreeSet<String> {
    spec.types
        .iter()
        .filter_map(|(name, decl)| {
            let module_path = match decl {
                TypeDeclSpec::External(binding) => {
                    external_type_module_path(&binding.external_type)
                }
                TypeDeclSpec::Record(_)
                | TypeDeclSpec::Enum(_)
                | TypeDeclSpec::Flags(_)
                | TypeDeclSpec::Variant(_) => None,
            };
            module_path
                .is_none_or(|module_path| module_path == &spec.module_path)
                .then(|| name.clone())
        })
        .collect()
}

fn external_type_module_path(external: &ExternalTypeSpec) -> Option<&ModulePath> {
    match external {
        ExternalTypeSpec::Proto(symbol) => symbol.module_path(),
        ExternalTypeSpec::Json(json_type) => json_type.name.module_path(),
        ExternalTypeSpec::Alias { name, .. } => name.module_path(),
    }
}

fn collect_tree_module_imports(
    node: &ApiSpecNode,
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
    spec: &ApiSpec,
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
    args: &FunctionArgsSpec,
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
    ty: Option<&TypeSpec>,
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
        collect_external_type_module_imports(source_module, wire_type, imports);
    }
}

fn collect_symbol_module_import(
    source_module: &ModulePath,
    symbol: &crate::spec::Symbol,
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

impl<'a> ApiPlanner<'a> {
    fn new(
        spec: ApiSpec,
        spec_data: PlannedSpecData,
        descriptors: &'a DescriptorIndex,
        mode: PlanningMode,
        language: Language,
    ) -> Result<Self> {
        let root_model_capabilities = root_model_capabilities(&spec, descriptors, language)?;
        Ok(Self {
            spec,
            spec_data,
            mode,
            language,
            descriptors,
            root_model_capabilities,
            resource_data: IndexMap::new(),
            record_plans: IndexMap::new(),
            used_json_models: BTreeSet::new(),
            used_enums: BTreeSet::new(),
            used_flags: BTreeSet::new(),
            used_variants: BTreeSet::new(),
        })
    }

    fn build(mut self) -> Result<PlannedSpec> {
        let mut planned_operations = IndexMap::new();

        let services = self.spec.services.clone();
        for service in &services {
            let planned_service = self.plan_service(service)?;
            planned_operations.insert(service.name.clone(), planned_service.operations);
        }

        let mut pruned_spec = self.spec.clone();
        self.prune_spec_to_plan(&mut pruned_spec);
        let mut planned_spec = self.plan_spec(pruned_spec);
        for service in &mut planned_spec.services {
            if let Some(operations) = planned_operations.swap_remove(&service.name) {
                service.operations = operations;
            }
        }

        Ok(planned_spec)
    }

    fn prune_spec_to_plan(&self, spec: &mut ApiSpec) {
        let model_names = self.record_plans.keys().cloned().collect::<BTreeSet<_>>();
        let proto_model_names = self
            .record_plans
            .values()
            .filter_map(|model| model.proto.as_ref().map(|proto| proto.full_name.clone()))
            .collect::<BTreeSet<_>>();

        spec.types.retain(|name, decl| {
            if !self.spec_data.module_exports.is_empty()
                && self.spec_data.module_exports.contains(name)
            {
                return true;
            }
            match decl {
                TypeDeclSpec::Record(record) => {
                    model_names.contains(name)
                        || matches!(
                            record.source_type.as_ref(),
                            Some(ExternalTypeSpec::Proto(proto_name))
                                if model_names.contains(proto_name.as_str())
                        )
                }
                TypeDeclSpec::Enum(_) => self.used_enums.contains(name),
                TypeDeclSpec::Flags(_) => self.used_flags.contains(name),
                TypeDeclSpec::Variant(_) => self.used_variants.contains(name),
                TypeDeclSpec::External(_) => {
                    if !self.spec_data.module_exports.is_empty()
                        && !self.spec_data.module_exports.contains(name)
                    {
                        return false;
                    }
                    model_names.contains(name)
                        || proto_model_names.contains(name)
                        || self.used_json_models.contains(name)
                        || self.spec_data.module_exports.contains(name)
                        || self.used_enums.contains(name)
                        || self.used_flags.contains(name)
                        || self.used_variants.contains(name)
                }
            }
        });
    }

    fn plan_spec(&mut self, spec: ApiSpec) -> PlannedSpec {
        let source_spec = spec.clone();
        let mut planned_spec = spec.map_names(PlannedTypeMapper {
            source_spec: &source_spec,
            planner: self,
        });
        self.resolve_planned_record_field_types(&source_spec, &mut planned_spec);
        planned_spec
    }

    fn resolve_planned_record_field_types(
        &mut self,
        source_spec: &ApiSpec,
        planned_spec: &mut PlannedSpec,
    ) {
        for (record_name, planned_decl) in &mut planned_spec.types {
            let TypeDeclSpec::Record(planned_record) = planned_decl else {
                continue;
            };
            let Some(source_record) = source_spec.record(record_name) else {
                continue;
            };
            let capabilities = planned_record.data.capabilities;
            for (field_name, field) in &mut planned_record.fields {
                let source_field = source_record
                    .fields
                    .get(field_name)
                    .expect("planned record field should exist in source record");
                field.field_type =
                    proto::planned_record_field_type(source_record, field_name, capabilities, self)
                        .unwrap_or_else(|| {
                            self.planned_type_from_authored(
                                source_field.field_type.without_option(),
                            )
                        });
            }
        }
    }

    pub(super) fn insert_enum(&mut self, enumeration: PlannedEnum) {
        self.used_enums.insert(enumeration.full_name);
    }

    fn mark_flags_used(&mut self, full_name: &str) {
        self.used_flags.insert(full_name.to_string());
    }

    fn mark_variant_used(&mut self, variant: &VariantSpec) {
        if !self.used_variants.insert(variant.full_name.clone()) {
            return;
        }

        for case in &variant.cases {
            if let Some(payload) = &case.payload {
                self.planned_value_type_from_authored(payload);
            }
        }
    }

    pub(super) fn insert_record_plan(&mut self, full_name: String, data: PlannedRecordData) {
        self.record_plans.insert(full_name, data);
    }

    fn plan_service(&mut self, service: &ServiceSpec) -> Result<PlannedServiceBuild> {
        let resolved_resources = if self.mode == PlanningMode::NativeApi {
            Some(resolve_service_resources(
                &self.spec,
                service,
                self.descriptors,
            )?)
        } else {
            None
        };
        let operation_builds = service
            .operations
            .iter()
            .map(|operation| {
                self.plan_operation(
                    service,
                    operation,
                    resolved_resources
                        .as_ref()
                        .and_then(|resources| resources.operation_returns.get(&operation.name)),
                )
            })
            .collect::<Result<Vec<_>>>()?;

        let resources = if let Some(resolved_resources) = &resolved_resources {
            let operation_bindings = operation_builds
                .iter()
                .map(|operation| OperationBindingInfo {
                    name: &operation.name,
                    direct_return: operation.output_transform().is_some()
                        || operation.data.output_resource_return.is_some()
                        || operation_output_direct_result(operation),
                })
                .collect::<Vec<_>>();
            resolved_resources
                .resources
                .iter()
                .map(|resource| {
                    let resource_plan =
                        self.plan_resource(service, resource, &operation_bindings)?;
                    Ok((resource.name.clone(), resource_plan))
                })
                .collect::<Result<IndexMap<_, _>>>()?
        } else {
            IndexMap::new()
        };

        self.resource_data.extend(resources);

        Ok(PlannedServiceBuild {
            operations: operation_builds,
        })
    }

    fn plan_operation(
        &mut self,
        service: &ServiceSpec,
        operation: &OperationSpec,
        output_resource_return: Option<&ResolvedResourceReturnSpec>,
    ) -> Result<OperationSpec<PlannedTypeFamily>> {
        let input = self.plan_operation_input(service, operation)?;
        let output = self.plan_operation_output(service, operation, output_resource_return)?;

        Ok(OperationSpec {
            name: operation.name.clone(),
            wire_name: operation.wire_name.clone(),
            experimental: operation.experimental,
            doc: operation.doc.clone(),
            return_doc: operation.return_doc.clone(),
            input,
            output,
            output_transform: operation.output_transform.clone(),
            data: PlannedOperationData {
                output_resource_return: plan_operation_resource_return(output_resource_return),
            },
        })
    }

    fn plan_operation_input(
        &mut self,
        service: &ServiceSpec,
        operation: &OperationSpec,
    ) -> Result<Option<PlannedType>> {
        match operation.input_type() {
            None => Ok(None),
            Some(TypeSpec::External(ExternalTypeSpec::Proto(type_ref))) => {
                let input_message =
                    self.descriptors.message(type_ref.as_str()).ok_or_else(|| {
                        Error::UnknownOperationInputProto {
                            service: service.name.clone(),
                            operation: operation.name.clone(),
                            type_name: type_ref.as_str().to_string(),
                        }
                    })?;
                Ok(Some(proto::planned_type_for_message(
                    input_message,
                    self.root_model_capabilities
                        .get(&input_message.full_name)
                        .copied()
                        .unwrap_or_else(|| self.operation_proto_input_capabilities()),
                    self,
                )))
            }
            Some(TypeSpec::External(ExternalTypeSpec::Json(json_type))) => Ok(Some(
                TypeSpec::External(ExternalTypeSpec::Json(self.map_json_type(json_type))),
            )),
            Some(TypeSpec::Record(record_name)) => {
                let record = self
                    .spec
                    .record(record_name.as_str())
                    .cloned()
                    .ok_or_else(|| Error::UnknownOperationInputProto {
                        service: service.name.clone(),
                        operation: operation.name.clone(),
                        type_name: record_name.as_str().to_string(),
                    })?;
                Ok(Some(TypeSpec::Record(self.plan_record_type(
                    &record,
                    ModelWireCapabilities::default(),
                ))))
            }
            Some(TypeSpec::Resource(resource_name)) => Err(Error::InvalidWit {
                path: std::path::PathBuf::from("<api-plan>"),
                reason: format!(
                    "operation `{}` uses resource `{resource_name}` as an input type, which is not supported for generated operations yet",
                    operation.name
                ),
            }),
            Some(_) => Err(Error::InvalidWit {
                path: std::path::PathBuf::from("<api-plan>"),
                reason: format!("operation `{}` input must be a named type", operation.name),
            }),
        }
    }

    fn plan_operation_output(
        &mut self,
        service: &ServiceSpec,
        operation: &OperationSpec,
        output_resource_return: Option<&ResolvedResourceReturnSpec>,
    ) -> Result<Option<PlannedType>> {
        match operation.output_type() {
            Some(TypeSpec::External(ExternalTypeSpec::Proto(output_proto))) => {
                let output_message =
                    self.descriptors
                        .message(output_proto.as_str())
                        .ok_or_else(|| Error::UnknownOperationOutputProto {
                            service: service.name.clone(),
                            operation: operation.name.clone(),
                            type_name: output_proto.as_str().to_string(),
                        })?;
                Ok(Some(proto::planned_type_for_message(
                    output_message,
                    self.root_model_capabilities
                        .get(&output_message.full_name)
                        .copied()
                        .unwrap_or_else(|| self.operation_proto_output_capabilities()),
                    self,
                )))
            }
            Some(TypeSpec::External(ExternalTypeSpec::Json(json_type))) => Ok(Some(
                TypeSpec::External(ExternalTypeSpec::Json(self.map_json_type(json_type))),
            )),
            Some(TypeSpec::Record(record_name)) => {
                let record = self
                    .spec
                    .record(record_name.as_str())
                    .cloned()
                    .ok_or_else(|| Error::UnknownOperationOutputProto {
                        service: service.name.clone(),
                        operation: operation.name.clone(),
                        type_name: record_name.as_str().to_string(),
                    })?;
                Ok(Some(TypeSpec::Record(self.plan_record_type(
                    &record,
                    ModelWireCapabilities::default(),
                ))))
            }
            Some(TypeSpec::Resource(resource_name)) => {
                if let Some(output_resource_return) = output_resource_return
                    && let Some(resource_output) = self.plan_operation_resource_output_record(
                        service,
                        operation,
                        resource_name,
                        output_resource_return,
                    )?
                {
                    Ok(Some(resource_output))
                } else {
                    Ok(Some(TypeSpec::Resource(
                        self.plan_operation_resource_type(service, operation, resource_name)?,
                    )))
                }
            }
            Some(_) => Err(Error::InvalidWit {
                path: std::path::PathBuf::from("<api-plan>"),
                reason: format!("operation `{}` output must be a named type", operation.name),
            }),
            None => Ok(None),
        }
    }

    fn plan_operation_resource_type(
        &mut self,
        service: &ServiceSpec,
        operation: &OperationSpec,
        resource: &AuthoredResourceType,
    ) -> Result<PlannedResourceType> {
        let Some(output_type) = resource.wire_type.as_ref() else {
            return Ok(PlannedResourceType {
                type_name: resource.as_str().to_upper_camel_case(),
                wire_type: None,
            });
        };

        let wire_type = match output_type {
            ExternalTypeSpec::Proto(output_proto) => {
                let output_message =
                    self.descriptors
                        .message(output_proto.as_str())
                        .ok_or_else(|| Error::UnknownOperationOutputProto {
                            service: service.name.clone(),
                            operation: operation.name.clone(),
                            type_name: output_proto.as_str().to_string(),
                        })?;
                ExternalTypeSpec::Proto(PlannedProtoType::Message(
                    proto::planned_message_reference(output_message, self),
                ))
            }
            ExternalTypeSpec::Json(json_type) => {
                ExternalTypeSpec::Json(self.map_json_type(json_type))
            }
            ExternalTypeSpec::Alias { name, .. } => {
                return Err(Error::UnknownOperationOutputProto {
                    service: service.name.clone(),
                    operation: operation.name.clone(),
                    type_name: name.as_str().to_string(),
                });
            }
        };

        Ok(PlannedResourceType {
            type_name: resource.as_str().to_upper_camel_case(),
            wire_type: Some(wire_type),
        })
    }

    fn plan_operation_resource_output_record(
        &mut self,
        service: &ServiceSpec,
        operation: &OperationSpec,
        resource: &AuthoredResourceType,
        resource_return: &ResolvedResourceReturnSpec,
    ) -> Result<Option<PlannedType>> {
        let Some(ExternalTypeSpec::Proto(output_proto)) = resource.wire_type.as_ref() else {
            return Ok(None);
        };
        let output_message = self
            .descriptors
            .message(output_proto.as_str())
            .ok_or_else(|| Error::UnknownOperationOutputProto {
                service: service.name.clone(),
                operation: operation.name.clone(),
                type_name: output_proto.as_str().to_string(),
            })?;

        let record_name = format!("{}Result", operation.name.to_upper_camel_case());
        let record_full_name = format!("__generated.{}.{}", service.name, record_name);
        if !self.spec.types.contains_key(&record_full_name) {
            let mut fields = IndexMap::new();
            for binding in &resource_return.bindings {
                let ResolvedResourceBindingSource::ResultField {
                    field_name,
                    proto_field_name,
                } = &binding.source
                else {
                    continue;
                };
                fields.insert(
                    proto_field_name.clone(),
                    RecordFieldSpec {
                        name: field_name.clone(),
                        doc: None,
                        annotation: None,
                        flattened_annotation: None,
                        field_type: TypeSpec::String,
                        default_value: None,
                        required: false,
                        visibility: RecordFieldVisibility::Public,
                        function: None,
                        data: (),
                    },
                );
            }

            self.spec.types.insert(
                record_full_name.clone(),
                TypeDeclSpec::Record(RecordSpec {
                    name: record_name,
                    full_name: record_full_name.clone(),
                    doc: LanguageStringSpec::default(),
                    source_type: Some(ExternalTypeSpec::Proto(Symbol::new(
                        output_message.full_name.clone(),
                    ))),
                    experimental: operation.experimental,
                    flatten_in_api: false,
                    fields,
                    data: (),
                }),
            );
        }
        let record = self
            .spec
            .record(&record_full_name)
            .cloned()
            .expect("generated resource output record should exist");
        Ok(Some(TypeSpec::Record(self.plan_record_type(
            &record,
            self.operation_proto_output_capabilities(),
        ))))
    }

    fn operation_proto_input_capabilities(&self) -> ModelWireCapabilities {
        if self.language == Language::Python {
            ModelWireCapabilities::BIDIRECTIONAL
        } else {
            ModelWireCapabilities::TO_WIRE
        }
    }

    fn operation_proto_output_capabilities(&self) -> ModelWireCapabilities {
        if matches!(self.language, Language::Go | Language::Python) {
            ModelWireCapabilities::BIDIRECTIONAL
        } else {
            ModelWireCapabilities::TO_WIRE
        }
    }

    fn plan_resource(
        &mut self,
        service: &ServiceSpec,
        resource: &ResolvedResourceSpec,
        operations: &[OperationBindingInfo<'_>],
    ) -> Result<PlannedResource> {
        let methods = resource
            .methods
            .iter()
            .map(|method| {
                let binding = match &method.binding {
                    ResolvedResourceMethodBinding::Operation {
                        operation_name,
                        request_plan,
                    } => {
                        let operation = operations
                            .iter()
                            .find(|operation| operation.name == operation_name)
                            .ok_or_else(|| Error::InvalidResourceMethod {
                                service: service.name.clone(),
                                resource: resource.name.to_upper_camel_case(),
                                method: method.name.to_string(),
                                reason: format!(
                                    "bound operation `{operation_name}` was not rendered"
                                ),
                            })?;
                        PlannedResourceMethodBindingSpec::Operation {
                            operation_name: operation.name.to_string(),
                            request_plan: request_plan.clone(),
                            direct_return: operation.direct_return,
                        }
                    }
                    ResolvedResourceMethodBinding::Stub => PlannedResourceMethodBindingSpec::Stub,
                };

                Ok(PlannedResourceMethod {
                    name: method.name.clone(),
                    params: method
                        .params
                        .iter()
                        .map(|field| self.planned_resource_field(field))
                        .collect(),
                    result: method
                        .result
                        .as_ref()
                        .map(|result| self.planned_resource_method_result(result)),
                    binding,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(PlannedResource {
            name: resource.name.clone(),
            type_name: resource.name.to_upper_camel_case(),
            fields: resource
                .fields
                .iter()
                .map(|field| self.planned_resource_field(field))
                .collect(),
            methods,
        })
    }

    fn planned_resource_method_result(
        &mut self,
        result: &crate::spec::ResourceResultSpec,
    ) -> PlannedResourceMethodResult {
        let optional = matches!(result.result_type, TypeSpec::Option(_));
        let kind = match result.result_type.without_option() {
            TypeSpec::Resource(resource) => PlannedResourceMethodResultKind::Resource {
                type_name: resource.as_str().to_upper_camel_case(),
            },
            _ => PlannedResourceMethodResultKind::Value(
                self.planned_type_from_authored(&result.result_type),
            ),
        };
        PlannedResourceMethodResult { kind, optional }
    }

    fn planned_resource_field(&mut self, field: &ResourceFieldSpec) -> PlannedResourceField {
        let kind = self.planned_type_from_authored(&field.field_type);
        PlannedResourceField {
            name: field.name.clone(),
            optional: field.optional,
            kind,
            function: field
                .function
                .as_ref()
                .map(|function| self.planned_function_from_authored(function)),
        }
    }

    fn plan_record_type(
        &mut self,
        record: &RecordSpec,
        requested_capabilities: ModelWireCapabilities,
    ) -> PlannedRecordType {
        let planned_record = PlannedRecordType {
            full_name: record.full_name.clone(),
            model_name: record.name.clone(),
        };
        self.ensure_record_model_plan(record, requested_capabilities);
        planned_record
    }

    fn ensure_record_model_plan(
        &mut self,
        record: &RecordSpec,
        requested_capabilities: ModelWireCapabilities,
    ) {
        if let Some(existing) = self.record_plans.get_mut(&record.full_name) {
            let previous_capabilities = existing.capabilities;
            existing.capabilities.merge(requested_capabilities);
            let merged_capabilities = existing.capabilities;
            if previous_capabilities != merged_capabilities {
                self.ensure_record_field_capabilities(record, merged_capabilities);
            }
            return;
        }

        self.insert_record_plan(
            record.full_name.clone(),
            PlannedRecordData {
                proto: proto::record_proto_info(record, &self.spec, self.descriptors),
                capabilities: requested_capabilities,
            },
        );

        self.ensure_record_field_capabilities(record, requested_capabilities);
    }

    fn ensure_record_field_capabilities(
        &mut self,
        record: &RecordSpec,
        requested_capabilities: ModelWireCapabilities,
    ) {
        let field_types = record
            .fields
            .iter()
            .filter(|(_, field)| field.visibility != RecordFieldVisibility::Omitted)
            .map(|(field_name, field)| (field_name.as_str(), &field.field_type))
            .map(|(field_name, field_type)| {
                proto::planned_record_field_type(record, field_name, requested_capabilities, self)
                    .unwrap_or_else(|| self.planned_type_from_authored(field_type))
            })
            .collect::<Vec<_>>();
        for field_type in field_types {
            self.ensure_planned_type_capabilities(&field_type, requested_capabilities);
        }
    }

    fn ensure_planned_type_capabilities(
        &mut self,
        kind: &PlannedType,
        requested_capabilities: ModelWireCapabilities,
    ) {
        match kind {
            TypeSpec::Option(inner) | TypeSpec::List(inner) => {
                self.ensure_planned_type_capabilities(inner, requested_capabilities);
            }
            TypeSpec::Map(key, value) => {
                self.ensure_planned_type_capabilities(key, requested_capabilities);
                self.ensure_planned_type_capabilities(value, requested_capabilities);
            }
            TypeSpec::Record(record_type) => {
                if let Some(record) = self.spec.record(&record_type.full_name).cloned() {
                    self.plan_record_type(&record, requested_capabilities);
                }
            }
            TypeSpec::Resource(resource) => {
                if let Some(wire_type) = &resource.wire_type {
                    self.ensure_planned_type_capabilities(
                        &TypeSpec::External(wire_type.clone()),
                        requested_capabilities,
                    );
                }
            }
            TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(message))) => {
                if let Some(message) = self.descriptors.message(&message.proto.full_name).cloned() {
                    proto::planned_type_for_message(&message, requested_capabilities, self);
                }
            }
            TypeSpec::External(ExternalTypeSpec::Json(_)) => {}
            TypeSpec::External(ExternalTypeSpec::Alias { target, .. }) => {
                self.ensure_planned_type_capabilities(target, requested_capabilities);
            }
            TypeSpec::Result { ok, err } => {
                if let Some(ok) = ok.as_deref() {
                    self.ensure_planned_type_capabilities(ok, requested_capabilities);
                }
                if let Some(err) = err.as_deref() {
                    self.ensure_planned_type_capabilities(err, requested_capabilities);
                }
            }
            TypeSpec::Tuple(items) => {
                for item in items {
                    self.ensure_planned_type_capabilities(item, requested_capabilities);
                }
            }
            _ => {}
        }
    }

    fn planned_function_from_authored(
        &mut self,
        function: &FunctionFieldSpec,
    ) -> FunctionFieldSpec<PlannedTypeFamily> {
        FunctionFieldSpec {
            primary: function.primary,
            result: match &function.result {
                FunctionResultSpec::Annotation(annotation) => {
                    FunctionResultSpec::Annotation(annotation.clone())
                }
                FunctionResultSpec::Authored(authored_type) => FunctionResultSpec::Authored(
                    self.planned_authored_type_override_from_authored(authored_type),
                ),
            },
            args_field: function.args_field.clone(),
            arg_fields: function.arg_fields.clone(),
            args: self.planned_function_args_from_authored(&function.args),
            alternate_type: function.alternate_type.as_ref().map(|alternate_type| {
                self.planned_authored_type_override_from_authored(alternate_type)
            }),
            converter: function.converter.clone(),
            name_extractor: function.name_extractor.clone(),
            call_extractor: function.call_extractor.clone(),
            result_type_parameter: function.result_type_parameter.clone(),
            type_descriptor: function.type_descriptor.clone(),
        }
    }

    fn planned_function_args_from_authored(
        &mut self,
        args: &FunctionArgsSpec,
    ) -> FunctionArgsSpec<PlannedTypeFamily> {
        match args {
            FunctionArgsSpec::Varargs {
                prefix,
                typescript_drop_prefix,
            } => FunctionArgsSpec::Varargs {
                prefix: prefix
                    .iter()
                    .map(|arg| self.planned_function_arg_from_authored(arg))
                    .collect(),
                typescript_drop_prefix: *typescript_drop_prefix,
            },
            FunctionArgsSpec::Fixed(args) => FunctionArgsSpec::Fixed(
                args.iter()
                    .map(|arg| self.planned_function_arg_from_authored(arg))
                    .collect(),
            ),
        }
    }

    fn planned_function_arg_from_authored(
        &mut self,
        arg: &FunctionArgSpec,
    ) -> FunctionArgSpec<PlannedTypeFamily> {
        FunctionArgSpec {
            name: arg.name.clone(),
            field_type: self.planned_authored_type_override_from_authored(&arg.field_type),
        }
    }

    fn planned_external_type_from_authored(
        &mut self,
        authored_type: &ExternalTypeSpec,
    ) -> ExternalTypeSpec<PlannedTypeFamily> {
        match authored_type {
            ExternalTypeSpec::Proto(proto_name) => {
                let planned_proto = if let Some(message) =
                    self.descriptors.message(proto_name.as_str())
                {
                    PlannedProtoType::Message(proto::planned_message_reference(message, self))
                } else if let Some(enumeration) = self.descriptors.enumeration(proto_name.as_str())
                {
                    PlannedProtoType::Enum(proto::planned_enum_reference(enumeration, &self.spec))
                } else {
                    panic!("proto `{proto_name}` should resolve during planning");
                };
                ExternalTypeSpec::Proto(planned_proto)
            }
            ExternalTypeSpec::Json(json_type) => {
                ExternalTypeSpec::Json(self.map_json_type(json_type))
            }
            ExternalTypeSpec::Alias {
                name,
                target,
                type_name,
            } => ExternalTypeSpec::Alias {
                name: PlannedAliasType {
                    name: name.as_str().to_string(),
                },
                target: Box::new(self.planned_authored_type_override_from_authored(target)),
                type_name: type_name.clone(),
            },
        }
    }

    fn planned_resource_type_from_authored(
        &mut self,
        resource: &AuthoredResourceType,
    ) -> PlannedResourceType {
        PlannedResourceType {
            type_name: resource.as_str().to_upper_camel_case(),
            wire_type: resource
                .wire_type
                .as_ref()
                .map(|wire_type| self.planned_external_type_from_authored(wire_type)),
        }
    }

    pub(super) fn planned_type_from_authored(&mut self, authored_type: &TypeSpec) -> PlannedType {
        if let Some(kind) = proto::planned_type_from_authored_proto(authored_type, self) {
            return kind;
        }

        match authored_type {
            TypeSpec::Option(inner) => self.planned_type_from_authored(inner),
            TypeSpec::List(inner) => TypeSpec::List(Box::new(
                self.planned_type_from_authored(inner.without_option()),
            )),
            TypeSpec::Map(key, value) => TypeSpec::Map(
                Box::new(self.planned_type_from_authored(key.without_option())),
                Box::new(self.planned_type_from_authored(value.without_option())),
            ),
            _ => self.planned_value_type_from_authored(authored_type.without_option()),
        }
    }

    pub(super) fn planned_authored_type_override_from_authored(
        &mut self,
        authored_type: &TypeSpec,
    ) -> PlannedType {
        match authored_type {
            TypeSpec::Option(inner) => TypeSpec::Option(Box::new(
                self.planned_authored_type_override_from_authored(inner),
            )),
            TypeSpec::List(inner) => TypeSpec::List(Box::new(
                self.planned_authored_type_override_from_authored(inner),
            )),
            TypeSpec::Tuple(items) => TypeSpec::Tuple(
                items
                    .iter()
                    .map(|item| self.planned_authored_type_override_from_authored(item))
                    .collect(),
            ),
            TypeSpec::Map(key, value) => TypeSpec::Map(
                Box::new(self.planned_authored_type_override_from_authored(key)),
                Box::new(self.planned_authored_type_override_from_authored(value)),
            ),
            TypeSpec::Result { ok, err } => TypeSpec::Result {
                ok: ok
                    .as_ref()
                    .map(|ok| Box::new(self.planned_authored_type_override_from_authored(ok))),
                err: err
                    .as_ref()
                    .map(|err| Box::new(self.planned_authored_type_override_from_authored(err))),
            },
            TypeSpec::External(ExternalTypeSpec::Alias {
                name,
                target,
                type_name,
            }) => TypeSpec::External(ExternalTypeSpec::Alias {
                name: PlannedAliasType {
                    name: name.as_str().to_string(),
                },
                target: Box::new(self.planned_authored_type_override_from_authored(target)),
                type_name: type_name.clone(),
            }),
            TypeSpec::External(ExternalTypeSpec::Json(json_type)) => {
                TypeSpec::External(ExternalTypeSpec::Json(self.map_json_type(json_type)))
            }
            _ => self.planned_value_type_from_authored(authored_type),
        }
    }

    fn planned_value_type_from_authored(&mut self, authored_type: &TypeSpec) -> PlannedType {
        match authored_type {
            TypeSpec::Bool => TypeSpec::Bool,
            TypeSpec::Int(int) => TypeSpec::Int(*int),
            TypeSpec::Float => TypeSpec::Float,
            TypeSpec::String => TypeSpec::String,
            TypeSpec::Bytes => TypeSpec::Bytes,
            TypeSpec::External(ExternalTypeSpec::Proto(proto_name)) => {
                proto::planned_value_type_from_authored_proto(proto_name.as_str(), self)
            }
            TypeSpec::External(ExternalTypeSpec::Json(json_type)) => {
                TypeSpec::External(ExternalTypeSpec::Json(self.map_json_type(json_type)))
            }
            TypeSpec::Record(record_name) => self
                .spec
                .record(record_name.as_str())
                .cloned()
                .map(|record| {
                    TypeSpec::Record(
                        self.plan_record_type(&record, ModelWireCapabilities::default()),
                    )
                })
                .unwrap_or(TypeSpec::String),
            TypeSpec::Enum(enum_name) => self
                .spec
                .enum_decl(enum_name.as_str())
                .cloned()
                .map(|enumeration| {
                    self.insert_enum(PlannedEnum {
                        full_name: enumeration.full_name.clone(),
                        proto: None,
                        name: enumeration.name.clone(),
                        values: enumeration
                            .values
                            .iter()
                            .map(|value| PlannedEnumValue {
                                name: value.name.clone(),
                                number: value.number,
                            })
                            .collect(),
                    });
                    TypeSpec::Enum(PlannedEnumType {
                        full_name: enumeration.full_name.clone(),
                        name: enumeration.name.clone(),
                    })
                })
                .unwrap_or(TypeSpec::String),
            TypeSpec::Flags(flags_name) => self
                .spec
                .flags_decl(flags_name.as_str())
                .cloned()
                .map(|flags| {
                    self.mark_flags_used(&flags.full_name);
                    TypeSpec::Flags(PlannedFlagsType {
                        full_name: flags.full_name.clone(),
                        name: flags.name.clone(),
                    })
                })
                .unwrap_or(TypeSpec::String),
            TypeSpec::Variant(variant_name) => self
                .spec
                .variant(variant_name.as_str())
                .cloned()
                .map(|variant| {
                    self.mark_variant_used(&variant);
                    TypeSpec::Variant(PlannedVariantType {
                        full_name: variant.full_name.clone(),
                        name: variant.name.clone(),
                    })
                })
                .unwrap_or(TypeSpec::String),
            TypeSpec::Resource(resource_name) => TypeSpec::Resource(PlannedResourceType {
                type_name: resource_name.as_str().to_upper_camel_case(),
                wire_type: resource_name
                    .wire_type
                    .as_ref()
                    .map(|wire_type| self.planned_external_type_from_authored(wire_type)),
            }),
            TypeSpec::Option(inner) => {
                self.planned_value_type_from_authored(inner.without_option())
            }
            TypeSpec::Tuple(items) => TypeSpec::Tuple(
                items
                    .iter()
                    .map(|item| self.planned_value_type_from_authored(item))
                    .collect(),
            ),
            TypeSpec::Result { ok, err } => TypeSpec::Result {
                ok: ok
                    .as_ref()
                    .map(|ok| Box::new(self.planned_value_type_from_authored(ok))),
                err: err
                    .as_ref()
                    .map(|err| Box::new(self.planned_value_type_from_authored(err))),
            },
            TypeSpec::External(ExternalTypeSpec::Alias {
                name,
                target,
                type_name,
            }) => {
                let fallback = self.planned_value_type_from_authored(target.without_option());
                TypeSpec::External(ExternalTypeSpec::Alias {
                    name: PlannedAliasType {
                        name: name.as_str().to_string(),
                    },
                    target: Box::new(fallback),
                    type_name: type_name.clone(),
                })
            }
            TypeSpec::List(inner) => TypeSpec::List(Box::new(
                self.planned_type_from_authored(inner.without_option()),
            )),
            TypeSpec::Map(key, value) => TypeSpec::Map(
                Box::new(self.planned_type_from_authored(key.without_option())),
                Box::new(self.planned_type_from_authored(value.without_option())),
            ),
        }
    }

    fn map_json_type(&mut self, json_type: &JsonModelSpec<crate::spec::Symbol>) -> PlannedJsonType {
        self.mark_json_model_used(json_type);
        PlannedJsonType {
            full_name: json_type.name.as_str().to_string(),
            module_path: json_type.name.module_path().cloned(),
            model_name: json_type.model_name.clone(),
            schema: json_type.schema.clone(),
        }
    }
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedEnum {
    pub(crate) full_name: String,
    pub(crate) proto: Option<PlannedProtoTypeInfo>,
    pub(crate) name: String,
    pub(crate) values: Vec<PlannedEnumValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedEnumValue {
    pub(crate) name: String,
    pub(crate) number: i32,
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

fn root_model_capabilities(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    language: Language,
) -> Result<BTreeMap<String, ModelWireCapabilities>> {
    let mut capabilities: BTreeMap<String, ModelWireCapabilities> = BTreeMap::new();

    for service in &spec.services {
        for operation in &service.operations {
            if language == Language::Python
                && let Some(TypeSpec::External(ExternalTypeSpec::Proto(input_proto))) =
                    operation.input_type()
                && let Some(input_message) = descriptors.message(input_proto.as_str())
            {
                capabilities
                    .entry(input_message.full_name.clone())
                    .or_default()
                    .merge(ModelWireCapabilities::BIDIRECTIONAL);
            }

            if matches!(language, Language::Go | Language::Python)
                && let Some(TypeSpec::External(ExternalTypeSpec::Proto(output_proto))) =
                    operation.output_type()
                && let Some(output_message) = descriptors.message(output_proto.as_str())
            {
                capabilities
                    .entry(output_message.full_name.clone())
                    .or_default()
                    .merge(ModelWireCapabilities::BIDIRECTIONAL);
            }
        }
    }

    Ok(capabilities)
}

fn plan_operation_resource_return(
    output_resource_return: Option<&ResolvedResourceReturnSpec>,
) -> Option<PlannedOperationResourceReturn> {
    output_resource_return.map(|resource_return| PlannedOperationResourceReturn {
        resource_type_name: resource_return.resource_name.to_upper_camel_case(),
        bindings: resource_return
            .bindings
            .iter()
            .map(|binding| PlannedOperationResourceFieldBinding {
                field_name: binding.field_name.clone(),
                optional: binding.optional,
                source: binding.source.clone(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::spec::{ExternalTypeSpec, LanguageStringSpec, SupportSpec, Symbol};

    #[test]
    fn descriptor_proto_refs_remain_external_models() {
        let proto_name = "temporal.api.common.v1.RetryPolicy";
        let spec = ApiSpec {
            module_path: crate::spec::ModulePath::default(),
            data: (),
            version: "1.0.0".to_string(),
            support: SupportSpec::default(),
            services: vec![ServiceSpec {
                name: "example-service".to_string(),
                wire_name: "ExampleService".to_string(),
                doc: LanguageStringSpec::default(),
                namespace: LanguageStringSpec::default(),
                operations_class: LanguageStringSpec::default(),
                endpoint: Some("example".to_string()),
                experimental: false,
                delay_load_temporalio_workflow: false,
                operations: vec![OperationSpec {
                    name: "example-operation".to_string(),
                    wire_name: "ExampleOperation".to_string(),
                    experimental: false,
                    doc: LanguageStringSpec::default(),
                    return_doc: LanguageStringSpec::default(),
                    input: Some(TypeSpec::External(ExternalTypeSpec::Proto(Symbol::new(
                        proto_name,
                    )))),
                    output: Some(TypeSpec::External(ExternalTypeSpec::Proto(Symbol::new(
                        proto_name,
                    )))),
                    output_transform: None,
                    data: (),
                }],
                resources: Vec::new(),
                data: (),
            }],
            types: BTreeMap::new(),
        };
        let descriptors = DescriptorIndex::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples/descriptors/temporal_api.bin"),
        )
        .unwrap();

        let plan = build_api_plan_with_mode(
            spec,
            &descriptors,
            PlanningMode::DefinitionsOnly,
            Language::Python,
        )
        .unwrap();

        assert!(plan.records().next().is_none());
        let operation = &plan.services[0].operations[0];
        for model_type in [operation.input_type(), operation.output_type()] {
            let Some(TypeSpec::External(ExternalTypeSpec::Proto(PlannedProtoType::Message(
                message,
            )))) = model_type
            else {
                panic!("descriptor-only proto reference should stay external");
            };
            assert_eq!(message.proto.full_name, proto_name);
        }
    }
}
