pub(crate) mod dotnet;
pub(crate) mod go;
pub(crate) mod java;
pub(crate) mod python;
pub(crate) mod typescript;

use crate::error::Result;
use crate::language::Language;
use crate::parser::{ManifestModel, ManifestService, NameManifest, build_name_manifest};
use crate::planning::{PlannedSpec, PlannedTypeFamily};
use crate::spec::{ExternalTypeSpec, ModulePath, TypeDeclSpec};
use crate::workspace::ApiSpecNode;

/// Builds the [`NameManifest`] for a planned spec — the single resolution of
/// every emitted identifier that both the load-time collision check and the
/// generators go through. Called once per backend `prepare`, over the models
/// that will actually be emitted, so a generator resolves type/service names by
/// lookup rather than re-deriving them (no drift, overrides honored uniformly).
pub(in crate::generator) fn build_json_name_manifest(
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
        // `full_name` is the qualified identity string; the unqualified tail is
        // only used in collision diagnostics (the authoritative message is
        // produced by the load-time build over the authored spec).
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

/// Overwrites each planned JSON model's `model_name` with its manifest-resolved
/// identifier (honoring `x-<lang>-name` type overrides) across every leaf. This
/// is the single resolution point per language; the per-backend `$ref` fixups
/// then only re-route ref recasing. A no-op for non-JSON (WIT/proto) inputs,
/// whose leaves carry no [`ExternalTypeSpec::Json`] declarations.
pub(in crate::generator) fn apply_name_manifest_to_planned_tree(
    node: &mut ApiSpecNode<PlannedTypeFamily>,
    language: Language,
) -> Result<()> {
    match node {
        ApiSpecNode::Leaf(leaf) => {
            let manifest = build_json_name_manifest(language, &leaf.spec)?;
            for decl in leaf.spec.types.values_mut() {
                if let TypeDeclSpec::External(binding) = decl
                    && let ExternalTypeSpec::Json(json) = &mut binding.external_type
                    && let Some(resolved) = manifest.type_name(&json.full_name)
                {
                    json.model_name = resolved.to_string();
                }
            }
            Ok(())
        }
        ApiSpecNode::Branch(branch) => {
            for child in branch.children.values_mut() {
                apply_name_manifest_to_planned_tree(child, language)?;
            }
            Ok(())
        }
    }
}
