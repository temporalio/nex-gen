use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::{GenerateRequest, generate_to_file};

mod common;
use common::json_input_path;

static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A property whose union has one inline structured object branch (named
/// `<Union>Object`) and one scalar branch.
/// Unions in positions with no property of their own: an array element (inline
/// and `$ref`), a map member (inline), plus a nullable element for contrast.
const ELEMENT_UNION_SCHEMA: &str = r##"$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  segments:
    type: array
    items:
      oneOf:
        - { type: string }
        - { type: integer }
  choices:
    type: array
    items: { $ref: "#/$defs/Choice" }
  entries: { $ref: "#/$defs/Entries" }
  slots:
    type: array
    items:
      oneOf:
        - { type: string }
        - { type: "null" }
$defs:
  Choice:
    oneOf:
      - { type: string }
      - { type: boolean }
  Entries:
    type: object
    additionalProperties:
      oneOf:
        - { type: string }
        - { type: integer }
"##;

const INLINE_OBJECT_BRANCH_SCHEMA: &str = r#"$schema: https://json-schema.org/draft/2020-12/schema
type: object
properties:
  payload:
    oneOf:
      - type: object
        required: [text]
        properties:
          text: { type: string, minLength: 1 }
      - { type: string }
"#;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The checked-in Java example root for a given generation mode and example.
/// Definitions are the beginner-facing samples; native-api is snapshot-only in
/// the advanced project.
fn java_example_output_path(root: &Path, mode: &str, example_id: &str) -> PathBuf {
    match mode {
        "definitions" => root
            .join("samples/java/src/main/java/json_schema/definitions")
            .join(example_id),
        _ => root
            .join("advanced/samples/java/src/main/java/json_schema/api")
            .join(example_id),
    }
}

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-{label}-{unique}-{counter}"))
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
        // The output directory's base name must equal the package's last
        // segment; the checked-in package is `json_schema.<mode>.<example>`.
        let output_path = temp_dir.join(example_id);

        generate_to_file(&GenerateRequest {
            language: nexgen::language::Language::Java,
            input_paths: vec![json_input_path(&root, example_id)],
            support_paths: Vec::new(),
            descriptor_paths: Vec::new(),
            output_path: output_path.clone(),
            format: false,
            generate_native_api,
            java_package_name: Some(format!("json_schema.{mode}.{example_id}")),
            ts_date_time_types: Default::default(),
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
            assert!(all.contains("private final @Nullable String legacyIdJava;"));
            assert!(all.contains("public @Nullable String getLegacyIdJava() {"));
            assert!(all.contains("gen.writeStringField(\"legacyId\", value.legacyIdJava);"));
            // An inline free-form object branch: the union declares a nested
            // wrapper holding the members verbatim (a named free-form model gets
            // the same catch-all on its POJO).
            assert!(all.contains("public static final class PayloadObject implements Payload {"));
            assert!(all.contains("private final Map<String, JsonNode> value;"));
            assert!(all.contains("public final class Extras {"));
            // A tagged union whose branches are written inline: each branch names
            // itself with `x-java-name` and implements the union interface.
            assert!(all.contains("public final class TextNote implements Note {"));
            assert!(all.contains("public final class LinkNote implements Note {"));
            // A structured inline object branch of a *property* union: the branch
            // is an ordinary POJO implementing the interface nested in the
            // declaring class, and the union parses through `fromNode` exactly as
            // a named union def does.
            assert!(
                all.contains(
                    "public final class ShowcaseDetailObject implements Showcase.Detail {"
                )
            );
            assert!(
                all.contains("return context.readTreeAsValue(node, ShowcaseDetailObject.class);")
            );
            assert!(
                all.contains("detail = Detail.fromNode(field, \"detail\", violations, context);")
            );
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

/// A structured inline object branch of a property-level union is named
/// `<Union>Object` by the load and emitted as an ordinary POJO — implementing the
/// union interface nested in the declaring class, so the interface's `fromNode`
/// dispatcher (the same one a named union def carries) delegates to the branch's
/// own deserializer. See `specs/json-schema/features/oneOf.md`.
#[test]
fn java_json_names_inline_object_union_branch() {
    let temp_dir = unique_output_path("java-json-inline-branch");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("detail.yaml");
    fs::write(&input_path, INLINE_OBJECT_BRANCH_SCHEMA).unwrap();
    let output_path = temp_dir.join("detail");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::Java,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: Some("detail".to_string()),
        ts_date_time_types: Default::default(),
    })
    .unwrap();

    let rendered = read_java_files(&output_path);
    let branch = &rendered[&PathBuf::from("DetailPayloadObject.java")];
    assert!(
        branch.contains("public final class DetailPayloadObject implements Detail.Payload {"),
        "{branch}"
    );
    // The branch is an ordinary model: its own constraints and catch-all.
    assert!(branch.contains("must have length >= 1"), "{branch}");
    assert!(
        branch.contains("private final Map<String, JsonNode> additionalProperties;"),
        "{branch}"
    );

    let declaring = &rendered[&PathBuf::from("Detail.java")];
    for expected in [
        "public interface Payload {",
        "static @Nullable Payload fromNode(JsonNode node, String path, List<Violation> violations, DeserializationContext context) {",
        "return context.readTreeAsValue(node, DetailPayloadObject.class);",
        "public static final class PayloadString implements Payload {",
        "payload = Payload.fromNode(field, \"payload\", violations, context);",
    ] {
        assert!(declaring.contains(expected), "{expected}\n{declaring}");
    }
    fs::remove_dir_all(temp_dir).unwrap();
}

/// A union in an element position decodes through the interface's own
/// `fromNode` dispatcher — Jackson cannot instantiate a sealed interface — with
/// the element index / member key in the violation path. A nullable element
/// takes the TYPE_USE annotation instead (the list stays non-null).
/// See `specs/json-schema/features/oneOf.md` ("Unions in element positions").
#[test]
fn java_json_decodes_element_position_unions() {
    let temp_dir = unique_output_path("java-json-element-union");
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("bag.yaml");
    fs::write(&input_path, ELEMENT_UNION_SCHEMA).unwrap();
    let output_path = temp_dir.join("bag");

    generate_to_file(&GenerateRequest {
        language: nexgen::language::Language::Java,
        input_paths: vec![input_path],
        support_paths: Vec::new(),
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        generate_native_api: false,
        java_package_name: Some("bag".to_string()),
        ts_date_time_types: Default::default(),
    })
    .unwrap();

    let rendered = read_java_files(&output_path);
    // The inline element union is named after its position and emitted as an
    // ordinary named union def.
    assert!(rendered.contains_key(&PathBuf::from("BagSegmentsItem.java")));
    assert!(rendered.contains_key(&PathBuf::from("EntriesValue.java")));

    let declaring = &rendered[&PathBuf::from("Bag.java")];
    for expected in [
        "private final @Nullable List<BagSegmentsItem> segments;",
        "String elementPath = \"segments\" + \"[\" + index + \"]\";",
        "BagSegmentsItem parsedItems = BagSegmentsItem.fromNode(element, elementPath, violations, context);",
        "Choice parsedItems = Choice.fromNode(element, elementPath, violations, context);",
        "private final @Nullable List<@Nullable String> slots;",
        "items.add(null);",
    ] {
        assert!(declaring.contains(expected), "{expected}\n{declaring}");
    }

    let map = &rendered[&PathBuf::from("Entries.java")];
    for expected in [
        "Map<String, EntriesValue> values",
        "EntriesValue parsedValues = EntriesValue.fromNode(element, key, violations, context);",
    ] {
        assert!(map.contains(expected), "{expected}\n{map}");
    }
    fs::remove_dir_all(temp_dir).unwrap();
}
