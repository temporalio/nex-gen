use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nexus_api_gen::generate_to_string_with_inputs;

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
    let status = Command::new(env!("CARGO_BIN_EXE_nexus-api-gen"))
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
    std::env::temp_dir().join(format!("nexus-api-gen-{label}-{unique}"))
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
fn go_type_showcase_generates_expected_types() {
    let root = project_root();
    let rendered = generate_to_string_with_inputs(
        nexus_api_gen::language::Language::Go,
        &example_input_paths(&root, "type-showcase"),
        &[descriptor_path(&root)],
    )
    .unwrap();

    // Service and operation constants
    assert!(rendered.contains("const ServiceName = \"type-showcase\""));
    assert!(rendered.contains("const Endpoint = \"type-showcase-endpoint\""));
    assert!(rendered.contains("const GetUserOp = \"GetUser\""));
    assert!(rendered.contains("const UpdateEmailOp = \"UpdateEmail\""));
    assert!(rendered.contains("const RenameOp = \"Rename\""));
    assert!(rendered.contains("const SetProfileOp = \"SetProfile\""));
    assert!(rendered.contains("const DeactivateOp = \"Deactivate\""));

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
    assert!(rendered.contains("type GetUserRequest struct"));
    assert!(rendered.contains("UserId string // required"));
    assert!(rendered.contains("ConsistencyToken string\n"));
    assert!(!rendered.contains("ConsistencyToken *string"));

    assert!(rendered.contains("type PostalAddress struct"));
    assert!(rendered.contains("Street string // required"));
    assert!(rendered.contains("City string // required"));
    assert!(rendered.contains("Country string // required"));
    // Tuple field generates a named struct with ordinal fields
    assert!(rendered.contains("Coordinates *Coordinates"));
    assert!(rendered.contains("type Coordinates struct"));
    assert!(rendered.contains("First float64 // required"));
    assert!(rendered.contains("Second float64 // required"));

    assert!(rendered.contains("type UserProfile struct"));
    assert!(rendered.contains("Tags []string // required"));
    assert!(rendered.contains("Metadata map[string]string // required"));
    assert!(rendered.contains("Capabilities UserCapability // required"));
    // Variant interface field
    assert!(rendered.contains("NotificationTarget NotificationTarget // required"));
    // Optional struct field keeps pointer
    assert!(rendered.contains("Address *PostalAddress"));
    assert!(!rendered.contains("Address *PostalAddress // required"));

    assert!(rendered.contains("type DeactivateRequest struct"));
    assert!(rendered.contains("UserId string // required"));
    // Optional scalar -- plain type, no pointer, no required comment
    assert!(rendered.contains("Reason string\n"));
    assert!(!rendered.contains("Reason *string"));

    // Resource struct
    assert!(rendered.contains("type User struct"));
    assert!(rendered.contains("DisplayName string // required"));
    assert!(rendered.contains("Status UserStatus // required"));
    assert!(rendered.contains("Profile UserProfile // required"));

    // Resource methods
    assert!(rendered
        .contains("func (u *User) UpdateEmail(ctx workflow.Context, email string) (*User, error)"));
    assert!(rendered.contains("UpdateEmailRequest{UserId: u.UserId, Email: email}"));
    assert!(rendered.contains(
        "func (u *User) Rename(ctx workflow.Context, displayName string) (*User, error)"
    ));
    assert!(rendered.contains("RenameRequest{UserId: u.UserId, DisplayName: displayName}"));
    // Void resource method
    assert!(
        rendered.contains("func (u *User) Deactivate(ctx workflow.Context, reason string) error")
    );
    assert!(rendered.contains("DeactivateRequest{UserId: u.UserId, Reason: reason}"));

    // Unexported operation wrapper functions
    assert!(rendered.contains("func getUser(ctx workflow.Context, request GetUserRequest) (*User, error)"));
    assert!(rendered.contains("func updateEmail(ctx workflow.Context, request UpdateEmailRequest) (*User, error)"));
    assert!(rendered.contains("workflow.NewNexusClient(Endpoint, ServiceName)"));
    assert!(rendered.contains("c.ExecuteOperation(ctx, GetUserOp, request, workflow.NexusOperationOptions{})"));
    // Void operation
    assert!(rendered.contains("func deactivate(ctx workflow.Context, request DeactivateRequest) error"));
    assert!(rendered.contains("return fut.Get(ctx, nil)"));

    // Exported convenience wrappers -- all required fields become positional args
    assert!(rendered.contains("func UpdateEmail(ctx workflow.Context, userId string, email string) (*User, error)"));
    assert!(rendered.contains("UpdateEmailRequest{UserId: userId, Email: email}"));
    // Optional fields produce an options struct
    assert!(rendered.contains("type GetUserOptions struct"));
    assert!(rendered.contains("ConsistencyToken string"));
    assert!(rendered.contains("func GetUser(ctx workflow.Context, userId string, opts ...GetUserOptions) (*User, error)"));
    assert!(rendered.contains("request.ConsistencyToken = opts[0].ConsistencyToken"));
    // Void convenience wrapper with options
    assert!(rendered.contains("type DeactivateOptions struct"));
    assert!(rendered.contains("func Deactivate(ctx workflow.Context, userId string, opts ...DeactivateOptions) error"));
}
