//! Coverage diagnostics for the .NET JSON-Schema backend.
//!
//! Unlike the Go / Java / Python / TypeScript backends, `json_schema::dotnet`
//! emits no constraint validator: assertion keywords survive parsing and
//! planning and are then dropped when the model is rendered. A payload the
//! other four targets reject is therefore accepted by the generated C#.
//!
//! `PRINCIPLES.md` says the generator would rather reject loudly than emit
//! something subtly wrong. Until the validator lands, this module supplies the
//! "loudly" half: every dropped keyword is reported as a generation warning
//! naming the keyword and the members carrying it, so a silent divergence
//! becomes a visible one.
//!
//! Each entry here is a standing TODO. When a keyword gains real enforcement in
//! the backend, delete it from [`UNENFORCED_KEYWORDS`] — the coverage test in
//! `tests/generate_dotnet.rs` asserts the warning disappears with it.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::planning::PlannedTypeFamily;
use crate::spec::ExternalTypeSpec;
use crate::workspace::{ApiSpecNode, ApiSpecTree};

/// Assertion keywords the .NET backend parses but does not enforce.
///
/// Deliberately excluded because the backend *does* honor them:
/// `maxProperties`, `additionalProperties`, `required`, `properties`, `type`,
/// `const`, `default`, `$ref`, `items`, `description`. `allOf` is excluded
/// because the loader merges it away before any backend sees it.
const UNENFORCED_KEYWORDS: &[&str] = &[
    // Numeric bounds.
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    // String assertions.
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "contentEncoding",
    "contentMediaType",
    "contentSchema",
    // Array assertions.
    "minItems",
    "maxItems",
    "uniqueItems",
    "contains",
    "minContains",
    "maxContains",
    "prefixItems",
    // Object assertions.
    "minProperties",
    "dependentRequired",
    "dependentSchemas",
    "propertyNames",
    "patternProperties",
    // Closed value sets.
    "enum",
];

/// How many member paths to name before eliding the rest, so a large schema
/// produces a readable warning instead of a wall of text.
const MAX_REPORTED_PATHS: usize = 4;

/// Collects one warning per unenforced construct across the whole planned tree.
///
/// Runs in both generation modes: the JSON-Schema samples are generated in
/// definitions mode, which is exactly where the missing validator matters most.
pub(in crate::generator) fn coverage_warnings(
    tree: &ApiSpecTree<PlannedTypeFamily>,
) -> Vec<String> {
    let mut findings: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    collect_node(&tree.root, &mut findings);

    findings
        .into_iter()
        .map(|(keyword, paths)| format_warning(keyword, &paths))
        .collect()
}

fn collect_node(
    node: &ApiSpecNode<PlannedTypeFamily>,
    findings: &mut BTreeMap<&'static str, Vec<String>>,
) {
    match node {
        ApiSpecNode::Leaf(leaf) => {
            for (_, binding) in leaf.spec.external_types() {
                let ExternalTypeSpec::Json(json) = &binding.external_type else {
                    continue;
                };
                collect_schema(&json.schema, &json.model_name, findings);
            }
        }
        ApiSpecNode::Branch(branch) => {
            for child in branch.children.values() {
                collect_node(child, findings);
            }
        }
    }
}

/// Walks a schema recursively, recording each unenforced keyword against the
/// dotted member path that carries it.
fn collect_schema(schema: &Value, path: &str, findings: &mut BTreeMap<&'static str, Vec<String>>) {
    let Value::Object(members) = schema else {
        return;
    };

    for keyword in UNENFORCED_KEYWORDS {
        if members.contains_key(*keyword) {
            findings.entry(keyword).or_default().push(path.to_string());
        }
    }

    // `oneOf` is only reported when it is a real sum type. The
    // `[<branch>, {"type": "null"}]` spelling is how the loader expresses an
    // optional-nullable member, and the backend does lower that correctly.
    if let Some(Value::Array(branches)) = members.get("oneOf")
        && !is_nullable_wrapper(branches)
    {
        findings.entry("oneOf").or_default().push(path.to_string());
    }

    for (key, value) in members {
        match key.as_str() {
            // Child schemas keyed by member name.
            "properties" | "$defs" | "patternProperties" => {
                if let Value::Object(children) = value {
                    for (name, child) in children {
                        collect_schema(child, &format!("{path}.{name}"), findings);
                    }
                }
            }
            // Child schemas in positional or branch position.
            "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                if let Value::Array(children) = value {
                    for child in children {
                        collect_schema(child, path, findings);
                    }
                }
            }
            // Single child schema.
            "items"
            | "additionalProperties"
            | "contains"
            | "not"
            | "propertyNames"
            | "contentSchema" => {
                collect_schema(value, path, findings);
            }
            _ => {}
        }
    }
}

/// True for the `oneOf: [<branch>, {"type": "null"}]` nullable spelling — one
/// non-null branch plus at least one explicit null branch.
fn is_nullable_wrapper(branches: &[Value]) -> bool {
    let null_branches = branches
        .iter()
        .filter(|branch| is_null_schema(branch))
        .count();
    null_branches > 0 && branches.len() - null_branches == 1
}

fn is_null_schema(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|ty| ty == "null")
}

fn format_warning(keyword: &str, paths: &[String]) -> String {
    let detail = match keyword {
        "oneOf" => "branches are dropped — the emitted class has no members",
        "enum" => "is emitted as a bare scalar with no closed value set",
        "format" => "is left as `string` — neither asserted nor materialized",
        "contentEncoding" => "is left as `string` — not decoded to bytes",
        _ => "is not enforced — .NET constraint validation is unimplemented",
    };

    let mut named = paths
        .iter()
        .take(MAX_REPORTED_PATHS)
        .cloned()
        .collect::<Vec<_>>();
    if paths.len() > MAX_REPORTED_PATHS {
        named.push(format!("and {} more", paths.len() - MAX_REPORTED_PATHS));
    }

    format!(
        "dotnet: `{keyword}` {detail}. Affects: {}",
        named.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn findings_for(schema: serde_json::Value) -> Vec<String> {
        let mut findings = BTreeMap::new();
        collect_schema(&schema, "Model", &mut findings);
        findings
            .into_iter()
            .map(|(keyword, paths)| format_warning(keyword, &paths))
            .collect()
    }

    #[test]
    fn reports_numeric_bound_on_a_member() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": { "order": { "type": "integer", "minimum": 0 } },
        }));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("`minimum`"), "{:?}", warnings);
        assert!(warnings[0].contains("Model.order"), "{:?}", warnings);
    }

    #[test]
    fn does_not_report_supported_keywords() {
        let warnings = findings_for(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["a"],
            "maxProperties": 50,
            "properties": {
                "a": { "type": "string", "const": "x" },
                "b": { "type": "integer", "default": 0 },
            },
        }));

        assert!(warnings.is_empty(), "{:?}", warnings);
    }

    #[test]
    fn treats_nullable_one_of_as_supported() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": {
                "page": { "oneOf": [{ "$ref": "page.json" }, { "type": "null" }] },
            },
        }));

        assert!(warnings.is_empty(), "{:?}", warnings);
    }

    #[test]
    fn reports_real_sum_type_one_of() {
        let warnings = findings_for(json!({
            "oneOf": [{ "$ref": "#/$defs/Circle" }, { "$ref": "#/$defs/Square" }],
        }));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("`oneOf`"), "{:?}", warnings);
        assert!(
            warnings[0].contains("branches are dropped"),
            "{:?}",
            warnings
        );
    }

    #[test]
    fn descends_into_nested_and_array_schemas() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string", "minLength": 1 } },
                "nested": {
                    "type": "object",
                    "properties": { "code": { "type": "string", "pattern": "^[A-Z]+$" } },
                },
            },
        }));

        assert_eq!(warnings.len(), 2, "{:?}", warnings);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`minLength`") && w.contains("Model.tags"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`pattern`") && w.contains("Model.nested.code"))
        );
    }

    #[test]
    fn elides_long_path_lists() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": {
                "a": { "type": "integer", "minimum": 0 },
                "b": { "type": "integer", "minimum": 0 },
                "c": { "type": "integer", "minimum": 0 },
                "d": { "type": "integer", "minimum": 0 },
                "e": { "type": "integer", "minimum": 0 },
                "f": { "type": "integer", "minimum": 0 },
            },
        }));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("and 2 more"), "{:?}", warnings);
    }
}
