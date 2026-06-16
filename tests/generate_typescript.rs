use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nex_gen::generate_to_string_with_inputs;

const PRIMARY_EXAMPLE_ID: &str = "workflow-service";
const START_WORKFLOW_EXAMPLE_ID: &str = "start-workflow";
const TYPE_ROUNDTRIP_EXAMPLE_ID: &str = "type-roundtrip";

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn descriptor_path(root: &Path) -> PathBuf {
    root.join("examples/descriptors/temporal_api.bin")
}

fn linked_inputs_path(root: &Path) -> PathBuf {
    root.join("examples/inputs/deps")
}

fn example_input_paths(root: &Path, example_id: &str) -> Vec<PathBuf> {
    vec![input_path(root, example_id), linked_inputs_path(root)]
}

fn typescript_root(root: &Path) -> PathBuf {
    root.join("examples/typescript")
}

fn input_path(root: &Path, example_id: &str) -> PathBuf {
    let flat_path = root
        .join("examples/inputs")
        .join(format!("{example_id}.wit"));
    if flat_path.is_file() {
        flat_path
    } else {
        root.join("examples/inputs")
            .join(example_id)
            .join("main.wit")
    }
}

fn typescript_output_path(root: &Path, example_id: &str) -> PathBuf {
    typescript_root(root).join(example_id)
}

fn typescript_example_ids(root: &Path) -> Vec<String> {
    let typescript_root = typescript_root(root);
    let mut ids = fs::read_dir(root.join("examples/inputs"))
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
            if typescript_root.join(&example_id).is_dir() {
                Some(example_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn ensure_typescript_dependencies(root: &Path) {
    let example_dir = typescript_root(root);
    if example_dir.join("node_modules").exists() {
        return;
    }

    let install_status = Command::new("npm")
        .current_dir(&example_dir)
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

fn generate_formatted_typescript_output(root: &Path, example_id: &str, output_path: &Path) {
    ensure_typescript_dependencies(root);

    let status = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "typescript",
            "--input",
            input_path(root, example_id).to_str().unwrap(),
            "--input",
            linked_inputs_path(root).to_str().unwrap(),
            "--descriptors",
            descriptor_path(root).to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
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

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nex-gen-{label}-{unique}"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "typescript",
            "--input",
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
fn typescript_example_suite_typechecks_and_tests() {
    let root = project_root();
    let example_dir = typescript_root(&root);
    ensure_typescript_dependencies(&root);

    let typecheck_status = Command::new("npm")
        .current_dir(&example_dir)
        .args(["run", "typecheck"])
        .status()
        .unwrap();
    assert!(typecheck_status.success());

    let test_status = Command::new("npm")
        .current_dir(&example_dir)
        .args(["run", "test"])
        .status()
        .unwrap();
    assert!(test_status.success());
}

#[test]
fn typescript_renders_required_fields_and_custom_message_types() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::TypeScript,
        &example_input_paths(&root, PRIMARY_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();

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
    assert!(rendered.contains("signalName: signalFunctionToProto("));
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
        "export { signalWithStartWorkflow } from './operations/signal-with-start-workflow.ts';"
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
    assert!(
        rendered.contains("export type { SignalWithStartWorkflowRequest } from './models.ts';")
    );
    assert!(!rendered.contains("export { WorkflowService } from './service.ts';"));
    assert!(!rendered.contains("export type { SignalWithStartWorkflowResponse"));
    assert!(rendered.contains("request: SignalWithStartWorkflowRequest<WorkflowFn, SignalValue>,"));
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

    let start_workflow_rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::TypeScript,
        &example_input_paths(&root, START_WORKFLOW_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();
    assert!(
        start_workflow_rendered
            .contains("export type CancelWorkflowResponse = Record<string, never>;")
    );
    assert!(!start_workflow_rendered.contains("export interface CancelWorkflowResponse {}"));

    let type_roundtrip_rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::TypeScript,
        &example_input_paths(&root, TYPE_ROUNDTRIP_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();
    assert!(type_roundtrip_rendered.contains("retryPolicy: common.RetryPolicy;"));
    assert!(type_roundtrip_rendered.contains("request: common.RetryPolicy,"));
}
