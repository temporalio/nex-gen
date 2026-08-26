use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use nexgen::error::{Error, Result};

#[derive(Clone, Copy)]
pub enum ValidationLanguage {
    Rust,
    Python,
    Typescript,
    Go,
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
    for root in sample_roots(repo_root, "python") {
        run(&root, "uv", &["sync", "--locked"])?;
        run(&root, "uv", &["run", "ruff", "check", "."])?;
        run(&root, "uv", &["run", "ruff", "format", "--check", "."])?;
        run(&root, "uv", &["run", "basedpyright"])?;
        run(&root, "uv", &["run", "pytest"])?;
    }
    Ok(())
}

fn validate_typescript(repo_root: &Path) -> Result<()> {
    for root in sample_roots(repo_root, "typescript") {
        run(&root, "npm", &["ci"])?;
        run(&root, "npm", &["exec", "--", "prettier", "--check", "."])?;
        run(&root, "npm", &["run", "typecheck"])?;
        run(&root, "npm", &["run", "test"])?;
    }
    Ok(())
}

fn validate_go(repo_root: &Path) -> Result<()> {
    for root in sample_roots(repo_root, "go") {
        let output = run_output(&root, "gofmt", &["-l", "."])?;
        if !output.is_empty() {
            return Err(Error::RunCommand {
                cwd: root,
                command: format!("gofmt required for:\n{output}"),
                source: io::Error::other("gofmt found unformatted files"),
            });
        }
        run(&root, "go", &["test", "./..."])?;
    }
    Ok(())
}

fn validate_java(repo_root: &Path) -> Result<()> {
    for root in sample_roots(repo_root, "java") {
        run(&root, "./gradlew", &["build", "--no-daemon"])?;
    }
    Ok(())
}

fn validate_dotnet(repo_root: &Path) -> Result<()> {
    for root in sample_roots(repo_root, "dotnet") {
        run(&root, "dotnet", &["test", "tests/", "--nologo"])?;
    }
    Ok(())
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
