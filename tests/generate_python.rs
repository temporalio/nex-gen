use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use heck::ToSnakeCase;
use nex_gen::generate_to_string_with_inputs;
use nex_gen::generator::{GeneratedOutputLayout, generate_files};

const PRIMARY_EXAMPLE_ID: &str = "workflow-service";
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

fn python_root(root: &Path) -> PathBuf {
    root.join("examples/python")
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

fn python_output_path(root: &Path, example_id: &str) -> PathBuf {
    python_root(root).join(example_id.to_snake_case())
}

fn python_example_ids(root: &Path) -> Vec<String> {
    let python_root = python_root(root);
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
            if python_root.join(example_id.to_snake_case()).is_dir() {
                Some(example_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn read_python_package_files(dir: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("py") {
                if path
                    .file_name()
                    .and_then(|file_name| file_name.to_str())
                    .is_some_and(|file_name| file_name.starts_with("test_"))
                {
                    continue;
                }
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

fn generate_formatted_python_output(root: &Path, example_id: &str, output_path: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "python",
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

    let format_status = Command::new("uv")
        .current_dir(python_root(root))
        .args([
            "run",
            "ruff",
            "format",
            "--line-length",
            "88",
            "--config",
            "pyproject.toml",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(format_status.success());
}

fn assert_python_310_syntax_compatible(package_dir: &Path) {
    let checker = r#"
import ast
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
for path in sorted(root.rglob("*.py")):
    source = path.read_text()
    try:
        ast.parse(source, filename=str(path), feature_version=(3, 10))
    except SyntaxError as exc:
        print(f"{path}: {exc}")
        raise
"#;
    let status = Command::new(
        project_root()
            .join("examples/python/.venv/bin/python")
            .to_str()
            .unwrap(),
    )
    .args(["-c", checker, package_dir.to_str().unwrap()])
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

#[test]
fn python_examples_generation_matches_checked_in_output() {
    let root = project_root();
    for example_id in python_example_ids(&root) {
        let output_path = unique_output_path(&format!("python-{example_id}"));
        generate_formatted_python_output(&root, &example_id, &output_path);
        assert_python_310_syntax_compatible(&output_path);
        let rendered = read_python_package_files(&output_path);
        let expected = read_python_package_files(&python_output_path(&root, &example_id));
        assert_eq!(rendered, expected, "snapshot mismatch for {example_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
}

#[test]
fn cli_generates_wit_direct_example_without_descriptors() {
    let root = project_root();
    let output_path = unique_output_path("python-user-service-no-descriptors");
    let output = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "python",
            "--input",
            input_path(&root, "user-service").to_str().unwrap(),
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
    assert!(output_path.join("__init__.py").is_file());
    assert!(output_path.join("models.py").is_file());
    fs::remove_dir_all(output_path).unwrap();
}

#[test]
fn python_example_suite_type_checks_and_runs() {
    let root = project_root();
    let example_dir = python_root(&root);

    let typecheck_status = Command::new("uv")
        .current_dir(&example_dir)
        .args(["run", "basedpyright"])
        .status()
        .unwrap();
    assert!(typecheck_status.success());

    let pytest_status = Command::new("uv")
        .current_dir(&example_dir)
        .args(["run", "pytest"])
        .status()
        .unwrap();
    assert!(pytest_status.success());
}

#[test]
fn python_request_models_are_write_only() {
    let root = project_root();
    let spec = nex_gen::spec::ApiSpec::load_for_language_with_inputs(
        nex_gen::language::Language::Python,
        &example_input_paths(&root, PRIMARY_EXAMPLE_ID),
    )
    .unwrap();
    let descriptors = nex_gen::descriptors::DescriptorIndex::load(&descriptor_path(&root)).unwrap();
    let generated = generate_files(
        nex_gen::language::Language::Python,
        &spec,
        &descriptors,
        &nex_gen::SupportFiles::default(),
    )
    .unwrap();
    assert_eq!(generated.layout, GeneratedOutputLayout::Directory);
    let models = generated
        .files
        .get(&PathBuf::from("models.py"))
        .expect("Python package should include models.py");
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Python,
        &example_input_paths(&root, PRIMARY_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(!rendered.contains("SignalWithStartWorkflowRequest.from_proto"));
    assert!(!rendered.contains(
        "proto: temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest,\n    ) -> SignalWithStartWorkflowRequest:"
    ));
    assert!(rendered.contains("class SignalWithStartWorkflowRequest:"));
    assert!(rendered.contains(
        "@dataclasses.dataclass(slots=True, kw_only=True)\nclass SignalWithStartWorkflowRequest:\n    \"\"\"\n    .. warning::\n        This API is experimental and subject to change.\n    \"\"\"\n    workflow: str | collections.abc.Callable[..., collections.abc.Awaitable[typing.Any]]\n    args: list[typing.Any] | None = None\n    id: str\n    task_queue: str\n    signal: str | collections.abc.Callable[..., None | collections.abc.Awaitable[None]]\n    signal_args: list[typing.Any] | None = None\n    execution_timeout: datetime.timedelta | None = None"
    ));
    assert!(rendered.contains("temporalio.common.WorkflowIDReusePolicy.ALLOW_DUPLICATE"));
    assert!(rendered.contains("args: list[typing.Any] | None = None"));
    assert!(rendered.contains("signal_args: list[typing.Any] | None = None"));
    assert!(!rendered.contains("args: tuple[typing.Any, ...] | None = None"));
    assert!(!rendered.contains("signal_args: tuple[typing.Any, ...] | None = None"));
    assert!(!rendered.contains("(typing.TypedDict, total=False):"));
    assert!(!rendered.contains("typing.Unpack["));
    assert!(!rendered.contains("namespace: str | None = None"));
    assert!(!rendered.contains("namespace: str | None"));
    assert!(rendered.contains("message.namespace = workflow_namespace()"));
    assert!(rendered.contains("result = await handle"));
    assert!(rendered.contains("return temporalio.workflow.get_external_workflow_handle("));
    assert!(rendered.contains("run_id=result.run_id"));
    assert!(rendered.contains("async def _signal_with_start_workflow("));
    assert!(rendered.contains("request: SignalWithStartWorkflowRequest"));
    assert!(rendered.contains(") -> temporalio.workflow.ExternalWorkflowHandle[typing.Any]:"));
    assert!(rendered.contains("async def signal_with_start_workflow("));
    assert!(rendered.contains("@typing.overload"));
    assert!(rendered.contains("workflow: str,"));
    assert!(rendered.contains("*positional_args: object,"));
    assert!(rendered.contains("id: str,"));
    assert!(rendered.contains("args: list[typing.Any] | None = ...,"));
    assert!(rendered.contains(
        "workflow: collections.abc.Callable[[typing.Any, typing_extensions.Unpack[WorkflowArgs]], collections.abc.Awaitable[WorkflowResult]],"
    ));
    assert!(rendered.contains("WorkflowArgs = typing_extensions.TypeVarTuple(\"WorkflowArgs\")"));
    assert!(rendered.contains("WorkflowResult = typing.TypeVar(\"WorkflowResult\")"));
    assert!(rendered.contains(") -> temporalio.workflow.ExternalWorkflowHandle[WorkflowResult]:"));
    assert!(rendered.contains("*positional_args: typing_extensions.Unpack[WorkflowArgs],"));
    assert!(rendered.contains("args: list[typing.Any],"));
    assert!(!rendered.contains("tuple[FirstWorkflowArg"));
    assert!(rendered.contains(
        "signal: collections.abc.Callable[[typing.Any, SignalArg], None | collections.abc.Awaitable[None]],"
    ));
    assert!(rendered.contains("SignalArg = typing.TypeVar(\"SignalArg\")"));
    assert!(rendered.contains("signal_args: SignalArg,"));
    assert!(rendered.contains("signal_args: list[typing.Any],"));
    assert!(rendered.contains(
        "async def signal_with_start_workflow(\n    workflow: str | collections.abc.Callable[..., collections.abc.Awaitable[typing.Any]],\n    *positional_args: object,\n    args: list[typing.Any] | None = None,\n    id: str,\n    task_queue: str,\n    signal: str | collections.abc.Callable[..., None | collections.abc.Awaitable[None]],\n    signal_args: object | list[typing.Any] | None = None,"
    ));
    assert!(rendered.contains(
        "signal: str | collections.abc.Callable[..., None | collections.abc.Awaitable[None]],"
    ));
    assert!(rendered.contains("args: list[typing.Any] | None = None,"));
    assert_eq!(
        rendered
            .matches("Signal a workflow, starting it first if needed.")
            .count(),
        1
    );
    assert!(rendered.contains(
        "\"\"\"Signal a workflow, starting it first if needed.\n\n    .. warning::\n        This API is experimental and subject to change.\n\n    Args:\n        workflow: Workflow type name or callable identifying the workflow to start.\n        positional_args: Positional arguments for workflow. Cannot be set if args is\n            set.\n        args: List-form arguments for workflow. Cannot be set if positional_args are\n            set. For typed workflow callables, list contents are not statically\n            typechecked; pass workflow arguments positionally for precise typechecking.\n        id: Unique identifier for the workflow execution.\n        task_queue: Task queue to run the workflow on.\n        signal: Signal name or callable to send with the start request.\n        signal_args: Argument value, or list of argument values, for signal. For typed\n            single-argument signals, scalar signal_args values are statically\n            typechecked. List-form signal_args values are not precisely typechecked. To\n            pass a single signal argument that is itself a list, wrap it in another\n            list; otherwise the list is interpreted as multiple signal arguments."
    ));
    assert!(rendered.contains(
        "cron_schedule: Cron schedule for recurring workflow executions. See\n            https://docs.temporal.io/cron-job."
    ));
    assert!(rendered.contains(
        "static_summary: Single-line fixed summary for the workflow execution that may\n            appear in UI and CLI. This can be in single-line Temporal Markdown format."
    ));
    assert!(rendered.contains(
        "\n\n    Returns:\n        A workflow handle to the started workflow.\n    \"\"\""
    ));
    assert!(rendered.contains(
        "id_reuse_policy: temporalio.common.WorkflowIDReusePolicy = (\n        temporalio.common.WorkflowIDReusePolicy.ALLOW_DUPLICATE\n    ),"
    ));
    assert!(
        rendered.contains(
            "id_conflict_policy: temporalio.common.WorkflowIDConflictPolicy | None = None,"
        )
    );
    assert!(rendered.contains("static_summary: str | None = None,"));
    assert!(rendered.contains("static_details: str | None = None,"));
    assert!(!rendered.contains("identity: str | None"));
    assert!(!rendered.contains("user_metadata_static_summary:"));
    assert!(!rendered.contains("user_metadata_static_details:"));
    assert!(rendered.contains("request = SignalWithStartWorkflowRequest("));
    assert!(rendered.contains("workflow=workflow,"));
    assert!(!rendered.contains("def _nexus_is_function_args_list("));
    assert!(!rendered.contains("def _nexus_normalize_function_args("));
    assert!(!rendered.contains("_nexus_arg_unset = object()"));
    assert!(rendered.contains("if positional_args and args is not None:"));
    assert!(
        rendered.contains("raise TypeError(\"cannot specify both positional arguments and args\")")
    );
    assert!(rendered.contains("normalized_args: list[typing.Any] | None = ("));
    assert!(rendered.contains("list(positional_args)"));
    assert!(rendered.contains("else args"));
    assert!(rendered.contains(
        "normalized_signal_args: list[typing.Any] | None\n    if signal_args is None:\n        normalized_signal_args = None\n    elif isinstance(signal_args, list):\n        normalized_signal_args = typing.cast(list[typing.Any], signal_args)\n    else:\n        normalized_signal_args = [signal_args]"
    ));
    assert!(rendered.contains("user_metadata = ("));
    assert!(rendered.contains("if static_summary is None and static_details is None"));
    assert!(rendered.contains("static_summary=static_summary,"));
    assert!(rendered.contains("static_details=static_details,"));
    assert!(rendered.contains("args=normalized_args,"));
    assert!(rendered.contains("id=id,"));
    assert!(rendered.contains("signal_args=normalized_signal_args,"));
    assert!(rendered.contains("user_metadata=user_metadata,"));
    assert!(rendered.contains("return await _signal_with_start_workflow(request)"));
    assert!(rendered.contains("message.input.CopyFrom(payloads_to_proto(self.args))"));
    assert!(models.contains("from ._support import ("));
    assert!(models.contains("retry_policy_to_proto,"));

    let type_roundtrip_rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Python,
        &example_input_paths(&root, TYPE_ROUNDTRIP_EXAMPLE_ID),
        &[descriptor_path(&root)],
    )
    .unwrap();
    assert!(type_roundtrip_rendered.contains("async def activity_options_operation("));
    assert!(type_roundtrip_rendered.contains("task_queue: str | None = None,"));
    assert!(type_roundtrip_rendered.contains("retry_policy: temporalio.common.RetryPolicy,"));
    assert!(type_roundtrip_rendered.contains("request = ActivityOptions("));
}
