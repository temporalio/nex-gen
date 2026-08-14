pub(crate) mod dotnet;
pub(crate) mod go;
pub(crate) mod java;
pub(crate) mod python;
pub(crate) mod typescript;

pub(in crate::generator) use crate::planning::build_json_name_manifest;

use std::collections::BTreeMap;

use crate::planning::PlannedSpec;

/// Registers the emitted identifier of every model declared in *another* module
/// in a backend's `$ref` registry, under both key forms a backend looks a
/// reference up by: the bare model full name and the resolved `$ref` text
/// `#/$defs/<full name>`.
///
/// A leaf's own name manifest covers only the models it declares, so without
/// this a cross-module `$ref` would be recased from the reference text and drop
/// the `x-<lang>-name` override the other input file declares. The identifiers
/// themselves are resolved once, tree-wide, by `EmittedNameResolutionPass`.
pub(in crate::generator) fn register_cross_module_ref_names(
    api_plan: &PlannedSpec,
    ref_names: &mut BTreeMap<String, String>,
) {
    for (full_name, model_name) in &api_plan.data.cross_module_model_names {
        ref_names.insert(full_name.clone(), model_name.clone());
        ref_names.insert(format!("#/$defs/{full_name}"), model_name.clone());
    }
}
