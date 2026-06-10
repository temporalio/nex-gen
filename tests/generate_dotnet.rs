use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nex_gen::generate_to_string_with_inputs;

const WORKFLOW_SERVICE_EXAMPLE_ID: &str = "workflow-service";
const TYPE_SHOWCASE_EXAMPLE_ID: &str = "type-showcase";

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn descriptor_path(root: &Path) -> PathBuf {
    root.join("examples/descriptors/temporal_api.bin")
}

fn linked_inputs_path(root: &Path) -> PathBuf {
    root.join("examples/inputs/deps")
}

fn dotnet_root(root: &Path) -> PathBuf {
    root.join("examples/dotnet")
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

fn example_input_paths(root: &Path, example_id: &str) -> Vec<PathBuf> {
    vec![input_path(root, example_id), linked_inputs_path(root)]
}

fn dotnet_output_path(root: &Path, example_id: &str) -> PathBuf {
    dotnet_root(root).join(example_id)
}

fn dotnet_example_ids(root: &Path) -> Vec<String> {
    let dotnet_root = dotnet_root(root);
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
            if dotnet_root.join(&example_id).is_dir() {
                Some(example_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn read_dotnet_output_files(dir: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("cs") {
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

fn generate_dotnet_output(root: &Path, example_id: &str, output_path: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "dotnet",
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
}

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nex-gen-{label}-{unique}"))
}

fn dotnet_msbuild_dir(path: &Path) -> String {
    let mut value = path.to_string_lossy().into_owned();
    if !value.ends_with(std::path::MAIN_SEPARATOR) {
        value.push(std::path::MAIN_SEPARATOR);
    }
    value
}

#[test]
fn dotnet_examples_generation_matches_checked_in_output() {
    let root = project_root();
    for example_id in dotnet_example_ids(&root) {
        let output_path = unique_output_path(&format!("dotnet-{example_id}"));
        generate_dotnet_output(&root, &example_id, &output_path);
        let rendered = read_dotnet_output_files(&output_path);
        let expected = read_dotnet_output_files(&dotnet_output_path(&root, &example_id));
        assert_eq!(rendered, expected, "snapshot mismatch for {example_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
}

#[test]
fn cli_generates_dotnet_support_file_from_parameter() {
    let root = project_root();
    let temp_dir = unique_output_path("dotnet-support-file-input");
    fs::create_dir_all(&temp_dir).unwrap();
    let support_path = temp_dir.join("CustomSupport.cs");
    let output_path = temp_dir.join("output");
    fs::write(
        &support_path,
        "namespace Custom;\npublic static class CustomSupport { }\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "dotnet",
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
        fs::read_to_string(output_path.join("Support/CustomSupport.cs"))
            .unwrap()
            .contains("public static class CustomSupport")
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn dotnet_example_project_builds() {
    let root = project_root();
    let build_path = unique_output_path("dotnet-build");
    fs::create_dir_all(&build_path).unwrap();
    let base_intermediate_output_path = dotnet_msbuild_dir(&build_path.join("obj"));
    let base_output_path = dotnet_msbuild_dir(&build_path.join("bin"));
    let base_intermediate_output_arg =
        format!("-p:BaseIntermediateOutputPath={base_intermediate_output_path}");
    let base_output_arg = format!("-p:BaseOutputPath={base_output_path}");

    let output = Command::new("dotnet")
        .current_dir(dotnet_root(&root))
        .args([
            "build",
            "NexusApiGen.DotNetExamples.csproj",
            "--nologo",
            "-p:RestoreUseStaticGraphEvaluation=true",
            &base_intermediate_output_arg,
            &base_output_arg,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(build_path).unwrap();
}

#[test]
fn dotnet_renders_nexus_service_interface_and_resources() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Dotnet,
        &example_input_paths(&root, TYPE_SHOWCASE_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains("[NexusService(\"TypeShowcase\")]"));
    assert!(rendered.contains("internal interface ITypeShowcase"));
    assert!(rendered.contains("[NexusOperation(\"SetProfile\")]"));
    assert!(rendered.contains("User SetProfile(SetProfileRequest request);"));
    assert!(rendered.contains("[Flags]\npublic enum UserCapability"));
    assert!(rendered.contains("public abstract class NotificationTarget"));
    assert!(rendered.contains("public class User"));
    assert!(rendered.contains("public class GetUserOptions"));
    assert!(rendered.contains("internal class GetUserRequest"));
    assert!(
        rendered.contains("private static async Task<User> GetUserAsync(GetUserRequest request)")
    );
    assert!(rendered.contains("public static Task<User> GetUserAsync(GetUserOptions options)"));
    assert!(
        !rendered.contains("public static async Task<User> GetUserAsync(GetUserRequest request)")
    );
}

#[test]
fn dotnet_renders_proto_backed_temporal_types() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Dotnet,
        &example_input_paths(&root, WORKFLOW_SERVICE_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains("internal interface IWorkflowService"));
    assert!(rendered.contains("Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionResponse SignalWithStartWorkflow(Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest request);"));
    assert!(rendered.contains("/// Signal a workflow, starting it first if needed."));
    assert!(rendered.contains("/// <returns>A workflow handle to the started workflow.</returns>"));
    assert!(
        rendered
            .contains("/// Request fields for signaling a workflow, starting it first if needed.")
    );
    assert!(rendered.contains("/// Unique identifier for the workflow execution."));
    assert!(rendered.contains("/// Cron schedule for recurring workflow executions. See https://docs.temporal.io/cron-job."));
    assert!(rendered.contains("/// Single-line fixed summary for the workflow execution that may appear in UI and CLI. This can be in single-line Temporal Markdown format."));
    assert!(rendered.contains("Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync<TWorkflow, TResult>"));
    assert!(!rendered.contains("this NexusWorkflowClient<"));
    assert!(
        rendered
            .contains("Workflow.CreateNexusWorkflowClient<IWorkflowService>(\"temporal-system\")")
    );
    assert!(rendered.contains("Expression<Func<TWorkflow, Task<TResult>>> workflow"));
    assert!(rendered.contains("public class SignalWithStartWorkflowOptions"));
    assert!(rendered.contains("required string Id { get; set; }"));
    assert!(rendered.contains("required string TaskQueue { get; set; }"));
    assert!(rendered.contains("required string Workflow { get; init; }"));
    assert!(rendered.contains("required string Id { get; init; }"));
    assert!(rendered.contains("required string TaskQueue { get; init; }"));
    assert!(rendered.contains("required string Signal { get; init; }"));
    assert!(rendered.contains("internal class SignalWithStartWorkflowRequest"));
    assert!(!rendered.contains("public class SignalWithStartWorkflowRequest"));
    assert!(!rendered.contains(" = default!;"));
    assert!(rendered.contains("IReadOnlyCollection<object?>? Args { get; set; }"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow, TResult>(Expression<Func<TWorkflow, Task<TResult>>> workflow, Expression<Func<TWorkflow, Task>> signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync(string workflow, string signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow, TResult>(Expression<Func<TWorkflow, Task<TResult>>> workflow, string signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow>(string workflow, Expression<Func<TWorkflow, Task>> signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("private static async Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("public static async Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("NexusWorkflowOperationOptions"));
    assert!(!rendered.contains("string id, string taskQueue"));
    assert!(!rendered.contains("System.TimeSpan? executionTimeout = null"));
    assert!(rendered.contains(
        "Workflow = NexusApiGen.Support.TemporalFunctionNames.WorkflowName(workflowMethod)"
    ));
    assert!(
        rendered.contains(
            "Signal = NexusApiGen.Support.TemporalFunctionNames.SignalName(signalMethod)"
        )
    );
    assert!(rendered.contains("Args = workflowArgs"));
    assert!(rendered.contains("Args = options.Args"));
    assert!(rendered.contains("SignalArgs = signalArgs"));
    assert!(rendered.contains("SignalArgs = options.SignalArgs"));
    assert!(!rendered.contains("WorkflowNameFromRunMethod"));
    assert!(!rendered.contains("SignalNameFromMethod"));
    assert!(rendered.contains("var protoRequest = request.ToProto();"));
    assert!(rendered.contains(
        "public Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToProto()"
    ));
    assert!(rendered.contains("public Temporalio.Api.Sdk.V1.UserMetadata ToProto()"));
    assert!(rendered.contains(
        "NexusApiGen.Support.ProtoConverters.ToProto<Temporalio.Api.Common.V1.WorkflowType>(Workflow)"
    ));
    assert!(rendered.contains(
        "NexusApiGen.Support.ProtoConverters.ToProto<Temporalio.Api.TaskQueue.V1.TaskQueue>(TaskQueue)"
    ));
    assert!(rendered.contains("proto.UserMetadata = userMetadata.ToProto();"));
    assert!(!rendered.contains("var protoRequest = ToProto(request);"));
    assert!(!rendered.contains("private static Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToProto(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("Temporalio.Api.Taskqueue.V1.TaskQueue"));
    assert!(rendered.contains("RetryPolicy = options.RetryPolicy"));
    assert!(rendered.contains("ExecutionTimeout = options.ExecutionTimeout"));
    assert!(rendered.contains("NexusApiGen.Support.ProtoConverters.ToProto<Temporalio.Api.Common.V1.RetryPolicy>(retryPolicy)"));
    assert!(rendered.contains("NexusApiGen.Support.ProtoConverters.ToProto<Google.Protobuf.WellKnownTypes.Duration>(executionTimeout)"));
    assert!(rendered.contains("Temporalio.Common.RetryPolicy? RetryPolicy"));
    assert!(rendered.contains("Temporalio.Api.Enums.V1.WorkflowIdReusePolicy? IdReusePolicy"));
    assert!(rendered.contains("System.TimeSpan? ExecutionTimeout"));
}
