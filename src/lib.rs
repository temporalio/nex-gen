mod planning;

pub mod add_rpc;
pub mod content_encoding;
pub mod descriptors;
pub mod error;
pub mod format;
pub mod generator;
pub mod language;
pub mod parser;
pub mod pattern;
pub mod resources;
pub mod spec;
pub mod validation;
pub mod workspace;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use descriptors::DescriptorIndex;
use error::Result;
use generator::go;
use generator::{GenerateFilesOptions, GeneratedFiles, GeneratedOutputLayout, GenerationMode};
use heck::ToSnakeCase;
use language::Language;
use spec::SupportFragmentSpec;
use workspace::{ApiSpecNode, ApiSpecTree};

pub use add_rpc::{AddRpcRequest, add_rpc_to_file, add_rpc_to_string};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportFiles {
    pub fragments: Vec<SupportFragmentSpec>,
}

pub struct GenerateRequest {
    pub language: Language,
    pub input_paths: Vec<PathBuf>,
    pub support_paths: Vec<PathBuf>,
    pub descriptor_paths: Vec<PathBuf>,
    pub output_path: PathBuf,
    pub format: bool,
    pub generate_native_api: bool,
    /// TypeScript-only: the in-memory representation for materialized temporal
    /// `format` fields. `Default` is `TsDateTimeTypes::String`.
    pub ts_date_time_types: generator::TsDateTimeTypes,
}

pub struct BuildExamplesRequest {
    pub languages: Vec<Language>,
    pub example_ids: Vec<String>,
}

pub fn generate_to_file(request: &GenerateRequest) -> Result<()> {
    let tree = crate::parser::load_api_spec_tree_for_language_with_inputs(
        request.language,
        &request.input_paths,
    )?;
    let descriptors = DescriptorIndex::load_many(&request.descriptor_paths)?;
    let support = load_support_files_for_tree(request.language, &tree, &request.support_paths)?;
    let options = GenerateFilesOptions {
        go_import_root: if request.language == Language::Go {
            infer_go_import_root(&request.output_path)?
        } else {
            None
        },
        java_package_root: if request.language == Language::Java {
            Some(infer_java_package_root(&request.output_path)?)
        } else {
            None
        },
        ts_date_time_types: request.ts_date_time_types,
    };
    let generated = generator::generate_files_for_tree_with_mode_and_options(
        request.language,
        tree,
        &descriptors,
        &support,
        if request.generate_native_api {
            GenerationMode::NativeApi
        } else {
            GenerationMode::DefinitionsOnly
        },
        options,
    )?;
    print_warnings(&generated);

    write_generated_files(&request.output_path, &generated)?;

    if request.format {
        format_generated_file(request.language, &request.output_path)?;
    }

    Ok(())
}

pub fn build_examples(request: &BuildExamplesRequest) -> Result<()> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let use_default_languages = request.languages.is_empty();
    let languages = if use_default_languages {
        vec![
            Language::Dotnet,
            Language::Python,
            Language::TypeScript,
            Language::Go,
        ]
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

pub fn build_json_examples(request: &BuildExamplesRequest) -> Result<()> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let languages = if request.languages.is_empty() {
        vec![
            Language::Python,
            Language::TypeScript,
            Language::Go,
            Language::Java,
        ]
    } else {
        request.languages.clone()
    };

    for language in languages {
        if !matches!(
            language,
            Language::Python | Language::TypeScript | Language::Go | Language::Java
        ) {
            return Err(error::Error::UnsupportedLanguage { language });
        }
        if language == Language::TypeScript {
            ensure_typescript_dependencies(&repo_root)?;
        }

        let example_ids = if request.example_ids.is_empty() {
            discover_json_example_ids(&repo_root)?
        } else {
            validate_json_example_ids(&repo_root, language, &request.example_ids)?
        };
        for example_id in example_ids {
            build_json_example(&repo_root, language, &example_id)?;
        }
    }

    Ok(())
}

fn infer_go_import_root(output_path: &Path) -> Result<Option<String>> {
    let output_path = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| error::Error::ReadFile {
                path: PathBuf::from("."),
                source,
            })?
            .join(output_path)
    };

    for ancestor in output_path.ancestors() {
        let go_mod_path = ancestor.join("go.mod");
        if !go_mod_path.is_file() {
            continue;
        }
        let module_name = read_go_module_name(&go_mod_path)?;
        let relative_output = output_path.strip_prefix(ancestor).unwrap_or(Path::new(""));
        let relative_output = relative_output
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        return Ok(Some(if relative_output.is_empty() {
            module_name
        } else {
            format!("{module_name}/{relative_output}")
        }));
    }

    Ok(None)
}

/// Derives the Java base package from the output directory, analogous to how
/// `infer_go_import_root` finds a `go.mod` ancestor. The package is the path
/// components from a `src/main/java` (or `src/test/java`) source-root ancestor
/// down to the output directory, joined with '.'. When no such ancestor exists,
/// falls back to the output directory's basename as a single-segment package.
fn infer_java_package_root(output_path: &Path) -> Result<String> {
    let output_path = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| error::Error::ReadFile {
                path: PathBuf::from("."),
                source,
            })?
            .join(output_path)
    };

    for ancestor in output_path.ancestors() {
        let is_source_root = ancestor.file_name().and_then(|name| name.to_str()) == Some("java")
            && ancestor
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, "main" | "test"))
            && ancestor
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                == Some("src");
        if !is_source_root {
            continue;
        }
        let relative = output_path.strip_prefix(ancestor).unwrap_or(Path::new(""));
        let segments = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            return Ok(segments.join("."));
        }
    }

    let basename = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("generated")
        .to_string();
    Ok(basename)
}

fn read_go_module_name(go_mod_path: &Path) -> Result<String> {
    let contents = fs::read_to_string(go_mod_path).map_err(|source| error::Error::ReadFile {
        path: go_mod_path.to_path_buf(),
        source,
    })?;
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("module "))
        .map(str::trim)
        .filter(|module| !module.is_empty())
        .map(str::to_string)
        .ok_or_else(|| error::Error::InvalidGeneratedPath {
            path: go_mod_path.to_path_buf(),
            reason: "go.mod does not contain a module directive".to_string(),
        })
}

fn format_generated_file(language: Language, output_path: &Path) -> Result<()> {
    if language == Language::Java {
        // No formatter dependency is required for Java; skip formatting.
        return Ok(());
    }
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
        Language::Go => Ok(("gofmt", vec!["-w".to_string(), output_path])),
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

fn load_support_files_for_tree(
    language: Language,
    tree: &ApiSpecTree,
    support_paths: &[PathBuf],
) -> Result<SupportFiles> {
    if !support_paths.is_empty() {
        return Ok(SupportFiles {
            fragments: load_support_fragments_from_paths(language, support_paths)?,
        });
    }
    let mut fragments = Vec::new();
    collect_tree_support_fragments(language, &tree.root, &mut fragments);
    Ok(SupportFiles { fragments })
}

fn collect_tree_support_fragments(
    language: Language,
    node: &ApiSpecNode,
    fragments: &mut Vec<SupportFragmentSpec>,
) {
    match node {
        ApiSpecNode::Leaf(leaf) => {
            fragments.extend(leaf.spec.support.fragments_for_language(language).to_vec());
        }
        ApiSpecNode::Branch(branch) => {
            for child in branch.children.values() {
                collect_tree_support_fragments(language, child, fragments);
            }
        }
    }
}

fn load_support_fragments_from_paths(
    language: Language,
    support_paths: &[PathBuf],
) -> Result<Vec<SupportFragmentSpec>> {
    support_paths
        .iter()
        .map(|path| {
            let contents = fs::read_to_string(path).map_err(|source| error::Error::ReadFile {
                path: path.clone(),
                source,
            })?;
            Ok(SupportFragmentSpec {
                path: path.to_string_lossy().replace('\\', "/"),
                namespace: infer_support_namespace(language, &contents),
                contents,
            })
        })
        .collect()
}

fn infer_support_namespace(language: Language, contents: &str) -> Option<String> {
    match language {
        Language::Dotnet => infer_dotnet_namespace(contents),
        _ => None,
    }
}

fn infer_dotnet_namespace(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let line = line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() != Some("namespace") {
            continue;
        }
        let namespace = parts
            .next()
            .unwrap_or("")
            .trim_end_matches(|character| character == '{' || character == ';');
        if !namespace.is_empty() {
            return Some(namespace.to_string());
        }
    }
    None
}

fn discover_example_ids(repo_root: &Path, language: Language) -> Result<Vec<String>> {
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
            if !example_is_excluded(language, &example_id)
                && example_output_path(repo_root, language, &example_id).is_dir()
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

fn example_is_excluded(_language: Language, _example_id: &str) -> bool {
    false
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

fn discover_json_example_ids(repo_root: &Path) -> Result<Vec<String>> {
    let input_root = repo_root.join("examples/json-inputs");
    let mut ids = fs::read_dir(&input_root)
        .map_err(|source| error::Error::ReadFile {
            path: input_root.clone(),
            source,
        })?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_dir() {
                return directory_contains_input_file(&path)
                    .then(|| path.file_name()?.to_str().map(str::to_string))
                    .flatten();
            }
            if !is_json_schema_input_path(&path) {
                return None;
            }
            Some(path.file_stem()?.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

fn directory_contains_input_file(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if directory_contains_input_file(&path) {
                return true;
            }
        } else if path.is_file() {
            return true;
        }
    }
    false
}

fn is_json_schema_input_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "json" | "yaml" | "yml"))
}

fn validate_json_example_ids(
    repo_root: &Path,
    language: Language,
    example_ids: &[String],
) -> Result<Vec<String>> {
    let available = discover_json_example_ids(repo_root)?
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
        support_paths: Vec::new(),
        descriptor_paths: vec![descriptor_path],
        output_path: output_path.clone(),
        format: false,
        generate_native_api: true,
        ts_date_time_types: Default::default(),
    })?;
    format_example_output(repo_root, language, &output_path)?;

    println!("Built {} with nex-gen", output_path.display());
    Ok(())
}

fn build_json_example(repo_root: &Path, language: Language, example_id: &str) -> Result<()> {
    // Default: the string temporal representation, written under the example's
    // own directory name.
    build_json_example_variant(
        repo_root,
        language,
        example_id,
        example_id,
        generator::TsDateTimeTypes::String,
    )?;

    // TypeScript is the only target with more than one temporal in-memory shape,
    // selected by `--ts-date-time-types`. Emit the `date` and `temporal` variants
    // of the `temporal` example into distinct directories so all three modes are
    // generated and snapshot-tested. Go / Java / Python are unaffected.
    if language == Language::TypeScript && example_id == "temporal" {
        build_json_example_variant(
            repo_root,
            language,
            example_id,
            "temporal-date",
            generator::TsDateTimeTypes::Date,
        )?;
        build_json_example_variant(
            repo_root,
            language,
            example_id,
            "temporal-temporal",
            generator::TsDateTimeTypes::Temporal,
        )?;
    }

    Ok(())
}

/// Generates one JSON example (definitions + native-api) from the `input_id`
/// schema into the `output_id` directory under the given TS temporal repr.
fn build_json_example_variant(
    repo_root: &Path,
    language: Language,
    input_id: &str,
    output_id: &str,
    ts_date_time_types: generator::TsDateTimeTypes,
) -> Result<()> {
    let input_path = json_example_input_path(repo_root, input_id);
    let definitions_output_path = json_example_output_path(
        repo_root,
        language,
        output_id,
        GenerationMode::DefinitionsOnly,
    );

    generate_to_file(&GenerateRequest {
        language,
        input_paths: vec![input_path.clone()],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: definitions_output_path.clone(),
        format: false,
        generate_native_api: false,
        ts_date_time_types,
    })?;
    format_example_output(repo_root, language, &definitions_output_path)?;

    println!("Built {} with nex-gen", definitions_output_path.display());

    let api_output_path =
        json_example_output_path(repo_root, language, output_id, GenerationMode::NativeApi);
    generate_to_file(&GenerateRequest {
        language,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: api_output_path.clone(),
        format: false,
        generate_native_api: true,
        ts_date_time_types,
    })?;
    format_example_output(repo_root, language, &api_output_path)?;

    println!("Built {} with nex-gen", api_output_path.display());

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

fn json_example_input_path(repo_root: &Path, example_id: &str) -> PathBuf {
    let dir_path = repo_root.join("examples/json-inputs").join(example_id);
    if dir_path.is_dir() {
        return dir_path;
    }
    for extension in ["yaml", "yml", "json"] {
        let path = repo_root
            .join("examples/json-inputs")
            .join(format!("{example_id}.{extension}"));
        if path.is_file() {
            return path;
        }
    }
    repo_root
        .join("examples/json-inputs")
        .join(format!("{example_id}.yaml"))
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
        Language::Go => example_language_root(repo_root, language)
            .join(example_directory_name(language, example_id)),
        _ => example_language_root(repo_root, language)
            .join("wit")
            .join(example_directory_name(language, example_id)),
    }
}

fn json_example_output_path(
    repo_root: &Path,
    language: Language,
    example_id: &str,
    generation_mode: GenerationMode,
) -> PathBuf {
    let mode_directory = match generation_mode {
        GenerationMode::NativeApi => "api",
        GenerationMode::DefinitionsOnly => "definitions",
    };
    if language == Language::Java {
        // Java lands under the Gradle source root so the derived package is
        // `json_schema.<mode>.<example>`, giving the api/definitions x chat/kb
        // variants distinct, non-colliding packages.
        return example_language_root(repo_root, language)
            .join("src/main/java/json_schema")
            .join(mode_directory)
            .join(example_directory_name(language, example_id));
    }
    example_language_root(repo_root, language)
        .join("json_schema")
        .join(mode_directory)
        .join(example_directory_name(language, example_id))
}

fn example_directory_name(language: Language, example_id: &str) -> String {
    match language {
        Language::Go => go::go_package_name(example_id),
        Language::Python => python_example_package_name(example_id),
        _ => example_id.to_string(),
    }
}

fn python_example_package_name(example_id: &str) -> String {
    example_id.to_snake_case()
}

fn format_example_output(repo_root: &Path, language: Language, output_path: &Path) -> Result<()> {
    let (cwd, program, args): (PathBuf, &str, Vec<String>) = match language {
        Language::Dotnet => return Ok(()),
        Language::Java => return Ok(()),
        Language::Go => (
            example_language_root(repo_root, language),
            "gofmt",
            vec!["-w".to_string(), output_path.to_string_lossy().into_owned()],
        ),
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

    use super::{format_formatter_command, formatter_command, infer_dotnet_namespace};
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

    #[test]
    fn infers_dotnet_support_namespace_from_file() {
        assert_eq!(
            infer_dotnet_namespace(
                r#"
using System;

namespace NexGen.Support
{
    internal static class TemporalSupport { }
}
"#
            )
            .as_deref(),
            Some("NexGen.Support")
        );
        assert_eq!(
            infer_dotnet_namespace("namespace NexGen.Support;\ninternal static class Support { }")
                .as_deref(),
            Some("NexGen.Support")
        );
    }
}
