use std::collections::{BTreeMap, BTreeSet};

use heck::{ToKebabCase, ToUpperCamelCase};
use prost_types::FieldDescriptorProto;
use prost_types::field_descriptor_proto::Type;

use crate::descriptors::DescriptorIndex;
use crate::error::{Error, Result};
use crate::spec::{
    ApiSpec, ExternalTypeSpec, OperationSpec, RecordFieldSpec, RecordFieldVisibility,
    ResourceFieldSpec, ResourceMethodSpec, ResourceResultSpec, ServiceSpec, TypeSpec,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedServiceResources {
    pub resources: Vec<ResolvedResourceSpec>,
    pub operation_returns: BTreeMap<String, ResolvedResourceReturnSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedResourceSpec {
    pub name: String,
    pub fields: Vec<ResourceFieldSpec>,
    pub methods: Vec<ResolvedResourceMethodSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedResourceMethodSpec {
    pub name: String,
    pub params: Vec<ResourceFieldSpec>,
    pub result: Option<ResourceResultSpec>,
    pub binding: ResolvedResourceMethodBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedResourceMethodBinding {
    Operation {
        operation_name: String,
        request_plan: RequestPlan,
    },
    Stub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResourceReturnSpec {
    pub resource_name: String,
    pub bindings: Vec<ResolvedResourceFieldBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResourceFieldBinding {
    pub field_name: String,
    pub optional: bool,
    pub source: ResolvedResourceBindingSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedResourceBindingSource {
    RequestField {
        field_name: String,
        proto_field_name: String,
        hidden: bool,
    },
    ResultField {
        field_name: String,
        proto_field_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestPlan {
    Source(RequestPlanSource),
    Construct {
        message_name: String,
        fields: Vec<RequestPlanField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPlanField {
    pub field_name: String,
    pub value: RequestPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestPlanSource {
    ResourceField(String),
    MethodParam(String),
}

pub(crate) fn render_request_plan<FName, FAssign, FConstruct, FResource, FParam>(
    plan: &RequestPlan,
    member_name: FName,
    render_assignment: FAssign,
    render_construct: FConstruct,
    render_resource_field_source: FResource,
    render_method_param_source: FParam,
) -> String
where
    FName: Fn(&str) -> String + Copy,
    FAssign: Fn(String, String) -> String + Copy,
    FConstruct: Fn(&str, Vec<String>) -> String + Copy,
    FResource: Fn(&str) -> String + Copy,
    FParam: Fn(&str) -> String + Copy,
{
    match plan {
        RequestPlan::Source(RequestPlanSource::ResourceField(name)) => {
            render_resource_field_source(name)
        }
        RequestPlan::Source(RequestPlanSource::MethodParam(name)) => {
            render_method_param_source(name)
        }
        RequestPlan::Construct {
            message_name,
            fields,
        } => {
            let rendered_fields = fields
                .iter()
                .map(|field| {
                    render_assignment(
                        member_name(&field.field_name),
                        render_request_plan(
                            &field.value,
                            member_name,
                            render_assignment,
                            render_construct,
                            render_resource_field_source,
                            render_method_param_source,
                        ),
                    )
                })
                .collect::<Vec<_>>();
            render_construct(message_name, rendered_fields)
        }
    }
}

#[derive(Debug, Clone)]
struct MessageFieldInfo {
    proto_name: String,
    api_name: String,
    required: bool,
    hidden: bool,
    message_name: Option<String>,
}

pub(crate) fn resolve_service_resources(
    spec: &ApiSpec,
    service: &ServiceSpec,
    descriptors: &DescriptorIndex,
) -> Result<ResolvedServiceResources> {
    let resources = service
        .resources
        .iter()
        .map(|resource| resolve_resource_methods(spec, service, resource, descriptors))
        .collect::<Result<Vec<_>>>()?;

    let mut operation_returns = BTreeMap::new();
    for operation in &service.operations {
        let Some(resource_name) = operation_output_resource_name(service, operation) else {
            continue;
        };
        if operation.output_resource_type.is_none() {
            continue;
        }
        let resource = service
            .resource(resource_name)
            .ok_or_else(|| Error::InvalidResource {
                service: service.name.clone(),
                resource: resource_name.to_string(),
                reason: format!(
                    "operation `{}` returns unknown resource `{resource_name}`",
                    operation.name
                ),
            })?;
        let bindings = resource
            .fields
            .iter()
            .map(|field| {
                let Some(input_message_name) =
                    operation_input_message_name(operation, descriptors)
                else {
                    return Err(Error::InvalidResource {
                        service: service.name.clone(),
                        resource: resource.name.to_upper_camel_case(),
                        reason: format!(
                            "operation `{}` returns a resource directly and does not need field bindings",
                            operation.name
                        ),
                    });
                };
                let Some(output_message_name) = operation_output_resource_message_name(operation)
                else {
                    return Err(Error::InvalidResource {
                        service: service.name.clone(),
                        resource: resource.name.to_upper_camel_case(),
                        reason: format!(
                            "operation `{}` returns a resource directly and does not need field bindings",
                            operation.name
                        ),
                    });
                };
                let source = bind_resource_return_field(
                    spec,
                    &service.name,
                    &resource.name,
                    descriptors,
                    input_message_name,
                    output_message_name,
                    &field.name,
                )?;
                Ok(ResolvedResourceFieldBinding {
                    field_name: field.name.clone(),
                    optional: field.optional,
                    source,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        operation_returns.insert(
            operation.name.clone(),
            ResolvedResourceReturnSpec {
                resource_name: resource.name.clone(),
                bindings,
            },
        );
    }

    let mut bound_operation_resources = BTreeMap::new();
    for resource in &resources {
        for method in &resource.methods {
            let ResolvedResourceMethodBinding::Operation { operation_name, .. } = &method.binding
            else {
                continue;
            };
            if let Some(existing_resource) =
                bound_operation_resources.insert(operation_name.clone(), resource.name.clone())
                && existing_resource != resource.name
            {
                return Err(Error::InvalidResourceMethod {
                    service: service.name.clone(),
                    resource: resource.name.to_upper_camel_case(),
                    method: method.name.to_string(),
                    reason: format!(
                        "bound operation `{operation_name}` is already owned by resource `{}`",
                        existing_resource.to_upper_camel_case()
                    ),
                });
            }
        }
    }

    Ok(ResolvedServiceResources {
        resources,
        operation_returns,
    })
}

fn resolve_resource_methods(
    spec: &ApiSpec,
    service: &ServiceSpec,
    resource: &crate::spec::ResourceSpec,
    descriptors: &DescriptorIndex,
) -> Result<ResolvedResourceSpec> {
    let methods = resource
        .methods
        .iter()
        .map(|method| resolve_resource_method(spec, service, resource, method, descriptors))
        .collect::<Result<Vec<_>>>()?;
    Ok(ResolvedResourceSpec {
        name: resource.name.clone(),
        fields: resource.fields.clone(),
        methods,
    })
}

fn resolve_resource_method(
    spec: &ApiSpec,
    service: &ServiceSpec,
    resource: &crate::spec::ResourceSpec,
    method: &ResourceMethodSpec,
    descriptors: &DescriptorIndex,
) -> Result<ResolvedResourceMethodSpec> {
    let mut environment = BTreeMap::new();
    for field in &resource.fields {
        environment.insert(
            field.name.clone(),
            RequestPlanSource::ResourceField(field.name.clone()),
        );
    }
    for param in &method.params {
        environment.insert(
            param.name.clone(),
            RequestPlanSource::MethodParam(param.name.clone()),
        );
    }

    if let Some(operation_name) = &method.operation_name {
        let operation =
            service
                .operation(operation_name)
                .ok_or_else(|| Error::InvalidResourceMethod {
                    service: service.name.clone(),
                    resource: resource.name.to_upper_camel_case(),
                    method: method.name.to_string(),
                    reason: format!("bound operation `{operation_name}` was not found"),
                })?;
        let Some(request_plan) =
            synthesize_operation_request_plan(spec, descriptors, operation, &environment)?
        else {
            return Err(Error::InvalidResourceMethod {
                service: service.name.clone(),
                resource: resource.name.to_upper_camel_case(),
                method: method.name.to_string(),
                reason: format!(
                    "bound operation `{operation_name}` cannot be called from resource fields and method parameters"
                ),
            });
        };
        if !resource_method_result_matches_operation(service, method, operation) {
            return Err(Error::InvalidResourceMethod {
                service: service.name.clone(),
                resource: resource.name.to_upper_camel_case(),
                method: method.name.to_string(),
                reason: format!(
                    "bound operation `{operation_name}` result does not match the method result"
                ),
            });
        }
        return Ok(ResolvedResourceMethodSpec {
            name: method.name.clone(),
            params: method.params.clone(),
            result: method.result.clone(),
            binding: ResolvedResourceMethodBinding::Operation {
                operation_name: operation.name.clone(),
                request_plan,
            },
        });
    }

    let implicit_operation_name = method.name.to_upper_camel_case();
    let mut matching_operations = Vec::new();
    for operation in &service.operations {
        if operation.name != implicit_operation_name {
            continue;
        }
        let Some(request_plan) =
            synthesize_operation_request_plan(spec, descriptors, operation, &environment)?
        else {
            continue;
        };

        if !resource_method_result_matches_operation(service, method, operation) {
            continue;
        }

        matching_operations.push((operation.name.clone(), request_plan));
    }

    let binding = match matching_operations.len() {
        0 => ResolvedResourceMethodBinding::Stub,
        1 => {
            let (operation_name, request_plan) = matching_operations.pop().expect("len checked");
            ResolvedResourceMethodBinding::Operation {
                operation_name,
                request_plan,
            }
        }
        _ => {
            let matches = matching_operations
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::InvalidResourceMethod {
                service: service.name.clone(),
                resource: resource.name.to_upper_camel_case(),
                method: method.name.to_string(),
                reason: format!("matches multiple operations: {matches}"),
            });
        }
    };

    Ok(ResolvedResourceMethodSpec {
        name: method.name.clone(),
        params: method.params.clone(),
        result: method.result.clone(),
        binding,
    })
}

fn resource_method_result_matches_operation(
    service: &ServiceSpec,
    method: &ResourceMethodSpec,
    operation: &OperationSpec,
) -> bool {
    let Some(result) = &method.result else {
        return true;
    };
    match result.result_type.without_option() {
        TypeSpec::Resource(resource_name) => {
            operation_output_resource_name(service, operation) == Some(resource_name.as_str())
        }
        TypeSpec::External(ExternalTypeSpec::Proto(proto_name)) => {
            operation_output_ref(operation) == Some(proto_name.as_str())
                || operation_output_resource_message_name(operation) == Some(proto_name.as_str())
        }
        _ => false,
    }
}

fn synthesize_operation_request_plan(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    operation: &OperationSpec,
    environment: &BTreeMap<String, RequestPlanSource>,
) -> Result<Option<RequestPlan>> {
    if let Some(TypeSpec::External(ExternalTypeSpec::Proto(input_ref))) = operation.input_type() {
        synthesize_request_plan(spec, descriptors, input_ref.as_str(), environment)
    } else if let Some(TypeSpec::Record(input_ref)) = operation.input_type() {
        synthesize_record_request_plan(spec, descriptors, input_ref.as_str(), environment)
    } else {
        Ok(None)
    }
}

fn bind_resource_return_field(
    spec: &ApiSpec,
    service_name: &str,
    resource_name: &str,
    descriptors: &DescriptorIndex,
    input_message_name: &str,
    output_message_name: &str,
    field_name: &str,
) -> Result<ResolvedResourceBindingSource> {
    if let Some(field) = find_message_field(spec, descriptors, input_message_name, field_name)? {
        return Ok(ResolvedResourceBindingSource::RequestField {
            field_name: field.api_name,
            proto_field_name: field.proto_name,
            hidden: field.hidden,
        });
    }
    if let Some(field) =
        find_visible_message_field(spec, descriptors, output_message_name, field_name)?
    {
        return Ok(ResolvedResourceBindingSource::ResultField {
            field_name: field.api_name,
            proto_field_name: field.proto_name,
        });
    }
    Err(Error::InvalidResource {
        service: service_name.to_string(),
        resource: resource_name.to_upper_camel_case(),
        reason: format!(
            "could not bind resource field `{field_name}` from operation input or output"
        ),
    })
}

fn synthesize_request_plan(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    message_name: &str,
    environment: &BTreeMap<String, RequestPlanSource>,
) -> Result<Option<RequestPlan>> {
    let mut fields = Vec::new();
    for field in visible_message_fields(spec, descriptors, message_name)? {
        if let Some(source) = environment.get(&field.api_name) {
            fields.push(RequestPlanField {
                field_name: field.api_name,
                value: RequestPlan::Source(source.clone()),
            });
            continue;
        }

        if let Some(child_message_name) = field.message_name.as_deref() {
            if spec.record_for_proto(child_message_name).is_some()
                && let Some(value) =
                    synthesize_request_plan(spec, descriptors, child_message_name, environment)?
            {
                fields.push(RequestPlanField {
                    field_name: field.api_name,
                    value,
                });
                continue;
            }
        }

        if field.required {
            return Ok(None);
        }
    }

    Ok(Some(RequestPlan::Construct {
        message_name: message_name.to_string(),
        fields,
    }))
}

fn synthesize_record_request_plan(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    record_name: &str,
    environment: &BTreeMap<String, RequestPlanSource>,
) -> Result<Option<RequestPlan>> {
    let mut fields = Vec::new();
    for field in visible_record_fields(spec, record_name)? {
        if let Some(source) = environment.get(&field.api_name) {
            fields.push(RequestPlanField {
                field_name: field.api_name,
                value: RequestPlan::Source(source.clone()),
            });
            continue;
        }

        if let Some(child_message_name) = field
            .message_name
            .as_deref()
            .and_then(|message_name| message_name.strip_prefix("proto:"))
            && let Some(value) =
                synthesize_request_plan(spec, descriptors, child_message_name, environment)?
        {
            fields.push(RequestPlanField {
                field_name: field.api_name,
                value,
            });
            continue;
        }

        if let Some(child_record_name) = field.message_name.as_deref()
            && let Some(value) =
                synthesize_record_request_plan(spec, descriptors, child_record_name, environment)?
        {
            fields.push(RequestPlanField {
                field_name: field.api_name,
                value,
            });
            continue;
        }

        if field.required {
            return Ok(None);
        }
    }

    Ok(Some(RequestPlan::Construct {
        message_name: record_name.to_string(),
        fields,
    }))
}

fn visible_message_fields(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    message_name: &str,
) -> Result<Vec<MessageFieldInfo>> {
    Ok(all_message_fields(spec, descriptors, message_name)?
        .into_iter()
        .filter(|field| !field.hidden)
        .collect())
}

fn visible_record_fields(spec: &ApiSpec, record_name: &str) -> Result<Vec<MessageFieldInfo>> {
    Ok(all_record_fields(spec, record_name)?
        .into_iter()
        .filter(|field| !field.hidden)
        .collect())
}

fn find_visible_message_field(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    message_name: &str,
    field_name: &str,
) -> Result<Option<MessageFieldInfo>> {
    Ok(visible_message_fields(spec, descriptors, message_name)?
        .into_iter()
        .find(|field| field.api_name == field_name))
}

fn find_message_field(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    message_name: &str,
    field_name: &str,
) -> Result<Option<MessageFieldInfo>> {
    Ok(all_message_fields(spec, descriptors, message_name)?
        .into_iter()
        .find(|field| field.api_name == field_name))
}

fn all_message_fields(
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    message_name: &str,
) -> Result<Vec<MessageFieldInfo>> {
    let message = descriptors
        .message(message_name)
        .ok_or_else(|| Error::UnknownTypeOverride {
            type_name: message_name.to_string(),
        })?;
    message
        .descriptor
        .field
        .iter()
        .map(|field| {
            build_message_field_info(field, spec.record_for_proto(message_name), descriptors)
        })
        .collect()
}

fn all_record_fields(spec: &ApiSpec, record_name: &str) -> Result<Vec<MessageFieldInfo>> {
    let record = spec
        .records
        .get(record_name)
        .ok_or_else(|| Error::UnknownTypeOverride {
            type_name: record_name.to_string(),
        })?;
    record
        .fields
        .iter()
        .filter(|(_, field)| field.visibility != RecordFieldVisibility::Omitted)
        .map(|(field_name, field)| build_record_field_info(field_name, field, spec))
        .collect()
}

fn build_record_field_info(
    field_name: &str,
    field: &RecordFieldSpec,
    spec: &ApiSpec,
) -> Result<MessageFieldInfo> {
    let api_name = field.name.clone();
    let required = field.required;
    let hidden = matches!(field.visibility, RecordFieldVisibility::Sourced { .. });
    let message_name = api_field_message_name(&field.field_type, spec);
    Ok(MessageFieldInfo {
        proto_name: field_name.to_string(),
        api_name,
        required,
        hidden,
        message_name,
    })
}

fn api_field_message_name(field_type: &TypeSpec, spec: &ApiSpec) -> Option<String> {
    match field_type.without_option() {
        TypeSpec::Record(record_name) => Some(record_name.as_str().to_string()),
        TypeSpec::External(ExternalTypeSpec::Proto(proto_name))
            if spec.record_for_proto(proto_name.as_str()).is_some() =>
        {
            Some(format!("proto:{proto_name}"))
        }
        _ => None,
    }
}

fn build_message_field_info(
    field: &FieldDescriptorProto,
    record: Option<&crate::spec::RecordSpec>,
    descriptors: &DescriptorIndex,
) -> Result<MessageFieldInfo> {
    let proto_name = field
        .name
        .as_deref()
        .expect("descriptor fields should be named");
    let api_name = record
        .and_then(|record| record.field_name_override(proto_name))
        .map(str::to_string)
        .unwrap_or_else(|| proto_name.to_kebab_case());
    let required = record.is_some_and(|record| record.field_required(proto_name));
    let hidden = record.is_some_and(|record| {
        record.field_omitted(proto_name) || record.field_source(proto_name).is_some()
    });
    let message_name = field_message_name(field, descriptors);
    Ok(MessageFieldInfo {
        proto_name: proto_name.to_string(),
        api_name,
        required,
        hidden,
        message_name,
    })
}

fn field_message_name(
    field: &FieldDescriptorProto,
    descriptors: &DescriptorIndex,
) -> Option<String> {
    if field.r#type() != Type::Message {
        return None;
    }
    let type_name = field.type_name.as_deref()?.trim_start_matches('.');
    descriptors.message(type_name)?;
    Some(type_name.to_string())
}

fn operation_input_message_name<'a>(
    operation: &'a OperationSpec,
    descriptors: &DescriptorIndex,
) -> Option<&'a str> {
    let Some(TypeSpec::External(ExternalTypeSpec::Proto(input_ref))) = operation.input_type()
    else {
        return None;
    };
    descriptors
        .message(input_ref.as_str())
        .map(|_| input_ref.as_str())
}

fn operation_output_resource_name<'a>(
    service: &'a ServiceSpec,
    operation: &'a OperationSpec,
) -> Option<&'a str> {
    let TypeSpec::Resource(resource_name) = operation.output_type()? else {
        return None;
    };
    service
        .resource(resource_name.as_str())
        .map(|_| resource_name.as_str())
}

fn operation_output_ref(operation: &OperationSpec) -> Option<&str> {
    operation.output_type()?.reference()
}

fn operation_output_resource_message_name(operation: &OperationSpec) -> Option<&str> {
    let Some(ExternalTypeSpec::Proto(type_name)) = operation.output_resource_type.as_ref() else {
        return None;
    };
    Some(type_name.as_ref())
}

pub(crate) fn ensure_unique_resource_names(spec: &ApiSpec) -> Result<()> {
    let mut seen_names = BTreeSet::new();
    for service in &spec.services {
        for resource in &service.resources {
            let generated_name = resource.name.to_upper_camel_case();
            if !seen_names.insert(generated_name.clone()) {
                return Err(Error::InvalidResource {
                    service: service.name.clone(),
                    resource: generated_name,
                    reason: "another resource uses the same generated name".to_string(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use prost_types::FileDescriptorSet;

    use crate::descriptors::DescriptorIndex;
    use crate::language::Language;
    use crate::spec::ApiSpec;

    use super::{
        RequestPlan, RequestPlanSource, ResolvedResourceMethodBinding, resolve_service_resources,
    };

    fn descriptors() -> DescriptorIndex {
        DescriptorIndex::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples/descriptors/temporal_api.bin"),
        )
        .unwrap()
    }

    fn empty_descriptors() -> DescriptorIndex {
        DescriptorIndex::from_descriptor_set(FileDescriptorSet { file: Vec::new() }).unwrap()
    }

    fn parse(language: Language, wit: &str) -> ApiSpec {
        let temporal_types_input =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/inputs/deps");
        crate::parser::parse_api_spec_from_wit_for_language_with_inputs(
            language,
            wit,
            PathBuf::from("inline.wit"),
            &[temporal_types_input],
        )
        .unwrap()
    }

    #[test]
    fn binds_implicit_resource_method_to_same_named_operation() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{payloads, task-queue, workflow-function};

  resource started-workflow {
    constructor(namespace: string, workflow-id: string, run-id: option<string>);

    restart-workflow: func(
      workflow: workflow-function,
      task-queue: task-queue,
      input: option<payloads>,
    ) -> start-workflow-result;
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.StartWorkflowExecutionRequest"
  record start-workflow-request {
    /// @nexus.proto-field "workflow_type"
    workflow: workflow-function,
    workflow-id: string,
    task-queue: task-queue,
    namespace: option<string>,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.StartWorkflowExecutionResponse"
  type start-workflow-result = own<started-workflow>;

  start-workflow: func(request: start-workflow-request) -> start-workflow-result;
  restart-workflow: func(request: start-workflow-request) -> start-workflow-result;
}
"#;
        let spec = parse(Language::Python, wit);
        let service = &spec.services[0];
        let resolved = resolve_service_resources(&spec, service, &descriptors()).unwrap();
        let method = &resolved.resources[0].methods[0];

        assert_eq!(method.name, "restart-workflow");
        match &method.binding {
            ResolvedResourceMethodBinding::Operation { operation_name, .. } => {
                assert_eq!(operation_name, "RestartWorkflow");
            }
            ResolvedResourceMethodBinding::Stub => {
                panic!("restart-workflow should bind to RestartWorkflow");
            }
        }
    }

    #[test]
    fn rejects_operation_bound_to_multiple_resources() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{payloads, task-queue, workflow-function};

  resource started-workflow {
    constructor(namespace: string, workflow-id: string, run-id: option<string>);

    restart-workflow: func(
      workflow: workflow-function,
      task-queue: task-queue,
      input: option<payloads>,
    );
  }

  resource archived-workflow {
    constructor(namespace: string, workflow-id: string, run-id: option<string>);

    restart-workflow: func(
      workflow: workflow-function,
      task-queue: task-queue,
      input: option<payloads>,
    );
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.StartWorkflowExecutionRequest"
  record start-workflow-request {
    /// @nexus.proto-field "workflow_type"
    workflow: workflow-function,
    workflow-id: string,
    task-queue: task-queue,
    namespace: option<string>,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.StartWorkflowExecutionResponse"
  record start-workflow-response {
    run-id: option<string>,
  }

  restart-workflow: func(request: start-workflow-request) -> start-workflow-response;
}
"#;
        let spec = parse(Language::Python, wit);
        let service = &spec.services[0];
        let error = resolve_service_resources(&spec, service, &descriptors()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "resource method `WorkflowService.ArchivedWorkflow.restart-workflow` is invalid: bound operation `RestartWorkflow` is already owned by resource `StartedWorkflow`"
        );
    }

    #[test]
    fn resource_method_does_not_bind_operation_when_operation_name_differs_without_annotation() {
        let wit = r#"
package temporal:users@1.0.0;

world system {
  export user-service;
}

/// @nexus.endpoint "__user_service"
interface user-service {
  resource user {
    constructor(user-id: string, email: string);

    update-email: func(email: string) -> user-result;
  }

  type user-result = own<user>;

  record update-email-request {
    user-id: string,
    email: string,
  }

  updates-email: func(request: update-email-request) -> user-result;
}
"#;
        let spec = crate::parser::parse_api_spec_from_wit_for_language(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
        )
        .unwrap();
        let service = &spec.services[0];
        let resolved = resolve_service_resources(&spec, service, &empty_descriptors()).unwrap();
        let method = &resolved.resources[0].methods[0];

        assert_eq!(method.name, "update-email");
        assert!(matches!(
            method.binding,
            ResolvedResourceMethodBinding::Stub
        ));
    }

    #[test]
    fn resource_method_can_bind_operation_when_operation_name_differs_with_annotation() {
        let wit = r#"
package temporal:users@1.0.0;

world system {
  export user-service;
}

/// @nexus.endpoint "__user_service"
interface user-service {
  resource user {
    constructor(user-id: string, email: string);

    /// @nexus.operation "updates-email"
    update-email: func(email: string) -> user-result;
  }

  type user-result = own<user>;

  record update-email-request {
    user-id: string,
    email: string,
  }

  updates-email: func(request: update-email-request) -> user-result;
}
"#;
        let spec = crate::parser::parse_api_spec_from_wit_for_language(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
        )
        .unwrap();
        let service = &spec.services[0];
        let resolved = resolve_service_resources(&spec, service, &empty_descriptors()).unwrap();
        let method = &resolved.resources[0].methods[0];

        assert_eq!(method.name, "update-email");
        match &method.binding {
            ResolvedResourceMethodBinding::Operation {
                operation_name,
                request_plan,
            } => {
                assert_eq!(operation_name, "UpdatesEmail");
                let RequestPlan::Construct {
                    message_name,
                    fields,
                } = request_plan
                else {
                    panic!("request plan should construct update-email-request");
                };
                assert_eq!(message_name, "user-service.update-email-request");
                assert_eq!(fields.len(), 2);
                assert!(fields.contains(&super::RequestPlanField {
                    field_name: "user-id".to_string(),
                    value: RequestPlan::Source(RequestPlanSource::ResourceField(
                        "user-id".to_string()
                    )),
                }));
                assert!(fields.contains(&super::RequestPlanField {
                    field_name: "email".to_string(),
                    value: RequestPlan::Source(RequestPlanSource::MethodParam("email".to_string())),
                }));
            }
            ResolvedResourceMethodBinding::Stub => {
                panic!("update-email should bind to UpdatesEmail");
            }
        }
    }

    #[test]
    fn resource_method_rejects_unknown_explicit_operation_binding() {
        let wit = r#"
package temporal:users@1.0.0;

world system {
  export user-service;
}

/// @nexus.endpoint "__user_service"
interface user-service {
  resource user {
    constructor(user-id: string, email: string);

    /// @nexus.operation "updates-email"
    update-email: func(email: string) -> user-result;
  }

  type user-result = own<user>;

  record update-email-request {
    user-id: string,
    email: string,
  }

  update-email: func(request: update-email-request) -> user-result;
}
"#;
        let spec = crate::parser::parse_api_spec_from_wit_for_language(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
        )
        .unwrap();
        let service = &spec.services[0];
        let error = resolve_service_resources(&spec, service, &empty_descriptors()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "resource method `UserService.User.update-email` is invalid: bound operation `UpdatesEmail` was not found"
        );
    }

    #[test]
    fn resource_method_does_not_bind_operation_when_required_request_field_name_differs() {
        let wit = r#"
package temporal:users@1.0.0;

world system {
  export user-service;
}

/// @nexus.endpoint "__user_service"
interface user-service {
  resource user {
    constructor(user-id: string, email: string);

    update-email: func(email: string) -> user-result;
  }

  type user-result = own<user>;

  record update-email-request {
    users-id: string,
    email: string,
  }

  update-email: func(request: update-email-request) -> user-result;
}
"#;
        let spec = crate::parser::parse_api_spec_from_wit_for_language(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
        )
        .unwrap();
        let service = &spec.services[0];
        let resolved = resolve_service_resources(&spec, service, &empty_descriptors()).unwrap();
        let method = &resolved.resources[0].methods[0];

        assert_eq!(method.name, "update-email");
        assert!(matches!(
            method.binding,
            ResolvedResourceMethodBinding::Stub
        ));
    }
}
