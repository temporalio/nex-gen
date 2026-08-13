//! `EmittedNameResolutionPass` fixes the final emitted JSON model identifiers.
//!
//! It runs after reachability so its manifest covers exactly the declarations a
//! backend will render. The pass owns this planning decision; generators only
//! consult the already-resolved planned graph.

use crate::error::{Error, Result};
use crate::language::Language;
use crate::parser::{ManifestModel, ManifestService, NameManifest, build_name_manifest};
use crate::spec::{ApiSpecLeaf, CompilerPass};
use crate::spec::{ExternalTypeSpec, ModulePath, TypeDeclSpec};

use super::{PlannedFamily, PlannedSpec};

pub(crate) struct EmittedNameResolutionPass {
    language: Language,
}

impl EmittedNameResolutionPass {
    pub(crate) fn new(language: Language) -> Self {
        Self { language }
    }
}

impl CompilerPass<PlannedFamily, PlannedFamily> for EmittedNameResolutionPass {
    type Error = Error;

    fn transform_leaf(
        &mut self,
        mut leaf: ApiSpecLeaf<PlannedFamily>,
    ) -> Result<ApiSpecLeaf<PlannedFamily>> {
        resolve_emitted_json_names(&mut leaf.spec, self.language)?;
        Ok(leaf)
    }
}

/// Builds the manifest over the post-reachability API surface.
pub(crate) fn build_json_name_manifest(
    language: Language,
    api_plan: &PlannedSpec,
) -> Result<NameManifest> {
    let mut models = Vec::new();
    for (_full_name, binding) in api_plan.external_types() {
        let ExternalTypeSpec::Json(json) = &binding.external_type else {
            continue;
        };
        let module_key = json
            .module_path
            .as_ref()
            .map(ModulePath::as_module_key)
            .unwrap_or_default();
        let local_name = json
            .full_name
            .rsplit(['#', '/'])
            .next()
            .unwrap_or(&json.full_name)
            .to_string();
        models.push(ManifestModel {
            full_name: json.full_name.clone(),
            local_name,
            model_name: json.model_name.clone(),
            module_key,
            schema: json.schema.clone(),
        });
    }
    let services = api_plan
        .services
        .iter()
        .map(|service| ManifestService {
            name: service.name.clone(),
            code_name: service.code_name.for_language(language).map(str::to_string),
        })
        .collect::<Vec<_>>();
    build_name_manifest(language, &models, &services)
}

fn resolve_emitted_json_names(spec: &mut PlannedSpec, language: Language) -> Result<()> {
    let manifest = build_json_name_manifest(language, spec)?;
    for entry in spec.types.values_mut() {
        if let TypeDeclSpec::External(binding) = &mut entry.declaration
            && let ExternalTypeSpec::Json(json) = &mut binding.external_type
            && let Some(resolved) = manifest.type_name(&json.full_name)
        {
            json.model_name = resolved.to_string();
        }
    }
    Ok(())
}
