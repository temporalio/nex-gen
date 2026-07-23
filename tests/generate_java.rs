use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nex_gen::{GenerateRequest, generate_to_file};

static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn json_input_path(root: &Path, example_id: &str) -> PathBuf {
    let dir_path = root.join("examples/json-inputs").join(example_id);
    if dir_path.is_dir() {
        return dir_path;
    }
    root.join("examples/json-inputs")
        .join(format!("{example_id}.yaml"))
}

/// The checked-in Java example root for a given generation mode and example.
fn java_example_output_path(root: &Path, mode: &str, example_id: &str) -> PathBuf {
    root.join("examples/java/src/main/java/json_schema")
        .join(mode)
        .join(example_id)
}

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nex-gen-{label}-{unique}-{counter}"))
}

fn read_java_files(dir: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("java") {
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

/// Regenerates one example into a temp Gradle-shaped tree so the derived Java
/// package matches the checked-in output, then compares the emitted files.
fn assert_regeneration_matches(mode: &str, generate_native_api: bool) {
    let root = project_root();
    for example_id in ["chat", "kb", "showcase", "temporal"] {
        let temp_dir = unique_output_path(&format!("java-json-{mode}-{example_id}"));
        // Mirror the checked-in layout: <root>/src/main/java/json_schema/<mode>/<example>
        let output_path = temp_dir
            .join("src/main/java/json_schema")
            .join(mode)
            .join(example_id);

        generate_to_file(&GenerateRequest {
            language: nex_gen::language::Language::Java,
            input_paths: vec![json_input_path(&root, example_id)],
            support_paths: Vec::new(),
            descriptor_paths: Vec::new(),
            output_path: output_path.clone(),
            format: false,
            generate_native_api,
            js_temporal_repr: Default::default(),
        })
        .unwrap();

        let rendered = read_java_files(&output_path);
        let expected = read_java_files(&java_example_output_path(&root, mode, example_id));
        assert_eq!(
            rendered, expected,
            "snapshot mismatch for {mode}/{example_id}"
        );
        if example_id == "showcase" {
            let all = rendered.values().cloned().collect::<Vec<_>>().join("\n");
            // Scalar defaults surface on read via the generated getter default.
            assert!(all.contains("public String getGreetingOrDefault() {"));
            assert!(all.contains("return greeting != null ? greeting : \"hello\";"));
            assert!(all.contains("public boolean getDebugOrDefault() {"));
            // `deprecated` → @Deprecated + @deprecated javadoc; `title` → summary.
            assert!(all.contains("@Deprecated"));
            assert!(all.contains("@deprecated This field is deprecated."));
            assert!(all.contains("Retry budget"));
            // `x-java-name` override (Stage 4): the emitted field/getter use the
            // override while the wire name (`@JsonProperty`) stays `legacyId`.
            assert!(all.contains("private final @Nullable String legacyID;"));
            assert!(all.contains("public @Nullable String getLegacyID() {"));
            assert!(all.contains("gen.writeStringField(\"legacyId\", value.legacyID);"));
        }
        fs::remove_dir_all(temp_dir).unwrap();
    }
}

#[test]
fn java_json_example_generation_matches_checked_in_output() {
    assert_regeneration_matches("definitions", false);
}

#[test]
fn java_json_api_example_generation_matches_checked_in_output() {
    assert_regeneration_matches("api", true);
}
