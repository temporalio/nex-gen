use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use nexgen::language::Language;
use nexgen::{BuildExamplesRequest, build_examples, build_json_examples};

#[derive(Parser)]
#[command(about = "Repository maintenance tasks for nexgen")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    BuildExamples(ExampleArgs),
    BuildJsonExamples(ExampleArgs),
}

#[derive(clap::Args)]
struct ExampleArgs {
    #[arg(long = "lang", value_enum)]
    langs: Vec<ExampleLanguage>,
    #[arg(value_name = "EXAMPLE_ID")]
    example_ids: Vec<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExampleLanguage {
    Dotnet,
    Go,
    Java,
    Python,
    Typescript,
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
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        Command::BuildJsonExamples(args) => build_json_examples(&BuildExamplesRequest {
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
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
