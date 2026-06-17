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
        "namespace Custom\n{\npublic static class CustomSupport { }\n}\n",
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
    let project =
        fs::read_to_string(dotnet_root(&root).join("NexusApiGen.DotNetExamples.csproj")).unwrap();
    assert!(project.contains("<LangVersion>9.0</LangVersion>"));
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
            "-p:LangVersion=9.0",
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
    assert!(!rendered.contains("public sealed class NexGenNexusOperation"));
    assert!(rendered.contains("public static class NexGenOperationRegistry"));
    assert!(
        rendered
            .contains("internal static IReadOnlyDictionary<string, ServiceDefinition> Services")
    );
    assert!(rendered.contains("[\"TypeShowcase\"] = ServiceDefinition.FromType<ITypeShowcase>()"));
    assert!(rendered.contains(
        "public static IReadOnlyDictionary<(string Service, string Operation), OperationDefinition> Operations"
    ));
    assert!(rendered.contains("[(\"TypeShowcase\", \"SetProfile\")]"));
    assert!(rendered.contains("Services[\"TypeShowcase\"].Operations[\"SetProfile\"]"));
    assert!(!rendered.contains("OperationDefinition.FromMethod"));
    assert!(!rendered.contains("endpoint: \"type-showcase\""));
    assert!(!rendered.contains("requestType: typeof(SetProfileRequest)"));
    assert!(!rendered.contains("responseType: typeof(User)"));
    assert!(!rendered.contains("responseType: typeof(void)"));
    assert!(rendered.contains("[Flags]\n    public enum UserCapability"));
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
    assert!(rendered.contains("namespace Temporalio.Workflows\n{"));
    assert!(rendered.contains("namespace NexGen.Support\n{"));
    assert!(!rendered.contains("namespace NexusApiGen."));
    assert!(!rendered.contains("namespace Temporalio.Workflows;"));
    assert!(!rendered.contains("namespace NexGen.Support;"));
    assert!(rendered.contains("Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionResponse SignalWithStartWorkflow(Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest request);"));
    assert!(rendered.contains(
        "[(\"temporal.api.workflowservice.v1.WorkflowService\", \"SignalWithStartWorkflowExecution\")]"
    ));
    assert!(rendered.contains(
        "[\"temporal.api.workflowservice.v1.WorkflowService\"] = ServiceDefinition.FromType<IWorkflowService>()"
    ));
    assert!(rendered.contains(
        "Services[\"temporal.api.workflowservice.v1.WorkflowService\"].Operations[\"SignalWithStartWorkflowExecution\"]"
    ));
    assert!(!rendered.contains("endpoint: \"temporal-system\""));
    assert!(!rendered.contains(
        "requestType: typeof(Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest)"
    ));
    assert!(!rendered.contains(
        "responseType: typeof(Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionResponse)"
    ));
    assert!(rendered.contains("/// Signal a workflow, starting it first if needed."));
    assert!(rendered.contains("/// <returns>A workflow handle to the started workflow.</returns>"));
    assert!(
        rendered
            .contains("/// Request fields for signaling a workflow, starting it first if needed.")
    );
    assert!(rendered.contains("/// <param name=\"workflow\">Workflow type name or workflow expression identifying the workflow to start.</param>"));
    assert!(rendered.contains("/// <param name=\"args\">Arguments for the workflow.</param>"));
    assert!(rendered.contains("/// <param name=\"signal\">Signal name or signal expression to send with the start request.</param>"));
    assert!(rendered.contains("/// <param name=\"signalArgs\">Arguments for the signal.</param>"));
    assert!(rendered.contains("/// <param name=\"options\">Request fields for signaling a workflow, starting it first if needed.</param>"));
    assert!(rendered.contains("/// Unique identifier for the workflow execution."));
    assert!(rendered.contains("/// Cron schedule for recurring workflow executions. See https://docs.temporal.io/cron-job."));
    assert!(rendered.contains("/// Single-line fixed summary for the workflow execution that may appear in UI and CLI. This can be in single-line Temporal Markdown format."));
    assert!(rendered.contains("/// <summary>\n        /// Arguments for the workflow.\n        /// </summary>\n        public IReadOnlyCollection<object?>? Args"));
    assert!(rendered.contains("/// <summary>\n        /// Arguments for the signal.\n        /// </summary>\n        public IReadOnlyCollection<object?>? SignalArgs"));
    assert!(rendered.contains("Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync<TWorkflow, TResult>"));
    assert!(rendered.contains("public static partial class Workflow"));
    assert!(!rendered.contains("WorkflowServiceOperations"));
    assert!(!rendered.contains("this NexusWorkflowClient<"));
    assert!(
        rendered
            .contains("Workflow.CreateNexusWorkflowClient<IWorkflowService>(\"temporal-system\")")
    );
    assert!(rendered.contains("Expression<Func<TWorkflow, Task<TResult>>> workflow"));
    assert!(rendered.contains("public class SignalWithStartWorkflowOptions"));
    assert!(
        rendered.contains("public SignalWithStartWorkflowOptions(string id, string taskQueue)")
    );
    assert!(rendered.contains("internal SignalWithStartWorkflowRequest(string workflow, string id, string taskQueue, string signal)"));
    assert!(rendered.contains("public string Id { get; set; }\n"));
    assert!(rendered.contains("public string TaskQueue { get; set; }\n"));
    assert!(rendered.contains("public string Workflow { get; init; }\n"));
    assert!(rendered.contains("public string Id { get; init; }\n"));
    assert!(rendered.contains("public string TaskQueue { get; init; }\n"));
    assert!(rendered.contains("public string Signal { get; init; }\n"));
    assert!(!rendered.contains("default!"));
    assert!(!rendered.contains("required "));
    assert!(rendered.contains("internal class SignalWithStartWorkflowRequest"));
    assert!(!rendered.contains("public class SignalWithStartWorkflowRequest"));
    assert!(!rendered.contains("IReadOnlyCollection<object?>? Args { get; set; }"));
    assert!(!rendered.contains("IReadOnlyCollection<object?>? SignalArgs { get; set; }"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow, TResult>(Expression<Func<TWorkflow, Task<TResult>>> workflow, Expression<Func<TWorkflow, Task>> signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync(string workflow, IReadOnlyCollection<object?>? args, string signal, IReadOnlyCollection<object?>? signalArgs, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow, TResult>(Expression<Func<TWorkflow, Task<TResult>>> workflow, string signal, IReadOnlyCollection<object?>? signalArgs, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow>(string workflow, IReadOnlyCollection<object?>? args, Expression<Func<TWorkflow, Task>> signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("private static async Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("public static async Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("NexusWorkflowOperationOptions"));
    assert!(!rendered.contains("System.TimeSpan? executionTimeout = null"));
    assert!(
        rendered.contains(
            "new SignalWithStartWorkflowRequest(NexGen.Support.TemporalFunctionNames.WorkflowName(workflowMethod), options.Id, options.TaskQueue"
        )
    );
    assert!(rendered.contains(
        "options.TaskQueue, NexGen.Support.TemporalFunctionNames.SignalName(signalMethod))"
    ));
    assert!(rendered.contains("Args = workflowArgs"));
    assert!(rendered.contains("Args = args"));
    assert!(rendered.contains("SignalArgs = signalArgs"));
    assert!(!rendered.contains("Args = workflowArgs.ToProto()"));
    assert!(!rendered.contains("Args = args == null ? null : args.ToProto()"));
    assert!(!rendered.contains("SignalArgs = signalArgs == null ? null : signalArgs.ToProto()"));
    assert!(!rendered.contains("Args = options.Args"));
    assert!(!rendered.contains("SignalArgs = options.SignalArgs"));
    assert!(!rendered.contains("WorkflowNameFromRunMethod"));
    assert!(!rendered.contains("SignalNameFromMethod"));
    assert!(rendered.contains("var protoRequest = request.ToProto();"));
    assert!(rendered.contains(
        "public Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToProto()"
    ));
    assert!(rendered.contains("public Temporalio.Api.Sdk.V1.UserMetadata ToProto()"));
    assert!(rendered.contains("using NexGen.Support;"));
    assert!(rendered.contains("Workflow.ToProto(default(Temporalio.Api.Common.V1.WorkflowType)!)"));
    assert!(
        rendered.contains("TaskQueue.ToProto(default(Temporalio.Api.TaskQueue.V1.TaskQueue)!)")
    );
    assert!(rendered.contains("proto.UserMetadata = userMetadata.ToProto();"));
    assert!(!rendered.contains("var protoRequest = ToProto(request);"));
    assert!(!rendered.contains("private static Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToProto(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("Temporalio.Api.Taskqueue.V1.TaskQueue"));
    assert!(rendered.contains("RetryPolicy = options.RetryPolicy"));
    assert!(rendered.contains("ExecutionTimeout = options.ExecutionTimeout"));
    assert!(rendered.contains("proto.RetryPolicy = retryPolicy.ToProto();"));
    assert!(rendered.contains("proto.WorkflowExecutionTimeout = executionTimeout.ToProto();"));
    assert!(rendered.contains("public IReadOnlyCollection<object?>? Args { get; init; }"));
    assert!(rendered.contains("proto.Input = args.ToProto();"));
    assert!(rendered.contains("Args = args,"));
    assert!(rendered.contains("proto.Summary = staticSummary.ToProto();"));
    assert!(
        rendered.contains(
            "internal static ApiCommon.Payloads ToProto(this IEnumerable<object?> value)"
        )
    );
    assert!(rendered.contains("internal static Duration ToProto(this TimeSpan value)"));
    assert!(rendered.contains(
        "internal static Temporalio.Common.RetryPolicy FromProto(this ApiCommon.RetryPolicy proto)"
    ));
    assert!(rendered.contains("internal static class TemporalWorkflowContext"));
    assert!(rendered.contains("internal static class TemporalFunctionNames"));
    assert!(rendered.contains("internal static class ProtoExtensions"));
    assert!(rendered.contains(
        "internal static ApiCommon.WorkflowType ToProto(this string value, ApiCommon.WorkflowType _)"
    ));
    assert!(rendered.contains(
        "internal static ApiTaskQueue.TaskQueue ToProto(this string value, ApiTaskQueue.TaskQueue _)"
    ));
    assert!(!rendered.contains("internal static TProto ToProto<TProto>(this string value)"));
    assert!(!rendered.contains("targetType == typeof"));
    assert!(!rendered.contains("public static class TemporalWorkflowContext"));
    assert!(!rendered.contains("public static class TemporalFunctionNames"));
    assert!(!rendered.contains("public static class ProtoExtensions"));
    assert!(!rendered.contains("ProtoConverters"));
    assert!(!rendered.contains("private static TValue Cast"));
    assert!(!rendered.contains("Cast<TProto>"));
    assert!(!rendered.contains("retryPolicy.ToProto<Temporalio.Api.Common.V1.RetryPolicy>()"));
    assert!(
        !rendered.contains("executionTimeout.ToProto<Google.Protobuf.WellKnownTypes.Duration>()")
    );
    assert!(rendered.contains("Temporalio.Common.RetryPolicy? RetryPolicy"));
    assert!(rendered.contains("Temporalio.Api.Enums.V1.WorkflowIdReusePolicy? IdReusePolicy"));
    assert!(rendered.contains("System.TimeSpan? ExecutionTimeout"));
}

#[test]
fn dotnet_uses_annotated_namespace_and_operations_class() {
    let temp_dir = unique_output_path("dotnet-annotated-namespace");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("main.wit");
    fs::write(
        &input_path,
        r#"
package example:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "temporal-system"
/// @nexus.namespace dotnet="Temporalio.Workflows"
/// @nexus.operations-class dotnet="Workflow"
interface workflow-service {
  record signal-request {
    id: string,
  }

  record signal-response {
    run-id: option<string>,
  }

  /// @nexus.operation name="SignalWithStartWorkflowExecution"
  signal-with-start-workflow: func(request: signal-request) -> signal-response;
}
"#,
    )
    .unwrap();

    let input_paths = vec![input_path];
    let rendered =
        generate_to_string_with_inputs(nex_gen::language::Language::Dotnet, &input_paths, &[])
            .unwrap();

    assert!(rendered.contains("namespace Temporalio.Workflows\n{"));
    assert!(rendered.contains("public static partial class Workflow"));
    assert!(!rendered.contains("WorkflowServiceOperations"));
    fs::remove_dir_all(temp_dir).unwrap();
}
