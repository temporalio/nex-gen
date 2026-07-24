mod planning;

pub mod add_rpc;
pub mod descriptors;
pub mod error;
pub mod generator;
pub mod json_schema;
pub mod language;
pub mod parser;
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
    // `write_generated_files` removes an existing output directory before
    // writing, so a resolved output path with no name at all (the
    // filesystem root, or `..` past it) must be rejected up front rather
    // than risking `fs::remove_dir_all` on it.
    if absolute_output_path(&request.output_path)?
        .file_name()
        .is_none()
    {
        return Err(error::Error::OutputPathIsRoot {
            path: request.output_path.clone(),
        });
    }

    let tree = crate::parser::load_api_spec_tree_for_language_with_inputs(
        request.language,
        &request.input_paths,
    )?;
    let descriptors = DescriptorIndex::load_many(&request.descriptor_paths)?;
    let support = load_support_files_for_tree(request.language, &tree, &request.support_paths)?;
    let options = GenerateFilesOptions {
        go_output_dir_name: if request.language == Language::Go {
            output_dir_name(&request.output_path)?
        } else {
            String::new()
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
            ensure_typescript_dependencies(&advanced_language_root(&repo_root, language))?;
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
            // Definitions land in samples/, native-api in advanced/; each needs
            // its own node_modules for the prettier formatting pass.
            ensure_typescript_dependencies(&samples_language_root(&repo_root, language))?;
            ensure_typescript_dependencies(&advanced_language_root(&repo_root, language))?;
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

/// The output directory's basename, used as the Go package name — matching
/// the Go convention that a package's name is its directory's name. Resolved
/// against the working directory so relative paths, including `.` and `..`,
/// name the real directory rather than being read as a literal path segment
/// (`Path::file_name` returns `None` for a path that lexically terminates in
/// `.` or `..`).
///
/// `generate_to_file` already rejects an output path that resolves to the
/// filesystem root before this runs, so `OutputPathIsRoot` here is
/// unreachable through that caller; the check exists so this function is
/// correct standing alone. A non-UTF-8 path component is a real, if rare,
/// case: reported as an error rather than guessed at.
fn output_dir_name(output_path: &Path) -> Result<String> {
    let resolved = absolute_output_path(output_path)?;
    let name = resolved
        .file_name()
        .ok_or_else(|| error::Error::OutputPathIsRoot {
            path: output_path.to_path_buf(),
        })?;
    name.to_str()
        .map(str::to_string)
        .ok_or_else(|| error::Error::OutputPathNotUtf8 {
            path: output_path.to_path_buf(),
        })
}

/// Resolves `output_path` to an absolute, lexically normalized path: relative
/// paths are joined onto the working directory, then `.`/`..` components are
/// collapsed without touching the filesystem (the output directory may not
/// exist yet, so this can't just `fs::canonicalize`).
fn absolute_output_path(output_path: &Path) -> Result<PathBuf> {
    let absolute = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| error::Error::ReadFile {
                path: PathBuf::from("."),
                source,
            })?
            .join(output_path)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
                if matches!(normalized.last(), Some(std::path::Component::Normal(_))) =>
            {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized.into_iter().collect()
}

/// Derives the Java base package from the output directory. The package is
/// the path components from a `src/main/java` (or `src/test/java`)
/// source-root ancestor down to the output directory, joined with '.'. When
/// no such ancestor exists, falls back to the output directory's basename as
/// a single-segment package.
fn infer_java_package_root(output_path: &Path) -> Result<String> {
    let output_path = absolute_output_path(output_path)?;

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
    let mut ids = fs::read_dir(repo_root.join("advanced/samples/inputs"))
        .map_err(|source| error::Error::ReadFile {
            path: repo_root.join("advanced/samples/inputs"),
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
    let input_root = repo_root.join("samples/schemas");
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
            let file_name = path.file_name()?.to_str()?;
            Some(crate::parser::strip_json_schema_extension(file_name).to_string())
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

fn ensure_typescript_dependencies(cwd: &Path) -> Result<()> {
    if cwd.join("node_modules").exists() {
        return Ok(());
    }

    let command = "npm install --no-fund --no-audit".to_string();
    let status = Command::new("npm")
        .current_dir(cwd)
        .args(["install", "--no-fund", "--no-audit"])
        .status()
        .map_err(|source| error::Error::RunCommand {
            cwd: cwd.to_path_buf(),
            command: command.clone(),
            source,
        })?;

    if !status.success() {
        return Err(error::Error::CommandFailed {
            cwd: cwd.to_path_buf(),
            command,
            status,
        });
    }

    Ok(())
}

fn build_example(repo_root: &Path, language: Language, example_id: &str) -> Result<()> {
    let descriptor_path = repo_root.join("advanced/samples/descriptors/temporal_api.bin");
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
    format_example_output(
        &advanced_language_root(repo_root, language),
        language,
        &output_path,
    )?;

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
    // selected by `--date-time-types`. Emit the `date` and `temporal` variants
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
    format_example_output(
        &samples_language_root(repo_root, language),
        language,
        &definitions_output_path,
    )?;

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
    format_example_output(
        &advanced_language_root(repo_root, language),
        language,
        &api_output_path,
    )?;

    println!("Built {} with nex-gen", api_output_path.display());

    Ok(())
}

/// Root of the beginner-facing JSON-Schema sample project for a language,
/// e.g. `samples/go`. Holds the definitions-mode outputs and their tests.
fn samples_language_root(repo_root: &Path, language: Language) -> PathBuf {
    repo_root.join("samples").join(language.as_str())
}

/// Root of the advanced sample project for a language, e.g. `advanced/samples/go`.
/// Holds the WIT outputs, their tests, and the snapshot-only JSON-Schema
/// native-api outputs.
fn advanced_language_root(repo_root: &Path, language: Language) -> PathBuf {
    repo_root.join("advanced/samples").join(language.as_str())
}

fn example_input_path(repo_root: &Path, example_id: &str) -> PathBuf {
    let flat_path = repo_root
        .join("advanced/samples/inputs")
        .join(format!("{example_id}.wit"));
    if flat_path.is_file() {
        flat_path
    } else {
        repo_root
            .join("advanced/samples/inputs")
            .join(example_id)
            .join("main.wit")
    }
}

fn json_example_input_path(repo_root: &Path, example_id: &str) -> PathBuf {
    let input_root = repo_root.join("samples/schemas");
    let dir_path = input_root.join(example_id);
    if dir_path.is_dir() {
        return dir_path;
    }
    for extension in ["yaml", "yml", "json"] {
        for stem in [example_id.to_string(), format!("{example_id}.nexusrpc")] {
            let path = input_root.join(format!("{stem}.{extension}"));
            if path.is_file() {
                return path;
            }
        }
    }
    input_root.join(format!("{example_id}.yaml"))
}

fn example_linked_input_paths(repo_root: &Path) -> Vec<PathBuf> {
    let linked_inputs = repo_root.join("advanced/samples/inputs/deps");
    if linked_inputs.is_dir() {
        vec![linked_inputs]
    } else {
        Vec::new()
    }
}

fn example_output_path(repo_root: &Path, language: Language, example_id: &str) -> PathBuf {
    match language {
        Language::Go => advanced_language_root(repo_root, language)
            .join(example_directory_name(language, example_id)),
        _ => advanced_language_root(repo_root, language)
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
    let dir_name = example_directory_name(language, example_id);
    match generation_mode {
        // Definitions-mode outputs are the beginner-facing samples: they live
        // directly under `samples/<lang>/<example>` (Java keeps its Gradle
        // source root so the derived package stays `json_schema.definitions.*`).
        GenerationMode::DefinitionsOnly => {
            if language == Language::Java {
                samples_language_root(repo_root, language)
                    .join("src/main/java/json_schema/definitions")
                    .join(dir_name)
            } else {
                samples_language_root(repo_root, language).join(dir_name)
            }
        }
        // Native-api outputs are snapshot-only and live under the advanced
        // project's `json_schema/api/<example>` (Java: `json_schema.api.*`).
        GenerationMode::NativeApi => {
            if language == Language::Java {
                advanced_language_root(repo_root, language)
                    .join("src/main/java/json_schema/api")
                    .join(dir_name)
            } else {
                advanced_language_root(repo_root, language)
                    .join("json_schema/api")
                    .join(dir_name)
            }
        }
    }
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

fn format_example_output(
    language_root: &Path,
    language: Language,
    output_path: &Path,
) -> Result<()> {
    let (cwd, program, args): (PathBuf, &str, Vec<String>) = match language {
        Language::Dotnet => return Ok(()),
        Language::Java => return Ok(()),
        Language::Go => (
            language_root.to_path_buf(),
            "gofmt",
            vec!["-w".to_string(), output_path.to_string_lossy().into_owned()],
        ),
        Language::Python => (
            language_root.to_path_buf(),
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
            language_root.to_path_buf(),
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

    use super::{
        format_formatter_command, formatter_command, infer_dotnet_namespace, output_dir_name,
    };
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
    fn output_dir_name_resolves_plain_absolute_path() {
        assert_eq!(output_dir_name(Path::new("/tmp/foo/bar")).unwrap(), "bar");
    }

    #[test]
    fn output_dir_name_collapses_trailing_current_dir() {
        // `Path::file_name` alone returns `None` for a path that lexically
        // terminates in `.`; normalization must resolve it to the real
        // directory name instead.
        assert_eq!(output_dir_name(Path::new("/tmp/foo/bar/.")).unwrap(), "bar");
    }

    #[test]
    fn output_dir_name_collapses_trailing_parent_dir() {
        // Same for a path that lexically terminates in `..`: it must resolve
        // to the parent directory's real name, not `None`.
        assert_eq!(
            output_dir_name(Path::new("/tmp/foo/bar/..")).unwrap(),
            "foo"
        );
    }

    #[test]
    fn output_dir_name_collapses_internal_parent_dir() {
        assert_eq!(
            output_dir_name(Path::new("/tmp/foo/../bar")).unwrap(),
            "bar"
        );
    }

    #[test]
    fn output_dir_name_rejects_filesystem_root() {
        // `generate_to_file` also rejects a root output path up front, but
        // this function must be correct standing alone rather than relying
        // on that caller.
        assert!(matches!(
            output_dir_name(Path::new("/")).unwrap_err(),
            crate::error::Error::OutputPathIsRoot { path } if path == Path::new("/")
        ));
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
