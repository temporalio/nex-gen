#![cfg(feature = "advanced")]

use std::fs;
use std::path::{Path, PathBuf};

use nexgen::SupportFiles;
use nexgen::descriptors::DescriptorIndex;
use nexgen::generator::generate_source;
use nexgen::language::Language;
use nexgen::parser::load_api_spec_from_wit_for_language_with_inputs;

fn descriptor_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("advanced/samples/descriptors/temporal_api.bin")
}

fn write_fixture(root: &Path, operation: bool, omit_oneof: bool) -> PathBuf {
    let operation = if operation {
        "  pause: func(request: pause-activity-request);\n"
    } else {
        ""
    };
    let omit = if omit_oneof {
        "    /// @nexus.omit\n"
    } else {
        ""
    };
    let source = format!(
        r#"package test:oneof@1.0.0;

world system {{
  export api;
}}

interface api {{
  variant activity-selection {{
    id(string),
    %type(string),
  }}

  /// @nexus.proto "temporal.api.common.v1.WorkflowExecution"
  record workflow-execution {{
    workflow-id: string,
    run-id: string,
  }}

  /// @nexus.proto "temporal.api.workflowservice.v1.PauseActivityRequest"
  record pause-activity-request {{
    namespace: string,
    execution: option<workflow-execution>,
    identity: string,
{omit}    activity: option<activity-selection>,
    reason: string,
    request-id: string,
  }}

{operation}}}
"#
    );
    let path = root.join("oneof.wit");
    fs::write(&path, source).unwrap();
    path
}

fn generate_typescript(path: &Path) -> nexgen::error::Result<String> {
    let spec = load_api_spec_from_wit_for_language_with_inputs(
        Language::TypeScript,
        &[path.to_path_buf()],
    )?;
    let descriptors = DescriptorIndex::load(&descriptor_path())?;
    generate_source(
        Language::TypeScript,
        spec,
        &descriptors,
        &SupportFiles::default(),
    )
}

#[test]
fn unsupported_backend_rejects_reachable_oneof_conversion() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), true, false);
    let error = generate_typescript(&path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("typescript protobuf conversion does not yet support oneof group"),
        "{error}"
    );
}

#[test]
fn unsupported_backend_allows_conversion_free_or_omitted_oneofs() {
    let temp = tempfile::tempdir().unwrap();
    let conversion_free = write_fixture(temp.path(), false, false);
    generate_typescript(&conversion_free).unwrap();

    let omitted = write_fixture(temp.path(), true, true);
    generate_typescript(&omitted).unwrap();
}
