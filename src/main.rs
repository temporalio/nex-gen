use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use nex_gen::generator::TsDateTimeTypes;
use nex_gen::language::Language;
#[cfg(feature = "advanced")]
use nex_gen::parser::write_prepared_wit_directory;
#[cfg(feature = "advanced")]
use nex_gen::{
    AddRpcRequest, BuildExamplesRequest, add_rpc_to_file, build_examples, build_json_examples,
};
use nex_gen::{GenerateRequest, generate_to_file};

#[derive(Parser)]
#[command(name = "nexgen")]
#[command(version)]
#[command(about = "Generate code from NexusRPC definition files containing services and types")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[cfg(feature = "advanced")]
    #[command(about = "Generate C# / .NET bindings")]
    Dotnet(GenerateArgs),
    #[command(about = "Generate Go bindings")]
    Go(GenerateArgs),
    #[command(about = "Generate Java bindings")]
    Java(JavaGenerateArgs),
    #[command(about = "Generate Python bindings")]
    Python(GenerateArgs),
    #[command(alias = "ts", about = "Generate TypeScript bindings (alias: ts)")]
    Typescript(TypescriptGenerateArgs),
    #[cfg(feature = "advanced")]
    #[command(about = "Rebuild the checked-in WIT example outputs")]
    BuildExamples(BuildExamplesArgs),
    #[cfg(feature = "advanced")]
    #[command(about = "Rebuild the checked-in JSON schema example outputs")]
    BuildJsonExamples(BuildExamplesArgs),
    #[cfg(feature = "advanced")]
    #[command(
        about = "Add an RPC scaffold to an existing WIT file, or generate standalone WIT for one RPC"
    )]
    AddRpc(AddRpcArgs),
    #[cfg(feature = "advanced")]
    #[command(about = "Write the prepared WIT workspace used for parsing to a directory")]
    DebugWitDir(DebugWitDirArgs),
}

#[derive(Args)]
struct GenerateArgs {
    #[arg(value_name = "INPUT", required = true)]
    inputs: Vec<PathBuf>,
    #[cfg(feature = "advanced")]
    #[arg(long = "support-file")]
    support_paths: Vec<PathBuf>,
    #[cfg(feature = "advanced")]
    #[arg(long)]
    descriptors: Vec<PathBuf>,
    #[arg(long)]
    output: PathBuf,
    #[cfg(feature = "advanced")]
    #[arg(long)]
    format: bool,
    #[cfg(feature = "advanced")]
    #[arg(long = "native-api")]
    generate_native_api: bool,
}

#[derive(Args)]
struct JavaGenerateArgs {
    #[command(flatten)]
    common: GenerateArgs,
    /// The base package for generated Java types. Its last dot-separated
    /// segment must match the `--output` directory's name.
    #[arg(long = "package-name")]
    package_name: String,
}

#[derive(Args)]
struct TypescriptGenerateArgs {
    #[command(flatten)]
    common: GenerateArgs,
    /// TypeScript-only: the in-memory representation for materialized temporal
    /// `format` fields (date-time/date/time/duration).
    #[arg(long = "date-time-types", value_enum, default_value_t = CliTsDateTimeTypes::String)]
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

#[cfg(feature = "advanced")]
#[derive(Args)]
struct BuildExamplesArgs {
    #[arg(long = "lang", value_enum)]
    langs: Vec<ExampleCliLanguage>,
    #[arg(value_name = "EXAMPLE_ID")]
    example_ids: Vec<String>,
}

#[cfg(feature = "advanced")]
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

#[cfg(feature = "advanced")]
#[derive(Args)]
struct DebugWitDirArgs {
    #[arg(long = "input", required = true)]
    inputs: Vec<PathBuf>,
    #[arg(long)]
    output: PathBuf,
}

#[cfg(feature = "advanced")]
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum ExampleCliLanguage {
    Dotnet,
    Go,
    Java,
    Python,
    Typescript,
}

#[cfg(feature = "advanced")]
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
        #[cfg(feature = "advanced")]
        Commands::Dotnet(args) => generate_to_file(&generate_request(
            Language::Dotnet,
            args,
            Default::default(),
            None,
        )),
        Commands::Go(args) => generate_to_file(&generate_request(
            Language::Go,
            args,
            Default::default(),
            None,
        )),
        Commands::Java(args) => generate_to_file(&generate_request(
            Language::Java,
            args.common,
            Default::default(),
            Some(args.package_name),
        )),
        Commands::Python(args) => generate_to_file(&generate_request(
            Language::Python,
            args,
            Default::default(),
            None,
        )),
        Commands::Typescript(args) => generate_to_file(&generate_request(
            Language::TypeScript,
            args.common,
            args.ts_date_time_types.into(),
            None,
        )),
        #[cfg(feature = "advanced")]
        Commands::BuildExamples(args) => build_examples(&BuildExamplesRequest {
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        #[cfg(feature = "advanced")]
        Commands::BuildJsonExamples(args) => build_json_examples(&BuildExamplesRequest {
            languages: args.langs.into_iter().map(Language::from).collect(),
            example_ids: args.example_ids,
        }),
        #[cfg(feature = "advanced")]
        Commands::AddRpc(args) => add_rpc_to_file(&AddRpcRequest {
            descriptor_paths: args.descriptors,
            rpc_name: args.rpc,
            input_paths: args.inputs,
            output_path: args.output,
        }),
        #[cfg(feature = "advanced")]
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

fn generate_request(
    language: Language,
    args: GenerateArgs,
    ts_date_time_types: TsDateTimeTypes,
    java_package_name: Option<String>,
) -> GenerateRequest {
    GenerateRequest {
        language,
        input_paths: args.inputs,
        #[cfg(feature = "advanced")]
        support_paths: args.support_paths,
        #[cfg(not(feature = "advanced"))]
        support_paths: Vec::new(),
        #[cfg(feature = "advanced")]
        descriptor_paths: args.descriptors,
        #[cfg(not(feature = "advanced"))]
        descriptor_paths: Vec::new(),
        output_path: args.output,
        #[cfg(feature = "advanced")]
        format: args.format,
        #[cfg(not(feature = "advanced"))]
        format: false,
        #[cfg(feature = "advanced")]
        generate_native_api: args.generate_native_api,
        #[cfg(not(feature = "advanced"))]
        generate_native_api: false,
        java_package_name,
        ts_date_time_types,
    }
}
