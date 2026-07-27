use std::path::{Path, PathBuf};

/// Resolves a JSON-Schema example id (e.g. `chat`, `kb`, `showcase`) to its
/// input path under `examples/json-schema-inputs`, trying both the bare
/// filename and the `.nexusrpc` naming-convention infix used by files that
/// declare a Nexus service/operation envelope.
pub fn json_input_path(root: &Path, example_id: &str) -> PathBuf {
    let input_root = root.join("examples/json-schema-inputs");
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
