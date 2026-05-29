mod api_plan;

pub mod add_rpc;
pub mod descriptors;
pub mod error;
pub mod generator;
pub mod language;
pub mod python;
pub mod resources;
pub mod spec;
pub mod typescript;
pub mod validation;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use add_rpc::generate_add_rpc_wit;
use descriptors::DescriptorIndex;
use error::Result;
use generator::{GeneratedFiles, GeneratedOutputLayout, generate_files};
use heck::ToSnakeCase;
use language::Language;
use spec::{ApiSpec, write_prepared_wit_directory};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportFiles {
    pub python: Option<String>,
    pub typescript: Option<String>,
}

pub struct GenerateRequest {
    pub language: Language,
    pub input_paths: Vec<PathBuf>,
    pub descriptor_paths: Vec<PathBuf>,
    pub output_path: PathBuf,
    pub format: bool,
}

pub struct AddRpcRequest {
    pub descriptor_paths: Vec<PathBuf>,
    pub rpc_name: String,
    pub input_paths: Vec<PathBuf>,
    pub output_path: Option<PathBuf>,
}

pub struct DebugWitDirRequest {
    pub input_paths: Vec<PathBuf>,
    pub output_path: PathBuf,
}

pub struct BuildExamplesRequest {
    pub languages: Vec<Language>,
    pub example_ids: Vec<String>,
}

pub fn generate_to_string(
    language: Language,
    input_path: impl AsRef<Path>,
    descriptor_paths: &[PathBuf],
) -> Result<String> {
    generate_to_string_with_inputs(
        language,
        &[input_path.as_ref().to_path_buf()],
        descriptor_paths,
    )
}

pub fn generate_to_string_with_inputs(
    language: Language,
    input_paths: &[PathBuf],
    descriptor_paths: &[PathBuf],
) -> Result<String> {
    let input_path = primary_input_path(input_paths)?;
    let spec = ApiSpec::load_for_language_with_inputs(language, input_paths)?;
    let descriptors = DescriptorIndex::load_many(descriptor_paths)?;
    let support = load_support_files(language, &spec, input_path)?;
    let generated = generate_files(language, &spec, &descriptors, &support)?;
    print_warnings(&generated);
    Ok(match generated.layout {
        GeneratedOutputLayout::SingleFile => generated
            .single_file_contents()
            .expect("single-file output should contain one file")
            .to_string(),
        GeneratedOutputLayout::Directory => render_generated_files_for_debug(&generated),
    })
}

pub fn generate_to_file(request: &GenerateRequest) -> Result<()> {
    let input_path = primary_input_path(&request.input_paths)?;
    let spec = ApiSpec::load_for_language_with_inputs(request.language, &request.input_paths)?;
    let descriptors = DescriptorIndex::load_many(&request.descriptor_paths)?;
    let support = load_support_files(request.language, &spec, input_path)?;
    let generated = generate_files(request.language, &spec, &descriptors, &support)?;
    print_warnings(&generated);

    write_generated_files(&request.output_path, &generated)?;

    if request.format {
        format_generated_file(request.language, &request.output_path)?;
    }

    Ok(())
}

pub fn add_rpc_to_string(
    descriptor_paths: &[PathBuf],
    rpc_name: &str,
    input_paths: &[PathBuf],
) -> Result<String> {
    let descriptors = DescriptorIndex::load_many(descriptor_paths)?;
    let (input_path, linked_input_paths) = add_rpc_input_parts(input_paths);
    if let Some(input_path) = input_path {
        let input = fs::read_to_string(input_path).map_err(|source| error::Error::ReadFile {
            path: input_path.to_path_buf(),
            source,
        })?;
        add_rpc::generate_add_rpc_wit_with_input(
            &descriptors,
            rpc_name,
            input_path,
            &input,
            linked_input_paths,
        )
    } else {
        generate_add_rpc_wit(&descriptors, rpc_name, input_paths)
    }
}

pub fn add_rpc_to_file(request: &AddRpcRequest) -> Result<()> {
    let output = add_rpc_to_string(
        &request.descriptor_paths,
        &request.rpc_name,
        &request.input_paths,
    )?;
    if let Some(path) = &request.output_path {
        fs::write(path, output).map_err(|source| error::Error::WriteFile {
            path: path.clone(),
            source,
        })?;
    } else {
        print!("{output}");
    }
    Ok(())
}

fn add_rpc_input_parts(input_paths: &[PathBuf]) -> (Option<&Path>, &[PathBuf]) {
    if let Some((first, rest)) = input_paths.split_first() {
        if first.is_file()
            || first
                .extension()
                .is_some_and(|extension| extension == "wit")
        {
            return (Some(first.as_path()), rest);
        }
    }
    (None, input_paths)
}

pub fn debug_wit_dir_to_file(request: &DebugWitDirRequest) -> Result<()> {
    write_prepared_wit_directory(&request.input_paths, &request.output_path)
}

pub fn build_examples(request: &BuildExamplesRequest) -> Result<()> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let use_default_languages = request.languages.is_empty();
    let languages = if use_default_languages {
        vec![Language::Python, Language::TypeScript]
    } else {
        request.languages.clone()
    };

    for language in languages {
        let example_ids = if request.example_ids.is_empty() {
            discover_example_ids(&repo_root, language)?
        } else if use_default_languages {
            filter_available_example_ids(&repo_root, language, &request.example_ids)?
        } else {
            validate_example_ids(&repo_root, language, &request.example_ids)?
        };
        if example_ids.is_empty() {
            continue;
        }

        if language == Language::TypeScript {
            ensure_typescript_dependencies(&repo_root)?;
        }

        for example_id in example_ids {
            build_example(&repo_root, language, &example_id)?;
        }
    }

    Ok(())
}

fn primary_input_path(input_paths: &[PathBuf]) -> Result<&Path> {
    input_paths
        .first()
        .map(PathBuf::as_path)
        .ok_or_else(|| error::Error::InvalidWit {
            path: PathBuf::from("<input>"),
            reason: "at least one WIT input path is required".to_string(),
        })
}

fn format_generated_file(language: Language, output_path: &Path) -> Result<()> {
    let (program, args) = formatter_command(language, output_path)?;
    let command = format_formatter_command(program, &args);
    let status = Command::new(program)
        .args(&args)
        .status()
        .map_err(|source| error::Error::RunFormatter {
            path: output_path.to_path_buf(),
            command: command.clone(),
            source,
        })?;

    if !status.success() {
        return Err(error::Error::FormatterFailed {
            path: output_path.to_path_buf(),
            command,
            status,
        });
    }

    Ok(())
}

fn render_generated_files_for_debug(generated: &GeneratedFiles) -> String {
    generated
        .files
        .iter()
        .map(|(path, contents)| format!("### {}\n{contents}", path.display()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn print_warnings(generated: &GeneratedFiles) {
    for warning in &generated.warnings {
        eprintln!("warning: {warning}");
    }
}

fn write_generated_files(output_path: &Path, generated: &GeneratedFiles) -> Result<()> {
    match generated.layout {
        GeneratedOutputLayout::SingleFile => {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|source| error::Error::WriteFile {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(
                output_path,
                generated
                    .single_file_contents()
                    .expect("single-file output should contain one file"),
            )
            .map_err(|source| error::Error::WriteFile {
                path: output_path.to_path_buf(),
                source,
            })?;
        }
        GeneratedOutputLayout::Directory => {
            if output_path.is_file() {
                return Err(error::Error::OutputPathExists {
                    path: output_path.to_path_buf(),
                });
            }
            if output_path.exists() {
                fs::remove_dir_all(output_path).map_err(|source| error::Error::WriteFile {
                    path: output_path.to_path_buf(),
                    source,
                })?;
            }
            fs::create_dir_all(output_path).map_err(|source| error::Error::WriteFile {
                path: output_path.to_path_buf(),
                source,
            })?;

            for (relative_path, contents) in &generated.files {
                let path = output_path.join(relative_path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|source| error::Error::WriteFile {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::write(&path, contents).map_err(|source| error::Error::WriteFile {
                    path: path.clone(),
                    source,
                })?;
            }
        }
    }

    Ok(())
}

fn formatter_command(
    language: Language,
    output_path: &Path,
) -> Result<(&'static str, Vec<String>)> {
    let output_path = output_path.to_string_lossy().into_owned();
    match language {
        Language::Python => Ok((
            "ruff",
            vec![
                "format".to_string(),
                "--line-length".to_string(),
                "88".to_string(),
                output_path,
            ],
        )),
        Language::TypeScript => Ok((
            "prettier",
            vec![
                "--write".to_string(),
                "--print-width".to_string(),
                "88".to_string(),
                output_path,
            ],
        )),
        _ => Err(error::Error::UnsupportedLanguage { language }),
    }
}

fn format_formatter_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn load_support_files(
    language: Language,
    spec: &ApiSpec,
    _input_path: &Path,
) -> Result<SupportFiles> {
    let support_fragments = spec.support.fragments_for_language(language);
    let support_contents = if support_fragments.is_empty() {
        None
    } else {
        Some(
            support_fragments
                .iter()
                .map(|fragment| fragment.contents.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    };

    Ok(match language {
        Language::Python => SupportFiles {
            python: support_contents,
            typescript: None,
        },
        Language::TypeScript => SupportFiles {
            python: None,
            typescript: support_contents,
        },
        _ => SupportFiles::default(),
    })
}

fn discover_example_ids(repo_root: &Path, language: Language) -> Result<Vec<String>> {
    let language_root = example_language_root(repo_root, language);
    let mut ids = fs::read_dir(repo_root.join("examples/inputs"))
        .map_err(|source| error::Error::ReadFile {
            path: repo_root.join("examples/inputs"),
            source,
        })?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let example_id = if path.is_file() {
                path.file_stem()?.to_string_lossy().into_owned()
            } else if path.join("main.wit").is_file() {
                path.file_name()?.to_string_lossy().into_owned()
            } else {
                return None;
            };
            if language_root
                .join(example_directory_name(language, &example_id))
                .is_dir()
            {
                Some(example_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

fn validate_example_ids(
    repo_root: &Path,
    language: Language,
    example_ids: &[String],
) -> Result<Vec<String>> {
    let available = discover_example_ids(repo_root, language)?
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    for example_id in example_ids {
        if !available.contains(example_id) {
            return Err(error::Error::UnknownExampleId {
                language,
                example_id: example_id.clone(),
            });
        }
    }
    Ok(example_ids.to_vec())
}

fn filter_available_example_ids(
    repo_root: &Path,
    language: Language,
    example_ids: &[String],
) -> Result<Vec<String>> {
    let available = discover_example_ids(repo_root, language)?
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    Ok(example_ids
        .iter()
        .filter(|example_id| available.contains(example_id.as_str()))
        .cloned()
        .collect())
}

fn ensure_typescript_dependencies(repo_root: &Path) -> Result<()> {
    let cwd = example_language_root(repo_root, Language::TypeScript);
    if cwd.join("node_modules").exists() {
        return Ok(());
    }

    let command = "npm install --no-fund --no-audit".to_string();
    let status = Command::new("npm")
        .current_dir(&cwd)
        .args(["install", "--no-fund", "--no-audit"])
        .status()
        .map_err(|source| error::Error::RunCommand {
            cwd: cwd.clone(),
            command: command.clone(),
            source,
        })?;

    if !status.success() {
        return Err(error::Error::CommandFailed {
            cwd,
            command,
            status,
        });
    }

    Ok(())
}

fn build_example(repo_root: &Path, language: Language, example_id: &str) -> Result<()> {
    let descriptor_path = repo_root.join("examples/descriptors/temporal_api.bin");
    let input_path = example_input_path(repo_root, example_id);
    let mut input_paths = vec![input_path];
    input_paths.extend(example_linked_input_paths(repo_root));
    let output_path = example_output_path(repo_root, language, example_id);

    generate_to_file(&GenerateRequest {
        language,
        input_paths,
        descriptor_paths: vec![descriptor_path],
        output_path: output_path.clone(),
        format: false,
    })?;
    format_example_output(repo_root, language, &output_path)?;

    println!("Built {} with nex-gen", output_path.display());
    Ok(())
}

fn example_language_root(repo_root: &Path, language: Language) -> PathBuf {
    match language {
        Language::Python => repo_root.join("examples/python"),
        Language::TypeScript => repo_root.join("examples/typescript"),
        _ => repo_root.join("examples").join(language.as_str()),
    }
}

fn example_input_path(repo_root: &Path, example_id: &str) -> PathBuf {
    let flat_path = repo_root
        .join("examples/inputs")
        .join(format!("{example_id}.wit"));
    if flat_path.is_file() {
        flat_path
    } else {
        repo_root
            .join("examples/inputs")
            .join(example_id)
            .join("main.wit")
    }
}

fn example_linked_input_paths(repo_root: &Path) -> Vec<PathBuf> {
    let linked_inputs = repo_root.join("examples/inputs/deps");
    if linked_inputs.is_dir() {
        vec![linked_inputs]
    } else {
        Vec::new()
    }
}

fn example_output_path(repo_root: &Path, language: Language, example_id: &str) -> PathBuf {
    match language {
        Language::Python => example_language_root(repo_root, language)
            .join(example_directory_name(language, example_id)),
        Language::TypeScript => example_language_root(repo_root, language).join(example_id),
        _ => example_language_root(repo_root, language).join(example_id),
    }
}

fn example_directory_name(language: Language, example_id: &str) -> String {
    match language {
        Language::Python => python_example_package_name(example_id),
        _ => example_id.to_string(),
    }
}

fn python_example_package_name(example_id: &str) -> String {
    example_id.to_snake_case()
}

fn format_example_output(repo_root: &Path, language: Language, output_path: &Path) -> Result<()> {
    let (cwd, program, args): (PathBuf, &str, Vec<String>) = match language {
        Language::Python => (
            example_language_root(repo_root, language),
            "uv",
            vec![
                "run".to_string(),
                "ruff".to_string(),
                "format".to_string(),
                "--line-length".to_string(),
                "88".to_string(),
                "--config".to_string(),
                "pyproject.toml".to_string(),
                output_path.to_string_lossy().into_owned(),
            ],
        ),
        Language::TypeScript => (
            example_language_root(repo_root, language),
            "npm",
            vec![
                "exec".to_string(),
                "--".to_string(),
                "prettier".to_string(),
                "--write".to_string(),
                "--print-width".to_string(),
                "88".to_string(),
                output_path.to_string_lossy().into_owned(),
            ],
        ),
        language => return Err(error::Error::UnsupportedLanguage { language }),
    };
    let command = format_command(program, &args);
    let status = Command::new(program)
        .current_dir(&cwd)
        .args(&args)
        .status()
        .map_err(|source| error::Error::RunCommand {
            cwd: cwd.clone(),
            command: command.clone(),
            source,
        })?;

    if !status.success() {
        return Err(error::Error::CommandFailed {
            cwd,
            command,
            status,
        });
    }

    Ok(())
}

fn format_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{format_formatter_command, formatter_command};
    use crate::language::Language;

    #[test]
    fn chooses_python_formatter_command() {
        let (program, args) = formatter_command(Language::Python, Path::new("output")).unwrap();
        assert_eq!(program, "ruff");
        assert_eq!(args, vec!["format", "--line-length", "88", "output"]);
        assert_eq!(
            format_formatter_command(program, &args),
            "ruff format --line-length 88 output"
        );
    }

    #[test]
    fn chooses_typescript_formatter_command() {
        let (program, args) = formatter_command(Language::TypeScript, Path::new("output")).unwrap();
        assert_eq!(program, "prettier");
        assert_eq!(args, vec!["--write", "--print-width", "88", "output"]);
        assert_eq!(
            format_formatter_command(program, &args),
            "prettier --write --print-width 88 output"
        );
    }
}
