use std::fs;
use std::path::{Path, PathBuf};

/// Resolves a JSON-Schema example id (e.g. `chat`, `kb`, `showcase`) to its
/// input path under `samples/schemas`, trying both the bare
/// filename and the `.nexusrpc` naming-convention infix used by files that
/// declare a Nexus service/operation envelope.
pub fn json_input_path(root: &Path, example_id: &str) -> PathBuf {
    let input_root = root.join("samples/schemas");
    let dir_path = input_root.join(example_id);
    if dir_path.is_dir() {
        return dir_path;
    }
    for extension in ["yaml", "yml", "json"] {
        for stem in [example_id.to_string(), format!("{example_id}.nexusrpc")] {
            let path = input_root.join(format!("{stem}.{extension}"));
            if path.is_file() {
                return path;
            }
        }
    }
    input_root.join(format!("{example_id}.yaml"))
}

/// Writes a three-file closure exercising a bare-ref file-root alias from both
/// an ordinary property and Nexus operation I/O.
pub fn write_bare_ref_alias_closure(root: &Path) -> PathBuf {
    let input = root.join("input");
    fs::create_dir_all(input.join("target")).unwrap();
    fs::create_dir_all(input.join("alias")).unwrap();
    fs::write(
        input.join("target/main.yaml"),
        r##"$schema: https://json-schema.org/draft/2020-12/schema
type: object
additionalProperties: false
required: [value]
properties: { value: { type: string } }
$defs:
  Mirror: { $ref: "#" }
"##,
    )
    .unwrap();
    fs::write(
        input.join("alias/alternate.yaml"),
        r#"$schema: https://json-schema.org/draft/2020-12/schema
$ref: ../target/main.yaml#
"#,
    )
    .unwrap();
    fs::write(
        input.join("service.nexusrpc.yaml"),
        r#"$schema: https://json-schema.org/draft/2020-12/schema
nexusrpc: "1.0.0"
services:
  AliasService:
    operations:
      echo:
        input: { $ref: "alias/alternate.yaml#" }
        output: { $ref: "alias/alternate.yaml#" }
$defs:
  Holder:
    type: object
    additionalProperties: false
    required: [item]
    properties:
      item: { $ref: "alias/alternate.yaml#" }
"#,
    )
    .unwrap();
    input
}
