use std::collections::BTreeMap;
use std::path::PathBuf;

use indexmap::IndexMap;

use crate::language::Language;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiSpec<F: TypeNameFamily = AuthoredNames> {
    pub module_path: ModulePath,
    pub data: F::SpecData,
    pub version: String,
    pub support: SupportSpec,
    pub services: Vec<ServiceSpec<F>>,
    pub types: BTreeMap<String, TypeDeclSpec<F>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    pub fn child(&self, segment: impl Into<String>) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment.into());
        Self(segments)
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_path_buf(&self) -> PathBuf {
        self.0.iter().collect()
    }

    pub fn as_module_key(&self) -> String {
        self.0.join("/")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol {
    name: String,
    module_path: Option<ModulePath>,
    local_name: String,
}

impl Symbol {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            name: value.clone(),
            module_path: None,
            local_name: value,
        }
    }

    pub fn qualified(
        module_path: ModulePath,
        name: impl Into<String>,
        local_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            module_path: Some(module_path),
            local_name: local_name.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }

    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    pub fn module_path(&self) -> Option<&ModulePath> {
        self.module_path.as_ref()
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredResourceType {
    pub name: Symbol,
    pub wire_type: Option<ExternalTypeSpec<AuthoredNames>>,
}

impl AuthoredResourceType {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Symbol::new(name),
            wire_type: None,
        }
    }

    pub fn as_str(&self) -> &str {
        self.name.as_str()
    }
}

impl AsRef<str> for AuthoredResourceType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for AuthoredResourceType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(formatter)
    }
}

pub trait TypeNameFamily {
    type SpecData: std::fmt::Debug + Clone + PartialEq;
    type Record: std::fmt::Debug + Clone + PartialEq;
    type Enum: std::fmt::Debug + Clone + PartialEq;
    type Flags: std::fmt::Debug + Clone + PartialEq;
    type Variant: std::fmt::Debug + Clone + PartialEq;
    type Resource: std::fmt::Debug + Clone + PartialEq;
    type Proto: std::fmt::Debug + Clone + PartialEq;
    type Json: std::fmt::Debug + Clone + PartialEq;
    type Alias: std::fmt::Debug + Clone + PartialEq;
    type ServiceData: std::fmt::Debug + Clone + PartialEq;
    type RecordData: std::fmt::Debug + Clone + PartialEq;
    type ResourceData: std::fmt::Debug + Clone + PartialEq;
    type OperationData: std::fmt::Debug + Clone + PartialEq;
    type FieldData: std::fmt::Debug + Clone + PartialEq;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoredNames;

impl TypeNameFamily for AuthoredNames {
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
    type ResourceData = ();
    type OperationData = ();
    type FieldData = ();
}

pub trait TypeNameMapper<From: TypeNameFamily, To: TypeNameFamily> {
    fn map_spec_data(&mut self, data: From::SpecData) -> To::SpecData;
    fn map_record(&mut self, name: From::Record) -> To::Record;
    fn map_enum(&mut self, name: From::Enum) -> To::Enum;
    fn map_flags(&mut self, name: From::Flags) -> To::Flags;
    fn map_variant(&mut self, name: From::Variant) -> To::Variant;
    fn map_resource(&mut self, name: From::Resource) -> To::Resource;
    fn map_proto(&mut self, name: From::Proto) -> To::Proto;
    fn map_json(&mut self, name: From::Json) -> To::Json;
    fn map_alias(&mut self, name: From::Alias) -> To::Alias;
    fn map_service_data(&mut self, name: &str, data: From::ServiceData) -> To::ServiceData;
    fn map_record_data(&mut self, full_name: &str, data: From::RecordData) -> To::RecordData;
    fn map_resource_data(&mut self, name: &str, data: From::ResourceData) -> To::ResourceData;
    fn map_operation_data(&mut self, name: &str, data: From::OperationData) -> To::OperationData;
    fn map_field_data(
        &mut self,
        record_full_name: &str,
        field_name: &str,
        data: From::FieldData,
    ) -> To::FieldData;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LanguageImportSpec {
    pub language: Language,
    pub reference: String,
    pub module: String,
    pub name: Option<String>,
    pub type_only: bool,
    pub import_style: LanguageImportStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LanguageImportStyle {
    Module,
    Namespace,
    Named,
}

pub type AuthoredApiSpec = ApiSpec<AuthoredNames>;
pub type AuthoredTypeSpec = TypeSpec<AuthoredNames>;

impl<F: TypeNameFamily> ApiSpec<F> {
    pub fn external_type_binding(&self, type_name: &str) -> Option<&ExternalTypeBindingSpec<F>> {
        match self.types.get(type_name.trim_start_matches('.')) {
            Some(TypeDeclSpec::External(binding)) => Some(binding),
            _ => None,
        }
    }

    pub fn external_types(&self) -> impl Iterator<Item = (&str, &ExternalTypeBindingSpec<F>)> {
        self.types.iter().filter_map(|(name, decl)| match decl {
            TypeDeclSpec::External(binding) => Some((name.as_str(), binding)),
            _ => None,
        })
    }

    pub fn records(&self) -> impl Iterator<Item = (&str, &RecordSpec<F>)> {
        self.types.iter().filter_map(|(name, decl)| match decl {
            TypeDeclSpec::Record(record) => Some((name.as_str(), record)),
            _ => None,
        })
    }

    pub fn record(&self, name: &str) -> Option<&RecordSpec<F>> {
        match self.types.get(name) {
            Some(TypeDeclSpec::Record(record)) => Some(record),
            _ => None,
        }
    }

    pub fn enums(&self) -> impl Iterator<Item = (&str, &EnumSpec)> {
        self.types.iter().filter_map(|(name, decl)| match decl {
            TypeDeclSpec::Enum(enumeration) => Some((name.as_str(), enumeration)),
            _ => None,
        })
    }

    pub fn enum_decl(&self, name: &str) -> Option<&EnumSpec> {
        match self.types.get(name) {
            Some(TypeDeclSpec::Enum(enumeration)) => Some(enumeration),
            _ => None,
        }
    }

    pub fn flags(&self) -> impl Iterator<Item = (&str, &FlagsSpec)> {
        self.types.iter().filter_map(|(name, decl)| match decl {
            TypeDeclSpec::Flags(flags) => Some((name.as_str(), flags)),
            _ => None,
        })
    }

    pub fn flags_decl(&self, name: &str) -> Option<&FlagsSpec> {
        match self.types.get(name) {
            Some(TypeDeclSpec::Flags(flags)) => Some(flags),
            _ => None,
        }
    }

    pub fn variants(&self) -> impl Iterator<Item = (&str, &VariantSpec<F>)> {
        self.types.iter().filter_map(|(name, decl)| match decl {
            TypeDeclSpec::Variant(variant) => Some((name.as_str(), variant)),
            _ => None,
        })
    }

    pub fn variant(&self, name: &str) -> Option<&VariantSpec<F>> {
        match self.types.get(name) {
            Some(TypeDeclSpec::Variant(variant)) => Some(variant),
            _ => None,
        }
    }

    pub fn map_names<G, M>(self, mut map: M) -> ApiSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        self.map_names_with(&mut map)
    }

    fn map_names_with<G, M>(self, map: &mut M) -> ApiSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        ApiSpec {
            module_path: self.module_path,
            data: map.map_spec_data(self.data),
            version: self.version,
            support: self.support,
            services: self
                .services
                .into_iter()
                .map(|service| service.map_names_with(map))
                .collect(),
            types: self
                .types
                .into_iter()
                .map(|(name, decl)| (name, decl.map_names_with(map)))
                .collect(),
        }
    }
}

impl ApiSpec<AuthoredNames> {
    pub fn record_for_proto(&self, proto_name: &str) -> Option<&RecordSpec> {
        let proto_name = proto_name.trim_start_matches('.');
        self.types.values().find_map(|decl| {
            let TypeDeclSpec::Record(record) = decl else {
                return None;
            };
            matches!(
                record.source_type.as_ref(),
                Some(ExternalTypeSpec::Proto(source_proto))
                    if source_proto.as_str() == proto_name
            )
            .then_some(record)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeDeclSpec<F: TypeNameFamily = AuthoredNames> {
    External(ExternalTypeBindingSpec<F>),
    Record(RecordSpec<F>),
    Enum(EnumSpec),
    Flags(FlagsSpec),
    Variant(VariantSpec<F>),
}

impl<F: TypeNameFamily> TypeDeclSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> TypeDeclSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        match self {
            TypeDeclSpec::External(binding) => TypeDeclSpec::External(binding.map_names_with(map)),
            TypeDeclSpec::Record(record) => TypeDeclSpec::Record(record.map_names_with(map)),
            TypeDeclSpec::Enum(enumeration) => TypeDeclSpec::Enum(enumeration),
            TypeDeclSpec::Flags(flags) => TypeDeclSpec::Flags(flags),
            TypeDeclSpec::Variant(variant) => TypeDeclSpec::Variant(variant.map_names_with(map)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    /// A per-language verbatim override of the emitted service code identifier
    /// (`x-<lang>-name` on a JSON-schema `services:` entry). `None` derives the
    /// identifier as usual. Never affects `wire_name`.
    pub code_name: Option<String>,
    pub wire_name: String,
    pub doc: LanguageStringSpec,
    pub namespace: LanguageStringSpec,
    pub operations_class: LanguageStringSpec,
    pub endpoint: Option<String>,
    pub experimental: bool,
    pub delay_load_temporalio_workflow: bool,
    pub operations: Vec<OperationSpec<F>>,
    pub resources: Vec<ResourceSpec<F>>,
    pub data: F::ServiceData,
}

impl<F: TypeNameFamily> ServiceSpec<F> {
    pub fn operation(&self, name: &str) -> Option<&OperationSpec<F>> {
        self.operations
            .iter()
            .find(|operation| operation.name == name)
    }

    pub fn resource(&self, name: &str) -> Option<&ResourceSpec<F>> {
        self.resources.iter().find(|resource| resource.name == name)
    }

    fn map_names_with<G, M>(self, map: &mut M) -> ServiceSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        let data = map.map_service_data(&self.name, self.data);
        ServiceSpec {
            name: self.name,
            code_name: self.code_name,
            wire_name: self.wire_name,
            doc: self.doc,
            namespace: self.namespace,
            operations_class: self.operations_class,
            endpoint: self.endpoint,
            experimental: self.experimental,
            delay_load_temporalio_workflow: self.delay_load_temporalio_workflow,
            operations: self
                .operations
                .into_iter()
                .map(|operation| operation.map_names_with(map))
                .collect(),
            resources: self
                .resources
                .into_iter()
                .map(|resource| resource.map_names_with(map))
                .collect(),
            data,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportSpec {
    pub fragments: BTreeMap<Language, Vec<SupportFragmentSpec>>,
}

impl SupportSpec {
    pub fn fragments_for_language(&self, language: Language) -> &[SupportFragmentSpec] {
        self.fragments
            .get(&language)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportFragmentSpec {
    pub path: String,
    pub contents: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    /// A per-language verbatim override of the emitted operation code identifier
    /// (`x-<lang>-name` on a JSON-schema `operations:` entry). `None` derives the
    /// identifier as usual. Never affects `wire_name`.
    pub code_name: Option<String>,
    pub wire_name: String,
    pub experimental: bool,
    pub doc: LanguageStringSpec,
    pub return_doc: LanguageStringSpec,
    pub input: Option<TypeSpec<F>>,
    pub output: Option<TypeSpec<F>>,
    pub output_transform: Option<OperationOutputTransformSpec>,
    pub data: F::OperationData,
}

impl<F: TypeNameFamily> OperationSpec<F> {
    pub fn input_type(&self) -> Option<&TypeSpec<F>> {
        self.input.as_ref()
    }

    pub fn output_type(&self) -> Option<&TypeSpec<F>> {
        self.output.as_ref()
    }

    pub fn output_transform(&self) -> Option<&OperationOutputTransformSpec> {
        self.output_transform.as_ref()
    }

    fn map_names_with<G, M>(self, map: &mut M) -> OperationSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        let data = map.map_operation_data(&self.name, self.data);
        OperationSpec {
            name: self.name,
            code_name: self.code_name,
            wire_name: self.wire_name,
            experimental: self.experimental,
            doc: self.doc,
            return_doc: self.return_doc,
            input: self.input.map(|input| input.map_names_with(map)),
            output: self.output.map(|output| output.map_names_with(map)),
            output_transform: self.output_transform,
            data,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub fields: Vec<ResourceFieldSpec<F>>,
    pub methods: Vec<ResourceMethodSpec<F>>,
    pub data: F::ResourceData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceFieldSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub optional: bool,
    pub field_type: TypeSpec<F>,
    pub function: Option<FunctionFieldSpec<F>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceMethodSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub params: Vec<ResourceFieldSpec<F>>,
    pub result: Option<ResourceResultSpec<F>>,
    pub operation_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceResultSpec<F: TypeNameFamily = AuthoredNames> {
    pub result_type: TypeSpec<F>,
}

impl<F: TypeNameFamily> ResourceSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> ResourceSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        let data = map.map_resource_data(&self.name, self.data);
        ResourceSpec {
            name: self.name,
            fields: self
                .fields
                .into_iter()
                .map(|field| field.map_names_with(map))
                .collect(),
            methods: self
                .methods
                .into_iter()
                .map(|method| method.map_names_with(map))
                .collect(),
            data,
        }
    }
}

impl<F: TypeNameFamily> ResourceFieldSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> ResourceFieldSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        ResourceFieldSpec {
            name: self.name,
            optional: self.optional,
            field_type: self.field_type.map_names_with(map),
            function: self.function.map(|function| function.map_names_with(map)),
        }
    }
}

impl<F: TypeNameFamily> ResourceMethodSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> ResourceMethodSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        ResourceMethodSpec {
            name: self.name,
            params: self
                .params
                .into_iter()
                .map(|param| param.map_names_with(map))
                .collect(),
            result: self.result.map(|result| result.map_names_with(map)),
            operation_name: self.operation_name,
        }
    }
}

impl<F: TypeNameFamily> ResourceResultSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> ResourceResultSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        ResourceResultSpec {
            result_type: self.result_type.map_names_with(map),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub full_name: String,
    pub doc: LanguageStringSpec,
    pub source_type: Option<ExternalTypeSpec<F>>,
    pub experimental: bool,
    pub flatten_in_api: bool,
    pub fields: IndexMap<String, RecordFieldSpec<F>>,
    pub data: F::RecordData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordFieldSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub doc: Option<LanguageStringSpec>,
    pub annotation: Option<LanguageStringSpec>,
    pub flattened_annotation: Option<LanguageStringSpec>,
    pub field_type: TypeSpec<F>,
    pub default_value: Option<FieldDefaultSpec>,
    pub required: bool,
    pub visibility: RecordFieldVisibility,
    pub function: Option<FunctionFieldSpec<F>>,
    pub data: F::FieldData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordFieldVisibility {
    Public,
    Omitted,
    Sourced { source_expr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSpec {
    pub name: String,
    pub full_name: String,
    pub values: Vec<EnumValueSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValueSpec {
    pub wire_name: String,
    pub name: String,
    pub number: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagsSpec {
    pub name: String,
    pub full_name: String,
    pub flags: Vec<FlagSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagSpec {
    pub name: String,
    pub bit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub full_name: String,
    pub cases: Vec<VariantCaseSpec<F>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantCaseSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub payload: Option<TypeSpec<F>>,
}

impl<F: TypeNameFamily> RecordSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> RecordSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        let full_name = self.full_name;
        let data = map.map_record_data(&full_name, self.data);
        RecordSpec {
            name: self.name,
            full_name: full_name.clone(),
            doc: self.doc,
            source_type: self
                .source_type
                .map(|source_type| source_type.map_names_with(map)),
            experimental: self.experimental,
            flatten_in_api: self.flatten_in_api,
            fields: self
                .fields
                .into_iter()
                .map(|(name, field)| {
                    let field = field.map_names_with(&full_name, &name, map);
                    (name, field)
                })
                .collect(),
            data,
        }
    }

    pub fn doc(&self) -> &LanguageStringSpec {
        &self.doc
    }

    pub fn public_fields(&self) -> impl Iterator<Item = (&str, &RecordFieldSpec<F>)> {
        self.fields
            .iter()
            .filter(|(_, field)| field.visibility == RecordFieldVisibility::Public)
            .map(|(name, field)| (name.as_str(), field))
    }

    pub fn sourced_fields(&self) -> impl Iterator<Item = (&str, &RecordFieldSpec<F>, &str)> {
        self.fields.iter().filter_map(|(name, field)| {
            let RecordFieldVisibility::Sourced { source_expr } = &field.visibility else {
                return None;
            };
            Some((name.as_str(), field, source_expr.as_str()))
        })
    }

    pub fn functions(&self) -> impl Iterator<Item = (&str, &FunctionFieldSpec<F>)> {
        self.fields.iter().filter_map(|(name, field)| {
            field
                .function
                .as_ref()
                .map(|function| (name.as_str(), function))
        })
    }

    pub fn is_empty_model(&self) -> bool {
        self.doc.is_empty()
            && self
                .fields
                .values()
                .all(|field| field.is_empty_model_field())
    }

    pub fn field_name_override(&self, field_name: &str) -> Option<&str> {
        self.fields.get(field_name).map(|field| field.name.as_str())
    }

    pub fn field_doc(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.fields
            .get(field_name)
            .and_then(|field| field.doc.as_ref())
    }

    pub fn field_annotation(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.fields
            .get(field_name)
            .and_then(|field| field.annotation.as_ref())
    }

    pub fn field_flattened_annotation(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.fields
            .get(field_name)
            .and_then(|field| field.flattened_annotation.as_ref())
    }

    pub fn field_type(&self, field_name: &str) -> Option<&TypeSpec<F>> {
        self.fields.get(field_name).map(|field| &field.field_type)
    }

    pub fn field_default(&self, field_name: &str) -> Option<&FieldDefaultSpec> {
        self.fields
            .get(field_name)
            .and_then(|field| field.default_value.as_ref())
    }

    pub fn field_source(&self, field_name: &str) -> Option<&str> {
        self.fields.get(field_name).and_then(|field| {
            let RecordFieldVisibility::Sourced { source_expr } = &field.visibility else {
                return None;
            };
            Some(source_expr.as_str())
        })
    }

    pub fn field_required(&self, field_name: &str) -> bool {
        self.fields
            .get(field_name)
            .is_some_and(|field| field.required)
    }

    pub fn field_omitted(&self, field_name: &str) -> bool {
        self.fields
            .get(field_name)
            .is_some_and(|field| field.visibility == RecordFieldVisibility::Omitted)
    }

    pub fn function(&self, field_name: &str) -> Option<&FunctionFieldSpec<F>> {
        self.fields
            .get(field_name)
            .and_then(|field| field.function.as_ref())
    }

    pub fn function_for_args_field(&self, field_name: &str) -> Option<&FunctionFieldSpec<F>> {
        self.fields.values().find_map(|field| {
            field.function.as_ref().filter(|function| {
                function
                    .arg_fields
                    .iter()
                    .any(|arg_field| arg_field == field_name)
            })
        })
    }
}

impl<F: TypeNameFamily> RecordFieldSpec<F> {
    fn map_names_with<G, M>(
        self,
        record_full_name: &str,
        field_name: &str,
        map: &mut M,
    ) -> RecordFieldSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        let data = map.map_field_data(record_full_name, field_name, self.data);
        RecordFieldSpec {
            name: self.name,
            doc: self.doc,
            annotation: self.annotation,
            flattened_annotation: self.flattened_annotation,
            field_type: self.field_type.map_names_with(map),
            default_value: self.default_value,
            required: self.required,
            visibility: self.visibility,
            function: self.function.map(|function| function.map_names_with(map)),
            data,
        }
    }

    fn is_empty_model_field(&self) -> bool {
        self.name.is_empty()
            && self.doc.is_none()
            && self.annotation.is_none()
            && self.flattened_annotation.is_none()
            && self.default_value.is_none()
            && !self.required
            && self.visibility == RecordFieldVisibility::Public
            && self.function.is_none()
    }
}

impl<F: TypeNameFamily> VariantSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> VariantSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        VariantSpec {
            name: self.name,
            full_name: self.full_name,
            cases: self
                .cases
                .into_iter()
                .map(|case| case.map_names_with(map))
                .collect(),
        }
    }
}

impl<F: TypeNameFamily> VariantCaseSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> VariantCaseSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        VariantCaseSpec {
            name: self.name,
            payload: self.payload.map(|payload| payload.map_names_with(map)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationOutputTransformSpec {
    pub type_name: LanguageStringSpec,
    pub transform: LanguageStringSpec,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageStringSpec {
    pub default: Option<String>,
    pub by_language: BTreeMap<Language, String>,
    pub default_import: Option<String>,
    pub imports: BTreeMap<Language, String>,
}

impl LanguageStringSpec {
    pub fn for_language(&self, language: Language) -> Option<&str> {
        self.by_language
            .get(&language)
            .or(self.default.as_ref())
            .map(String::as_str)
    }

    pub fn import_for_language(&self, language: Language) -> Option<&str> {
        self.imports
            .get(&language)
            .or(self.default_import.as_ref())
            .map(String::as_str)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.default.is_none() && self.by_language.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalTypeBindingSpec<F: TypeNameFamily = AuthoredNames> {
    pub external_type: ExternalTypeSpec<F>,
    pub reference: LanguageStringSpec,
    pub type_name: LanguageStringSpec,
    pub replacement: Option<TypeReplacementSpec>,
    pub authored_type: Option<TypeSpec<F>>,
}

impl<F: TypeNameFamily> ExternalTypeBindingSpec<F> {
    pub fn type_name(&self) -> &LanguageStringSpec {
        &self.type_name
    }

    pub fn reference(&self) -> &LanguageStringSpec {
        &self.reference
    }

    pub fn replacement(&self) -> Option<&TypeReplacementSpec> {
        self.replacement.as_ref()
    }

    fn map_names_with<G, M>(self, map: &mut M) -> ExternalTypeBindingSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        ExternalTypeBindingSpec {
            external_type: self.external_type.map_names_with(map),
            reference: self.reference,
            type_name: self.type_name,
            replacement: self.replacement,
            authored_type: self
                .authored_type
                .map(|authored_type| authored_type.map_names_with(map)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeReplacementSpec {
    pub type_name: LanguageStringSpec,
    pub from_proto: LanguageStringSpec,
    pub to_proto: LanguageStringSpec,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonModelSpec<N> {
    pub name: N,
    pub model_name: String,
    pub schema: serde_json::Value,
}

impl<N: AsRef<str>> AsRef<str> for JsonModelSpec<N> {
    fn as_ref(&self) -> &str {
        self.name.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefaultSpec {
    pub enum_case: String,
    pub enum_value: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeSpec<F: TypeNameFamily = AuthoredNames> {
    Bool,
    Int(IntSpec),
    Float,
    String,
    Bytes,
    Option(Box<TypeSpec<F>>),
    List(Box<TypeSpec<F>>),
    Tuple(Vec<TypeSpec<F>>),
    Map(Box<TypeSpec<F>>, Box<TypeSpec<F>>),
    Result {
        ok: Option<Box<TypeSpec<F>>>,
        err: Option<Box<TypeSpec<F>>>,
    },
    Record(F::Record),
    Enum(F::Enum),
    Flags(F::Flags),
    Variant(F::Variant),
    Resource(F::Resource),
    External(ExternalTypeSpec<F>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntSpec {
    I32,
    I64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExternalTypeSpec<F: TypeNameFamily = AuthoredNames> {
    Proto(F::Proto),
    Json(F::Json),
    Alias {
        name: F::Alias,
        target: Box<TypeSpec<F>>,
        type_name: LanguageStringSpec,
    },
}

impl<F: TypeNameFamily> TypeSpec<F> {
    pub fn map_names<G, M>(self, mut map: M) -> TypeSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        self.map_names_with(&mut map)
    }

    fn map_names_with<G, M>(self, map: &mut M) -> TypeSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        match self {
            TypeSpec::Bool => TypeSpec::Bool,
            TypeSpec::Int(int) => TypeSpec::Int(int),
            TypeSpec::Float => TypeSpec::Float,
            TypeSpec::String => TypeSpec::String,
            TypeSpec::Bytes => TypeSpec::Bytes,
            TypeSpec::Option(inner) => TypeSpec::Option(Box::new(inner.map_names_with(map))),
            TypeSpec::List(inner) => TypeSpec::List(Box::new(inner.map_names_with(map))),
            TypeSpec::Tuple(items) => TypeSpec::Tuple(
                items
                    .into_iter()
                    .map(|item| item.map_names_with(map))
                    .collect(),
            ),
            TypeSpec::Map(key, value) => TypeSpec::Map(
                Box::new(key.map_names_with(map)),
                Box::new(value.map_names_with(map)),
            ),
            TypeSpec::Result { ok, err } => TypeSpec::Result {
                ok: ok.map(|ok| Box::new(ok.map_names_with(map))),
                err: err.map(|err| Box::new(err.map_names_with(map))),
            },
            TypeSpec::Record(type_name) => TypeSpec::Record(map.map_record(type_name)),
            TypeSpec::Enum(type_name) => TypeSpec::Enum(map.map_enum(type_name)),
            TypeSpec::Flags(type_name) => TypeSpec::Flags(map.map_flags(type_name)),
            TypeSpec::Variant(type_name) => TypeSpec::Variant(map.map_variant(type_name)),
            TypeSpec::Resource(type_name) => TypeSpec::Resource(map.map_resource(type_name)),
            TypeSpec::External(external) => TypeSpec::External(external.map_names_with(map)),
        }
    }

    pub(crate) fn without_option(&self) -> &TypeSpec<F> {
        match self {
            TypeSpec::Option(inner) => inner.without_option(),
            _ => self,
        }
    }

    pub(crate) fn validation_type(&self) -> &TypeSpec<F> {
        match self {
            TypeSpec::External(ExternalTypeSpec::Alias { target, .. }) => target.validation_type(),
            _ => self,
        }
    }
}

impl<F> TypeSpec<F>
where
    F: TypeNameFamily,
    F::Record: AsRef<str>,
    F::Enum: AsRef<str>,
    F::Flags: AsRef<str>,
    F::Variant: AsRef<str>,
    F::Resource: AsRef<str>,
    F::Proto: AsRef<str>,
    F::Json: AsRef<str>,
    F::Alias: AsRef<str>,
{
    pub fn reference(&self) -> Option<&str> {
        match self {
            TypeSpec::Record(type_name) => Some(type_name.as_ref()),
            TypeSpec::Enum(type_name) => Some(type_name.as_ref()),
            TypeSpec::Flags(type_name) => Some(type_name.as_ref()),
            TypeSpec::Variant(type_name) => Some(type_name.as_ref()),
            TypeSpec::Resource(type_name) => Some(type_name.as_ref()),
            TypeSpec::External(external) => external.reference(),
            _ => None,
        }
    }

    pub(crate) fn to_type_string(&self) -> String {
        match self {
            TypeSpec::Bool => "bool".to_string(),
            TypeSpec::Int(IntSpec::I32) => "s32".to_string(),
            TypeSpec::Int(IntSpec::I64) => "s64".to_string(),
            TypeSpec::Float => "float64".to_string(),
            TypeSpec::String => "string".to_string(),
            TypeSpec::Bytes => "bytes".to_string(),
            TypeSpec::Option(inner) => {
                format!("option<{}>", inner.to_type_string())
            }
            TypeSpec::List(inner) => format!("list<{}>", inner.to_type_string()),
            TypeSpec::Tuple(items) => format!(
                "tuple<{}>",
                items
                    .iter()
                    .map(TypeSpec::to_type_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeSpec::Map(key, value) => {
                format!("map<{}, {}>", key.to_type_string(), value.to_type_string())
            }
            TypeSpec::Result { ok, err } => match (ok, err) {
                (Some(ok), Some(err)) => {
                    format!("result<{}, {}>", ok.to_type_string(), err.to_type_string())
                }
                (Some(ok), None) => format!("result<{}>", ok.to_type_string()),
                (None, Some(err)) => format!("result<_, {}>", err.to_type_string()),
                (None, None) => "result".to_string(),
            },
            TypeSpec::Record(type_name) => type_name.as_ref().to_string(),
            TypeSpec::Enum(type_name) => type_name.as_ref().to_string(),
            TypeSpec::Flags(type_name) => type_name.as_ref().to_string(),
            TypeSpec::Variant(type_name) => type_name.as_ref().to_string(),
            TypeSpec::Resource(type_name) => type_name.as_ref().to_string(),
            TypeSpec::External(external) => external.to_type_string(),
        }
    }
}

impl<F: TypeNameFamily> ExternalTypeSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> ExternalTypeSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        match self {
            ExternalTypeSpec::Proto(type_name) => ExternalTypeSpec::Proto(map.map_proto(type_name)),
            ExternalTypeSpec::Json(type_name) => ExternalTypeSpec::Json(map.map_json(type_name)),
            ExternalTypeSpec::Alias {
                name,
                target,
                type_name,
            } => ExternalTypeSpec::Alias {
                name: map.map_alias(name),
                target: Box::new(target.map_names_with(map)),
                type_name,
            },
        }
    }
}

impl<F> ExternalTypeSpec<F>
where
    F: TypeNameFamily,
    F::Proto: AsRef<str>,
    F::Json: AsRef<str>,
    F::Alias: AsRef<str>,
{
    pub fn reference(&self) -> Option<&str> {
        match self {
            ExternalTypeSpec::Proto(type_name) => Some(type_name.as_ref()),
            ExternalTypeSpec::Json(type_name) => Some(type_name.as_ref()),
            ExternalTypeSpec::Alias { name, .. } => Some(name.as_ref()),
        }
    }

    pub(crate) fn to_type_string(&self) -> String {
        self.reference().unwrap_or_default().to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionFieldSpec<F: TypeNameFamily = AuthoredNames> {
    pub primary: bool,
    pub result: FunctionResultSpec<F>,
    pub args_field: String,
    pub arg_fields: Vec<String>,
    pub args: FunctionArgsSpec<F>,
    pub alternate_type: Option<TypeSpec<F>>,
    pub converter: Option<String>,
    pub name_extractor: Option<String>,
    pub call_extractor: Option<String>,
    pub result_type_parameter: Option<String>,
    pub type_descriptor: Option<FunctionTypeDescriptorSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionArgsSpec<F: TypeNameFamily = AuthoredNames> {
    Varargs {
        prefix: Vec<FunctionArgSpec<F>>,
        typescript_drop_prefix: bool,
    },
    Fixed(Vec<FunctionArgSpec<F>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionArgSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub field_type: TypeSpec<F>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionResultSpec<F: TypeNameFamily = AuthoredNames> {
    Authored(TypeSpec<F>),
    Annotation(LanguageStringSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionTypeDescriptorSpec {
    pub value_type: LanguageStringSpec,
    pub args_type: LanguageStringSpec,
}

impl<F: TypeNameFamily> FunctionFieldSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> FunctionFieldSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        FunctionFieldSpec {
            primary: self.primary,
            result: self.result.map_names_with(map),
            args_field: self.args_field,
            arg_fields: self.arg_fields,
            args: self.args.map_names_with(map),
            alternate_type: self
                .alternate_type
                .map(|alternate_type| alternate_type.map_names_with(map)),
            converter: self.converter,
            name_extractor: self.name_extractor,
            call_extractor: self.call_extractor,
            result_type_parameter: self.result_type_parameter,
            type_descriptor: self.type_descriptor,
        }
    }
}

impl<F: TypeNameFamily> FunctionArgsSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> FunctionArgsSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        match self {
            FunctionArgsSpec::Varargs {
                prefix,
                typescript_drop_prefix,
            } => FunctionArgsSpec::Varargs {
                prefix: prefix
                    .into_iter()
                    .map(|arg| arg.map_names_with(map))
                    .collect(),
                typescript_drop_prefix,
            },
            FunctionArgsSpec::Fixed(args) => FunctionArgsSpec::Fixed(
                args.into_iter()
                    .map(|arg| arg.map_names_with(map))
                    .collect(),
            ),
        }
    }
}

impl<F: TypeNameFamily> FunctionArgSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> FunctionArgSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        FunctionArgSpec {
            name: self.name,
            field_type: self.field_type.map_names_with(map),
        }
    }
}

impl<F: TypeNameFamily> FunctionResultSpec<F> {
    fn map_names_with<G, M>(self, map: &mut M) -> FunctionResultSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        match self {
            FunctionResultSpec::Authored(authored_type) => {
                FunctionResultSpec::Authored(authored_type.map_names_with(map))
            }
            FunctionResultSpec::Annotation(annotation) => {
                FunctionResultSpec::Annotation(annotation)
            }
        }
    }
}
