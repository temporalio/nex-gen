use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nex_gen::generate_to_string_with_inputs;

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

fn go_root(root: &Path) -> PathBuf {
    root.join("examples/go")
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

fn go_package_name(example_id: &str) -> String {
    example_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

fn go_output_path(root: &Path, example_id: &str) -> PathBuf {
    go_root(root).join(go_package_name(example_id))
}

fn go_example_ids(root: &Path) -> Vec<String> {
    let go_root = go_root(root);
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
            if go_root.join(go_package_name(&example_id)).is_dir() {
                Some(example_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn read_go_output_files(dir: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("go") {
                if path
                    .file_name()
                    .and_then(|file_name| file_name.to_str())
                    .is_some_and(|file_name| file_name.ends_with("_test.go"))
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

fn generate_formatted_go_output(root: &Path, example_id: &str, output_path: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "go",
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

    let format_status = Command::new("gofmt")
        .args(["-w", output_path.to_str().unwrap()])
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
fn cli_generates_go_support_file_from_parameter() {
    let root = project_root();
    let temp_dir = unique_output_path("go-support-file-input");
    fs::create_dir_all(&temp_dir).unwrap();
    let support_path = temp_dir.join("custom_support.go");
    let output_path = temp_dir.join("output");
    fs::write(
        &support_path,
        "package placeholder\n\nfunc CustomSupportHook() string {\n\treturn \"custom\"\n}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "go",
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
    // The explicit support file is emitted even though the WIT-direct
    // user-service package performs no proto conversion, and its package
    // declaration is rewritten to the generated package name.
    let support_contents = fs::read_to_string(output_path.join("custom_support.go")).unwrap();
    assert!(support_contents.starts_with("package userservice\n"));
    assert!(support_contents.contains("func CustomSupportHook() string"));
    assert!(output_path.join("userservice.go").is_file());
    assert!(!output_path.join("api.go").exists());
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn cli_generates_go_with_package_self_imports_removed() {
    let root = project_root();
    let temp_dir = unique_output_path("go-namespace");
    let output_path = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).unwrap();
    let temp_input_path = temp_dir.join("user-service.wit");
    let input = fs::read_to_string(input_path(&root, "user-service"))
        .unwrap()
        .replace(
            "/// @nexus.endpoint \"user-service\"\n",
            "/// @nexus.endpoint \"user-service\"\n/// @nexus.namespace go=\"go.temporal.io/sdk/workflow\"\n",
        );
    fs::write(&temp_input_path, input).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nex-gen"))
        .args([
            "generate",
            "--lang",
            "go",
            "--input",
            temp_input_path.to_str().unwrap(),
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
    let api = fs::read_to_string(output_path.join("userservice.go")).unwrap();
    assert!(api.contains("package workflow\n"));
    assert!(!api.contains("\"go.temporal.io/sdk/workflow\""));
    assert!(api.contains("func getUser(ctx Context, request getUserRequest) NexusOperationFuture"));
    assert!(api.contains("c := NewNexusClient(\"user-service\", \"UserService\")"));
    assert!(!api.contains("const ServiceName"));
    assert!(!api.contains("const Endpoint"));
    assert!(!api.contains("GetUserOp"));
    assert!(api.contains("NexusOperationOptions{}"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_sourced_map_field_converts_to_proto() {
    let root = project_root();
    let temp_dir = unique_output_path("go-sourced-map-input");
    fs::create_dir_all(&temp_dir).unwrap();
    let wit_path = temp_dir.join("sourced-map.wit");
    fs::write(
        &wit_path,
        r#"package temporal:sourced-map@1.0.0;

world system {
  export namespace-service;
}

/// @nexus.endpoint "namespace-service"
interface namespace-service {
  /// String-shaped placeholder for omitted fields.
  type placeholder = string;

  /// @nexus.proto "temporal.api.namespace.v1.NamespaceInfo"
  record namespace-info {
    name: option<string>,
    /// @nexus.source go="NamespaceData"
    data: option<map<string, string>>,
    /// @nexus.omit
    state: placeholder,
    /// @nexus.omit
    description: placeholder,
    /// @nexus.omit
    owner-email: placeholder,
    /// @nexus.omit
    id: placeholder,
    /// @nexus.omit
    capabilities: placeholder,
    /// @nexus.omit
    limits: placeholder,
    /// @nexus.omit
    supports-schedules: placeholder,
  }

  describe-namespace: func(request: namespace-info);
}
"#,
    )
    .unwrap();

    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &[wit_path],
        &[descriptor_path(&root)],
    )
    .unwrap();

    // The sourced map is bound to a field-unique local, evaluated once, and
    // copied into a properly typed proto map.
    assert!(rendered.contains("sourcedData := NamespaceData()"));
    assert!(rendered.contains("if len(sourcedData) > 0 {"));
    assert!(rendered.contains("message.Data = make(map[string]string, len(sourcedData))"));
    assert!(rendered.contains("for k, v := range sourcedData {"));
    assert!(rendered.contains("message.Data[k] = v"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_proto_enum_field_infers_descriptor_go_package() {
    let root = project_root();
    let temp_dir = unique_output_path("go-unconvertible-input");
    fs::create_dir_all(&temp_dir).unwrap();
    let wit_path = temp_dir.join("unconvertible.wit");
    // The `namespace-state` alias is type-replaced for Python only. Go should
    // still infer the proto enum import and conversion from descriptor
    // `go_package` metadata.
    fs::write(
        &wit_path,
        r#"package temporal:unconvertible@1.0.0;

world system {
  export namespace-service;
}

/// @nexus.endpoint "namespace-service"
interface namespace-service {
  /// String-shaped placeholder for omitted and replaced fields.
  type placeholder = string;

  /// @nexus.proto "temporal.api.enums.v1.NamespaceState"
  /// @nexus.type python="int"
  type namespace-state = placeholder;

  /// @nexus.proto "temporal.api.namespace.v1.NamespaceInfo"
  record namespace-info {
    name: option<string>,
    state: option<namespace-state>,
    /// @nexus.omit
    description: placeholder,
    /// @nexus.omit
    owner-email: placeholder,
    /// @nexus.omit
    data: placeholder,
    /// @nexus.omit
    id: placeholder,
    /// @nexus.omit
    capabilities: placeholder,
    /// @nexus.omit
    limits: placeholder,
    /// @nexus.omit
    supports-schedules: placeholder,
  }

  describe-namespace: func(request: namespace-info);
}
"#,
    )
    .unwrap();

    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &[wit_path],
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains("namespace \"go.temporal.io/api/namespace/v1\""));
    assert!(rendered.contains("enums \"go.temporal.io/api/enums/v1\""));
    assert!(rendered.contains("type NamespaceState int32"));
    assert!(rendered.contains("State *NamespaceState"));
    assert!(rendered.contains("message.State = enums.NamespaceState((*m.State))"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_output_transform_returns_transformed_type() {
    let temp_dir = unique_output_path("go-output-transform");
    fs::create_dir_all(&temp_dir).unwrap();
    let wit_path = temp_dir.join("go-output-transform.wit");
    fs::write(
        &wit_path,
        r#"package temporal:go-output-transform@1.0.0;

world system {
  export sample-service;
}

/// @nexus.endpoint "sample-service"
interface sample-service {
  record transform-request {
    id: string,
  }

  record transform-response {
    value: string,
  }

  /// @nexus.output-transform
  ///   go-type="example.com/nexgen/handles:handles.ValueHandle"
  ///   go="handles.NewValueHandle(request.Id, result.Value)"
  get-handle: func(request: transform-request) -> transform-response;
}
"#,
    )
    .unwrap();

    let rendered =
        generate_to_string_with_inputs(nex_gen::language::Language::Go, &[wit_path], &[]).unwrap();

    assert!(rendered.contains("\"example.com/nexgen/handles\""));
    assert!(rendered.contains(
        "func getHandle(ctx workflow.Context, request transformRequest) workflow.NexusOperationFuture {"
    ));
    assert!(rendered.contains("return &nexGenNexusOperationFuture{operation: fut, get: func(ctx workflow.Context, valuePtr any) error {"));
    assert!(rendered.contains("\tvar result transformResponse\n"));
    assert!(rendered.contains("\t\tif err := fut.Get(ctx, &result); err != nil {\n"));
    assert!(
        rendered.contains("\t\tvalue, err := handles.NewValueHandle(request.Id, result.Value)\n")
    );
    assert!(rendered.contains("\t\t*typedValue = value\n"));
    assert!(rendered.contains(
        "func GetHandle(ctx workflow.Context, id string) workflow.NexusOperationFuture {"
    ));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_examples_generation_matches_checked_in_output() {
    let root = project_root();
    for example_id in go_example_ids(&root) {
        let output_path = unique_output_path(&format!("go-{example_id}"));
        generate_formatted_go_output(&root, &example_id, &output_path);
        let rendered = read_go_output_files(&output_path);
        let expected = read_go_output_files(&go_output_path(&root, &example_id));
        assert_eq!(rendered, expected, "snapshot mismatch for {example_id}");
        fs::remove_dir_all(output_path).unwrap();
    }
}

#[test]
fn go_function_fields_accept_strings_or_exact_function_pointers() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &example_input_paths(&root, "function-execution"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    let service_rendered = rendered
        .split("### functionexecution.go")
        .nth(1)
        .and_then(|contents| contents.split("### model_overrides.go").next())
        .expect("functionexecution.go should be rendered");
    let support_rendered = rendered
        .split("### model_overrides.go")
        .nth(1)
        .expect("model_overrides.go should be rendered");

    assert!(!service_rendered.contains("\"reflect\""));
    assert!(!service_rendered.contains("\"runtime\""));
    assert!(!service_rendered.contains("\"strings\""));
    assert!(service_rendered.contains("\"errors\""));
    assert!(!service_rendered.contains("func nexGenFunctionName[F any](value F) string"));
    assert!(!service_rendered.contains("type nexGenNexusOperationFuture struct"));
    assert!(support_rendered.contains("\"reflect\""));
    assert!(support_rendered.contains("\"runtime\""));
    assert!(support_rendered.contains("\"strings\""));
    assert!(support_rendered.contains("\"errors\""));
    assert!(support_rendered.contains("func nexGenFunctionName[F any](value F) string"));
    assert!(support_rendered.contains("return strings.TrimSuffix(shortName, \"-fm\")"));
    assert!(support_rendered.contains("type nexGenNexusOperationFuture struct"));
    assert!(support_rendered.contains("func nexGenFailedNexusOperationFuture"));

    // Internal request structs remain wire-shaped.
    assert!(
        rendered
            .contains("type executeFunctionRequest struct {\n\t// Required.\n\tFunction string")
    );
    assert!(
        rendered.contains(
            "type executeNamedFunctionRequest struct {\n\t// Required.\n\tFunction string"
        )
    );

    // Required public function fields accept either a string name or the exact
    // WIT-derived Go function signature.
    assert!(rendered.contains(
        "func ExecuteFunction[FunctionF interface{ ~string | func(string, bool) string }]("
    ));
    assert!(rendered.contains(
        "func ExecuteCountedFunction[FunctionF interface{ ~string | func(string, int32) string }]("
    ));
    assert!(rendered.contains(
        "func ExecuteVarargsFunction[FunctionF interface{ ~string | func(...string) string }]"
    ));
    assert!(rendered.contains("\tfunction FunctionF,\n"));
    assert!(rendered.contains("\t\tFunction: nexGenFunctionName(function),\n"));
    assert!(!rendered.contains("func ExecuteFunctionWithArgs"));
    assert!(!rendered.contains("func ExecuteCountedFunctionWithArgs"));
    assert!(!rendered.contains("func ExecuteNamedFunctionWithArgs"));

    // Optional function-adjacent args stay in the options struct with their
    // normal wire-compatible type.
    assert!(rendered.contains("type ExecuteVarargsFunctionOptions struct {\n\t// Arguments for the function.\n\tArgs []string\n}"));
    assert!(rendered.contains(
        "func ExecuteVarargsFunction[FunctionF interface{ ~string | func(...string) string }](ctx workflow.Context, function FunctionF, opts ExecuteVarargsFunctionOptions) workflow.NexusOperationFuture {"
    ));
    assert!(rendered.contains("\t\tArgs: opts.Args,\n"));

    // Primary varargs function fields also get a trailing-args convenience
    // wrapper that matches Python's positional/list-form conflict behavior.
    assert!(rendered.contains(
        "func ExecuteVarargsFunctionWithArgs[FunctionF interface{ ~string | func(...string) string }]("
    ));
    assert!(rendered.contains(
        "func ExecuteNamedVarargsFunctionWithArgs[FunctionF interface{ ~string | func(...string) string }]("
    ));
    assert!(rendered.contains("\targs ...string,\n"));
    assert!(rendered.contains(
        "\tif len(args) > 0 && opts.Args != nil {\n\t\treturn nexGenFailedNexusOperationFuture(ctx, errors.New(\"cannot specify both positional arguments and args\"))\n\t}\n"
    ));
    assert!(rendered.contains("\tif len(args) == 0 {\n\t\targs = opts.Args\n\t}\n"));
    assert!(rendered.contains("\t\tArgs: args,\n"));
    assert!(rendered.contains(
        "// Input name: The name argument for the function.\n// Input enabled: The enabled argument for the function.\nfunc ExecuteFunction"
    ));
    assert!(rendered.contains(
        "// Input args: Arguments for the function.\nfunc ExecuteVarargsFunctionWithArgs"
    ));

    let user_rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &[input_path(&root, "user-service")],
        &[],
    )
    .unwrap();
    assert!(!user_rendered.contains("nexGenFunctionName"));
    assert!(!user_rendered.contains("\"reflect\""));
    assert!(!user_rendered.contains("\"runtime\""));
    assert!(!user_rendered.contains("\"strings\""));
    assert!(!user_rendered.contains("\"errors\""));
}

#[test]
fn go_temporal_function_constraints_use_workflow_context_prefix() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &example_input_paths(&root, "workflow-service"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains(
        "func SignalWithStartWorkflow[WorkflowF interface{ ~string | func(workflow.Context, ...any) any }, SignalF interface{ ~string | func(workflow.Context, ...any) any }]("
    ));
    assert!(rendered.contains(
        "func SignalWithStartWorkflowWithArgs[WorkflowF interface{ ~string | func(workflow.Context, ...any) any }, SignalF interface{ ~string | func(workflow.Context, ...any) any }]("
    ));
    assert!(rendered.contains("\tworkflow WorkflowF,\n"));
    assert!(rendered.contains("\tsignal SignalF,\n"));
    assert!(rendered.contains("\targs ...any,\n"));
    assert!(rendered.contains("\t\tWorkflow: nexGenFunctionName(workflow),\n"));
    assert!(rendered.contains("\t\tSignal: nexGenFunctionName(signal),\n"));
    assert!(rendered.contains("\t\tArgs: args,\n"));
    assert!(rendered.contains("\t\tSignalArgs: opts.SignalArgs,\n"));
}

#[test]
fn go_type_showcase_generates_expected_types() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &example_input_paths(&root, "type-showcase"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    // Service and operation names are inlined at call sites, not exported as constants.
    assert!(!rendered.contains("const ServiceName"));
    assert!(!rendered.contains("const Endpoint"));
    assert!(!rendered.contains("const GetUserOp"));
    assert!(!rendered.contains("const UpdateEmailOp"));
    assert!(!rendered.contains("const RenameOp"));
    assert!(!rendered.contains("const SetProfileOp"));
    assert!(!rendered.contains("const DeactivateOp"));

    // Enums
    assert!(rendered.contains("type UserStatus int32"));
    assert!(rendered.contains("UserStatusActive"));
    assert!(rendered.contains("UserStatusSuspended"));
    assert!(rendered.contains("UserStatusDeleted"));

    // Flags
    assert!(rendered.contains("type UserCapability int32"));
    assert!(rendered.contains("UserCapabilityReadProfile"));
    assert!(rendered.contains("1 << 0"));
    assert!(rendered.contains("1 << 1"));
    assert!(rendered.contains("1 << 2"));

    // Variants -- sealed interface pattern
    assert!(rendered.contains("type NotificationTarget interface {"));
    assert!(rendered.contains("isNotificationTarget()"));
    // Case structs with payload
    assert!(rendered.contains("type NotificationTargetEmail struct {"));
    assert!(rendered.contains("Value string"));
    assert!(rendered.contains("func (NotificationTargetEmail) isNotificationTarget() {}"));
    assert!(rendered.contains("type NotificationTargetSms struct {"));
    assert!(rendered.contains("func (NotificationTargetSms) isNotificationTarget() {}"));
    // Payload-less case struct
    assert!(rendered.contains("type NotificationTargetNone struct{}"));
    assert!(rendered.contains("func (NotificationTargetNone) isNotificationTarget() {}"));

    // Records with required/optional fields
    assert!(rendered.contains("type getUserRequest struct"));
    assert!(!rendered.contains("type GetUserRequest struct"));
    assert!(rendered.contains("\t// Required.\n\tUserId string"));
    // Optional scalar fields are rendered as pointers so absence is
    // representable as nil (distinct from a present zero value).
    assert!(rendered.contains("ConsistencyToken *string"));

    assert!(rendered.contains("type PostalAddress struct"));
    assert!(rendered.contains("\t// Required.\n\tStreet string"));
    assert!(rendered.contains("\t// Required.\n\tCity string"));
    assert!(rendered.contains("\t// Required.\n\tCountry string"));
    // Tuple field generates a named struct with ordinal fields
    assert!(rendered.contains("Coordinates *Coordinates"));
    assert!(rendered.contains("type Coordinates struct"));
    assert!(rendered.contains("\t// Required.\n\tFirst float64"));
    assert!(rendered.contains("\t// Required.\n\tSecond float64"));

    assert!(rendered.contains("type UserProfile struct"));
    assert!(rendered.contains("\t// Required.\n\tTags []string"));
    assert!(rendered.contains("\t// Required.\n\tMetadata map[string]string"));
    assert!(rendered.contains("\t// Required.\n\tCapabilities UserCapability"));
    // Result field generates a named struct
    assert!(rendered.contains("\t// Required.\n\tSyncState SyncState"));
    assert!(rendered.contains("type SyncState struct"));
    assert!(rendered.contains("\t// Required.\n\tResult string"));
    assert!(rendered.contains("\t// Required.\n\tError string"));
    // Variant interface field
    assert!(rendered.contains("\t// Required.\n\tNotificationTarget NotificationTarget"));
    // Optional struct field keeps pointer and is not marked required.
    assert!(rendered.contains("Address *PostalAddress"));
    assert!(!rendered.contains("\t// Required.\n\tAddress *PostalAddress"));

    assert!(rendered.contains("type deactivateRequest struct"));
    assert!(!rendered.contains("type DeactivateRequest struct"));
    assert!(rendered.contains("\t// Required.\n\tUserId string"));
    // Optional scalar -- pointer so absence is representable as nil.
    assert!(rendered.contains("Reason *string"));

    // Tuples and results inside containers instantiate shared generic helper
    // types instead of field-named structs.
    assert!(rendered.contains("type SyncReport struct"));
    assert!(rendered.contains("\t// Required.\n\tRoute []Tuple2[float64, float64]"));
    assert!(rendered.contains("\t// Required.\n\tAttempts []Result[string, string]"));
    assert!(rendered.contains("\t// Required.\n\tRegionStatus map[string]Result[string, string]"));
    assert!(rendered.contains("type Tuple2[T1, T2 any] struct {"));
    assert!(rendered.contains("First T1"));
    assert!(rendered.contains("Second T2"));
    assert!(rendered.contains("type Result[T, E any] struct {"));
    assert!(rendered.contains("Result T"));
    assert!(rendered.contains("Error E"));

    // Resource struct
    assert!(rendered.contains("type User struct"));
    assert!(rendered.contains("\t// Required.\n\tDisplayName string"));
    assert!(rendered.contains("\t// Required.\n\tStatus UserStatus"));
    assert!(rendered.contains("\t// Required.\n\tProfile UserProfile"));

    // Resource methods
    assert!(
        rendered.contains(
            "func (u *User) UpdateEmail(ctx workflow.Context, email string) workflow.NexusOperationFuture"
        )
    );
    assert!(rendered.contains("updateEmailRequest{UserId: u.UserId, Email: email}"));
    assert!(rendered.contains(
        "func (u *User) Rename(ctx workflow.Context, displayName string) workflow.NexusOperationFuture"
    ));
    assert!(rendered.contains("renameRequest{UserId: u.UserId, DisplayName: displayName}"));
    // Void resource method -- optional param is a pointer.
    assert!(
        rendered.contains("func (u *User) Deactivate(ctx workflow.Context, reason *string) workflow.NexusOperationFuture")
    );
    assert!(rendered.contains("deactivateRequest{UserId: u.UserId, Reason: reason}"));

    // Unexported operation wrapper functions
    assert!(rendered.contains(
        "func getUser(ctx workflow.Context, request getUserRequest) workflow.NexusOperationFuture"
    ));
    assert!(rendered.contains(
        "func updateEmail(ctx workflow.Context, request updateEmailRequest) workflow.NexusOperationFuture"
    ));
    assert!(rendered.contains("workflow.NewNexusClient(\"type-showcase\", \"TypeShowcase\")"));
    assert!(rendered.contains(
        "c.ExecuteOperation(ctx, \"GetUser\", request, workflow.NexusOperationOptions{})"
    ));
    // Void operation
    assert!(
        rendered.contains("func deactivate(ctx workflow.Context, request deactivateRequest) workflow.NexusOperationFuture")
    );
    assert!(rendered.contains("\treturn fut\n"));

    // Exported convenience wrappers -- all required fields become positional args
    assert!(rendered.contains(
        "func UpdateEmail(ctx workflow.Context, userId string, email string) workflow.NexusOperationFuture"
    ));
    // The request struct is always constructed across multiple lines.
    assert!(rendered.contains("updateEmailRequest{\n\t\tUserId: userId,\n\t\tEmail: email,\n\t}"));
    // Optional fields produce an options struct (pointer-typed)
    assert!(rendered.contains("type GetUserOptions struct"));
    assert!(rendered.contains("ConsistencyToken *string"));
    assert!(rendered.contains(
        "func GetUser(ctx workflow.Context, userId string, opts GetUserOptions) workflow.NexusOperationFuture"
    ));
    assert!(rendered.contains(
        "getUserRequest{\n\t\tUserId: userId,\n\t\tConsistencyToken: opts.ConsistencyToken,\n\t}"
    ));
    // Void convenience wrapper with options
    assert!(rendered.contains("type DeactivateOptions struct"));
    assert!(rendered.contains(
        "func Deactivate(ctx workflow.Context, userId string, opts DeactivateOptions) workflow.NexusOperationFuture"
    ));
}

#[test]
fn go_type_roundtrip_generates_proto_conversions() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &example_input_paths(&root, "type-roundtrip"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    // Aliased proto imports derived from the descriptors' `go_package` option.
    assert!(rendered.contains("activity \"go.temporal.io/api/activity/v1\""));
    assert!(rendered.contains("common \"go.temporal.io/api/common/v1\""));

    // Optional override-typed fields are rendered as pointers; the required
    // retry-policy field stays a value. (Assertions use the un-gofmt'd output,
    // so fields are single-space separated.)
    assert!(rendered.contains("TaskQueue *string"));
    assert!(rendered.contains("\t// Required.\n\tRetryPolicy temporal.RetryPolicy"));
    assert!(rendered.contains("ScheduleToCloseTimeout *time.Duration"));
    assert!(rendered.contains("Priority *temporal.Priority"));

    // Generated model gets a context-aware ToProto method targeting the proto
    // message type and returning conversion errors.
    assert!(rendered
        .contains("func (m ActivityOptions) toProto(ctx workflow.Context) (*activity.ActivityOptions, error) {"));
    assert!(rendered.contains("message := &activity.ActivityOptions{}"));
    // Optional override fields pass the pointer straight to the nil-safe
    // converter; the required field passes its address.
    assert!(rendered.contains("converted, err := retryPolicyToProto(ctx, &m.RetryPolicy)"));
    assert!(rendered.contains("converted, err := taskQueueToProto(ctx, m.TaskQueue)"));
    assert!(rendered.contains("converted, err := priorityToProto(ctx, m.Priority)"));
    assert!(rendered.contains("converted, err := durationToProto(ctx, m.ScheduleToCloseTimeout)"));
    assert!(rendered.contains("return message, nil"));

    // Generated model gets a context-aware FromProto constructor. Optional
    // override fields assign the converter's pointer result directly; the
    // required field is dereferenced with a nil guard.
    assert!(rendered.contains(
        "func activityOptionsFromProto(ctx workflow.Context, proto *activity.ActivityOptions) (ActivityOptions, error) {"
    ));
    assert!(rendered.contains("converted, err := taskQueueFromProto(ctx, proto.GetTaskQueue())"));
    assert!(
        rendered.contains("converted, err := retryPolicyFromProto(ctx, proto.GetRetryPolicy())")
    );
    assert!(rendered.contains("value.RetryPolicy = *converted"));
    assert!(rendered.contains("\t\t*typedValue = *value\n"));

    // Operation functions convert the request to proto before the SDK call and
    // decode the proto response afterwards.
    assert!(rendered.contains("requestProto, err := request.toProto(ctx)"));
    assert!(rendered.contains(
        "fut := c.ExecuteOperation(ctx, \"ActivityOptionsOperation\", requestProto, workflow.NexusOperationOptions{})"
    ));
    assert!(rendered.contains("var result activity.ActivityOptions"));
    assert!(rendered.contains("value, err := activityOptionsFromProto(ctx, &result)"));

    // Replacement-typed operations (request is a native SDK value) convert via
    // the hand-written converter (passing the address) and return the proto
    // response converted to a pointer directly.
    assert!(rendered.contains("requestProto, err := retryPolicyToProto(ctx, &request)"));
    assert!(rendered.contains(
        "fut := c.ExecuteOperation(ctx, \"RetryPolicyOperation\", requestProto, workflow.NexusOperationOptions{})"
    ));
    assert!(rendered.contains("var result common.RetryPolicy"));
    assert!(rendered.contains("value, err := retryPolicyFromProto(ctx, &result)"));
    assert!(rendered.contains("return value, nil"));

    // The hand-written support fragment is emitted alongside the generated
    // service file with the pointer-in/pointer-out converter contract.
    assert!(rendered.contains("### model_overrides.go"));
    assert!(
        rendered.contains("func retryPolicyToProto(_ workflow.Context, p *temporal.RetryPolicy) (*common.RetryPolicy, error) {")
    );
    assert!(
        rendered
            .contains("func retryPolicyFromProto(_ workflow.Context, p *common.RetryPolicy) (*temporal.RetryPolicy, error) {")
    );
}

#[test]
fn go_proto_resource_return_converts_request_and_constructs_resource() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &example_input_paths(&root, "start-workflow"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains("\trequestProto, err := request.toProto(ctx)\n"));
    assert!(rendered.contains(
        "\tif err != nil {\n\t\treturn nexGenFailedNexusOperationFuture(ctx, err)\n\t}\n"
    ));
    assert!(rendered.contains(
        "fut := c.ExecuteOperation(ctx, \"StartWorkflow\", requestProto, workflow.NexusOperationOptions{})"
    ));
    assert!(rendered.contains("\tvar result workflowservice.StartWorkflowExecutionResponse\n"));
    assert!(rendered.contains("\tnamespace := requestProto.GetNamespace()\n"));
    assert!(rendered.contains("\trunIdValue := result.GetRunId()\n"));
    assert!(rendered.contains("\tvar runId *string\n"));
    assert!(rendered.contains("\t\tNamespace: namespace,\n"));
    assert!(rendered.contains("\t\tWorkflowId: request.WorkflowId,\n"));
    assert!(rendered.contains("\t\tRunId: runId,\n"));
    assert!(rendered.contains(
        "fut := c.ExecuteOperation(ctx, \"RestartWorkflow\", requestProto, workflow.NexusOperationOptions{})"
    ));
    assert!(rendered.contains(
        "func StartWorkflowWithArgs[WorkflowF interface{ ~string | func(workflow.Context, ...any) any }]("
    ));
    assert!(rendered.contains(
        "func RestartWorkflowWithArgs[WorkflowF interface{ ~string | func(workflow.Context, ...any) any }]("
    ));
    assert!(rendered.contains("\tif len(args) > 0 && opts.Args != nil {\n\t\treturn nexGenFailedNexusOperationFuture(ctx, errors.New(\"cannot specify both positional arguments and args\"))\n\t}\n"));
    assert!(rendered.contains("\tif len(args) == 0 {\n\t\targs = opts.Args\n\t}\n"));
    assert!(rendered.contains("\t\tArgs: args,\n"));
}

#[test]
fn go_resource_return_binding_handles_optional_proto_scalars() {
    let root = project_root();
    let temp_dir = unique_output_path("go-resource-return-scalars");
    fs::create_dir_all(&temp_dir).unwrap();
    let wit_path = temp_dir.join("resource-return-scalars.wit");
    fs::write(
        &wit_path,
        r#"package temporal:resource-return-scalars@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "temporal-system"
interface workflow-service {
  type placeholder = string;

  resource signal-result {
    constructor(namespace: string, started: option<bool>);
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.RequestCancelWorkflowExecutionRequest"
  record cancel-request {
    /// @nexus.source go="workflowNamespace"
    namespace: string,
    /// @nexus.omit
    workflow-execution: placeholder,
    /// @nexus.omit
    reason: placeholder,
    /// @nexus.omit
    identity: placeholder,
    /// @nexus.omit
    request-id: placeholder,
    /// @nexus.omit
    first-execution-run-id: placeholder,
    /// @nexus.omit
    links: placeholder,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse"
  type signal-result-response = own<signal-result>;

  signal-with-start: func(request: cancel-request) -> signal-result-response;
}
"#,
    )
    .unwrap();

    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &[wit_path],
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains("\tnamespace := requestProto.GetNamespace()\n"));
    assert!(rendered.contains("\tstartedValue := result.GetStarted()\n"));
    assert!(rendered.contains("\tvar started *bool\n"));
    assert!(rendered.contains("\tif startedValue != false {\n"));
    assert!(rendered.contains("\t\tStarted: started,\n"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_flatten_in_api_embeds_options_value() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nex_gen::language::Language::Go,
        &example_input_paths(&root, "workflow-service"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    assert!(rendered.contains("type SignalWithStartWorkflowOptions struct {"));
    assert!(rendered.contains("\tUserMetadata\n}\n\n// Signal a workflow"));
    assert!(!rendered.contains("\tUserMetadata *UserMetadata\n}\n\n// Signal a workflow"));
    assert!(rendered.contains("\t\tUserMetadata: &opts.UserMetadata,\n"));
    assert!(rendered.contains("\tStaticSummary any"));
    assert!(rendered.contains("\tStaticDetails any"));
}

#[test]
fn go_doc_directives_render_godoc_comments() {
    let temp_dir = unique_output_path("go-doc-directives-input");
    fs::create_dir_all(&temp_dir).unwrap();
    let wit_path = temp_dir.join("doc-service.wit");
    fs::write(
        &wit_path,
        r#"package temporal:doc-demo@1.0.0;

world system {
  export doc-service;
}

/// @nexus.endpoint "doc-service"
interface doc-service {
  record greet-request {
    /// @nexus.doc "Name of the person to greet."
    name: string,
    /// @nexus.doc "Default greeting doc." go="Go-specific greeting doc."
    greeting: option<string>,
    /// @nexus.doc python="Python-only field doc."
    locale: option<string>,
    /// @nexus.doc "A very long field doc that has to be wrapped because it exceeds the generated comment line width by a comfortable margin for testing."
    salutation: option<string>,
  }

  record greet-response {
    message: string,
  }

  /// @nexus.doc
  ///   "Greets the given person."
  ///   returns="The rendered greeting."
  greet: func(request: greet-request) -> greet-response;
}
"#,
    )
    .unwrap();

    let rendered =
        generate_to_string_with_inputs(nex_gen::language::Language::Go, &[wit_path], &[]).unwrap();

    // Field docs become godoc comments above the request struct fields.
    // Required fields fold a `Required.` prefix into the doc comment.
    // (Assertions use the un-gofmt'd output, so fields are single-space
    // separated.)
    assert!(rendered.contains("\t// Required. Name of the person to greet.\n\tName string"));

    // Required fields without any doc text get a bare `// Required.` comment.
    assert!(rendered.contains("\t// Required.\n\tMessage string"));

    // The `go=` override wins over the default text, on both the request
    // struct and the options struct; the default-only doc falls through.
    assert!(rendered.contains("\t// Go-specific greeting doc.\n\tGreeting *string"));
    assert!(!rendered.contains("Default greeting doc."));

    // Per-language docs without a default or `go=` key are omitted from Go.
    assert!(!rendered.contains("Python-only field doc."));

    // Long docs wrap across comment lines.
    assert!(rendered.contains(
        "\t// A very long field doc that has to be wrapped because it exceeds the generated\n\t// comment line width by a comfortable margin for testing.\n\tSalutation *string"
    ));

    // Operation docs render on the exported convenience wrapper, with the
    // `returns=` text in a separate paragraph.
    assert!(rendered.contains(
        "// Greets the given person.\n//\n// Input name: Name of the person to greet.\n//\n// Returns: The rendered greeting.\nfunc Greet("
    ));

    fs::remove_dir_all(temp_dir).unwrap();
}
