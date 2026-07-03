use std::collections::{BTreeMap, BTreeSet};

use crate::language::Language;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiSpec<F: TypeNameFamily = AuthoredNames> {
    pub version: String,
    pub support: SupportSpec,
    pub services: Vec<ServiceSpec<F>>,
    pub external_types: BTreeMap<String, ExternalTypeBindingSpec<F>>,
    pub records: BTreeMap<String, RecordSpec<F>>,
    pub enums: BTreeMap<String, EnumSpec>,
    pub flags: BTreeMap<String, FlagsSpec>,
    pub variants: BTreeMap<String, VariantSpec<F>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiRef(String);

impl ApiRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ApiRef {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for ApiRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

pub trait TypeNameFamily {
    type Record: std::fmt::Debug + Clone + PartialEq;
    type Enum: std::fmt::Debug + Clone + PartialEq;
    type Flags: std::fmt::Debug + Clone + PartialEq;
    type Variant: std::fmt::Debug + Clone + PartialEq;
    type Resource: std::fmt::Debug + Clone + PartialEq;
    type Proto: std::fmt::Debug + Clone + PartialEq;
    type Alias: std::fmt::Debug + Clone + PartialEq;
    type ServiceData: std::fmt::Debug + Clone + PartialEq;
    type RecordData: std::fmt::Debug + Clone + PartialEq;
    type ResourceData: std::fmt::Debug + Clone + PartialEq;
    type OperationData: std::fmt::Debug + Clone + PartialEq;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoredNames;

impl TypeNameFamily for AuthoredNames {
    type Record = ApiRef;
    type Enum = ApiRef;
    type Flags = ApiRef;
    type Variant = ApiRef;
    type Resource = ApiRef;
    type Proto = ApiRef;
    type Alias = ApiRef;
    type ServiceData = ();
    type RecordData = ();
    type ResourceData = ();
    type OperationData = ();
}

pub trait TypeNameMapper<From: TypeNameFamily, To: TypeNameFamily> {
    fn map_record(&mut self, name: From::Record) -> To::Record;
    fn map_enum(&mut self, name: From::Enum) -> To::Enum;
    fn map_flags(&mut self, name: From::Flags) -> To::Flags;
    fn map_variant(&mut self, name: From::Variant) -> To::Variant;
    fn map_resource(&mut self, name: From::Resource) -> To::Resource;
    fn map_proto(&mut self, name: From::Proto) -> To::Proto;
    fn map_alias(&mut self, name: From::Alias) -> To::Alias;
    fn map_service_data(&mut self, name: &str, data: From::ServiceData) -> To::ServiceData;
    fn map_record_data(&mut self, full_name: &str, data: From::RecordData) -> To::RecordData;
    fn map_resource_data(&mut self, name: &str, data: From::ResourceData) -> To::ResourceData;
    fn map_operation_data(&mut self, name: &str, data: From::OperationData) -> To::OperationData;
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
        self.external_types.get(type_name.trim_start_matches('.'))
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
            version: self.version,
            support: self.support,
            services: self
                .services
                .into_iter()
                .map(|service| service.map_names_with(map))
                .collect(),
            external_types: self
                .external_types
                .into_iter()
                .map(|(name, binding)| (name, binding.map_names_with(map)))
                .collect(),
            records: self
                .records
                .into_iter()
                .map(|(name, record)| (name, record.map_names_with(map)))
                .collect(),
            enums: self.enums,
            flags: self.flags,
            variants: self
                .variants
                .into_iter()
                .map(|(name, variant)| (name, variant.map_names_with(map)))
                .collect(),
        }
    }
}

impl ApiSpec<AuthoredNames> {
    pub fn record_for_proto(&self, proto_name: &str) -> Option<&RecordSpec> {
        let proto_name = proto_name.trim_start_matches('.');
        self.records.values().find(|record| {
            matches!(
                record.source_type.as_ref(),
                Some(TypeSpec::External(ExternalTypeSpec::Proto(source_proto)))
                    if source_proto.as_str() == proto_name
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSpec<F: TypeNameFamily = AuthoredNames> {
    pub name: String,
    pub wire_name: String,
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
            wire_name: self.wire_name,
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
    pub wire_name: String,
    pub experimental: bool,
    pub doc: LanguageStringSpec,
    pub return_doc: LanguageStringSpec,
    pub input: TypeSpec<F>,
    pub output: Option<TypeSpec<F>>,
    pub output_resource_type: Option<ExternalTypeSpec<F>>,
    pub output_transform: Option<OperationOutputTransformSpec>,
    pub data: F::OperationData,
}

impl<F: TypeNameFamily> OperationSpec<F> {
    pub fn input_type(&self) -> &TypeSpec<F> {
        &self.input
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
            wire_name: self.wire_name,
            experimental: self.experimental,
            doc: self.doc,
            return_doc: self.return_doc,
            input: self.input.map_names_with(map),
            output: self.output.map(|output| output.map_names_with(map)),
            output_resource_type: self
                .output_resource_type
                .map(|output_resource_type| output_resource_type.map_names_with(map)),
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
    pub source_type: Option<TypeSpec<F>>,
    pub experimental: bool,
    pub required_fields: BTreeSet<String>,
    pub omitted_fields: BTreeSet<String>,
    pub flatten_in_api: bool,
    pub generated_model: GeneratedModelSpec<F>,
    pub data: F::RecordData,
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
        let data = map.map_record_data(&self.full_name, self.data);
        RecordSpec {
            name: self.name,
            full_name: self.full_name,
            source_type: self
                .source_type
                .map(|source_type| source_type.map_names_with(map)),
            experimental: self.experimental,
            required_fields: self.required_fields,
            omitted_fields: self.omitted_fields,
            flatten_in_api: self.flatten_in_api,
            generated_model: self.generated_model.map_names_with(map),
            data,
        }
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
pub struct GeneratedModelSpec<F: TypeNameFamily = AuthoredNames> {
    pub doc: LanguageStringSpec,
    pub declared_fields: Vec<String>,
    pub field_names: BTreeMap<String, String>,
    pub field_docs: BTreeMap<String, LanguageStringSpec>,
    pub field_annotations: BTreeMap<String, LanguageStringSpec>,
    pub field_flattened_annotations: BTreeMap<String, LanguageStringSpec>,
    pub field_types: BTreeMap<String, TypeSpec<F>>,
    pub field_defaults: BTreeMap<String, FieldDefaultSpec>,
    pub field_sources: BTreeMap<String, String>,
    pub functions: BTreeMap<String, FunctionFieldSpec<F>>,
}

impl<F: TypeNameFamily> Default for GeneratedModelSpec<F> {
    fn default() -> Self {
        Self {
            doc: LanguageStringSpec::default(),
            declared_fields: Vec::new(),
            field_names: BTreeMap::new(),
            field_docs: BTreeMap::new(),
            field_annotations: BTreeMap::new(),
            field_flattened_annotations: BTreeMap::new(),
            field_types: BTreeMap::new(),
            field_defaults: BTreeMap::new(),
            field_sources: BTreeMap::new(),
            functions: BTreeMap::new(),
        }
    }
}

impl<F: TypeNameFamily> GeneratedModelSpec<F> {
    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
            && self.declared_fields.is_empty()
            && self.field_names.is_empty()
            && self.field_docs.is_empty()
            && self.field_annotations.is_empty()
            && self.field_flattened_annotations.is_empty()
            && self.field_types.is_empty()
            && self.field_defaults.is_empty()
            && self.field_sources.is_empty()
            && self.functions.is_empty()
    }

    pub fn field_name_override(&self, field_name: &str) -> Option<&str> {
        self.field_names.get(field_name).map(String::as_str)
    }

    pub fn doc(&self) -> &LanguageStringSpec {
        &self.doc
    }

    pub fn field_doc(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.field_docs.get(field_name)
    }

    pub fn field_annotation(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.field_annotations.get(field_name)
    }

    pub fn field_flattened_annotation(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.field_flattened_annotations.get(field_name)
    }

    pub fn field_type(&self, field_name: &str) -> Option<&TypeSpec<F>> {
        self.field_types.get(field_name)
    }

    pub fn field_default(&self, field_name: &str) -> Option<&FieldDefaultSpec> {
        self.field_defaults.get(field_name)
    }

    pub fn field_source(&self, field_name: &str) -> Option<&str> {
        self.field_sources.get(field_name).map(String::as_str)
    }

    pub fn function(&self, field_name: &str) -> Option<&FunctionFieldSpec<F>> {
        self.functions.get(field_name)
    }

    pub fn function_for_args_field(&self, field_name: &str) -> Option<&FunctionFieldSpec<F>> {
        self.functions.values().find(|function| {
            function
                .arg_fields
                .iter()
                .any(|arg_field| arg_field == field_name)
        })
    }

    fn map_names_with<G, M>(self, map: &mut M) -> GeneratedModelSpec<G>
    where
        G: TypeNameFamily,
        M: TypeNameMapper<F, G>,
    {
        GeneratedModelSpec {
            doc: self.doc,
            declared_fields: self.declared_fields,
            field_names: self.field_names,
            field_docs: self.field_docs,
            field_annotations: self.field_annotations,
            field_flattened_annotations: self.field_flattened_annotations,
            field_types: self
                .field_types
                .into_iter()
                .map(|(name, field_type)| (name, field_type.map_names_with(map)))
                .collect(),
            field_defaults: self.field_defaults,
            field_sources: self.field_sources,
            functions: self
                .functions
                .into_iter()
                .map(|(name, function)| (name, function.map_names_with(map)))
                .collect(),
        }
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
    F::Alias: AsRef<str>,
{
    pub fn reference(&self) -> Option<&str> {
        match self {
            ExternalTypeSpec::Proto(type_name) => Some(type_name.as_ref()),
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
