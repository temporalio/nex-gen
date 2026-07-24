use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) mod dotnet;
pub(crate) mod go;
pub(crate) mod java;
pub(crate) mod json_schema;
pub(crate) mod proto;
pub(crate) mod python;
pub(crate) mod typescript;

use crate::SupportFiles;
use crate::descriptors::DescriptorIndex;
use crate::error::{Error, Result};
use crate::language::Language;
use crate::planning::{
    PlannedSpec, PlannedType, PlannedTypeFamily, PlanningMode, build_api_plans_for_tree_with_mode,
    build_leaf_api_plan_with_mode,
};
use crate::resources::ensure_unique_resource_names;
use crate::spec::{ApiSpec, RecordSpec};
use crate::validation::validate_external_type_bindings;
use crate::workspace::{ApiSpecNode, ApiSpecTree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedOutputLayout {
    SingleFile,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFiles {
    pub layout: GeneratedOutputLayout,
    pub files: BTreeMap<PathBuf, String>,
    pub warnings: Vec<String>,
}

impl GeneratedFiles {
    pub fn single_file(contents: String) -> Self {
        let mut files = BTreeMap::new();
        files.insert(PathBuf::from("output"), contents);
        Self {
            layout: GeneratedOutputLayout::SingleFile,
            files,
            warnings: Vec::new(),
        }
    }

    pub fn directory(files: BTreeMap<PathBuf, String>) -> Self {
        Self {
            layout: GeneratedOutputLayout::Directory,
            files,
            warnings: Vec::new(),
        }
    }

    pub fn single_file_contents(&self) -> Option<&str> {
        (self.layout == GeneratedOutputLayout::SingleFile)
            .then(|| self.files.values().next().map(String::as_str))
            .flatten()
    }
}

/// Tracks which public-model/wire-model conversion directions must be generated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ModelWireCapabilities {
    pub(crate) from_wire: bool,
    pub(crate) to_wire: bool,
}

impl ModelWireCapabilities {
    pub(crate) const TO_WIRE: Self = Self {
        from_wire: false,
        to_wire: true,
    };

    pub(crate) const BIDIRECTIONAL: Self = Self {
        from_wire: true,
        to_wire: true,
    };

    pub(crate) fn merge(&mut self, other: Self) {
        self.from_wire |= other.from_wire;
        self.to_wire |= other.to_wire;
    }
}

pub(crate) trait ExternalModelBackend<ModelType = PlannedType> {
    type ModelFragments;
    type WireConversion;

    /// Give the backend a chance to precompute model metadata from the planned spec.
    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()>;

    /// Render model definitions owned by this backend.
    fn render_models(&self) -> Result<Self::ModelFragments>;

    /// Render support files owned by this backend.
    fn render_support_files(&self) -> Result<BTreeMap<PathBuf, String>> {
        Ok(BTreeMap::new())
    }

    /// Return the target-language type annotation/name for a model reference.
    fn model_type_annotation(&self, model_type: &ModelType) -> Option<String>;

    /// Return the stable wire/runtime type identifier for a model reference.
    fn wire_type_identifier(&self, model_type: &ModelType) -> Option<String>;

    /// Return conversion templates between public model values and wire values.
    ///
    /// `planned_record` is present when the type has already resolved to a planned
    /// record model, which lets backends handle generated/local model conversions.
    fn wire_conversion(
        &self,
        model_type: &ModelType,
        planned_record: Option<&RecordSpec<PlannedTypeFamily>>,
    ) -> Option<Self::WireConversion>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GenerationMode {
    #[default]
    NativeApi,
    DefinitionsOnly,
}

/// The TypeScript in-memory representation of a materialized temporal `format`
/// field, selected by the `--date-time-types` generator flag (P16 API parity).
/// Affects **only** the TypeScript output; Go / Java / Python are unchanged. See
/// `specs/json-schema/features/format.md` (JS temporal representation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TsDateTimeTypes {
    /// Every temporal is a `string` holding the generator-serialized form
    /// (lossless, still materialized — the narrowed grammar rejects `:60`).
    #[default]
    String,
    /// `date-time` → `Date` (UTC instant, ms, offset folded); the others stay
    /// `string`.
    Date,
    /// `date-time` → `Temporal.ZonedDateTime`, `date` → `Temporal.PlainDate`,
    /// `duration` → `Temporal.Duration`; `time` stays `string`.
    Temporal,
}

pub fn generate_files(
    language: Language,
    spec: ApiSpec,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
) -> Result<GeneratedFiles> {
    generate_files_with_mode(
        language,
        spec,
        descriptors,
        support,
        GenerationMode::NativeApi,
    )
}

pub fn generate_files_with_mode(
    language: Language,
    spec: ApiSpec,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
    mode: GenerationMode,
) -> Result<GeneratedFiles> {
    generate_files_for_tree_with_mode(
        language,
        ApiSpecTree::single(spec),
        descriptors,
        support,
        mode,
    )
}

pub fn generate_files_for_tree_with_mode(
    language: Language,
    tree: ApiSpecTree,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
    mode: GenerationMode,
) -> Result<GeneratedFiles> {
    generate_files_for_tree_with_mode_and_options(
        language,
        tree,
        descriptors,
        support,
        mode,
        GenerateFilesOptions::default(),
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GenerateFilesOptions {
    pub(crate) go_output_dir_name: String,
    pub(crate) java_package_root: Option<String>,
    /// The TypeScript temporal representation (`--date-time-types`); ignored by
    /// the non-TypeScript backends.
    pub(crate) ts_date_time_types: TsDateTimeTypes,
}

pub(crate) fn generate_files_for_tree_with_mode_and_options(
    language: Language,
    tree: ApiSpecTree,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
    mode: GenerationMode,
    options: GenerateFilesOptions,
) -> Result<GeneratedFiles> {
    validate_tree_specs(&tree.root, descriptors, language)?;
    let mut planned_tree = match tree.root {
        ApiSpecNode::Leaf(leaf) => {
            let planned = build_leaf_api_plan_with_mode(
                leaf.spec,
                descriptors,
                planning_mode(mode),
                language,
            )?;
            ApiSpecTree {
                root: ApiSpecNode::Leaf(crate::workspace::ApiSpecLeaf {
                    module_path: leaf.module_path,
                    source_root: leaf.source_root,
                    source_path: leaf.source_path,
                    spec: planned,
                }),
            }
        }
        ApiSpecNode::Branch(_) => {
            build_api_plans_for_tree_with_mode(tree, descriptors, planning_mode(mode), language)?
        }
    };
    // Resolve every emitted type identifier once against the name manifest and
    // adopt it as each JSON model's `model_name`, so every consumer — the JSON
    // model backends, the outer service/file-name/I/O emitters — reads the same
    // (override-applied) identifier. This is the single resolution point per
    // language; the per-backend `$ref` fixups then only re-route ref recasing.
    // A no-op for non-JSON (WIT/proto) inputs.
    json_schema::apply_name_manifest_to_planned_tree(&mut planned_tree.root, language)?;
    generate_files_from_planned_tree(language, &planned_tree, support, mode, options)
}

fn validate_tree_specs(
    node: &ApiSpecNode,
    descriptors: &DescriptorIndex,
    language: Language,
) -> Result<()> {
    match node {
        ApiSpecNode::Leaf(leaf) => {
            validate_external_type_bindings(&leaf.spec, descriptors, language)?;
            ensure_unique_resource_names(&leaf.spec)
        }
        ApiSpecNode::Branch(branch) => {
            for child in branch.children.values() {
                validate_tree_specs(child, descriptors, language)?;
            }
            Ok(())
        }
    }
}

fn generate_files_from_planned_tree(
    language: Language,
    tree: &ApiSpecTree<PlannedTypeFamily>,
    support: &SupportFiles,
    mode: GenerationMode,
    options: GenerateFilesOptions,
) -> Result<GeneratedFiles> {
    let mut generated = match language {
        Language::Dotnet => dotnet::generate(tree, support, mode),
        Language::Go => generate_go_tree(tree, support, mode, options),
        Language::Java => java::generate(tree, support, mode, options.java_package_root.as_deref()),
        Language::Python => python::generate(tree, support, mode),
        Language::TypeScript => {
            typescript::generate(tree, support, mode, options.ts_date_time_types)
        }
        language => Err(Error::UnsupportedLanguage { language }),
    }?;
    generated.warnings = if mode == GenerationMode::NativeApi {
        generation_warnings_for_tree(tree)
    } else {
        Vec::new()
    };
    Ok(generated)
}

fn generate_go_tree(
    tree: &ApiSpecTree<PlannedTypeFamily>,
    support: &SupportFiles,
    mode: GenerationMode,
    options: GenerateFilesOptions,
) -> Result<GeneratedFiles> {
    go::generate_tree(
        tree,
        support,
        &go::GoOptions {
            output_dir_name: options.go_output_dir_name,
            ..go::GoOptions::default()
        },
        mode,
    )
}

fn planning_mode(mode: GenerationMode) -> PlanningMode {
    match mode {
        GenerationMode::NativeApi => PlanningMode::NativeApi,
        GenerationMode::DefinitionsOnly => PlanningMode::DefinitionsOnly,
    }
}

pub fn generate_source(
    language: Language,
    spec: ApiSpec,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
) -> Result<String> {
    generate_source_with_mode(
        language,
        spec,
        descriptors,
        support,
        GenerationMode::NativeApi,
    )
}

pub fn generate_source_with_mode(
    language: Language,
    spec: ApiSpec,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
    mode: GenerationMode,
) -> Result<String> {
    let generated = generate_files_with_mode(language, spec, descriptors, support, mode)?;
    Ok(match generated.layout {
        GeneratedOutputLayout::SingleFile => generated
            .single_file_contents()
            .expect("single-file output should contain one file")
            .to_string(),
        GeneratedOutputLayout::Directory => generated
            .files
            .iter()
            .map(|(path, contents)| format!("### {}\n{contents}", path.display()))
            .collect::<Vec<_>>()
            .join("\n\n"),
    })
}

fn generation_warnings(plan: &PlannedSpec) -> Vec<String> {
    plan.services
        .iter()
        .flat_map(|service| {
            service.resources.iter().flat_map(|resource| {
                resource.data.methods.iter().filter_map(|method| {
                    matches!(
                        method.binding,
                        crate::planning::PlannedResourceMethodBindingSpec::Stub
                    )
                    .then(|| {
                        format!(
                            "resource method `{}.{}` generated as a stub because no operation could be bound",
                            resource.data.type_name, method.name
                        )
                    })
                })
            })
        })
        .collect()
}

fn generation_warnings_for_tree(tree: &ApiSpecTree<PlannedTypeFamily>) -> Vec<String> {
    fn collect(node: &ApiSpecNode<PlannedTypeFamily>, warnings: &mut Vec<String>) {
        match node {
            ApiSpecNode::Leaf(leaf) => warnings.extend(generation_warnings(&leaf.spec)),
            ApiSpecNode::Branch(branch) => {
                for child in branch.children.values() {
                    collect(child, warnings);
                }
            }
        }
    }

    let mut warnings = Vec::new();
    collect(&tree.root, &mut warnings);
    warnings
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use prost_types::FileDescriptorSet;

    use crate::SupportFiles;
    use crate::descriptors::DescriptorIndex;
    use crate::language::Language;

    use super::{GenerationMode, generate_files, generate_files_with_mode};

    #[test]
    fn warns_when_resource_method_generates_as_stub() {
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
        let descriptors =
            DescriptorIndex::from_descriptor_set(FileDescriptorSet { file: Vec::new() }).unwrap();
        let generated = generate_files(
            Language::Python,
            spec,
            &descriptors,
            &SupportFiles::default(),
        )
        .unwrap();

        assert_eq!(
            generated.warnings,
            vec![
                "resource method `User.update-email` generated as a stub because no operation could be bound"
                    .to_string()
            ]
        );
    }

    #[test]
    fn definitions_only_generation_does_not_require_endpoint() {
        let wit = r#"
package temporal:example@1.0.0;

world system {
  export example-service;
}

interface example-service {
  record request {
    name: string,
  }

  record response {
    message: string,
  }

  example-operation: func(request: request) -> response;
}
"#;
        let spec = crate::parser::parse_api_spec_from_wit_for_language(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
        )
        .unwrap();
        let descriptors =
            DescriptorIndex::from_descriptor_set(FileDescriptorSet { file: Vec::new() }).unwrap();

        let generated = generate_files_with_mode(
            Language::Python,
            spec,
            &descriptors,
            &SupportFiles::default(),
            GenerationMode::DefinitionsOnly,
        )
        .unwrap();

        assert!(generated.files.contains_key(&PathBuf::from("models.py")));
        assert!(generated.files.contains_key(&PathBuf::from("services.py")));
        assert!(
            !generated
                .files
                .contains_key(&PathBuf::from("operations/example_operation.py"))
        );
        assert!(generated.files[&PathBuf::from("services.py")].contains("class ExampleService"));
    }
}
