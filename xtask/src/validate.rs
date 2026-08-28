use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use nexgen::error::{Error, Result};

use crate::build_examples::{BuildExamplesRequest, build_examples};

#[derive(Clone, Copy)]
pub enum ValidationLanguage {
    Rust,
    Python,
    Typescript,
    Go,
    GoRegular,
    GoAdvanced,
    Java,
    Dotnet,
}

pub struct ValidateRequest {
    pub language: Option<ValidationLanguage>,
}

pub fn validate(request: &ValidateRequest) -> Result<()> {
    let repo_root = repo_root();
    let languages = match request.language {
        Some(language) => vec![language],
        None => vec![
            ValidationLanguage::Rust,
            ValidationLanguage::Python,
            ValidationLanguage::Typescript,
            ValidationLanguage::Go,
            ValidationLanguage::Java,
            ValidationLanguage::Dotnet,
        ],
    };

    for language in languages {
        match language {
            ValidationLanguage::Rust => validate_rust(&repo_root)?,
            ValidationLanguage::Python => validate_python(&repo_root)?,
            ValidationLanguage::Typescript => validate_typescript(&repo_root)?,
            ValidationLanguage::Go => validate_go(&repo_root)?,
            ValidationLanguage::GoRegular => validate_go_root(&repo_root.join("samples/go"))?,
            ValidationLanguage::GoAdvanced => {
                validate_go_root(&repo_root.join("advanced/samples/go"))?
            }
            ValidationLanguage::Java => validate_java(&repo_root)?,
            ValidationLanguage::Dotnet => validate_dotnet(&repo_root)?,
        }
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn sample_roots(repo_root: &Path, language: &str) -> [PathBuf; 2] {
    [
        repo_root.join("samples").join(language),
        repo_root.join("advanced/samples").join(language),
    ]
}

fn validate_rust(repo_root: &Path) -> Result<()> {
    run(repo_root, "cargo", &["fmt", "--check"])?;
    // The `advanced` feature exposes the WIT/proto CLI surface exercised by the
    // integration tests, so validation must include it.
    run(repo_root, "cargo", &["test", "--features", "advanced"])
}

fn validate_python(repo_root: &Path) -> Result<()> {
    validate_generated_examples(repo_root, "python")?;
    for root in sample_roots(repo_root, "python") {
        run(&root, "uv", &["sync", "--locked"])?;
        run(&root, "uv", &["run", "ruff", "check", "."])?;
        run(&root, "uv", &["run", "ruff", "format", "--check", "."])?;
        run(&root, "uv", &["run", "basedpyright", "--warnings"])?;
        run(&root, "uv", &["run", "pytest"])?;
    }
    Ok(())
}

fn validate_typescript(repo_root: &Path) -> Result<()> {
    validate_generated_examples(repo_root, "typescript")?;
    for root in sample_roots(repo_root, "typescript") {
        run(&root, "npm", &["ci"])?;
        run(&root, "npm", &["exec", "--", "prettier", "--check", "."])?;
        run(&root, "npm", &["run", "typecheck"])?;
        run(&root, "npm", &["run", "test"])?;
    }
    Ok(())
}

fn validate_go(repo_root: &Path) -> Result<()> {
    validate_generated_examples(repo_root, "go")?;
    for root in sample_roots(repo_root, "go") {
        validate_go_root(&root)?;
    }
    Ok(())
}

fn validate_go_root(root: &Path) -> Result<()> {
    let output = run_output(root, "gofmt", &["-l", "."])?;
    if !output.is_empty() {
        return Err(Error::RunCommand {
            cwd: root.to_path_buf(),
            command: format!("gofmt required for:\n{output}"),
            source: io::Error::other("gofmt found unformatted files"),
        });
    }
    run(root, "go", &["test", "./..."])
}

fn validate_java(repo_root: &Path) -> Result<()> {
    validate_generated_examples(repo_root, "java")?;
    for root in sample_roots(repo_root, "java") {
        run(&root, "./gradlew", &["build", "--no-daemon"])?;
    }
    Ok(())
}

fn validate_dotnet(repo_root: &Path) -> Result<()> {
    validate_generated_examples(repo_root, "dotnet")?;
    for root in sample_roots(repo_root, "dotnet") {
        run(&root, "dotnet", &["test", "tests/", "--nologo"])?;
    }
    let workflow_service_docs_root = repo_root.join("advanced/samples/dotnet");
    run(
        &workflow_service_docs_root,
        "dotnet",
        &[
            "build",
            "Nexgen.DotNetWorkflowServiceDocs.csproj",
            "--nologo",
        ],
    )?;
    Ok(())
}

fn validate_generated_examples(repo_root: &Path, language: &str) -> Result<()> {
    let language = match language {
        "python" => nexgen::language::Language::Python,
        "typescript" => nexgen::language::Language::TypeScript,
        "go" => nexgen::language::Language::Go,
        "java" => nexgen::language::Language::Java,
        "dotnet" => nexgen::language::Language::Dotnet,
        _ => unreachable!("validation language must support generated examples"),
    };
    build_examples(&BuildExamplesRequest {
        format: None,
        languages: vec![language],
        example_ids: Vec::new(),
    })?;
    run(
        repo_root,
        "git",
        &["diff", "--exit-code", "--", "samples", "advanced/samples"],
    )
}

fn run(cwd: &Path, program: &str, args: &[&str]) -> Result<()> {
    println!(
        "\n==> (cd {} && {})",
        cwd.display(),
        format_command(program, args)
    );
    let status = Command::new(program)
        .current_dir(cwd)
        .args(args)
        .status()
        .map_err(|source| Error::RunCommand {
            cwd: cwd.to_path_buf(),
            command: format_command(program, args),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CommandFailed {
            cwd: cwd.to_path_buf(),
            command: format_command(program, args),
            status,
        })
    }
}

fn run_output(cwd: &Path, program: &str, args: &[&str]) -> Result<String> {
    println!(
        "\n==> (cd {} && {})",
        cwd.display(),
        format_command(program, args)
    );
    let output = Command::new(program)
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|source| Error::RunCommand {
            cwd: cwd.to_path_buf(),
            command: format_command(program, args),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            cwd: cwd.to_path_buf(),
            command: format_command(program, args),
            status: output.status,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn format_command(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}
