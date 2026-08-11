//! `OperationBindingPass` attaches resolved operation/resource relationships
//! to their owning nodes.
//!
//! It consumes resource-bound IR and produces `OperationBoundNames`. This is
//! deliberately semantic only: type materialization belongs to a later pass.

use super::*;

pub(crate) struct OperationBindingPass;

impl OperationBindingPass {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl CompilerPass<ResourceBoundNames, OperationBoundNames> for OperationBindingPass {
    type Error = Error;

    fn transform_leaf(
        &mut self,
        leaf: ApiSpecLeaf<ResourceBoundNames>,
    ) -> Result<ApiSpecLeaf<OperationBoundNames>> {
        let bindings = leaf.spec.data.bindings.clone();
        let mut spec = leaf.spec.map_names(OperationBindingMapper);

        for service in &mut spec.services {
            let resolved = bindings.get(&service.name);
            for operation in &mut service.operations {
                let output_resource_return = resolved
                    .and_then(|resources| resources.operation_returns.get(&operation.name))
                    .cloned();
                operation.data = OperationBoundOperation {
                    direct_return: operation.output_transform().is_some()
                        || output_resource_return.is_some()
                        || matches!(
                            operation.output_type(),
                            Some(TypeSpec::Resource(resource)) if resource.wire_type.is_none()
                        ),
                    output_resource_return,
                };
            }
            for resource in &mut service.resources {
                resource.data = OperationBoundResource {
                    resolved: resolved.and_then(|resources| {
                        resources
                            .resources
                            .iter()
                            .find(|candidate| candidate.name == resource.name)
                            .cloned()
                    }),
                };
            }
        }

        Ok(ApiSpecLeaf {
            module_path: leaf.module_path,
            source_root: leaf.source_root,
            source_path: leaf.source_path,
            spec,
        })
    }
}

struct OperationBindingMapper;

impl ApiSpecTransform<ResourceBoundNames, OperationBoundNames> for OperationBindingMapper {
    fn map_spec_data(&mut self, _: ResourceBoundData) {}
    fn map_record(&mut self, value: Symbol) -> Symbol {
        value
    }
    fn map_enum(&mut self, value: Symbol) -> Symbol {
        value
    }
    fn map_flags(&mut self, value: Symbol) -> Symbol {
        value
    }
    fn map_variant(&mut self, value: Symbol) -> Symbol {
        value
    }
    fn map_resource(&mut self, value: AuthoredResourceType) -> AuthoredResourceType {
        value
    }
    fn map_proto(&mut self, value: Symbol) -> Symbol {
        value
    }
    fn map_json(&mut self, value: JsonModelSpec<Symbol>) -> JsonModelSpec<Symbol> {
        value
    }
    fn map_alias(&mut self, value: Symbol) -> Symbol {
        value
    }
    fn map_service_data(&mut self, _: &str, _: ()) {}
    fn map_record_data(&mut self, _: &str, _: ()) {}
    fn map_resource_data(&mut self, _: &str, _: ()) -> OperationBoundResource {
        OperationBoundResource { resolved: None }
    }
    fn map_operation_data(&mut self, _: &str, _: ()) -> OperationBoundOperation {
        OperationBoundOperation {
            output_resource_return: None,
            direct_return: false,
        }
    }
    fn map_field_data(&mut self, _: &str, _: &str, _: ()) {}
    fn map_text(&mut self, value: SelectedTextSpec) -> SelectedTextSpec {
        value
    }
    fn map_support(&mut self, value: SelectedSupportSpec) -> SelectedSupportSpec {
        value
    }
}
