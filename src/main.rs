use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use nex_gen::language::Language;
use nex_gen::{
    AddRpcRequest, BuildExamplesRequest, DebugWitDirRequest, GenerateRequest, add_rpc_to_file,
    build_examples, debug_wit_dir_to_file, generate_to_file,
};

#[derive(Parser)]
#[command(name = "nex-gen")]
#[command(about = "Generate language-specific Nexus operation bindings from WIT")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Generate(GenerateArgs),
    #[command(about = "Rebuild the checked-in Python and TypeScript example outputs")]
    BuildExamples(BuildExamplesArgs),
    #[command(
        about = "Add an RPC scaffold to an existing WIT file, or generate standalone WIT for one RPC"
    )]
    AddRpc(AddRpcArgs),
    #[command(about = "Write the prepared WIT workspace used for parsing to a directory")]
    DebugWitDir(DebugWitDirArgs),
}

#[derive(Args)]
struct GenerateArgs {
    #[arg(long, value_enum)]
    lang: CliLanguage,
    #[arg(long = "input", required = true)]
    inputs: Vec<PathBuf>,
    #[arg(long = "support-file")]
    support_paths: Vec<PathBuf>,
    #[arg(long)]
    descriptors: Vec<PathBuf>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    format: bool,
}

#[derive(Args)]
struct BuildExamplesArgs {
    #[arg(long = "lang", value_enum)]
    langs: Vec<ExampleCliLanguage>,
    #[arg(value_name = "EXAMPLE_ID")]
    example_ids: Vec<String>,
}

#[derive(Args)]
struct AddRpcArgs {
    #[arg(long, required = true)]
    descriptors: Vec<PathBuf>,
    #[arg(long)]
    rpc: String,
    #[arg(long = "input")]
    inputs: Vec<PathBuf>,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct DebugWitDirArgs {
    #[arg(long = "input", required = true)]
    inputs: Vec<PathBuf>,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliLanguage {
    Go,
    Python,
    Typescript,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum ExampleCliLanguage {
    Go,
    Python,
    Typescript,
}

impl From<CliLanguage> for Language {
    fn from(value: CliLanguage) -> Self {
        match value {
            CliLanguage::Go => Language::Go,
            CliLanguage::Python => Language::Python,
            CliLanguage::Typescript => Language::TypeScript,
        }
    }
}

impl From<ExampleCliLanguage> for Language {
    fn from(value: ExampleCliLanguage) -> Self {
        match value {
            ExampleCliLanguage::Go => Language::Go,
            ExampleCliLanguage::Python => Language::Python,
            ExampleCliLanguage::Typescript => Language::TypeScript,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Generate(args) => generate_to_file(&GenerateRequest {
            language: args.lang.into(),
            input_paths: args.inputs,
            support_paths: args.support_paths,
            descriptor_paths: args.descriptors,
            output_path: args.output,
            format: args.format,
        }),
        Commands::BuildExamples(args) => build_examples(&BuildExamplesRequest {
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        Commands::AddRpc(args) => add_rpc_to_file(&AddRpcRequest {
            descriptor_paths: args.descriptors,
            rpc_name: args.rpc,
            input_paths: args.inputs,
            output_path: args.output,
        }),
        Commands::DebugWitDir(args) => debug_wit_dir_to_file(&DebugWitDirRequest {
            input_paths: args.inputs,
            output_path: args.output,
        }),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
