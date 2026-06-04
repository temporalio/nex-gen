use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::SupportFiles;
use crate::api_plan::build_api_plan;
use crate::descriptors::DescriptorIndex;
use crate::error::{Error, Result};
use crate::language::Language;
use crate::go;
use crate::python;
use crate::resources::ensure_unique_resource_names;
use crate::spec::ApiSpec;
use crate::typescript;
use crate::validation::validate_type_overrides;

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

#[derive(Debug, Clone, Copy, Default)]
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

pub fn generate_files(
    language: Language,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
) -> Result<GeneratedFiles> {
    validate_type_overrides(spec, descriptors, language)?;
    ensure_unique_resource_names(spec)?;
    let plan = build_api_plan(spec, descriptors)?;
    let warnings = generation_warnings(&plan);

    let mut generated = match language {
        Language::Go => go::generate(&plan, spec.support.fragments_for_language(language)),
        Language::Python => python::generate(&plan, spec.support.fragments_for_language(language)),
        Language::TypeScript => typescript::generate(&plan, support),
        language => Err(Error::UnsupportedLanguage { language }),
    }?;
    generated.warnings = warnings;
    Ok(generated)
}

pub fn generate_source(
    language: Language,
    spec: &ApiSpec,
    descriptors: &DescriptorIndex,
    support: &SupportFiles,
) -> Result<String> {
    let generated = generate_files(language, spec, descriptors, support)?;
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

fn generation_warnings(plan: &crate::api_plan::ApiPlan) -> Vec<String> {
    plan.services
        .iter()
        .flat_map(|service| {
            service.resources.iter().flat_map(|resource| {
                resource.methods.iter().filter_map(|method| {
                    matches!(
                        method.binding,
                        crate::api_plan::PlannedResourceMethodBindingSpec::Stub
                    )
                    .then(|| {
                        format!(
                            "resource method `{}.{}` generated as a stub because no operation could be bound",
                            resource.type_name, method.name
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
    use crate::spec::ApiSpec;

    use super::generate_files;

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
        let spec = ApiSpec::parse_for_language(Language::Python, wit, PathBuf::from("inline.wit"))
            .unwrap();
        let descriptors =
            DescriptorIndex::from_descriptor_set(FileDescriptorSet { file: Vec::new() }).unwrap();
        let generated = generate_files(
            Language::Python,
            &spec,
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
}
