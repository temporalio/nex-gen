// The `add-message` CLI surface lives behind the `advanced` feature.
#![cfg(feature = "advanced")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nex_gen::add_message_to_string;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn descriptor_path(root: &std::path::Path) -> PathBuf {
    root.join("advanced/samples/descriptors/temporal_api.bin")
}

fn linked_inputs_path(root: &std::path::Path) -> PathBuf {
    root.join("advanced/samples/inputs/deps")
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nex-gen-{name}-{unique}"))
}

fn write_temp_wit(name: &str, contents: &str) -> PathBuf {
    let temp_dir = unique_temp_dir(name);
    fs::create_dir_all(&temp_dir).unwrap();
    let path = temp_dir.join("input.wit");
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn cli_add_message_generates_a_standalone_message_tree() {
    let root = project_root();
    let output = Command::new(env!("CARGO_BIN_EXE_nexgen"))
        .args([
            "add-message",
            "--descriptors",
            descriptor_path(&root).to_str().unwrap(),
            "--message",
            "WorkflowExecutionInfo",
            "--input",
            linked_inputs_path(&root).to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let rendered = String::from_utf8(output.stdout).unwrap();
    assert!(rendered.contains("world system {\n  export workflow-execution-info;\n}"));
    assert!(rendered.contains("interface workflow-execution-info {"));
    assert!(rendered.contains("record workflow-execution-info {"));
    assert!(rendered.contains("record workflow-execution {"));
    assert!(rendered.contains("enum workflow-execution-status {"));
    assert!(rendered.contains("use nexus:temporal-types/model@1.0.0.{"));
    assert!(!rendered.contains("@nexus.endpoint"));
    assert!(!rendered.contains(": func("));
}

#[test]
fn add_message_inserts_only_missing_types_into_the_sole_exported_interface() {
    let root = project_root();
    let input_path = write_temp_wit(
        "add-message-existing",
        r#"package temporal:nexus@1.0.0;

world system {
  export models;
}

interface models {
  /// @nexus.proto "temporal.api.common.v1.WorkflowExecution"
  record workflow-execution {
    workflow-id: string,
    run-id: string,
  }
}
"#,
    );
    let rendered = add_message_to_string(
        &[descriptor_path(&root)],
        "WorkflowExecution",
        &[input_path.clone(), linked_inputs_path(&root)],
    )
    .unwrap();

    assert_eq!(rendered, fs::read_to_string(input_path).unwrap());

    let input_path = write_temp_wit(
        "add-message-insert",
        r#"package temporal:nexus@1.0.0;

world system {
  export models;
}

interface models {
}
"#,
    );
    let rendered = add_message_to_string(
        &[descriptor_path(&root)],
        "WorkflowExecution",
        &[input_path, linked_inputs_path(&root)],
    )
    .unwrap();
    assert!(rendered.contains("interface models {\n  /// @nexus.proto"));
    assert!(rendered.contains("record workflow-execution {"));
}
