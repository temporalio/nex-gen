use std::collections::BTreeMap;

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use prost_types::FieldDescriptorProto;
use prost_types::field_descriptor_proto::{Label, Type};

use crate::descriptors::{DescriptorIndex, EnumMetadata, MessageMetadata};
use crate::error::{Error, Result};
use crate::generator::ModelCapabilities;
use crate::resources::{
    RequestPlan, ResolvedResourceBindingSource, ResolvedResourceMethodBinding,
    ResolvedResourceReturnSpec, ResolvedResourceSpec, resolve_service_resources,
};
use crate::spec::{
    ApiSpec, AuthoredFieldTypeSpec, FunctionFieldSpec, GeneratedModelSpec, LanguageStringSpec,
    OperationOutputTransformSpec, OperationSpec, ResourceFieldSpec, ServiceSpec,
    TypeReplacementSpec, WitEnumSpec, WitFlagsSpec, WitRecordSpec, WitVariantSpec,
    WithArgumentsFieldSpec,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct ApiPlan {
    pub(crate) services: Vec<PlannedService>,
    pub(crate) enums: IndexMap<String, PlannedEnum>,
    pub(crate) flags: IndexMap<String, PlannedFlags>,
    pub(crate) variants: IndexMap<String, PlannedVariant>,
    pub(crate) models: IndexMap<String, PlannedModel>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedService {
    pub(crate) name: String,
    pub(crate) wire_name: String,
    pub(crate) endpoint: String,
    pub(crate) experimental: bool,
    pub(crate) delay_load_temporalio_workflow: bool,
    pub(crate) operations: Vec<PlannedOperation>,
    pub(crate) resources: Vec<PlannedResource>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedOperation {
    pub(crate) name: String,
    pub(crate) wire_name: String,
    pub(crate) experimental: bool,
    pub(crate) doc: LanguageStringSpec,
    pub(crate) return_doc: LanguageStringSpec,
    pub(crate) input: PlannedMessageType,
    pub(crate) output: PlannedOperationOutput,
    pub(crate) output_transform: Option<OperationOutputTransformSpec>,
    pub(crate) output_resource_return: Option<PlannedOperationResourceReturn>,
    pub(crate) output_direct_result: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum PlannedOperationOutput {
    Message(PlannedMessageType),
    Resource { type_name: String },
    None,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedOperationResourceReturn {
    pub(crate) resource_type_name: String,
    pub(crate) bindings: Vec<PlannedOperationResourceFieldBinding>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedOperationResourceFieldBinding {
    pub(crate) field_name: String,
    pub(crate) optional: bool,
    pub(crate) source: ResolvedResourceBindingSource,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedResource {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) fields: Vec<PlannedResourceField>,
    pub(crate) methods: Vec<PlannedResourceMethod>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedResourceField {
    pub(crate) name: String,
    pub(crate) optional: bool,
    pub(crate) kind: PlannedFieldKind,
    pub(crate) function: Option<FunctionFieldSpec>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedResourceMethod {
    pub(crate) name: String,
    pub(crate) params: Vec<PlannedResourceField>,
    pub(crate) result: Option<PlannedResourceMethodResult>,
    pub(crate) binding: PlannedResourceMethodBindingSpec,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedResourceMethodResult {
    pub(crate) kind: PlannedResourceMethodResultKind,
    pub(crate) optional: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum PlannedResourceMethodResultKind {
    Resource { type_name: String },
    Value(PlannedFieldKind),
}

#[derive(Debug, Clone)]
pub(crate) enum PlannedResourceMethodBindingSpec {
    Operation {
        operation_name: String,
        request_plan: RequestPlan,
        direct_return: bool,
    },
    Stub,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedTypeInfo {
    pub(crate) full_name: String,
    pub(crate) package: String,
    pub(crate) file_name: Option<String>,
    pub(crate) proto_type_name: crate::spec::LanguageStringSpec,
}

impl PlannedTypeInfo {
    fn from_message(message: &MessageMetadata, spec: &ApiSpec) -> Self {
        Self {
            full_name: message.full_name.clone(),
            package: message.package.clone(),
            file_name: message.file_name.clone(),
            proto_type_name: spec
                .type_override(&message.full_name)
                .map(|type_override| type_override.proto_type_name().clone())
                .unwrap_or_default(),
        }
    }

    fn from_enum(enumeration: &EnumMetadata, spec: &ApiSpec) -> Self {
        Self {
            full_name: enumeration.full_name.clone(),
            package: enumeration.package.clone(),
            file_name: enumeration.file_name.clone(),
            proto_type_name: spec
                .type_override(&enumeration.full_name)
                .map(|type_override| type_override.proto_type_name().clone())
                .unwrap_or_default(),
        }
    }

    fn from_wit_enum(enumeration: &WitEnumSpec) -> Self {
        Self {
            full_name: enumeration.full_name.clone(),
            package: String::new(),
            file_name: None,
            proto_type_name: crate::spec::LanguageStringSpec::default(),
        }
    }

    fn from_wit_flags(flags: &WitFlagsSpec) -> Self {
        Self {
            full_name: flags.full_name.clone(),
            package: String::new(),
            file_name: None,
            proto_type_name: crate::spec::LanguageStringSpec::default(),
        }
    }

    fn from_wit_record(record: &WitRecordSpec) -> Self {
        Self {
            full_name: record.full_name.clone(),
            package: String::new(),
            file_name: None,
            proto_type_name: crate::spec::LanguageStringSpec::default(),
        }
    }

    fn from_wit_variant(variant: &WitVariantSpec) -> Self {
        Self {
            full_name: variant.full_name.clone(),
            package: String::new(),
            file_name: None,
            proto_type_name: crate::spec::LanguageStringSpec::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedEnum {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) name: String,
    pub(crate) values: Vec<PlannedEnumValue>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedEnumValue {
    pub(crate) name: String,
    pub(crate) number: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedFlags {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) name: String,
    pub(crate) flags: Vec<PlannedFlag>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedFlag {
    pub(crate) name: String,
    pub(crate) bit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedVariant {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) name: String,
    pub(crate) cases: Vec<PlannedVariantCase>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedVariantCase {
    pub(crate) name: String,
    pub(crate) payload: Option<PlannedValueType>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedModel {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) name: String,
    pub(crate) capabilities: ModelCapabilities,
    pub(crate) flatten_in_api: bool,
    pub(crate) experimental: bool,
    pub(crate) generated_model: GeneratedModelSpec,
    pub(crate) fields: Vec<PlannedField>,
    pub(crate) sourced_fields: Vec<PlannedSourcedField>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedField {
    pub(crate) owner_name: String,
    pub(crate) proto_name: String,
    pub(crate) authored_name: String,
    pub(crate) doc: Option<LanguageStringSpec>,
    pub(crate) annotation_override: Option<crate::spec::LanguageStringSpec>,
    pub(crate) default_value: Option<PlannedFieldDefault>,
    pub(crate) required: bool,
    pub(crate) has_presence: bool,
    pub(crate) role: PlannedFieldRole,
    pub(crate) function: Option<FunctionFieldSpec>,
    pub(crate) function_args: bool,
    pub(crate) with_arguments: Option<WithArgumentsFieldSpec>,
    pub(crate) with_arguments_args: bool,
    pub(crate) kind: PlannedFieldKind,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedFieldDefault {
    pub(crate) enum_case: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedSourcedField {
    pub(crate) proto_name: String,
    pub(crate) source_expr: String,
    pub(crate) kind: PlannedFieldKind,
}

#[derive(Debug, Clone)]
pub(crate) enum PlannedFieldRole {
    Plain,
    Function(FunctionFieldSpec),
    FunctionArgs(FunctionFieldSpec),
    WithArguments,
    WithArgumentsArgs,
}

#[derive(Debug, Clone)]
pub(crate) enum PlannedFieldKind {
    Singular(PlannedValueType),
    Repeated(PlannedValueType),
    Map {
        key: PlannedValueType,
        value: PlannedValueType,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum PlannedValueType {
    Scalar(PlannedScalarType),
    Enum(PlannedEnumType),
    Flags(PlannedFlagsType),
    Variant(PlannedVariantType),
    Message(PlannedMessageType),
    Tuple(Vec<PlannedValueType>),
    Result {
        ok: Option<Box<PlannedValueType>>,
        err: Option<Box<PlannedValueType>>,
    },
    External {
        type_name: crate::spec::LanguageStringSpec,
        fallback: Box<PlannedValueType>,
    },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlannedScalarType {
    Float,
    Int32,
    Int64,
    Bool,
    String,
    Bytes,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedEnumType {
    pub(crate) info: Option<PlannedTypeInfo>,
    pub(crate) name: Option<String>,
    pub(crate) replacement: Option<TypeReplacementSpec>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedFlagsType {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedVariantType {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedMessageType {
    pub(crate) info: PlannedTypeInfo,
    pub(crate) model_name: String,
    pub(crate) replacement: Option<TypeReplacementSpec>,
    pub(crate) authored_type: Option<AuthoredFieldTypeSpec>,
    pub(crate) source: PlannedMessageSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlannedMessageSource {
    Proto,
    Wit,
}

pub(crate) fn message_model_name(full_name: &str) -> String {
    full_name
        .rsplit('.')
        .next()
        .expect("descriptor names should not be empty")
        .to_upper_camel_case()
        .to_string()
}

fn planned_proto_model_name(message: &MessageMetadata, spec: &ApiSpec) -> String {
    spec.type_override(&message.full_name)
        .and_then(|type_override| type_override.model_name())
        .map(str::to_string)
        .unwrap_or_else(|| message_model_name(&message.full_name))
}

pub(crate) fn enum_name(full_name: &str) -> String {
    full_name
        .rsplit('.')
        .next()
        .expect("descriptor names should not be empty")
        .to_string()
}

pub(crate) fn relative_descriptor_name(full_name: &str, package: &str) -> String {
    if package.is_empty() {
        full_name.to_string()
    } else {
        full_name
            .strip_prefix(&format!("{package}."))
            .unwrap_or(full_name)
            .to_string()
    }
}

pub(crate) fn field_has_presence(field: &FieldDescriptorProto, field_type: Option<Type>) -> bool {
    matches!(field_type, Some(Type::Message))
        || field.proto3_optional.unwrap_or(false)
        || field.oneof_index.is_some()
}

pub(crate) fn field_label(field: &FieldDescriptorProto) -> Option<Label> {
    field.label.and_then(|label| Label::try_from(label).ok())
}

pub(crate) fn field_type(field: &FieldDescriptorProto) -> Option<Type> {
    field
        .r#type
        .and_then(|field_type| Type::try_from(field_type).ok())
}

pub(crate) fn build_api_plan(spec: &ApiSpec, descriptors: &DescriptorIndex) -> Result<ApiPlan> {
    let mut plan = ApiPlan::default();
    let root_model_capabilities = root_model_capabilities(spec, descriptors)?;

    for service in &spec.services {
        let planned_service = plan_service(
            service,
            spec,
            descriptors,
            &root_model_capabilities,
            &mut plan,
        )?;
        plan.services.push(planned_service);
    }

    Ok(plan)
}

pub(crate) fn plan_message_type(
    message: &MessageMetadata,
    requested_capabilities: ModelCapabilities,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedMessageType {
    let planned_message = planned_message_reference(message, spec);
    if planned_message.replacement.is_none() && planned_message.authored_type.is_none() {
        ensure_model_plan(message, requested_capabilities, spec, descriptors, plan);
    }
    planned_message
}

fn root_model_capabilities(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
) -> Result<BTreeMap<String, ModelCapabilities>> {
    let mut capabilities: BTreeMap<String, ModelCapabilities> = BTreeMap::new();

    for service in &spec.services {
        for operation in &service.operations {
            let Some(input_proto) = operation.input_proto() else {
                continue;
            };
            let input_message = descriptors.message(input_proto).ok_or_else(|| {
                Error::UnknownOperationInputProto {
                    service: service.name.clone(),
                    operation: operation.name.clone(),
                    type_name: input_proto.to_string(),
                }
            })?;
            capabilities
                .entry(input_message.full_name.clone())
                .or_default()
                .merge(ModelCapabilities::TO_PROTO_ONLY);

            if operation.output_transform().is_some() || operation.output_resource().is_some() {
                continue;
            }

            let Some(output_proto) = operation.output_proto() else {
                continue;
            };
            let output_message = descriptors.message(output_proto).ok_or_else(|| {
                Error::UnknownOperationOutputProto {
                    service: service.name.clone(),
                    operation: operation.name.clone(),
                    type_name: output_proto.to_string(),
                }
            })?;
            capabilities
                .entry(output_message.full_name.clone())
                .or_default()
                .merge(ModelCapabilities::BIDIRECTIONAL);
        }
    }

    Ok(capabilities)
}

fn plan_service(
    service: &ServiceSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    root_model_capabilities: &BTreeMap<String, ModelCapabilities>,
    plan: &mut ApiPlan,
) -> Result<PlannedService> {
    let endpoint = service
        .endpoint
        .as_deref()
        .ok_or_else(|| Error::MissingServiceEndpoint {
            service: service.name.clone(),
        })?
        .to_string();

    let resolved_resources = resolve_service_resources(spec, service, descriptors)?;
    let operations = service
        .operations
        .iter()
        .map(|operation| {
            plan_operation(
                &service.name,
                operation,
                spec,
                descriptors,
                root_model_capabilities,
                plan,
                resolved_resources.operation_returns.get(&operation.name),
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let operation_bindings = operations
        .iter()
        .map(|operation| OperationBindingInfo {
            name: &operation.name,
            direct_return: operation.output_transform.is_some()
                || operation.output_resource_return.is_some()
                || operation.output_direct_result,
        })
        .collect::<Vec<_>>();
    let resources = resolved_resources
        .resources
        .iter()
        .map(|resource| {
            plan_resource(
                service,
                resource,
                &operation_bindings,
                spec,
                descriptors,
                plan,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(PlannedService {
        name: service.name.clone(),
        wire_name: service.wire_name.clone(),
        endpoint,
        experimental: service.experimental,
        delay_load_temporalio_workflow: service.delay_load_temporalio_workflow,
        operations,
        resources,
    })
}

fn plan_operation(
    service_name: &str,
    operation: &OperationSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    root_model_capabilities: &BTreeMap<String, ModelCapabilities>,
    plan: &mut ApiPlan,
    output_resource_return: Option<&ResolvedResourceReturnSpec>,
) -> Result<PlannedOperation> {
    let input = plan_operation_input(
        service_name,
        operation,
        spec,
        descriptors,
        root_model_capabilities,
        plan,
    )?;
    let output = plan_operation_output(
        service_name,
        operation,
        spec,
        descriptors,
        root_model_capabilities,
        plan,
        output_resource_return,
    )?;

    Ok(PlannedOperation {
        name: operation.name.clone(),
        wire_name: operation.wire_name.clone(),
        experimental: operation.experimental,
        doc: operation.doc.clone(),
        return_doc: operation.return_doc.clone(),
        input,
        output,
        output_transform: operation.output_transform().cloned(),
        output_resource_return: plan_operation_resource_return(output_resource_return),
        output_direct_result: operation.output_proto().is_none()
            && operation.output_record().is_none()
            && operation.output_resource().is_some(),
    })
}

fn plan_operation_input(
    service_name: &str,
    operation: &OperationSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    root_model_capabilities: &BTreeMap<String, ModelCapabilities>,
    plan: &mut ApiPlan,
) -> Result<PlannedMessageType> {
    if let Some(input_proto) = operation.input_proto() {
        let input_message =
            descriptors
                .message(input_proto)
                .ok_or_else(|| Error::UnknownOperationInputProto {
                    service: service_name.to_string(),
                    operation: operation.name.clone(),
                    type_name: input_proto.to_string(),
                })?;

        return Ok(plan_message_type(
            input_message,
            root_model_capabilities
                .get(&input_message.full_name)
                .copied()
                .unwrap_or(ModelCapabilities::TO_PROTO_ONLY),
            spec,
            descriptors,
            plan,
        ));
    }

    let record_name = operation.input_record().ok_or_else(|| Error::InvalidWit {
        path: std::path::PathBuf::from("<api-plan>"),
        reason: format!(
            "operation `{}` has no proto-backed or WIT-native input type",
            operation.name
        ),
    })?;
    let record = spec
        .records
        .get(record_name)
        .ok_or_else(|| Error::InvalidWit {
            path: std::path::PathBuf::from("<api-plan>"),
            reason: format!(
                "operation `{}` references unknown WIT record `{record_name}`",
                operation.name
            ),
        })?;
    Ok(plan_wit_record_type(
        record,
        ModelCapabilities::default(),
        spec,
        descriptors,
        plan,
    ))
}

fn plan_operation_output(
    service_name: &str,
    operation: &OperationSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    root_model_capabilities: &BTreeMap<String, ModelCapabilities>,
    plan: &mut ApiPlan,
    output_resource_return: Option<&ResolvedResourceReturnSpec>,
) -> Result<PlannedOperationOutput> {
    if let Some(output_proto) = operation.output_proto() {
        let output_message = descriptors.message(output_proto).ok_or_else(|| {
            Error::UnknownOperationOutputProto {
                service: service_name.to_string(),
                operation: operation.name.clone(),
                type_name: output_proto.to_string(),
            }
        })?;
        let output = planned_message_reference(output_message, spec);
        if operation.output_transform().is_none() && output_resource_return.is_none() {
            let _ = plan_message_type(
                output_message,
                root_model_capabilities
                    .get(&output_message.full_name)
                    .copied()
                    .unwrap_or(ModelCapabilities::BIDIRECTIONAL),
                spec,
                descriptors,
                plan,
            );
        }
        return Ok(PlannedOperationOutput::Message(output));
    }

    if let Some(record_name) = operation.output_record() {
        let record = spec
            .records
            .get(record_name)
            .ok_or_else(|| Error::InvalidWit {
                path: std::path::PathBuf::from("<api-plan>"),
                reason: format!(
                    "operation `{}` references unknown WIT record `{record_name}`",
                    operation.name
                ),
            })?;
        return Ok(PlannedOperationOutput::Message(plan_wit_record_type(
            record,
            ModelCapabilities::default(),
            spec,
            descriptors,
            plan,
        )));
    }

    if let Some(resource_name) = operation.output_resource() {
        return Ok(PlannedOperationOutput::Resource {
            type_name: resource_name.to_upper_camel_case(),
        });
    }

    Ok(PlannedOperationOutput::None)
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

#[derive(Debug, Clone, Copy)]
struct OperationBindingInfo<'a> {
    name: &'a str,
    direct_return: bool,
}

fn plan_resource(
    service: &ServiceSpec,
    resource: &ResolvedResourceSpec,
    operations: &[OperationBindingInfo<'_>],
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
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
                            reason: format!("bound operation `{operation_name}` was not rendered"),
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
                    .map(|field| planned_resource_field(field, spec, descriptors, plan))
                    .collect(),
                result: method
                    .result
                    .as_ref()
                    .map(|result| planned_resource_method_result(result, spec, descriptors, plan)),
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
            .map(|field| planned_resource_field(field, spec, descriptors, plan))
            .collect(),
        methods,
    })
}

fn planned_resource_method_result(
    result: &crate::spec::ResourceResultSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedResourceMethodResult {
    let optional = matches!(result.result_type, AuthoredFieldTypeSpec::Option(_));
    let kind = if let Some(resource) = result.resource.as_ref() {
        PlannedResourceMethodResultKind::Resource {
            type_name: resource.to_upper_camel_case(),
        }
    } else {
        PlannedResourceMethodResultKind::Value(planned_field_kind_from_authored(
            &result.result_type,
            spec,
            descriptors,
            plan,
        ))
    };
    PlannedResourceMethodResult { kind, optional }
}

fn planned_resource_field(
    field: &ResourceFieldSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedResourceField {
    let kind = planned_field_kind_from_authored(&field.field_type, spec, descriptors, plan);
    PlannedResourceField {
        name: field.name.clone(),
        optional: field.optional,
        kind,
        function: field.function.clone(),
    }
}

fn planned_message_reference(message: &MessageMetadata, spec: &ApiSpec) -> PlannedMessageType {
    let type_override = spec.type_override(&message.full_name);
    PlannedMessageType {
        info: PlannedTypeInfo::from_message(message, spec),
        model_name: planned_proto_model_name(message, spec),
        replacement: type_override
            .and_then(|type_override| type_override.replacement())
            .cloned(),
        authored_type: type_override.and_then(|type_override| type_override.authored_type.clone()),
        source: PlannedMessageSource::Proto,
    }
}

fn plan_wit_record_type(
    record: &WitRecordSpec,
    requested_capabilities: ModelCapabilities,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedMessageType {
    let planned_message = PlannedMessageType {
        info: PlannedTypeInfo::from_wit_record(record),
        model_name: record.name.clone(),
        replacement: None,
        authored_type: None,
        source: PlannedMessageSource::Wit,
    };
    ensure_wit_model_plan(record, requested_capabilities, spec, descriptors, plan);
    planned_message
}

fn ensure_wit_model_plan(
    record: &WitRecordSpec,
    requested_capabilities: ModelCapabilities,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) {
    if let Some(existing) = plan.models.get_mut(&record.full_name) {
        existing.capabilities.merge(requested_capabilities);
        return;
    }

    plan.models.insert(
        record.full_name.clone(),
        PlannedModel {
            info: PlannedTypeInfo::from_wit_record(record),
            name: record.name.clone(),
            capabilities: requested_capabilities,
            flatten_in_api: false,
            experimental: record.experimental,
            generated_model: record.generated_model.clone(),
            fields: Vec::new(),
            sourced_fields: Vec::new(),
        },
    );

    let fields = record
        .generated_model
        .declared_fields
        .iter()
        .filter(|field_name| {
            !record
                .generated_model
                .field_sources
                .contains_key(*field_name)
        })
        .map(|field_name| plan_wit_field(record, field_name, spec, descriptors, plan))
        .collect();

    let model = plan
        .models
        .get_mut(&record.full_name)
        .expect("WIT model should be inserted before recursive field planning");
    model.fields = fields;
}

fn ensure_model_plan(
    message: &MessageMetadata,
    requested_capabilities: ModelCapabilities,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) {
    if spec
        .type_override(&message.full_name)
        .and_then(|type_override| type_override.replacement())
        .is_some()
    {
        return;
    }

    if let Some(existing) = plan.models.get_mut(&message.full_name) {
        existing.capabilities.merge(requested_capabilities);
        return;
    }

    let type_override = spec.type_override(&message.full_name);
    let generated_model = type_override
        .and_then(|type_override| type_override.generated_model())
        .cloned()
        .unwrap_or_default();
    let flatten_in_api = type_override.is_some_and(|type_override| type_override.flatten_in_api());
    let experimental = type_override.is_some_and(|type_override| type_override.experimental());

    plan.models.insert(
        message.full_name.clone(),
        PlannedModel {
            info: PlannedTypeInfo::from_message(message, spec),
            name: planned_proto_model_name(message, spec),
            capabilities: requested_capabilities,
            flatten_in_api,
            experimental,
            generated_model: generated_model.clone(),
            fields: Vec::new(),
            sourced_fields: Vec::new(),
        },
    );

    let fields = planned_message_fields(message, &generated_model)
        .into_iter()
        .filter(|field| {
            let proto_name = field
                .name
                .as_deref()
                .expect("descriptor fields should be named");
            !spec
                .type_override(&message.full_name)
                .is_some_and(|type_override| type_override.is_field_hidden(proto_name))
        })
        .map(|field| plan_field(message, field, spec, descriptors, plan))
        .collect();

    let sourced_fields = planned_message_fields(message, &generated_model)
        .into_iter()
        .filter_map(|field| {
            let proto_name = field
                .name
                .as_deref()
                .expect("descriptor fields should be named");
            spec.type_override(&message.full_name)
                .and_then(|type_override| type_override.field_source(proto_name))
                .map(|source_expr| {
                    plan_sourced_field(message, field, source_expr, spec, descriptors, plan)
                })
        })
        .collect();

    let model = plan
        .models
        .get_mut(&message.full_name)
        .expect("model should be inserted before recursive field planning");
    model.fields = fields;
    model.sourced_fields = sourced_fields;
}

fn planned_message_fields<'a>(
    message: &'a MessageMetadata,
    generated_model: &GeneratedModelSpec,
) -> Vec<&'a FieldDescriptorProto> {
    if generated_model.declared_fields.is_empty() {
        return message.descriptor.field.iter().collect();
    }

    generated_model
        .declared_fields
        .iter()
        .map(|field_name| descriptor_field_by_name(message, field_name))
        .collect()
}

fn descriptor_field_by_name<'a>(
    message: &'a MessageMetadata,
    field_name: &str,
) -> &'a FieldDescriptorProto {
    message
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some(field_name))
        .expect("declared generated model field should exist in descriptor")
}

fn ensure_enum_plan(enumeration: &EnumMetadata, spec: &ApiSpec, plan: &mut ApiPlan) {
    plan.enums
        .entry(enumeration.full_name.clone())
        .or_insert_with(|| PlannedEnum {
            info: PlannedTypeInfo::from_enum(enumeration, spec),
            name: enum_name(&enumeration.full_name),
            values: enumeration
                .descriptor
                .value
                .iter()
                .filter_map(|value| {
                    Some(PlannedEnumValue {
                        name: value.name.as_deref()?.to_string(),
                        number: value.number?,
                    })
                })
                .collect(),
        });
}

fn ensure_wit_enum_plan(enumeration: &WitEnumSpec, plan: &mut ApiPlan) {
    plan.enums
        .entry(enumeration.full_name.clone())
        .or_insert_with(|| PlannedEnum {
            info: PlannedTypeInfo::from_wit_enum(enumeration),
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
}

fn ensure_wit_flags_plan(flags: &WitFlagsSpec, plan: &mut ApiPlan) {
    plan.flags
        .entry(flags.full_name.clone())
        .or_insert_with(|| PlannedFlags {
            info: PlannedTypeInfo::from_wit_flags(flags),
            name: flags.name.clone(),
            flags: flags
                .flags
                .iter()
                .map(|flag| PlannedFlag {
                    name: flag.name.clone(),
                    bit: flag.bit,
                })
                .collect(),
        });
}

fn ensure_wit_variant_plan(
    variant: &WitVariantSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) {
    if plan.variants.contains_key(&variant.full_name) {
        return;
    }

    let cases = variant
        .cases
        .iter()
        .map(|case| PlannedVariantCase {
            name: case.name.clone(),
            payload: case
                .payload
                .as_ref()
                .map(|payload| planned_value_type_from_authored(payload, spec, descriptors, plan)),
        })
        .collect();
    plan.variants.insert(
        variant.full_name.clone(),
        PlannedVariant {
            info: PlannedTypeInfo::from_wit_variant(variant),
            name: variant.name.clone(),
            cases,
        },
    );
}

fn plan_field(
    message: &MessageMetadata,
    field: &FieldDescriptorProto,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedField {
    let proto_name = field
        .name
        .as_deref()
        .expect("descriptor fields should be named")
        .to_string();
    let generated_model = spec
        .type_override(&message.full_name)
        .and_then(|type_override| type_override.generated_model());

    PlannedField {
        owner_name: planned_proto_model_name(message, spec),
        authored_name: generated_model
            .and_then(|generated_model| generated_model.field_name_override(&proto_name))
            .unwrap_or(&proto_name)
            .to_string(),
        doc: generated_model
            .and_then(|generated_model| generated_model.field_doc(&proto_name))
            .cloned(),
        annotation_override: generated_model
            .and_then(|generated_model| generated_model.field_annotation(&proto_name))
            .cloned(),
        default_value: generated_model
            .and_then(|generated_model| generated_model.field_default(&proto_name))
            .map(|field_default| PlannedFieldDefault {
                enum_case: field_default.enum_case.clone(),
            }),
        required: spec
            .type_override(&message.full_name)
            .is_some_and(|type_override| type_override.is_field_required(&proto_name)),
        has_presence: field_has_presence(field, field_type(field)),
        role: planned_field_role(generated_model, &proto_name),
        function: generated_model
            .and_then(|generated_model| generated_model.function(&proto_name))
            .cloned(),
        function_args: generated_model
            .and_then(|generated_model| generated_model.function_for_args_field(&proto_name))
            .is_some(),
        with_arguments: generated_model
            .and_then(|generated_model| generated_model.with_arguments(&proto_name))
            .cloned(),
        with_arguments_args: generated_model
            .and_then(|generated_model| generated_model.with_arguments_for_args_field(&proto_name))
            .is_some(),
        kind: planned_field_kind(field, spec, descriptors, plan),
        proto_name,
    }
}

fn plan_wit_field(
    record: &WitRecordSpec,
    field_name: &str,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedField {
    let wit_type = record
        .generated_model
        .field_wit_type(field_name)
        .expect("declared WIT field should have a WIT type");
    PlannedField {
        owner_name: record.name.clone(),
        proto_name: field_name.to_string(),
        authored_name: record
            .generated_model
            .field_name_override(field_name)
            .unwrap_or(field_name)
            .to_string(),
        doc: record.generated_model.field_doc(field_name).cloned(),
        annotation_override: record.generated_model.field_annotation(field_name).cloned(),
        default_value: record
            .generated_model
            .field_default(field_name)
            .map(|field_default| PlannedFieldDefault {
                enum_case: field_default.enum_case.clone(),
            }),
        required: record.required_fields.contains(field_name),
        has_presence: !record.required_fields.contains(field_name),
        role: planned_field_role(Some(&record.generated_model), field_name),
        function: record.generated_model.function(field_name).cloned(),
        function_args: record
            .generated_model
            .function_for_args_field(field_name)
            .is_some(),
        with_arguments: record.generated_model.with_arguments(field_name).cloned(),
        with_arguments_args: record
            .generated_model
            .with_arguments_for_args_field(field_name)
            .is_some(),
        kind: planned_field_kind_from_authored(wit_type, spec, descriptors, plan),
    }
}

fn planned_field_kind_from_authored(
    wit_type: &AuthoredFieldTypeSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedFieldKind {
    if let AuthoredFieldTypeSpec::Proto(proto_name) = wit_type {
        if let Some(type_override) = spec.type_override(proto_name) {
            if type_override.replacement.is_none() {
                if let Some(authored_type) = type_override.authored_type.as_ref() {
                    return planned_field_kind_from_authored(
                        authored_type,
                        spec,
                        descriptors,
                        plan,
                    );
                }
            }
        }
    }

    match wit_type {
        AuthoredFieldTypeSpec::Option(inner) => {
            planned_field_kind_from_authored(inner, spec, descriptors, plan)
        }
        AuthoredFieldTypeSpec::List(inner) => PlannedFieldKind::Repeated(
            planned_value_type_from_authored(inner.without_option(), spec, descriptors, plan),
        ),
        AuthoredFieldTypeSpec::Map(key, value) => PlannedFieldKind::Map {
            key: planned_value_type_from_authored(key.without_option(), spec, descriptors, plan),
            value: planned_value_type_from_authored(
                value.without_option(),
                spec,
                descriptors,
                plan,
            ),
        },
        _ => PlannedFieldKind::Singular(planned_value_type_from_authored(
            wit_type.without_option(),
            spec,
            descriptors,
            plan,
        )),
    }
}

fn planned_value_type_from_authored(
    wit_type: &AuthoredFieldTypeSpec,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedValueType {
    match wit_type {
        AuthoredFieldTypeSpec::Bool => PlannedValueType::Scalar(PlannedScalarType::Bool),
        AuthoredFieldTypeSpec::Int => PlannedValueType::Scalar(PlannedScalarType::Int32),
        AuthoredFieldTypeSpec::Float => PlannedValueType::Scalar(PlannedScalarType::Float),
        AuthoredFieldTypeSpec::String => PlannedValueType::Scalar(PlannedScalarType::String),
        AuthoredFieldTypeSpec::Bytes => PlannedValueType::Scalar(PlannedScalarType::Bytes),
        AuthoredFieldTypeSpec::Proto(proto_name) => {
            if let Some(message) = descriptors.message(proto_name) {
                PlannedValueType::Message(plan_message_type(
                    message,
                    ModelCapabilities::BIDIRECTIONAL,
                    spec,
                    descriptors,
                    plan,
                ))
            } else if let Some(enumeration) = descriptors.enumeration(proto_name) {
                let replacement = spec
                    .type_override(&enumeration.full_name)
                    .and_then(|type_override| type_override.replacement())
                    .cloned();
                if replacement.is_none() {
                    ensure_enum_plan(enumeration, spec, plan);
                }
                PlannedValueType::Enum(PlannedEnumType {
                    info: Some(PlannedTypeInfo::from_enum(enumeration, spec)),
                    name: Some(enum_name(&enumeration.full_name)),
                    replacement,
                })
            } else {
                PlannedValueType::Unknown
            }
        }
        AuthoredFieldTypeSpec::Enum(enum_name) => spec
            .enums
            .get(enum_name)
            .map(|enumeration| {
                ensure_wit_enum_plan(enumeration, plan);
                PlannedValueType::Enum(PlannedEnumType {
                    info: Some(PlannedTypeInfo::from_wit_enum(enumeration)),
                    name: Some(enumeration.name.clone()),
                    replacement: None,
                })
            })
            .unwrap_or(PlannedValueType::Unknown),
        AuthoredFieldTypeSpec::Flags(flags_name) => spec
            .flags
            .get(flags_name)
            .map(|flags| {
                ensure_wit_flags_plan(flags, plan);
                PlannedValueType::Flags(PlannedFlagsType {
                    info: PlannedTypeInfo::from_wit_flags(flags),
                    name: flags.name.clone(),
                })
            })
            .unwrap_or(PlannedValueType::Unknown),
        AuthoredFieldTypeSpec::Variant(variant_name) => spec
            .variants
            .get(variant_name)
            .map(|variant| {
                ensure_wit_variant_plan(variant, spec, descriptors, plan);
                PlannedValueType::Variant(PlannedVariantType {
                    info: PlannedTypeInfo::from_wit_variant(variant),
                    name: variant.name.clone(),
                })
            })
            .unwrap_or(PlannedValueType::Unknown),
        AuthoredFieldTypeSpec::Record(record_name) => spec
            .records
            .get(record_name)
            .map(|record| {
                PlannedValueType::Message(plan_wit_record_type(
                    record,
                    ModelCapabilities::default(),
                    spec,
                    descriptors,
                    plan,
                ))
            })
            .unwrap_or(PlannedValueType::Unknown),
        AuthoredFieldTypeSpec::Resource(_) => PlannedValueType::Unknown,
        AuthoredFieldTypeSpec::Option(inner) => {
            planned_value_type_from_authored(inner.without_option(), spec, descriptors, plan)
        }
        AuthoredFieldTypeSpec::Tuple(items) => PlannedValueType::Tuple(
            items
                .iter()
                .map(|item| planned_value_type_from_authored(item, spec, descriptors, plan))
                .collect(),
        ),
        AuthoredFieldTypeSpec::Result { ok, err } => PlannedValueType::Result {
            ok: ok.as_ref().map(|ok| {
                Box::new(planned_value_type_from_authored(
                    ok,
                    spec,
                    descriptors,
                    plan,
                ))
            }),
            err: err.as_ref().map(|err| {
                Box::new(planned_value_type_from_authored(
                    err,
                    spec,
                    descriptors,
                    plan,
                ))
            }),
        },
        AuthoredFieldTypeSpec::Alias {
            target, type_name, ..
        } => {
            let fallback =
                planned_value_type_from_authored(target.without_option(), spec, descriptors, plan);
            PlannedValueType::External {
                type_name: type_name.clone(),
                fallback: Box::new(fallback),
            }
        }
        AuthoredFieldTypeSpec::List(_) | AuthoredFieldTypeSpec::Map(_, _) => {
            PlannedValueType::Unknown
        }
    }
}

fn plan_sourced_field(
    _message: &MessageMetadata,
    field: &FieldDescriptorProto,
    source_expr: &str,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedSourcedField {
    PlannedSourcedField {
        proto_name: field
            .name
            .as_deref()
            .expect("descriptor fields should be named")
            .to_string(),
        source_expr: source_expr.to_string(),
        kind: planned_field_kind(field, spec, descriptors, plan),
    }
}

fn planned_field_role(
    generated_model: Option<&GeneratedModelSpec>,
    proto_name: &str,
) -> PlannedFieldRole {
    if let Some(function) =
        generated_model.and_then(|generated_model| generated_model.function(proto_name))
    {
        return PlannedFieldRole::Function(function.clone());
    }
    if let Some(function) = generated_model
        .and_then(|generated_model| generated_model.function_for_args_field(proto_name))
    {
        return PlannedFieldRole::FunctionArgs(function.clone());
    }
    if generated_model
        .and_then(|generated_model| generated_model.with_arguments(proto_name))
        .is_some()
    {
        return PlannedFieldRole::WithArguments;
    }
    if generated_model
        .and_then(|generated_model| generated_model.with_arguments_for_args_field(proto_name))
        .is_some()
    {
        return PlannedFieldRole::WithArgumentsArgs;
    }
    PlannedFieldRole::Plain
}

fn planned_field_kind(
    field: &FieldDescriptorProto,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedFieldKind {
    if let Some((key, value)) = map_field_value_types(field, spec, descriptors, plan) {
        return PlannedFieldKind::Map { key, value };
    }

    let value = planned_value_type(field, spec, descriptors, plan);
    if field_label(field) == Some(Label::Repeated) {
        PlannedFieldKind::Repeated(value)
    } else {
        PlannedFieldKind::Singular(value)
    }
}

fn map_field_value_types(
    field: &FieldDescriptorProto,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> Option<(PlannedValueType, PlannedValueType)> {
    if field_label(field) != Some(Label::Repeated) || field_type(field) != Some(Type::Message) {
        return None;
    }

    let entry_name = field.type_name.as_deref()?.trim_start_matches('.');
    let entry = descriptors.message(entry_name)?;
    let is_map_entry = entry
        .descriptor
        .options
        .as_ref()
        .and_then(|options| options.map_entry)
        .unwrap_or(false);
    if !is_map_entry {
        return None;
    }

    let key_field = entry
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some("key"))?;
    let value_field = entry
        .descriptor
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some("value"))?;

    Some((
        planned_value_type(key_field, spec, descriptors, plan),
        planned_value_type(value_field, spec, descriptors, plan),
    ))
}

fn planned_value_type(
    field: &FieldDescriptorProto,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedValueType {
    match field_type(field) {
        Some(Type::Double | Type::Float) => PlannedValueType::Scalar(PlannedScalarType::Float),
        Some(Type::Int64 | Type::Uint64 | Type::Fixed64 | Type::Sfixed64 | Type::Sint64) => {
            PlannedValueType::Scalar(PlannedScalarType::Int64)
        }
        Some(Type::Int32 | Type::Fixed32 | Type::Uint32 | Type::Sfixed32 | Type::Sint32) => {
            PlannedValueType::Scalar(PlannedScalarType::Int32)
        }
        Some(Type::Bool) => PlannedValueType::Scalar(PlannedScalarType::Bool),
        Some(Type::String) => PlannedValueType::Scalar(PlannedScalarType::String),
        Some(Type::Bytes) => PlannedValueType::Scalar(PlannedScalarType::Bytes),
        Some(Type::Enum) => PlannedValueType::Enum(plan_enum_type(field, spec, descriptors, plan)),
        Some(Type::Message) | Some(Type::Group) => {
            if let Some(message) = field
                .type_name
                .as_deref()
                .and_then(|type_name| descriptors.message(type_name.trim_start_matches('.')))
            {
                PlannedValueType::Message(plan_message_type(
                    message,
                    ModelCapabilities::BIDIRECTIONAL,
                    spec,
                    descriptors,
                    plan,
                ))
            } else {
                PlannedValueType::Unknown
            }
        }
        None => PlannedValueType::Unknown,
    }
}

fn plan_enum_type(
    field: &FieldDescriptorProto,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    plan: &mut ApiPlan,
) -> PlannedEnumType {
    let Some(enumeration) = field
        .type_name
        .as_deref()
        .and_then(|type_name| descriptors.enumeration(type_name.trim_start_matches('.')))
    else {
        return PlannedEnumType {
            info: None,
            name: None,
            replacement: None,
        };
    };

    let replacement = spec
        .type_override(&enumeration.full_name)
        .and_then(|type_override| type_override.replacement())
        .cloned();
    if replacement.is_none() {
        ensure_enum_plan(enumeration, spec, plan);
    }

    PlannedEnumType {
        info: Some(PlannedTypeInfo::from_enum(enumeration, spec)),
        name: Some(enum_name(&enumeration.full_name)),
        replacement,
    }
}
