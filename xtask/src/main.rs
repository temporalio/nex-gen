use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use nexgen::language::Language;

mod build_examples;
mod validate;
use build_examples::{BuildExamplesRequest, ExampleFormat, build_examples};
use validate::{ValidateRequest, validate};

#[derive(Parser)]
#[command(about = "Repository maintenance tasks for nexgen")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    BuildExamples(ExampleArgs),
    Validate(ValidateArgs),
}

#[derive(clap::Args)]
struct ExampleArgs {
    /// Restrict generation to one authored input format. By default, both formats run.
    #[arg(long, value_enum)]
    format: Option<ExampleFormatArg>,
    #[arg(long = "lang", value_enum)]
    langs: Vec<ExampleLanguage>,
    #[arg(value_name = "EXAMPLE_ID")]
    example_ids: Vec<String>,
}

#[derive(clap::Args)]
struct ValidateArgs {
    #[arg(value_enum, value_name = "LANGUAGE")]
    language: Option<ValidationLanguage>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExampleLanguage {
    Dotnet,
    Go,
    Java,
    Python,
    Typescript,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExampleFormatArg {
    Wit,
    JsonSchema,
}

#[derive(Clone, Copy, ValueEnum)]
enum ValidationLanguage {
    Rust,
    Python,
    Typescript,
    Go,
    GoRegular,
    GoAdvanced,
    Java,
    Dotnet,
}

impl From<ExampleLanguage> for Language {
    fn from(value: ExampleLanguage) -> Self {
        match value {
            ExampleLanguage::Dotnet => Language::Dotnet,
            ExampleLanguage::Go => Language::Go,
            ExampleLanguage::Java => Language::Java,
            ExampleLanguage::Python => Language::Python,
            ExampleLanguage::Typescript => Language::TypeScript,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::BuildExamples(args) => build_examples(&BuildExamplesRequest {
            format: args.format.map(Into::into),
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        Command::Validate(args) => validate(&ValidateRequest {
            language: args.language.map(Into::into),
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

impl From<ExampleFormatArg> for ExampleFormat {
    fn from(value: ExampleFormatArg) -> Self {
        match value {
            ExampleFormatArg::Wit => Self::Wit,
            ExampleFormatArg::JsonSchema => Self::JsonSchema,
        }
    }
}

impl From<ValidationLanguage> for validate::ValidationLanguage {
    fn from(value: ValidationLanguage) -> Self {
        match value {
            ValidationLanguage::Rust => Self::Rust,
            ValidationLanguage::Python => Self::Python,
            ValidationLanguage::Typescript => Self::Typescript,
            ValidationLanguage::Go => Self::Go,
            ValidationLanguage::GoRegular => Self::GoRegular,
            ValidationLanguage::GoAdvanced => Self::GoAdvanced,
            ValidationLanguage::Java => Self::Java,
            ValidationLanguage::Dotnet => Self::Dotnet,
        }
    }
}
