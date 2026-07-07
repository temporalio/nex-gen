use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) mod dotnet;
pub(crate) mod json;
pub(crate) mod proto;
pub(crate) mod python;
pub(crate) mod typescript;

use crate::SupportFiles;
use crate::descriptors::DescriptorIndex;
use crate::error::{Error, Result};
use crate::language::Language;
use crate::planning::{
    PlannedSpec, PlannedType, PlannedTypeFamily, PlanningMode, build_api_plan_with_mode,
    build_api_plans_for_tree_with_mode,
};
use crate::resources::ensure_unique_resource_names;
use crate::spec::{ApiSpec, RecordSpec};
use crate::validation::validate_external_type_bindings;
use crate::workspace::{ApiSpecBranch, ApiSpecLeaf, ApiSpecNode, ApiSpecTree};

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
    pub(crate) const BIDIRECTIONAL: Self = Self {
        from_wire: true,
        to_wire: true,
    };
    pub(crate) const TO_WIRE_ONLY: Self = Self {
        from_wire: false,
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
    validate_external_type_bindings(&spec, descriptors, language)?;
    ensure_unique_resource_names(&spec)?;
    let support_fragments = if support.fragments.is_empty() {
        spec.support.fragments_for_language(language).to_vec()
    } else {
        support.fragments.clone()
    };
    let plan = build_api_plan_with_mode(spec, descriptors, planning_mode(mode))?;
    generate_files_from_plan(language, plan, &support_fragments, mode)
}

fn generate_files_from_plan(
    language: Language,
    plan: PlannedSpec,
    support_fragments: &[crate::spec::SupportFragmentSpec],
    mode: GenerationMode,
) -> Result<GeneratedFiles> {
    let warnings = if mode == GenerationMode::NativeApi {
        generation_warnings(&plan)
    } else {
        Vec::new()
    };

    let mut generated = match language {
        Language::Dotnet => dotnet::generate(&plan, support_fragments, mode),
        Language::Python => python::generate(&plan, support_fragments, mode),
        Language::TypeScript => typescript::generate(&plan, support_fragments, mode),
        language => Err(Error::UnsupportedLanguage { language }),
    }?;
    generated.warnings = warnings;
    Ok(generated)
}

pub fn generate_files_for_tree_with_mode(
    language: Language,
    tree: ApiSpecTree,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
    mode: GenerationMode,
) -> Result<GeneratedFiles> {
    match &tree.root {
        ApiSpecNode::Leaf(leaf) => {
            generate_files_with_mode(language, leaf.spec.clone(), descriptors, support, mode)
        }
        ApiSpecNode::Branch(_) => {
            validate_tree_specs(&tree.root, descriptors, language)?;
            let planned_tree =
                build_api_plans_for_tree_with_mode(tree, descriptors, planning_mode(mode))?;
            let mut files = BTreeMap::new();
            let mut warnings = Vec::new();
            let ApiSpecNode::Branch(branch) = planned_tree.root else {
                unreachable!("planned tree root should stay a branch");
            };
            insert_branch_index_files(language, &branch, &mut files)?;
            insert_tree_support_files(language, &branch, &mut files)?;
            if language == Language::Python {
                let path = PathBuf::from("_rebuild.py");
                if files
                    .insert(
                        path.clone(),
                        python::render_tree_model_rebuild_module(&branch),
                    )
                    .is_some()
                {
                    return Err(Error::GeneratedFileConflict { path });
                }
            }
            for node in branch.children.into_values() {
                generate_planned_tree_node(
                    language,
                    node,
                    support,
                    mode,
                    &mut files,
                    &mut warnings,
                )?;
            }
            Ok(GeneratedFiles {
                layout: GeneratedOutputLayout::Directory,
                files,
                warnings,
            })
        }
    }
}

fn insert_tree_support_files(
    language: Language,
    branch: &ApiSpecBranch<PlannedTypeFamily>,
    files: &mut BTreeMap<PathBuf, String>,
) -> Result<()> {
    let support_files = match language {
        Language::Python => python::render_tree_support_files(branch),
        Language::TypeScript => typescript::render_tree_support_files(branch),
        _ => BTreeMap::new(),
    };
    for (path, contents) in support_files {
        if files.insert(path.clone(), contents).is_some() {
            return Err(Error::GeneratedFileConflict { path });
        }
    }
    Ok(())
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

fn generate_planned_tree_node(
    language: Language,
    node: ApiSpecNode<PlannedTypeFamily>,
    support: &SupportFiles,
    mode: GenerationMode,
    files: &mut BTreeMap<PathBuf, String>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    match node {
        ApiSpecNode::Leaf(leaf) => {
            insert_planned_leaf_generated_files(language, leaf, support, mode, files, warnings)
        }
        ApiSpecNode::Branch(branch) => {
            insert_branch_index_files(language, &branch, files)?;
            for node in branch.children.into_values() {
                generate_planned_tree_node(language, node, support, mode, files, warnings)?;
            }
            Ok(())
        }
    }
}

fn insert_branch_index_files(
    language: Language,
    branch: &crate::workspace::ApiSpecBranch<impl crate::spec::TypeNameFamily>,
    files: &mut BTreeMap<PathBuf, String>,
) -> Result<()> {
    let Some((path, contents)) = branch_index_file(language, branch) else {
        return Ok(());
    };
    if files.insert(path.clone(), contents).is_some() {
        return Err(Error::GeneratedFileConflict { path });
    }
    Ok(())
}

fn branch_index_file(
    language: Language,
    branch: &crate::workspace::ApiSpecBranch<impl crate::spec::TypeNameFamily>,
) -> Option<(PathBuf, String)> {
    let path = branch.module_path.to_path_buf();
    match language {
        Language::Python => {
            let mut file_path = path;
            file_path.push("__init__.py");
            let mut contents = String::from("# Generated by nex-gen. DO NOT EDIT!\n\n");
            for name in branch.children.keys() {
                contents.push_str("from .");
                contents.push_str(&python_package_segment(name));
                contents.push_str(" import *  # noqa: F401,F403\n");
            }
            if branch.module_path.is_root() {
                contents.push_str("from . import _rebuild as _rebuild  # noqa: F401\n");
            }
            Some((file_path, contents))
        }
        Language::TypeScript => {
            let mut file_path = path;
            file_path.push("index.ts");
            let mut contents = String::from("// Generated by nex-gen. DO NOT EDIT!\n\n");
            for name in branch.children.keys() {
                contents.push_str("export * from './");
                contents.push_str(name);
                contents.push_str("';\n");
            }
            Some((file_path, contents))
        }
        _ => None,
    }
}

fn python_package_segment(segment: &str) -> String {
    segment.replace('-', "_")
}

fn insert_planned_leaf_generated_files(
    language: Language,
    leaf: ApiSpecLeaf<PlannedTypeFamily>,
    support: &SupportFiles,
    mode: GenerationMode,
    files: &mut BTreeMap<PathBuf, String>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let prefix = leaf.module_path.to_path_buf();
    let support_fragments = if support.fragments.is_empty() {
        leaf.spec.support.fragments_for_language(language).to_vec()
    } else {
        support.fragments.clone()
    };
    let generated = generate_files_from_plan(language, leaf.spec, &support_fragments, mode)?;
    warnings.extend(generated.warnings);
    for (path, contents) in generated.files {
        let path = prefix.join(path);
        if files.insert(path.clone(), contents).is_some() {
            return Err(Error::GeneratedFileConflict { path });
        }
    }
    Ok(())
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
        assert!(generated.files.contains_key(&PathBuf::from("service.py")));
        assert!(
            !generated
                .files
                .contains_key(&PathBuf::from("operations/example_operation.py"))
        );
        assert!(generated.files[&PathBuf::from("service.py")].contains("class ExampleService"));
    }
}
