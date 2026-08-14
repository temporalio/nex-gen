// Drives the `nexgen` binary over the WIT/proto CLI surface (`--descriptors`,
// `--native-api`, `--support-file`), all behind the `advanced` feature.
#![cfg(feature = "advanced")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::generate_source;
use nexgen::spec::SupportFragmentSpec;
use nexgen::{GenerateRequest, SupportFiles, generate_to_file};

mod common;
use common::json_input_path;

const PRIMARY_EXAMPLE_ID: &str = "workflow-service";
const START_WORKFLOW_EXAMPLE_ID: &str = "start-workflow";
const TYPE_ROUNDTRIP_EXAMPLE_ID: &str = "type-roundtrip";
static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A property whose union has one inline structured object branch (named
/// `<Union>Object`) and one scalar branch.
/// Unions in positions with no property of their own: an array element (inline
/// and `$ref`), a map member (inline), plus a nullable element for contrast.
const ELEMENT_UNION_SCHEMA: &str = r##"$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  segments:
    type: array
    items:
      oneOf:
        - { type: string }
        - { type: integer }
  choices:
    type: array
    items: { $ref: "#/$defs/Choice" }
  entries: { $ref: "#/$defs/Entries" }
  slots:
    type: array
    items:
      oneOf:
        - { type: string }
        - { type: "null" }
$defs:
  Choice:
    oneOf:
      - { type: string }
      - { type: boolean }
  Entries:
    type: object
    additionalProperties:
      oneOf:
        - { type: string }
        - { type: integer }
"##;

const INLINE_OBJECT_BRANCH_SCHEMA: &str = r#"$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  payload:
    oneOf:
      - type: object
        required: [text]
        properties:
          text: { type: string, minLength: 1 }
      - { type: string }
"#;

/// A union whose **non-object** branches each declare constraints of their own:
/// once the wire token selects a branch, the value is held to everything that
/// branch declares — including a closed value set — in both directions.
const BRANCH_CONSTRAINT_SCHEMA: &str = r#"$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  value:
    oneOf:
      - { type: string, minLength: 3, pattern: "^[a-z]+$" }
      - { type: integer, minimum: 1 }
  listOrName:
    oneOf:
      - { type: array, items: { type: number }, minItems: 1, uniqueItems: true }
      - { type: string, enum: [auto, manual] }
"#;

/// A service whose two operations are each one-sided: one declares only an
/// `input`, the other only an `output`.
const ONE_SIDED_OPERATION_SCHEMA: &str = r##"$schema: https://json-schema.org/draft/2020-12/schema
nexusrpc: "1.0.0"
services:
  Jobs:
    fqn: example.jobs.v1.Jobs
    operations:
      accept:
        input: { $ref: "#/$defs/Job" }
      produce:
        output: { $ref: "#/$defs/Job" }
$defs:
  Job:
    type: object
    properties:
      id: { type: string }
"##;

/// An operation whose output type carries an `x-ts-name` override.
const OPERATION_IO_TS_NAME_SCHEMA: &str = r##"$schema: https://json-schema.org/draft/2020-12/schema
nexusrpc: "1.0.0"
services:
  Pages:
    fqn: example.pages.v1.Pages
    operations:
      get:
        input: { $ref: "#/$defs/GetInput" }
        output: { $ref: "#/$defs/Page" }
$defs:
  GetInput:
    type: object
    properties:
      id: { type: string }
  Page:
    type: object
    x-ts-name: RenamedPage
    properties:
      title: { type: string }
"##;

/// The entry file of a two-file closure. `get`'s output is the model the *other*
/// file declares, and `FindOutput.page` `$ref`s it from a property, so both
/// cross-module reference shapes are covered.
const CROSS_MODULE_ENTRY_SCHEMA: &str = r##"$schema: https://json-schema.org/draft/2020-12/schema
nexusrpc: "1.0.0"
services:
  Pages:
    fqn: example.pages.v1.Pages
    operations:
      get:
        input: { $ref: "#/$defs/GetInput" }
        output: { $ref: "content/page.json" }
      find:
        input: { $ref: "#/$defs/GetInput" }
        output: { $ref: "#/$defs/FindOutput" }
$defs:
  GetInput:
    type: object
    additionalProperties: false
    properties:
      id: { type: string }
  FindOutput:
    type: object
    additionalProperties: false
    properties:
      page: { $ref: "content/page.json" }
"##;

/// The referenced file. Its model carries the name override the *consuming*
/// module has to resolve through.
const CROSS_MODULE_PAGE_SCHEMA: &str = r##"$schema: https://json-schema.org/draft/2020-12/schema
type: object
additionalProperties: false
x-ts-name: RenamedPage
properties:
  title: { type: string }
"##;

/// Writes the two-file cross-module closure into `dir` and returns the input
/// directory to generate from.
fn write_cross_module_closure(dir: &Path) -> PathBuf {
    let input_dir = dir.join("input");
    fs::create_dir_all(input_dir.join("content")).unwrap();
    fs::write(
        input_dir.join("kb.nexusrpc.yaml"),
        CROSS_MODULE_ENTRY_SCHEMA,
    )
    .unwrap();
    fs::write(
        input_dir.join("content/page.json"),
        CROSS_MODULE_PAGE_SCHEMA,
    )
    .unwrap();
    input_dir
}

/// A property carrying a per-language name override alongside a `default`, a
/// `const`, and an inline object — one schema covering all three synthesized-name
/// families at once.
const MEMBER_DERIVED_NAME_SCHEMA: &str = r#"$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  retryCount:
    type: integer
    default: 3
    x-ts-name: attempts
    x-go-name: Attempts
  kind:
    type: string
    const: widget
    x-ts-name: category
    x-go-name: Category
  address:
    type: object
    x-ts-name: location
    x-go-name: Location
    properties:
      street: { type: string }
"#;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn descriptor_path(root: &Path) -> PathBuf {
    root.join("advanced/samples/descriptors/temporal_api.bin")
}

fn linked_inputs_path(root: &Path) -> PathBuf {
    root.join("advanced/samples/inputs/deps")
}

fn example_input_paths(root: &Path, example_id: &str) -> Vec<PathBuf> {
    vec![input_path(root, example_id), linked_inputs_path(root)]
}

fn typescript_root(root: &Path) -> PathBuf {
    root.join("advanced/samples/typescript")
}

fn input_path(root: &Path, example_id: &str) -> PathBuf {
    let flat_path = root
        .join("advanced/samples/inputs")
        .join(format!("{example_id}.wit"));
    if flat_path.is_file() {
        flat_path
    } else {
        root.join("advanced/samples/inputs")
            .join(example_id)
            .join("main.wit")
    }
}

fn typescript_output_path(root: &Path, example_id: &str) -> PathBuf {
    typescript_root(root).join("wit").join(example_id)
}

fn samples_typescript_root(root: &Path) -> PathBuf {
    root.join("samples/typescript")
}

fn typescript_json_definitions_output_path(root: &Path, example_id: &str) -> PathBuf {
    // Definitions are the beginner-facing samples, flattened directly under
    // `samples/typescript/<example>`.
    samples_typescript_root(root).join(example_id)
}

fn typescript_json_api_output_path(root: &Path, example_id: &str) -> PathBuf {
    typescript_root(root)
        .join("json_schema")
        .join("api")
        .join(example_id)
}

fn typescript_example_ids(root: &Path) -> Vec<String> {
    let mut ids = fs::read_dir(root.join("advanced/samples/inputs"))
        .unwrap()
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
            if typescript_output_path(root, &example_id).is_dir() {
                Some(example_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn ensure_typescript_dependencies(example_dir: &Path) {
    if example_dir.join("node_modules").exists() {
        return;
    }

    let install_status = Command::new("npm")
        .current_dir(example_dir)
        .args(["install", "--no-fund", "--no-audit"])
        .status()
        .unwrap();
    assert!(install_status.success());
}

fn read_typescript_output_files(dir: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("ts") {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read_to_string(&path).unwrap(),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(dir, dir, &mut files);
    files
}

fn render_output_files(files: BTreeMap<PathBuf, String>) -> String {
    files
        .into_iter()
        .map(|(path, contents)| format!("### {}\n{contents}", path.display()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn generate_typescript_to_string(input_paths: &[PathBuf], descriptor_paths: &[PathBuf]) -> String {
    let temp_dir = unique_output_path("typescript-rendered");
    let output_path = temp_dir.join("output");
    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: input_paths.to_vec(),
        support_paths: Vec::new(),
        descriptor_paths: descriptor_paths.to_vec(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: true,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let rendered = if output_path.is_file() {
        fs::read_to_string(&output_path).unwrap()
    } else {
        render_output_files(read_typescript_output_files(&output_path))
    };
    fs::remove_dir_all(temp_dir).unwrap();
    rendered
}

fn generate_formatted_typescript_output(root: &Path, example_id: &str, output_path: &Path) {
    ensure_typescript_dependencies(&typescript_root(root));

    let status = Command::new(env!("CARGO_BIN_EXE_nexgen"))
        .args([
            "typescript",
            input_path(root, example_id).to_str().unwrap(),
            linked_inputs_path(root).to_str().unwrap(),
            "--descriptors",
            descriptor_path(root).to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
            "--native-api",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let format_status = Command::new("npm")
        .current_dir(typescript_root(root))
        .args([
            "exec",
            "--",
            "prettier",
            "--write",
            "--print-width",
            "88",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(format_status.success());
}

fn generate_formatted_json_typescript_output(
    root: &Path,
    example_id: &str,
    output_path: &Path,
    generate_native_api: bool,
) {
    generate_formatted_json_typescript_output_repr(
        root,
        example_id,
        output_path,
        generate_native_api,
        None,
    );
}

/// Like `generate_formatted_json_typescript_output`, but reads the input from
/// `input_id` (so the repr variants reuse `temporal.yaml`) and threads an
/// optional `--date-time-types`.
fn generate_formatted_json_typescript_output_repr(
    root: &Path,
    input_id: &str,
    output_path: &Path,
    generate_native_api: bool,
    repr: Option<&str>,
) {
    ensure_typescript_dependencies(&typescript_root(root));

    let input_path = json_input_path(root, input_id);
    let mut args = vec![
        "typescript",
        input_path.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ];
    if let Some(repr) = repr {
        args.push("--date-time-types");
        args.push(repr);
    }
    if generate_native_api {
        args.push("--native-api");
    }

    let status = Command::new(env!("CARGO_BIN_EXE_nexgen"))
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());

    let format_status = Command::new("npm")
        .current_dir(typescript_root(root))
        .args([
            "exec",
            "--",
            "prettier",
            "--write",
            "--print-width",
            "88",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(format_status.success());
}

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-{label}-{unique}-{counter}"))
}

#[test]
fn typescript_examples_generation_matches_checked_in_output() {
    let root = project_root();
    for example_id in typescript_example_ids(&root) {
        let output_path = unique_output_path(&format!("typescript-{example_id}"));
        generate_formatted_typescript_output(&root, &example_id, &output_path);
        let rendered = read_typescript_output_files(&output_path);
        let expected = read_typescript_output_files(&typescript_output_path(&root, &example_id));
        assert_eq!(rendered, expected, "snapshot mismatch for {example_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
}

#[test]
fn typescript_json_example_generation_matches_checked_in_output() {
    let root = project_root();
    for example_id in ["chat", "kb", "showcase", "temporal"] {
        let output_path = unique_output_path(&format!("typescript-json-{example_id}"));
        generate_formatted_json_typescript_output(&root, example_id, &output_path, false);
        let rendered = read_typescript_output_files(&output_path);
        let expected = read_typescript_output_files(&typescript_json_definitions_output_path(
            &root, example_id,
        ));
        assert_eq!(rendered, expected, "snapshot mismatch for {example_id}");
        if example_id == "showcase" {
            let all = rendered.values().cloned().collect::<Vec<_>>().join("\n");
            // Scalar defaults → emitted DEFAULT_<FIELD> module constants.
            assert!(all.contains("export const DEFAULT_GREETING = \"hello\";"));
            assert!(all.contains("export const DEFAULT_DEBUG = false;"));
            // `deprecated` → JSDoc @deprecated tag; `title` → JSDoc summary line.
            assert!(all.contains("@deprecated"));
            assert!(all.contains("Retry budget"));
            // `x-ts-name` override (Stage 4): the interface member + (de)serialize
            // use the override while the wire key stays `legacyId`.
            assert!(all.contains("legacyIdTs?: string;"));
            assert!(all.contains("legacyIdTs = raw.legacyId;"));
            assert!(all.contains("out.legacyId = value.legacyIdTs;"));
            // A free-form object stays an anonymous `Record` — narrowed on the
            // object token as a union branch, and the sole member of the named
            // `Extras` interface.
            assert!(all.contains("payload?: Record<string, unknown> | string;"));
            assert!(all.contains("export interface Extras {"));
            assert!(all.contains("additionalProperties: Record<string, unknown>;"));
            // A tagged union whose branches are written inline: each branch names
            // itself with `x-ts-name` and is emitted as an interface + converter.
            assert!(all.contains("export type Note = TextNote | LinkNote;"));
            assert!(all.contains("export interface TextNote {"));
            assert!(all.contains(
                "export const linkNoteTransferTypeConverter =\n  new (class implements TransferTypeConverter<LinkNote> {"
            ));
            // The lone inline object branch of a property union derives its name
            // from the union it belongs to.
            assert!(all.contains("detail?: ShowcaseDetailObject | string;"));
            assert!(all.contains("export interface ShowcaseDetailObject {"));
            assert!(all.contains("out.detail = serializeShowcaseDetail(value.detail);"));
            // Each operation carries its models' converters as operation type
            // info; the `x-ts-name` override flows into the converter identifier.
            assert!(all.contains(
                "inputType: { transferTypeConverter: getShowcaseInputTransferTypeConverter },"
            ));
            assert!(
                all.contains(
                    "outputType: { transferTypeConverter: showcaseTransferTypeConverter },"
                )
            );
            assert!(all.contains("export const contactTsTransferTypeConverter ="));
        }
        if example_id == "chat" {
            let services = rendered
                .get(std::path::Path::new("services.ts"))
                .expect("chat services module");
            // A void side has no value to convert, so it carries no type info.
            assert!(services.contains("ping: nexus.operation<void, void>({ name: \"Ping\" }),"));
            assert!(services.contains(
                "inputType: { transferTypeConverter: sendMessageInputTransferTypeConverter },"
            ));
        }
        if example_id == "kb" {
            // A cross-module I/O model's converter imports as a value from the
            // module that declares it, alongside the type-only model import.
            let services = rendered
                .get(std::path::Path::new("kb/services.ts"))
                .expect("kb services module");
            assert!(services.contains(
                "import { blockTransferTypeConverter } from \"../content/block/models\";"
            ));
            assert!(
                services
                    .contains("outputType: { transferTypeConverter: pageTransferTypeConverter },")
            );
        }
        fs::remove_dir_all(output_path).unwrap();
    }
    // The `--date-time-types` date/temporal variants of the temporal example.
    for (output_id, repr) in [("temporal-date", "date"), ("temporal-temporal", "temporal")] {
        let output_path = unique_output_path(&format!("typescript-json-{output_id}"));
        generate_formatted_json_typescript_output_repr(
            &root,
            "temporal",
            &output_path,
            false,
            Some(repr),
        );
        let rendered = read_typescript_output_files(&output_path);
        let expected = read_typescript_output_files(&typescript_json_definitions_output_path(
            &root, output_id,
        ));
        assert_eq!(rendered, expected, "snapshot mismatch for {output_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
}

#[test]
fn typescript_json_api_example_generation_matches_checked_in_output() {
    let root = project_root();
    for example_id in ["chat", "kb", "showcase", "temporal"] {
        let output_path = unique_output_path(&format!("typescript-json-api-{example_id}"));
        generate_formatted_json_typescript_output(&root, example_id, &output_path, true);
        let rendered = read_typescript_output_files(&output_path);
        let expected =
            read_typescript_output_files(&typescript_json_api_output_path(&root, example_id));
        assert_eq!(rendered, expected, "snapshot mismatch for {example_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
    for (output_id, repr) in [("temporal-date", "date"), ("temporal-temporal", "temporal")] {
        let output_path = unique_output_path(&format!("typescript-json-api-{output_id}"));
        generate_formatted_json_typescript_output_repr(
            &root,
            "temporal",
            &output_path,
            true,
            Some(repr),
        );
        let rendered = read_typescript_output_files(&output_path);
        let expected =
            read_typescript_output_files(&typescript_json_api_output_path(&root, output_id));
        assert_eq!(rendered, expected, "snapshot mismatch for {output_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
}

#[test]
fn cli_generates_typescript_support_file_from_parameter() {
    let root = project_root();
    let temp_dir = unique_output_path("typescript-support-file-input");
    fs::create_dir_all(&temp_dir).unwrap();
    let support_path = temp_dir.join("custom-support.ts");
    let output_path = temp_dir.join("output");
    fs::write(
        &support_path,
        "export function customSupportHook(): string {\n  return \"custom\";\n}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nexgen"))
        .args([
            "typescript",
            input_path(&root, "user-service").to_str().unwrap(),
            "--support-file",
            support_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(output_path.join("support.ts"))
            .unwrap()
            .contains("export function customSupportHook()")
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn typescript_rejects_support_namespace() {
    let root = project_root();
    let spec = nexgen::parser::load_api_spec_from_wit_for_language_with_inputs(
        nexgen::language::Language::TypeScript,
        &example_input_paths(&root, PRIMARY_EXAMPLE_ID),
    )
    .unwrap();
    let descriptors = nexgen::descriptors::DescriptorIndex::load(&descriptor_path(&root)).unwrap();
    let err = generate_source(
        nexgen::language::Language::TypeScript,
        spec.clone(),
        &descriptors,
        &SupportFiles {
            fragments: vec![SupportFragmentSpec {
                path: "support.ts".to_string(),
                contents: String::new(),
                namespace: Some("example.support".to_string()),
            }],
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("support namespace"));
}

#[test]
fn typescript_example_suite_typechecks_and_tests() {
    let root = project_root();
    // The advanced project holds the WIT examples + snapshot-only native-api
    // output; the samples project holds the JSON-Schema definitions + their
    // runtime tests. Typecheck and test both so neither tier loses coverage.
    for example_dir in [samples_typescript_root(&root), typescript_root(&root)] {
        ensure_typescript_dependencies(&example_dir);

        let typecheck_status = Command::new("npm")
            .current_dir(&example_dir)
            .args(["run", "typecheck"])
            .status()
            .unwrap();
        assert!(
            typecheck_status.success(),
            "typecheck failed in {example_dir:?}"
        );

        let test_status = Command::new("npm")
            .current_dir(&example_dir)
            .args(["run", "test"])
            .status()
            .unwrap();
        assert!(test_status.success(), "tests failed in {example_dir:?}");
    }
}

#[test]
fn typescript_renders_required_fields_and_custom_message_types() {
    let root = project_root();
    let rendered = generate_typescript_to_string(
        &example_input_paths(&root, PRIMARY_EXAMPLE_ID),
        &[descriptor_path(&root)],
    );

    assert!(!rendered.contains("type _RequestWithFunctionField<"));
    assert!(!rendered.contains("type _RequestWithArgumentsField<"));
    assert!(!rendered.contains("type SignalWithStartWorkflowRequestBase = {"));
    assert!(rendered.contains("export type SignalWithStartWorkflowRequest<"));
    assert!(rendered.contains("export type ReplaceSignalWithStartWorkflowRequest<Base, New>"));
    assert!(rendered.contains(
        "WorkflowFn extends (...args: any[]) => Promise<any> = (...args: any[]) => Promise<any>,"
    ));
    assert!(rendered.contains(
        "SignalValue extends workflow.SignalDefinition<any[]> = workflow.SignalDefinition<any[]>"
    ));
    assert!(rendered.contains("> = ReplaceSignalWithStartWorkflowRequest<"));
    assert!(
        rendered.contains(
            "SignalValue extends workflow.SignalDefinition<infer Args, any> ? Args : never"
        )
    );
    assert!(rendered.contains("SignalArgs extends any[] = SignalValue extends"));
    assert!(rendered.contains("signalArgs: SignalArgs | Readonly<SignalArgs>;"));
    assert!(rendered.contains("signalArgs?: SignalArgs | Readonly<SignalArgs>;"));
    assert!(
        rendered
            .contains("Workflow type name or workflow function identifying the workflow to start.")
    );
    assert!(rendered.contains("workflow: string;"));
    assert!(rendered.contains("Arguments for workflow."));
    assert!(rendered.contains("args?: ReadonlyArray<unknown>;"));
    assert!(rendered.contains("Arguments for signal."));
    assert!(rendered.contains("signalArgs?: ReadonlyArray<unknown>;"));
    assert!(rendered.contains("* Unique identifier for the workflow execution."));
    assert!(!rendered.contains("@property workflow"));
    assert!(rendered.contains("* @returns A workflow handle to the started workflow."));
    assert!(rendered.contains("id: string;"));
    assert!(rendered.contains("taskQueue: string;"));
    assert!(rendered.contains("runTimeout?: common.Duration;"));
    assert!(rendered.contains("idReusePolicy?: common.WorkflowIdReusePolicy;"));
    assert!(rendered.contains("idConflictPolicy?: common.WorkflowIdConflictPolicy;"));
    assert!(!rendered.contains("identity?: string;"));
    assert!(rendered.contains("memo?: Record<string, unknown>;"));
    assert!(rendered.contains("searchAttributes?: common.TypedSearchAttributes;"));
    assert!(!rendered.contains("common.TypedSearchAttributes | common.SearchAttributes"));
    assert!(rendered.contains("versioningOverride?: common.VersioningOverride;"));
    assert!(rendered.contains("priority?: common.Priority;"));
    assert!(rendered.contains("signal: string;"));
    assert!(rendered.contains("staticSummary?: string;"));
    assert!(rendered.contains("staticDetails?: string;"));
    assert!(!rendered.contains("userMetadata?: UserMetadata;"));
    assert!(rendered.contains("### support.ts"));
    assert!(rendered.contains("### index.ts"));
    let index_rendered = rendered
        .split("### index.ts")
        .nth(1)
        .expect("rendered output should include index.ts");
    assert!(!index_rendered.contains("export * from './support.ts';"));
    assert!(rendered.contains("export function retryPolicyFromProto("));
    assert!(!index_rendered.contains("export const SignalWithStartWorkflowRequest = {"));
    assert!(!index_rendered.contains("const UserMetadata = {"));
    assert!(index_rendered.contains("function userMetadataFromProto("));
    assert!(index_rendered.contains("function signalWithStartWorkflowRequestToProto<"));
    assert!(rendered.contains("workflowType: workflowTypeToProto("));
    assert!(rendered.contains("workflowFunctionName("));
    assert!(rendered.contains("input: requestArgsToPayloads(model.args),"));
    assert!(rendered.contains("signalInput: requestArgsToPayloads(model.signalArgs),"));
    assert!(!rendered.contains("_RequestArgsToPayloads"));
    let signal_request_to_proto = rendered
        .split("function signalWithStartWorkflowRequestToProto<")
        .nth(1)
        .and_then(|body| {
            body.split("export async function signalWithStartWorkflow")
                .next()
        })
        .expect("signal-with-start request renderer should be present");
    let workflow_type_index = signal_request_to_proto
        .find("workflowType: workflowTypeToProto(")
        .expect("workflow type should be serialized");
    let input_index = signal_request_to_proto
        .find("input: requestArgsToPayloads(model.args),")
        .expect("workflow args should be serialized");
    let workflow_id_index = signal_request_to_proto
        .find("workflowId: requiredField(")
        .expect("workflow id should be serialized");
    let task_queue_index = signal_request_to_proto
        .find("taskQueue: taskQueueToProto(")
        .expect("task queue should be serialized");
    let signal_name_index = signal_request_to_proto
        .find("signalName:")
        .expect("signal name should be serialized");
    assert!(workflow_type_index < input_index);
    assert!(input_index < workflow_id_index);
    assert!(workflow_id_index < task_queue_index);
    assert!(task_queue_index < signal_name_index);
    assert!(rendered.contains("signalName: signalFunctionName("));
    assert!(!rendered.contains("signalName: ((value) =>"));
    assert!(rendered.contains("workflowType: workflowTypeToProto("));
    assert!(rendered.contains("taskQueue: taskQueueToProto("));
    assert!(rendered.contains(
        "workflowRunTimeout: model.runTimeout == null ? undefined : durationToProto(model.runTimeout),"
    ));
    assert!(rendered.contains("common.WorkflowIdReusePolicy.ALLOW_DUPLICATE"));
    assert!(rendered.contains("workflowIdReusePolicy: workflowIdReusePolicyToProto("));
    assert!(!rendered.contains("workflowIdReusePolicyToProto(0"));
    assert!(rendered.contains("workflowIdConflictPolicy:"));
    assert!(!rendered.contains("workflowIdConflictPolicyToProto(0"));
    assert!(rendered.contains("model.idConflictPolicy == null"));
    assert!(rendered.contains("workflowIdConflictPolicyToProto(model.idConflictPolicy)"));
    assert!(rendered.contains("memo: model.memo == null ? undefined : memoToProto(model.memo),"));
    assert!(rendered.contains(
        "searchAttributes: model.searchAttributes == null ? undefined : searchAttributesToProto(model.searchAttributes),"
    ));
    assert!(rendered.contains(
        "priority: model.priority == null ? undefined : priorityToProto(model.priority),"
    ));
    assert!(rendered.contains("model.staticSummary == null && model.staticDetails == null"));
    assert!(rendered.contains("summary: model.staticSummary == null"));
    assert!(rendered.contains("configuredPayloadConverter().toPayload(model.staticSummary)"));
    assert!(rendered.contains("common.toPayloads(configuredPayloadConverter(), ...args)"));
    assert!(!rendered.contains("common.defaultPayloadConverter"));
    assert!(!rendered.contains("payloadToProto(payload: unknown"));
    assert!(!rendered.contains("function isPayload("));
    assert!(rendered.contains(
        "versioningOverride: model.versioningOverride == null ? undefined : versioningOverrideToProto(model.versioningOverride),"
    ));
    assert!(rendered.contains("export function taskQueueFromProto("));
    assert!(rendered.contains("export function taskQueueToProto("));
    assert!(rendered.contains(
        "): temporal.api.workflowservice.v1.ISignalWithStartWorkflowExecutionRequest | undefined {"
    ));
    assert!(rendered.contains("const result = await handle.result();"));
    assert!(rendered.contains("return workflow.getExternalWorkflowHandle("));
    assert!(rendered.contains("request.id"));
    assert!(rendered.contains("result.runId ?? undefined"));
    assert!(rendered.contains("export async function signalWithStartWorkflow<"));
    assert!(rendered.contains(
        "export { signalWithStartWorkflow } from './operations/signal-with-start-workflow';"
    ));
    assert!(rendered.contains("export const operationRegistry = ["));
    assert!(rendered.contains("service: \"temporal.api.workflowservice.v1.WorkflowService\""));
    assert!(rendered.contains("operation: \"SignalWithStartWorkflowExecution\""));
    assert!(rendered.contains(
        "inputType: \"temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest\""
    ));
    assert!(rendered.contains(
        "outputType: \"temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse\""
    ));
    assert!(rendered.contains("export type { SignalWithStartWorkflowRequest } from './models';"));
    assert!(!rendered.contains("export { WorkflowService } from './services';"));
    assert!(!rendered.contains("export type { SignalWithStartWorkflowResponse"));
    assert!(rendered.contains("headers?: never"));
    assert!(rendered.contains("request: SignalWithStartWorkflowInput<WorkflowFn, SignalValue>,"));
    assert!(rendered.contains("const client = workflow.createNexusServiceClient({"));
    assert!(!rendered.contains("export class WorkflowServiceClient"));
    assert!(rendered.contains("): Promise<workflow.ExternalWorkflowHandle> {"));
    assert!(!rendered.contains("SignalWithStartWorkflowRequest = {\n  fromProto("));
    assert!(!rendered.contains("export interface SignalWithStartWorkflowRequest {"));
    assert!(!rendered.contains("export interface RetryPolicy"));
    assert!(!rendered.contains("export interface WorkflowType"));
    assert!(!rendered.contains("export interface TaskQueue"));
    assert!(!rendered.contains("export interface Duration"));
    assert!(!rendered.contains("export interface Memo"));
    assert!(!rendered.contains("export interface SearchAttributes"));
    assert!(!rendered.contains("export interface Priority"));
    assert!(!rendered.contains("export interface VersioningOverride"));
    assert!(!rendered.contains("export enum WorkflowIdReusePolicy"));
    assert!(!rendered.contains("export enum WorkflowIdConflictPolicy"));
    assert!(!rendered.contains("signalWithStartWorkflowExecution("));
    assert!(!rendered.contains("from './temporal_model_converters.ts'"));

    let start_workflow_rendered = generate_typescript_to_string(
        &example_input_paths(&root, START_WORKFLOW_EXAMPLE_ID),
        &[descriptor_path(&root)],
    );
    assert!(
        start_workflow_rendered
            .contains("export type CancelWorkflowResponse = Record<string, never>;")
    );
    assert!(!start_workflow_rendered.contains("export interface CancelWorkflowResponse {}"));

    let type_roundtrip_rendered = generate_typescript_to_string(
        &example_input_paths(&root, TYPE_ROUNDTRIP_EXAMPLE_ID),
        &[descriptor_path(&root)],
    );
    assert!(type_roundtrip_rendered.contains("retryPolicy: common.RetryPolicy;"));
    assert!(!type_roundtrip_rendered.contains("retryPolicyOperation"));
}

/// An inline **structured** object `oneOf` branch on a property: the branch is
/// named `<Union>Object` and emitted as an interface with its own converter, and
/// the union's serialize side routes through it (the in-memory
/// `additionalProperties` member must not reach the wire).
/// See `specs/json-schema/features/oneOf.md` ("Object branches").
#[test]
fn typescript_json_names_inline_object_union_branch() {
    let temp_dir = unique_output_path("ts-json-inline-branch");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("detail.yaml");
    fs::write(&input_path, INLINE_OBJECT_BRANCH_SCHEMA).unwrap();
    let output_path = temp_dir.join("detail");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let rendered = fs::read_to_string(output_path.join("models.ts")).unwrap();

    assert!(rendered.contains("payload?: DetailPayloadObject | string;"));
    assert!(rendered.contains("export interface DetailPayloadObject {"));
    assert!(rendered.contains(
        "export const detailPayloadObjectTransferTypeConverter = new class implements TransferTypeConverter<DetailPayloadObject> {"
    ));
    // Parse and serialize both route the object token through the branch converter.
    assert!(
        rendered.contains("detailPayloadObjectTransferTypeConverter.fromTransferType(raw.payload)")
    );
    assert!(rendered.contains(
        "function serializeDetailPayload(value: DetailPayloadObject | string): unknown {"
    ));
    assert!(rendered.contains("out.payload = serializeDetailPayload(value.payload);"));
    fs::remove_dir_all(temp_dir).unwrap();
}

/// Every constraint a **non-object** branch declares is checked once the token
/// narrows to it, on both sides of the converter (P12). A `const`/`enum` branch
/// also narrows the member *type* to its literal set, without which the narrowed
/// assignment would not even typecheck.
/// See `specs/json-schema/features/oneOf.md` ("Validator mapping").
#[test]
fn typescript_json_validates_non_object_union_branch_constraints() {
    let temp_dir = unique_output_path("ts-json-branch-constraints");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("bc.yaml");
    fs::write(&input_path, BRANCH_CONSTRAINT_SCHEMA).unwrap();
    let output_path = temp_dir.join("bc");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let rendered = fs::read_to_string(output_path.join("models.ts")).unwrap();

    // A closed value set narrows the branch type itself.
    assert!(rendered.contains("listOrName?: number[] | \"auto\" | \"manual\";"));
    // Parse: each branch's own predicates run under the union's path.
    assert!(rendered.contains("if ([...(value as string)].length < 3) {"));
    assert!(rendered.contains("if (!PATTERN_C182F89FDB221836.test((value as string))) {"));
    assert!(rendered.contains("if ((value as number) < 1) {"));
    assert!(rendered.contains("if ((listOrName as number[]).length < 1) {"));
    assert!(rendered.contains("duplicate items: element at index ${index}"));
    assert!(rendered.contains(
        "if ((listOrName as \"auto\" | \"manual\") !== \"auto\" && (listOrName as \"auto\" | \"manual\") !== \"manual\") {"
    ));
    // Serialize: the same predicates over the in-memory member, aggregated into
    // the model's own violations before the wire object is built.
    assert!(rendered.contains("if ([...(value.value as string)].length < 3) {"));
    assert!(rendered.contains("if ((value.value as number) < 1) {"));
    fs::remove_dir_all(temp_dir).unwrap();
}

/// A union in an element position: the loader names it, so TypeScript emits an
/// ordinary union alias plus converter and runs it per element/member —
/// including on the serialize side, where an element model's catch-all bag would
/// otherwise reach the wire. A nullable element parenthesizes (`(T | null)[]`),
/// which `T | null[]` would silently misread.
/// See `specs/json-schema/features/oneOf.md` ("Unions in element positions").
#[test]
fn typescript_json_maps_element_position_unions() {
    let temp_dir = unique_output_path("ts-json-element-union");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("bag.yaml");
    fs::write(&input_path, ELEMENT_UNION_SCHEMA).unwrap();
    let output_path = temp_dir.join("bag");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let rendered = fs::read_to_string(output_path.join("models.ts")).unwrap();

    assert!(rendered.contains("export type BagSegmentsItem = string | number;"));
    assert!(rendered.contains("segments?: BagSegmentsItem[];"));
    assert!(rendered.contains("bagSegmentsItemTransferTypeConverter.fromTransferType(element)"));
    assert!(rendered.contains("choiceTransferTypeConverter.fromTransferType(element)"));
    // A map member runs the member converter in both directions.
    assert!(rendered.contains("entriesValueTransferTypeConverter.fromTransferType(raw[key])"));
    assert!(
        rendered.contains("out[key] = entriesValueTransferTypeConverter.toTransferType(entry);")
    );
    // Element nullability is the element's own concern, and parenthesized.
    assert!(rendered.contains("slots?: (string | null)[];"));
    fs::remove_dir_all(temp_dir).unwrap();
}

/// A one-sided operation: the non-void side carries its converter as operation
/// type info, the void side carries no field at all (there is no value to
/// convert, so an empty `TypeInfo` would assert a conversion that does not
/// exist). See `specs/json-schema/services.md` ("TypeScript operation type
/// info"); the checked-in samples only cover void-on-both-sides.
#[test]
fn typescript_json_one_sided_operation_type_info() {
    let temp_dir = unique_output_path("ts-json-one-sided-type-info");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("jobs.nexusrpc.yaml");
    fs::write(&input_path, ONE_SIDED_OPERATION_SCHEMA).unwrap();
    let output_path = temp_dir.join("jobs");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let rendered = fs::read_to_string(output_path.join("services.ts")).unwrap();

    // Input present, output omitted: `inputType` only.
    assert!(rendered.contains(
        "  >({ name: \"Accept\", inputType: { transferTypeConverter: jobTransferTypeConverter } }),"
    ));
    // The mirror: output present, input omitted.
    assert!(rendered.contains(
        "  >({ name: \"Produce\", outputType: { transferTypeConverter: jobTransferTypeConverter } }),"
    ));
    fs::remove_dir_all(temp_dir).unwrap();
}

/// An `x-ts-name` override on an operation's I/O type moves every emitted
/// reference with the type: the operation generic, the model/converter imports,
/// and the converter named in the operation type info (the identifier is derived
/// from the *resolved* type name).
#[test]
fn typescript_json_operation_type_info_follows_ts_name_override() {
    let temp_dir = unique_output_path("ts-json-type-info-override");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("pages.nexusrpc.yaml");
    fs::write(&input_path, OPERATION_IO_TS_NAME_SCHEMA).unwrap();
    let output_path = temp_dir.join("pages");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let models = fs::read_to_string(output_path.join("models.ts")).unwrap();
    let services = fs::read_to_string(output_path.join("services.ts")).unwrap();

    assert!(models.contains("export interface RenamedPage {"));
    assert!(models.contains("export const renamedPageTransferTypeConverter = new class"));
    assert!(services.contains("import { getInputTransferTypeConverter, renamedPageTransferTypeConverter } from './models';"));
    assert!(services.contains("import type { GetInput, RenamedPage } from './models';"));
    assert!(services.contains("    RenamedPage\n"));
    assert!(
        services.contains(
            "outputType: { transferTypeConverter: renamedPageTransferTypeConverter } }),"
        )
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

/// An `x-ts-name` override on a model in *another* input file moves every
/// reference the consuming module emits: the operation generic, the type-only
/// model import, the converter value import, and the property annotation of a
/// cross-module `$ref`. The override is declared in the referenced file, so only
/// the tree-wide name manifest can resolve it (P14/P15).
#[test]
fn typescript_json_cross_module_ts_name_override_moves_every_reference() {
    let temp_dir = unique_output_path("ts-json-cross-module-override");
    let input_dir = write_cross_module_closure(&temp_dir);
    let output_path = temp_dir.join("output");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_dir],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();

    let declaring = fs::read_to_string(output_path.join("content/page/models.ts")).unwrap();
    assert!(declaring.contains("export interface RenamedPage {"));
    assert!(declaring.contains("export const renamedPageTransferTypeConverter = new class"));

    let services = fs::read_to_string(output_path.join("kb/services.ts")).unwrap();
    for expected in [
        "import { renamedPageTransferTypeConverter } from '../content/page/models';",
        "import type { RenamedPage } from '../content/page/models';",
        "    RenamedPage\n",
        "outputType: { transferTypeConverter: renamedPageTransferTypeConverter } }),",
    ] {
        assert!(services.contains(expected), "{expected}\n{services}");
    }

    let models = fs::read_to_string(output_path.join("kb/models.ts")).unwrap();
    for expected in [
        "import { renamedPageTransferTypeConverter } from '../content/page/models';",
        "import type { RenamedPage } from '../content/page/models';",
        "  page?: RenamedPage;",
        "page = renamedPageTransferTypeConverter.fromTransferType(raw.page);",
    ] {
        assert!(models.contains(expected), "{expected}\n{models}");
    }
    // Nothing names the pre-override identifier.
    for stale in ["{ Page }", "pageTransferTypeConverter", ": Page", " Page\n"] {
        assert!(!services.contains(stale), "{stale}\n{services}");
        assert!(!models.contains(stale), "{stale}\n{models}");
    }
    fs::remove_dir_all(temp_dir).unwrap();
}

/// A name synthesized from a member follows that member's `x-ts-name`: the
/// `DEFAULT_<FIELD>` constant is built from the emitted identifier, not the JSON
/// key. A shape named after its *position* does not move — the hoisted inline
/// object keeps `<Model><Property>`.
/// See `specs/json-schema/PRINCIPLES.md` §15 and
/// `specs/json-schema/features/default.md`.
#[test]
fn typescript_json_override_moves_member_derived_names_only() {
    let temp_dir = unique_output_path("ts-json-member-derived-names");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("probe.yaml");
    fs::write(&input_path, MEMBER_DERIVED_NAME_SCHEMA).unwrap();
    let output_path = temp_dir.join("probe");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::TypeScript,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let rendered = fs::read_to_string(output_path.join("models.ts")).unwrap();

    // Member-derived: the override moves the constant with the field.
    assert!(rendered.contains("export const DEFAULT_ATTEMPTS = 3;"));
    assert!(!rendered.contains("DEFAULT_RETRY_COUNT"));
    assert!(rendered.contains("attempts?: number;"));
    // Position-derived: the hoisted shape keeps the position's name even though
    // the declaring member is renamed.
    assert!(rendered.contains("location?: ProbeAddress;"));
    assert!(rendered.contains("export interface ProbeAddress {"));
    assert!(!rendered.contains("ProbeLocation"));
    fs::remove_dir_all(temp_dir).unwrap();
}
