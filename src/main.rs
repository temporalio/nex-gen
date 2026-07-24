use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use nex_gen::generator::TsDateTimeTypes;
use nex_gen::language::Language;
use nex_gen::parser::write_prepared_wit_directory;
use nex_gen::{
    AddRpcRequest, BuildExamplesRequest, GenerateRequest, add_rpc_to_file, build_examples,
    build_json_examples, generate_to_file,
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
    Generate {
        #[command(subcommand)]
        language: GenerateLanguage,
    },
    #[command(about = "Rebuild the checked-in WIT example outputs")]
    BuildExamples(BuildExamplesArgs),
    #[command(about = "Rebuild the checked-in JSON schema example outputs")]
    BuildJsonExamples(BuildExamplesArgs),
    #[command(
        about = "Add an RPC scaffold to an existing WIT file, or generate standalone WIT for one RPC"
    )]
    AddRpc(AddRpcArgs),
    #[command(about = "Write the prepared WIT workspace used for parsing to a directory")]
    DebugWitDir(DebugWitDirArgs),
}

#[derive(Args)]
struct GenerateArgs {
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
    #[arg(long = "native-api")]
    generate_native_api: bool,
}

#[derive(Subcommand)]
enum GenerateLanguage {
    Dotnet(GenerateArgs),
    Go(GenerateArgs),
    Java(GenerateArgs),
    Python(GenerateArgs),
    Typescript(TypescriptGenerateArgs),
}

#[derive(Args)]
struct TypescriptGenerateArgs {
    #[command(flatten)]
    common: GenerateArgs,
    /// TypeScript-only: the in-memory representation for materialized temporal
    /// `format` fields (date-time/date/time/duration).
    #[arg(long = "ts-date-time-types", value_enum, default_value_t = CliTsDateTimeTypes::String)]
    ts_date_time_types: CliTsDateTimeTypes,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliTsDateTimeTypes {
    String,
    Date,
    Temporal,
}

impl From<CliTsDateTimeTypes> for TsDateTimeTypes {
    fn from(value: CliTsDateTimeTypes) -> Self {
        match value {
            CliTsDateTimeTypes::String => TsDateTimeTypes::String,
            CliTsDateTimeTypes::Date => TsDateTimeTypes::Date,
            CliTsDateTimeTypes::Temporal => TsDateTimeTypes::Temporal,
        }
    }
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
enum ExampleCliLanguage {
    Dotnet,
    Go,
    Java,
    Python,
    Typescript,
}

impl From<ExampleCliLanguage> for Language {
    fn from(value: ExampleCliLanguage) -> Self {
        match value {
            ExampleCliLanguage::Dotnet => Language::Dotnet,
            ExampleCliLanguage::Go => Language::Go,
            ExampleCliLanguage::Java => Language::Java,
            ExampleCliLanguage::Python => Language::Python,
            ExampleCliLanguage::Typescript => Language::TypeScript,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Generate { language } => generate_to_file(&language.into()),
        Commands::BuildExamples(args) => build_examples(&BuildExamplesRequest {
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        Commands::BuildJsonExamples(args) => build_json_examples(&BuildExamplesRequest {
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        Commands::AddRpc(args) => add_rpc_to_file(&AddRpcRequest {
            descriptor_paths: args.descriptors,
            rpc_name: args.rpc,
            input_paths: args.inputs,
            output_path: args.output,
        }),
        Commands::DebugWitDir(args) => write_prepared_wit_directory(&args.inputs, &args.output),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

impl From<GenerateLanguage> for GenerateRequest {
    fn from(value: GenerateLanguage) -> Self {
        let (language, args, ts_date_time_types) = match value {
            GenerateLanguage::Dotnet(args) => (Language::Dotnet, args, Default::default()),
            GenerateLanguage::Go(args) => (Language::Go, args, Default::default()),
            GenerateLanguage::Java(args) => (Language::Java, args, Default::default()),
            GenerateLanguage::Python(args) => (Language::Python, args, Default::default()),
            GenerateLanguage::Typescript(args) => (
                Language::TypeScript,
                args.common,
                args.ts_date_time_types.into(),
            ),
        };
        Self {
            language,
            input_paths: args.inputs,
            support_paths: args.support_paths,
            descriptor_paths: args.descriptors,
            output_path: args.output,
            format: args.format,
            generate_native_api: args.generate_native_api,
            ts_date_time_types,
        }
    }
}
