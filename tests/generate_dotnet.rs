// The `dotnet` generate target lives behind the `advanced` feature.
#![cfg(feature = "advanced")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::{GenerateRequest, generate_to_file};

const WORKFLOW_SERVICE_EXAMPLE_ID: &str = "workflow-service";
const TYPE_SHOWCASE_EXAMPLE_ID: &str = "type-showcase";
static DOTNET_COMMAND_LOCK: Mutex<()> = Mutex::new(());
static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn descriptor_path(root: &Path) -> PathBuf {
    root.join("advanced/samples/descriptors/temporal_api.bin")
}

fn linked_inputs_path(root: &Path) -> PathBuf {
    root.join("advanced/samples/inputs/deps")
}

fn dotnet_root(root: &Path) -> PathBuf {
    root.join("advanced/samples/dotnet")
}

fn dotnet_command() -> (MutexGuard<'static, ()>, Command) {
    let guard = DOTNET_COMMAND_LOCK.lock().unwrap();
    let mut command = Command::new("dotnet");
    command
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_NOLOGO", "1");
    (guard, command)
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

fn example_input_paths(root: &Path, example_id: &str) -> Vec<PathBuf> {
    let input = input_path(root, example_id);
    let mut paths = vec![input.clone()];
    if fs::read_to_string(&input)
        .unwrap()
        .contains("use nexus:temporal-types/")
    {
        paths.push(linked_inputs_path(root));
    }
    paths
}

fn dotnet_output_path(root: &Path, example_id: &str) -> PathBuf {
    dotnet_root(root).join("wit").join(example_id)
}

fn dotnet_example_ids(root: &Path) -> Vec<String> {
    let dotnet_root = dotnet_root(root);
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
            if dotnet_root.join("wit").join(&example_id).is_dir() {
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

fn render_output_files(files: BTreeMap<PathBuf, String>) -> String {
    files
        .into_iter()
        .map(|(path, contents)| format!("### {}\n{contents}", path.display()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn generate_dotnet_to_string(input_paths: &[PathBuf], descriptor_paths: &[PathBuf]) -> String {
    render_output_files(generate_dotnet_files(input_paths, descriptor_paths))
}

fn generate_dotnet_files(
    input_paths: &[PathBuf],
    descriptor_paths: &[PathBuf],
) -> BTreeMap<PathBuf, String> {
    let temp_dir = unique_output_path("dotnet-rendered");
    let output_path = temp_dir.join("output");
    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::Dotnet,
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
    let files = if output_path.is_file() {
        BTreeMap::from([(
            PathBuf::from("output.cs"),
            fs::read_to_string(&output_path).unwrap(),
        )])
    } else {
        read_dotnet_output_files(&output_path)
    };
    fs::remove_dir_all(temp_dir).unwrap();
    files
}

fn generate_dotnet_output(root: &Path, example_id: &str, output_path: &Path) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nexgen"));
    command
        .arg("dotnet")
        .args(example_input_paths(root, example_id))
        .args([
            "--descriptors",
            descriptor_path(root).to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
            "--native-api",
        ]);
    let status = command.status().unwrap();
    assert!(status.success());
}

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-{label}-{unique}-{counter}"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_nexgen"))
        .args([
            "dotnet",
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
        fs::read_to_string(dotnet_root(&root).join("Nexgen.DotNetExamples.csproj")).unwrap();
    assert!(project.contains("<LangVersion>9.0</LangVersion>"));
    let build_path = unique_output_path("dotnet-build");
    fs::create_dir_all(&build_path).unwrap();
    let base_intermediate_output_path = dotnet_msbuild_dir(&build_path.join("obj"));
    let base_output_path = dotnet_msbuild_dir(&build_path.join("bin"));
    let base_intermediate_output_arg =
        format!("-p:BaseIntermediateOutputPath={base_intermediate_output_path}");
    let base_output_arg = format!("-p:BaseOutputPath={base_output_path}");

    let (_dotnet_guard, mut command) = dotnet_command();
    let output = command
        .current_dir(dotnet_root(&root))
        .args([
            "build",
            "Nexgen.DotNetExamples.csproj",
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
    let files = generate_dotnet_files(
        &example_input_paths(&root, TYPE_SHOWCASE_EXAMPLE_ID),
        &[descriptor_path(&root)],
    );
    let services = files.get(&PathBuf::from("Services.cs")).unwrap().clone();
    let operations = files.get(&PathBuf::from("Operations.cs")).unwrap().clone();
    let rendered = render_output_files(files);

    assert!(rendered.contains("[NexusService(\"TypeShowcase\")]"));
    assert!(rendered.contains("internal interface ITypeShowcase"));
    assert!(rendered.contains("[NexusOperation(\"SetProfile\")]"));
    assert!(rendered.contains("User SetProfile(SetProfileRequest request);"));
    assert!(!rendered.contains("public sealed class NexgenNexusOperation"));
    assert!(rendered.contains("NexgenOperationRegistry"));
    assert!(rendered.contains("NexgenOperationInfo<GetUserRequest, User>"));
    assert!(!rendered.contains("serializationContext:"));
    assert!(!rendered.contains("WorkflowServiceSerializationContexts"));
    assert!(services.contains("NexgenOperationRegistry"));
    assert!(
        services.contains(
            "IReadOnlyDictionary<(string Service, string Operation), INexgenOperationInfo>"
        )
    );
    assert!(services.contains(
        "private static readonly ServiceDefinition TypeShowcaseServiceDefinition = ServiceDefinition.FromType<ITypeShowcase>();"
    ));
    assert!(services.contains("TypeShowcaseServiceDefinition.Operations[\"GetUser\"]"));
    assert!(!services.contains("OperationDefinition.FromMethod"));
    assert!(!operations.contains("NexgenOperationRegistry"));
    assert!(!operations.contains("ServiceDefinition.FromType"));
    assert!(!rendered.contains("endpoint: \"type-showcase\""));
    assert!(!rendered.contains("requestType: typeof(SetProfileRequest)"));
    assert!(!rendered.contains("responseType: typeof(User)"));
    assert!(!rendered.contains("responseType: typeof(void)"));
    assert!(rendered.contains("[Flags]\n    public enum UserCapability"));
    assert!(rendered.contains("public abstract record NotificationTarget"));
    assert!(rendered.contains("public sealed record Email(string Value) : NotificationTarget;"));
    assert!(rendered.contains("public sealed record None : NotificationTarget;"));
    assert!(rendered.contains("public class User"));
    assert!(rendered.contains("public class GetUserOptions"));
    assert!(rendered.contains("internal class GetUserRequest"));
    assert!(rendered.contains("using System.Threading.Tasks;"));
    assert!(
        rendered.contains("private static async Task<User> GetUserAsync(GetUserRequest request)")
    );
    assert!(rendered.contains("public static Task<User> GetUserAsync(GetUserOptions options)"));
    assert!(
        rendered.contains("[GeneratedCode(\"nexgen\", null)]\n    public static class Operations")
    );
    assert!(!rendered.contains("public static partial class Operations"));
    assert!(!rendered.contains("TypeShowcaseOperations"));
    assert!(
        !rendered.contains("public static async Task<User> GetUserAsync(GetUserRequest request)")
    );
    assert!(rendered.contains("public Task<User> UpdateEmailAsync(string email)\n        {\n            var request = new UpdateEmailOptions(UserId, email);\n            return Operations.UpdateEmailAsync(request);\n        }"));
    assert!(rendered.contains("public Task<User> RenameAsync(string displayName)\n        {\n            var request = new RenameOptions(UserId, displayName);\n            return Operations.RenameAsync(request);\n        }"));
    assert!(rendered.contains("public Task DeactivateAsync(string? reason)\n        {\n            var request = new DeactivateOptions(UserId) { Reason = reason };\n            return Operations.DeactivateAsync(request);\n        }"));
    assert!(!rendered.contains("Resource methods require a bound Nexus client."));
}

#[test]
fn dotnet_renders_proto_backed_temporal_types() {
    let root = project_root();
    let files = generate_dotnet_files(
        &example_input_paths(&root, WORKFLOW_SERVICE_EXAMPLE_ID),
        &[descriptor_path(&root)],
    );
    let services = files.get(&PathBuf::from("Services.cs")).unwrap().clone();
    let operations = files.get(&PathBuf::from("Operations.cs")).unwrap().clone();
    let rendered = render_output_files(files);

    assert!(rendered.contains("internal interface IWorkflowService"));
    assert!(rendered.contains("namespace Temporalio.Workflows\n{"));
    assert!(rendered.contains("namespace Nexgen.Support\n{"));
    assert!(!rendered.contains("namespace Temporalio.Workflows;"));
    assert!(!rendered.contains("namespace Nexgen.Support;"));
    assert!(rendered.contains(
        "SignalWithStartWorkflowResponse SignalWithStartWorkflow(SignalWithStartWorkflowRequest request);"
    ));
    assert!(rendered.contains("NexgenOperationRegistry"));
    assert!(rendered.contains(
        "NexgenOperationInfo<SignalWithStartWorkflowRequest, SignalWithStartWorkflowResponse>"
    ));
    assert!(rendered.contains(
        "serializationContext: Nexgen.Support.WorkflowServiceSerializationContexts.SignalWithStartWorkflow"
    ));
    assert!(services.contains("NexgenOperationRegistry"));
    assert!(services.contains(
        "serializationContext: Nexgen.Support.WorkflowServiceSerializationContexts.SignalWithStartWorkflow"
    ));
    assert!(
        services.contains(
            "IReadOnlyDictionary<(string Service, string Operation), INexgenOperationInfo>"
        )
    );
    assert!(services.contains(
        "private static readonly ServiceDefinition WorkflowServiceServiceDefinition = ServiceDefinition.FromType<IWorkflowService>();"
    ));
    assert!(services.contains(
        "WorkflowServiceServiceDefinition.Operations[\"SignalWithStartWorkflowExecution\"]"
    ));
    assert!(!services.contains("OperationDefinition.FromMethod"));
    assert!(!operations.contains("NexgenOperationRegistry"));
    assert!(rendered.contains(
        "internal static ISerializationContext SignalWithStartWorkflow(SignalWithStartWorkflowRequest request)"
    ));
    assert!(rendered.contains("new ISerializationContext.Workflow(request.Namespace, request.Id)"));
    assert!(!operations.contains("ServiceDefinition.FromType"));
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
    assert!(rendered.contains("public static class Operations"));
    assert!(!rendered.contains("public static partial class Workflow"));
    assert!(!rendered.contains("this NexusWorkflowClient<"));
    assert!(
        rendered.contains("private const string WorkflowServiceEndpoint = \"temporal-system\";")
    );
    assert!(
        rendered.contains(
            "Workflow.CreateNexusWorkflowClient<IWorkflowService>(WorkflowServiceEndpoint)"
        )
    );
    assert!(
        !rendered
            .contains("Workflow.CreateNexusWorkflowClient<IWorkflowService>(\"temporal-system\")")
    );
    assert!(rendered.contains("Expression<Func<TWorkflow, Task<TResult>>> workflow"));
    assert!(rendered.contains("using System.CodeDom.Compiler;"));
    assert!(rendered.contains("[GeneratedCode(\"nexgen\", null)]\n    [NexusService(\"temporal.api.workflowservice.v1.WorkflowService\")]\n    internal interface IWorkflowService"));
    assert!(rendered.contains(
        "[GeneratedCode(\"nexgen\", null)]\n    internal class SignalWithStartWorkflowRequest"
    ));
    assert!(rendered.contains(
        "[GeneratedCode(\"nexgen\", null)]\n    public class SignalWithStartWorkflowOptions"
    ));
    assert!(
        !rendered.contains(
            "[GeneratedCode(\"nexgen\", null)]\n    public static partial class Workflow"
        )
    );
    assert!(!rendered.contains(
        "public static partial class Workflow\n    {\n        [GeneratedCode(\"nexgen\", null)]\n        private const string WorkflowServiceEndpoint"
    ));
    assert!(rendered.contains(
        "/// <remarks>WARNING: This API is experimental and may change in the future.</remarks>"
    ));
    assert!(rendered.contains("public class SignalWithStartWorkflowOptions"));
    assert!(
        rendered.contains("public SignalWithStartWorkflowOptions(string id, string taskQueue)")
    );
    assert!(rendered.contains("internal SignalWithStartWorkflowRequest(string workflow, string id, string taskQueue, string signal)"));
    assert!(rendered.contains("public string Id { get; set; }\n"));
    assert!(rendered.contains("public string TaskQueue { get; set; }\n"));
    assert!(rendered.contains("public string Workflow { get; set; }\n"));
    assert!(rendered.contains("public string Id { get; set; }\n"));
    assert!(rendered.contains("public string TaskQueue { get; set; }\n"));
    assert!(rendered.contains("public string Signal { get; set; }\n"));
    assert!(!rendered.contains("default!"));
    assert!(!rendered.contains("required "));
    assert!(rendered.contains("internal class SignalWithStartWorkflowRequest"));
    assert!(!rendered.contains("public class SignalWithStartWorkflowRequest"));
    assert!(rendered.contains("internal class UserMetadata"));
    assert!(!rendered.contains("public class UserMetadata"));
    assert!(rendered.contains("IReadOnlyCollection<object?>? Args { get; set; }"));
    assert!(rendered.contains("IReadOnlyCollection<object?>? SignalArgs { get; set; }"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow, TResult>(Expression<Func<TWorkflow, Task<TResult>>> workflow, Expression<Func<TWorkflow, Task>> signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync(string workflow, IReadOnlyCollection<object?>? args, string signal, IReadOnlyCollection<object?>? signalArgs, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow, TResult>(Expression<Func<TWorkflow, Task<TResult>>> workflow, string signal, IReadOnlyCollection<object?>? signalArgs, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("SignalWithStartWorkflowAsync<TWorkflow>(string workflow, IReadOnlyCollection<object?>? args, Expression<Func<TWorkflow, Task>> signal, SignalWithStartWorkflowOptions options)"));
    assert!(rendered.contains("Nexgen.Support.TemporalFunctionNames.ExtractCall(workflow)"));
    assert!(rendered.contains("Nexgen.Support.TemporalFunctionNames.ExtractCall(signal)"));
    assert!(!rendered.contains(
        "private static (MethodInfo Method, IReadOnlyCollection<object?> Args) ExtractCall"
    ));
    assert!(rendered.contains("private static async Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("public static async Task<Temporalio.Workflows.ExternalWorkflowHandle> SignalWithStartWorkflowAsync(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("NexusWorkflowOperationOptions"));
    assert!(!rendered.contains("System.TimeSpan? executionTimeout = null"));
    assert!(
        rendered.contains(
            "new SignalWithStartWorkflowRequest(Nexgen.Support.TemporalFunctionNames.WorkflowName(workflowMethod), options.Id, options.TaskQueue"
        )
    );
    assert!(rendered.contains(
        "options.TaskQueue, Nexgen.Support.TemporalFunctionNames.SignalName(signalMethod))"
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
    assert!(!rendered.contains("var wireRequest = request.ToProto();"));
    assert!(rendered.contains("svc.SignalWithStartWorkflow(request)"));
    assert!(rendered.contains(
        "[Temporalio.Converters.TemporalTransferTypeConverter(typeof(SignalWithStartWorkflowRequest.TransferTypeConverter))]"
    ));
    assert!(rendered.contains(
        "public sealed class TransferTypeConverter : Temporalio.Converters.ITemporalTransferTypeConverter"
    ));
    assert!(!rendered.contains("ITemporalIntermediate"));
    assert!(rendered.contains("internal static SignalWithStartWorkflowRequest FromTransferType("));
    assert!(!rendered.contains("TemporalToIntermediate("));
    assert!(rendered.contains(
        "internal Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToTransferType()"
    ));
    assert!(!rendered.contains(
        "public Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToProto(Temporalio.Converters.IPayloadConverter? payloadConverter = null)"
    ));
    assert!(!rendered.contains(
        "public Temporalio.Api.Sdk.V1.UserMetadata ToProto(Temporalio.Converters.IPayloadConverter? payloadConverter = null)"
    ));
    assert!(rendered.contains("using Nexgen.Support;"));
    assert!(rendered.contains("Nexgen.Support.ProtoExtensions.ToWorkflowTypeProto(Workflow"));
    assert!(rendered.contains("Nexgen.Support.ProtoExtensions.ToTaskQueueProto(TaskQueue"));
    assert!(rendered.contains(
        "proto.UserMetadata = (Temporalio.Api.Sdk.V1.UserMetadata)userMetadata.ToTransferType();"
    ));
    assert!(!rendered.contains("var wireRequest = ToProto(request);"));
    assert!(!rendered.contains("private static Temporalio.Api.WorkflowService.V1.SignalWithStartWorkflowExecutionRequest ToProto(SignalWithStartWorkflowRequest request)"));
    assert!(!rendered.contains("Temporalio.Api.Taskqueue.V1.TaskQueue"));
    assert!(rendered.contains("RetryPolicy = options.RetryPolicy"));
    assert!(rendered.contains("ExecutionTimeout = options.ExecutionTimeout"));
    assert!(rendered.contains("proto.RetryPolicy = retryPolicy.ToProto();"));
    assert!(rendered.contains("proto.WorkflowExecutionTimeout = executionTimeout.ToProto();"));
    assert!(rendered.contains("public IReadOnlyCollection<object?>? Args { get; set; }"));
    assert!(rendered.contains("proto.Input = Nexgen.Support.ProtoExtensions.ToPayloads(args);"));
    assert!(rendered.contains("Args = args,"));
    assert!(
        rendered
            .contains("proto.Summary = Nexgen.Support.ProtoExtensions.ToPayload(staticSummary);")
    );
    assert!(rendered.contains("internal static ApiCommon.Payload ToPayload(object? value)"));
    assert!(
        rendered
            .contains("internal static ApiCommon.Payloads ToPayloads(IEnumerable<object?> values)")
    );
    assert!(!rendered.contains("ToProto(this object? value)"));
    assert!(!rendered.contains("ToProto(this IEnumerable<object?> value)"));
    assert!(rendered.contains("internal static Duration ToProto(this TimeSpan value)"));
    assert!(!rendered.contains(" FromProto("));
    assert!(rendered.contains("internal static class TemporalWorkflowContext"));
    assert!(rendered.contains("internal static class TemporalFunctionNames"));
    assert!(rendered.contains("internal static class SystemNexusConverterContext"));
    assert!(rendered.contains("internal static class ProtoExtensions"));
    assert!(rendered.contains("SystemNexusConverterContext.PayloadConverter.ToPayload(value)"));
    assert!(rendered.contains("SystemNexusConverterContext.FailureConverter.ToFailure("));
    assert!(!rendered.contains("CurrentUserPayloadConverter"));
    assert!(
        rendered.contains(
            "internal static ApiCommon.WorkflowType ToWorkflowTypeProto(this string value)"
        )
    );
    assert!(
        rendered
            .contains("internal static ApiTaskQueue.TaskQueue ToTaskQueueProto(this string value)")
    );
    assert!(!rendered.contains("ToProto(default("));
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
    let rendered = generate_dotnet_to_string(&input_paths, &[]);

    assert!(rendered.contains("namespace Temporalio.Workflows\n{"));
    assert!(rendered.contains("public static partial class Workflow"));
    assert!(!rendered.contains("WorkflowServiceOperations"));
    fs::remove_dir_all(temp_dir).unwrap();
}
