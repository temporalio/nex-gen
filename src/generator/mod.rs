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
use crate::planning::{PlannedSpec, PlanningMode, build_api_plan_with_mode};
use crate::resources::ensure_unique_resource_names;
use crate::spec::ApiSpec;
use crate::validation::validate_external_type_bindings;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ModelCapabilities {
    pub(crate) from_proto: bool,
    pub(crate) to_proto: bool,
}

impl ModelCapabilities {
    pub(crate) const BIDIRECTIONAL: Self = Self {
        from_proto: true,
        to_proto: true,
    };
    pub(crate) const TO_PROTO_ONLY: Self = Self {
        from_proto: false,
        to_proto: true,
    };

    pub(crate) fn merge(&mut self, other: Self) {
        self.from_proto |= other.from_proto;
        self.to_proto |= other.to_proto;
    }
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
    let warnings = if mode == GenerationMode::NativeApi {
        generation_warnings(&plan)
    } else {
        Vec::new()
    };

    let mut generated = match language {
        Language::Dotnet => dotnet::generate(&plan, &support_fragments, mode),
        Language::Python => python::generate(&plan, &support_fragments, mode),
        Language::TypeScript => typescript::generate(&plan, &support_fragments, mode),
        language => Err(Error::UnsupportedLanguage { language }),
    }?;
    generated.warnings = warnings;
    Ok(generated)
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
