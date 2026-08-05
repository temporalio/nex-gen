//! Coverage diagnostics for the .NET JSON-Schema backend.
//!
//! `json_schema::dotnet` does not yet enforce the whole assertion vocabulary the
//! Go / Java / Python / TypeScript backends do. A keyword it does not handle
//! survives parsing and planning and is then dropped when the model is rendered,
//! so a payload the other four targets reject is accepted by the generated C#.
//!
//! `PRINCIPLES.md` says the generator would rather reject loudly than emit
//! something subtly wrong. Until every keyword is covered, this module supplies
//! the "loudly" half: each still-unenforced keyword is reported as a generation
//! warning naming the keyword and the members carrying it, so a silent divergence
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
    // Numeric bounds are enforced; see `render_constraint_validator`.
    // String assertions. minLength/maxLength/pattern are enforced.
    "format",
    "contentEncoding",
    "contentMediaType",
    "contentSchema",
    // Array assertions. minItems/maxItems/uniqueItems are enforced, and so is
    // `contains` in its `const` form — see the shape check below.
    "prefixItems",
    // Object assertions. minProperties/maxProperties/dependentRequired are
    // enforced, and so is `propertyNames` on a map-shaped object — see the shape
    // check below.
    "dependentSchemas",
    "patternProperties",
    // Closed value sets (`enum`) are enforced.
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
            // Deciding whether a `$ref` union is lowered needs the branch schemas,
            // which are sibling models, so index them before walking.
            let mut siblings = BTreeMap::new();
            for (_, binding) in leaf.spec.external_types() {
                if let ExternalTypeSpec::Json(json) = &binding.external_type {
                    siblings.insert(reference_tail(&json.full_name), &json.schema);
                }
            }
            for (_, binding) in leaf.spec.external_types() {
                let ExternalTypeSpec::Json(json) = &binding.external_type else {
                    continue;
                };
                collect_schema(&json.schema, &json.model_name, &siblings, findings);
            }
        }
        ApiSpecNode::Branch(branch) => {
            for child in branch.children.values() {
                collect_node(child, findings);
            }
        }
    }
}

/// The trailing name of a `$ref` or qualified model identity — the segment after
/// the last `/` or `#`.
fn reference_tail(reference: &str) -> String {
    reference
        .rsplit(['#', '/'])
        .next()
        .unwrap_or(reference)
        .to_string()
}

/// Walks a schema recursively, recording each unenforced keyword against the
/// dotted member path that carries it.
fn collect_schema(
    schema: &Value,
    path: &str,
    siblings: &BTreeMap<String, &Value>,
    findings: &mut BTreeMap<&'static str, Vec<String>>,
) {
    let Value::Object(members) = schema else {
        return;
    };

    for keyword in UNENFORCED_KEYWORDS {
        if members.contains_key(*keyword) {
            findings.entry(keyword).or_default().push(path.to_string());
        }
    }

    // `oneOf` is reported only for the spellings the backend does not lower.
    // Two are lowered: the `[<branch>, {"type": "null"}]` nullable wrapper, and a
    // tagged union over `$ref` branches (abstract base plus a routing converter).
    // What remains — a disjoint-kind scalar union such as
    // `oneOf: [{type: string}, {type: integer}]` — still degrades to `object`.
    if let Some(Value::Array(branches)) = members.get("oneOf")
        && !is_nullable_wrapper(branches)
        && !is_tagged_union(branches, siblings)
    {
        findings.entry("oneOf").or_default().push(path.to_string());
    }

    // `propertyNames` is lowered only for a map-shaped object, whose extension bag
    // holds every wire member. On an object with declared properties the keyword
    // also governs those declared names, which the bag does not carry.
    if members.contains_key("propertyNames")
        && members
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| !properties.is_empty())
    {
        findings
            .entry("propertyNames")
            .or_default()
            .push(path.to_string());
    }

    // `contains` is lowered only for a bare `const` branch; matching an arbitrary
    // subschema per element would need the validator to be reentrant over element
    // values. `minContains`/`maxContains` ride along with it, so they are reported
    // only when the `contains` they qualify is itself unsupported.
    if let Some(contains) = members.get("contains")
        && !contains_is_supported(contains)
    {
        for keyword in ["contains", "minContains", "maxContains"] {
            if keyword == "contains" || members.contains_key(keyword) {
                findings.entry(keyword).or_default().push(path.to_string());
            }
        }
    }

    for (key, value) in members {
        match key.as_str() {
            // Child schemas keyed by member name.
            "properties" | "$defs" | "patternProperties" => {
                if let Value::Object(children) = value {
                    for (name, child) in children {
                        collect_schema(child, &format!("{path}.{name}"), siblings, findings);
                    }
                }
            }
            // Child schemas in positional or branch position.
            "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                if let Value::Array(children) = value {
                    for child in children {
                        collect_schema(child, path, siblings, findings);
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
                collect_schema(value, path, siblings, findings);
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

/// True when the branches form the tagged union the backend lowers: two or more
/// `$ref`s that all resolve to objects sharing a required `const` discriminator.
///
/// Resolving the branches matters. The loader rewrites an inline union into
/// `$ref`s to synthesized types, so showcase's disjoint-kind
/// `oneOf: [{type: string}, {type: integer}]` reaches this pass as two `$ref`
/// branches and is indistinguishable from `Circle | Square` by shape alone. Only
/// following the refs separates the union that is lowered from the one that still
/// degrades to `object`.
fn is_tagged_union(branches: &[Value], siblings: &BTreeMap<String, &Value>) -> bool {
    let resolved = branches
        .iter()
        .filter(|branch| !is_null_schema(branch))
        .map(|branch| {
            branch
                .get("$ref")
                .and_then(Value::as_str)
                .map(reference_tail)
                .and_then(|name| siblings.get(&name).copied())
        })
        .collect::<Option<Vec<_>>>();
    let Some(resolved) = resolved else {
        return false;
    };
    if resolved.len() < 2 {
        return false;
    }
    discriminator_names(resolved[0])
        .into_iter()
        .any(|candidate| {
            resolved
                .iter()
                .all(|branch| discriminator_names(branch).contains(&candidate))
        })
}

/// The member names a schema declares as required with a string `const` — the
/// candidates for a union discriminator.
fn discriminator_names(schema: &Value) -> Vec<String> {
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .filter(|(name, property)| {
                    required.contains(name) && property.get("const").is_some_and(Value::is_string)
                })
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// True when a `contains` subschema is the `const` form the backend lowers.
fn contains_is_supported(contains: &Value) -> bool {
    contains.get("const").is_some()
}

fn is_null_schema(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|ty| ty == "null")
}

fn format_warning(keyword: &str, paths: &[String]) -> String {
    let detail = match keyword {
        "oneOf" => {
            "is a disjoint-kind scalar union, which degrades to `object` — only \
             `$ref` tagged unions are lowered"
        }
        "enum" => "is emitted as a bare scalar with no closed value set",
        "format" => "is left as `string` — neither asserted nor materialized",
        "contentEncoding" => "is left as `string` — not decoded to bytes",
        "contains" | "minContains" | "maxContains" => {
            "is enforced only for a `const` branch, and this one is not"
        }
        "propertyNames" => {
            "is enforced only on a map-shaped object, and this one declares properties"
        }
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

    // These cases deliberately use `contentMediaType` / `dependentSchemas` — real
    // entries in UNENFORCED_KEYWORDS that no planned phase implements — so the
    // classifier's behavior can be asserted without the case needing an edit every
    // time a keyword gains enforcement. Coverage of the *current* gap set lives in
    // `tests/generate_dotnet.rs`, which is meant to churn.
    fn findings_for(schema: serde_json::Value) -> Vec<String> {
        findings_with_siblings(schema, &[])
    }

    /// `siblings` supplies the `$ref` targets a union's branches resolve to,
    /// keyed by their trailing name.
    fn findings_with_siblings(
        schema: serde_json::Value,
        siblings: &[(&str, serde_json::Value)],
    ) -> Vec<String> {
        let resolved = siblings
            .iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<BTreeMap<_, _>>();
        let mut findings = BTreeMap::new();
        collect_schema(&schema, "Model", &resolved, &mut findings);
        findings
            .into_iter()
            .map(|(keyword, paths)| format_warning(keyword, &paths))
            .collect()
    }

    #[test]
    fn reports_an_unenforced_constraint_on_a_member() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": { "name": { "type": "string", "contentMediaType": "text/plain" } },
        }));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("`contentMediaType`"), "{:?}", warnings);
        assert!(warnings[0].contains("Model.name"), "{:?}", warnings);
    }

    #[test]
    fn does_not_report_enforced_constraints() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": {
                "order": { "type": "integer", "minimum": 0, "maximum": 10 },
                "level": { "type": "integer", "exclusiveMinimum": 0 },
                "ratio": { "type": "number", "multipleOf": 5 },
                "name": { "type": "string", "minLength": 1, "maxLength": 8 },
                "code": { "type": "string", "pattern": "^[A-Z]+$" },
            },
        }));

        assert!(warnings.is_empty(), "{:?}", warnings);
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
    fn does_not_report_a_ref_union_lowered_to_a_tagged_union() {
        let warnings = findings_with_siblings(
            json!({
                "oneOf": [{ "$ref": "#/$defs/Circle" }, { "$ref": "#/$defs/Square" }],
            }),
            &[
                (
                    "Circle",
                    json!({
                        "type": "object",
                        "required": ["kind"],
                        "properties": { "kind": { "type": "string", "const": "circle" } },
                    }),
                ),
                (
                    "Square",
                    json!({
                        "type": "object",
                        "required": ["kind"],
                        "properties": { "kind": { "type": "string", "const": "square" } },
                    }),
                ),
            ],
        );

        assert!(warnings.is_empty(), "{:?}", warnings);
    }

    /// The remaining `oneOf` gap: a disjoint-kind scalar union, which still
    /// degrades to `object`.
    ///
    /// The loader rewrites an inline scalar union into `$ref`s to synthesized
    /// types, so this arrives looking structurally like the tagged union above.
    /// Only resolving the branches tells them apart.
    #[test]
    fn reports_a_ref_union_whose_branches_are_scalars() {
        let warnings = findings_with_siblings(
            json!({
                "type": "object",
                "properties": {
                    "idOrName": {
                        "oneOf": [
                            { "$ref": "#/$defs/IdOrNameString" },
                            { "$ref": "#/$defs/IdOrNameInteger" },
                        ],
                    },
                },
            }),
            &[
                ("IdOrNameString", json!({ "type": "string" })),
                ("IdOrNameInteger", json!({ "type": "integer" })),
            ],
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("`oneOf`"), "{:?}", warnings);
        assert!(warnings[0].contains("Model.idOrName"), "{:?}", warnings);
    }

    /// A `$ref` union whose branches are objects but share no `const`
    /// discriminator is not a tagged union either, and must still be reported.
    #[test]
    fn reports_a_ref_union_with_no_shared_discriminator() {
        let warnings = findings_with_siblings(
            json!({
                "oneOf": [{ "$ref": "#/$defs/Left" }, { "$ref": "#/$defs/Right" }],
            }),
            &[
                ("Left", json!({ "type": "object" })),
                ("Right", json!({ "type": "object" })),
            ],
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("`oneOf`"), "{:?}", warnings);
    }

    #[test]
    fn descends_into_nested_and_array_schemas() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string", "contentMediaType": "text/plain" },
                },
                "nested": {
                    "type": "object",
                    "dependentSchemas": { "a": { "type": "object" } },
                },
            },
        }));

        assert_eq!(warnings.len(), 2, "{:?}", warnings);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`contentMediaType`") && w.contains("Model.tags"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`dependentSchemas`") && w.contains("Model.nested"))
        );
    }

    #[test]
    fn elides_long_path_lists() {
        let warnings = findings_for(json!({
            "type": "object",
            "properties": {
                "a": { "type": "string", "contentMediaType": "text/plain" },
                "b": { "type": "string", "contentMediaType": "text/plain" },
                "c": { "type": "string", "contentMediaType": "text/plain" },
                "d": { "type": "string", "contentMediaType": "text/plain" },
                "e": { "type": "string", "contentMediaType": "text/plain" },
                "f": { "type": "string", "contentMediaType": "text/plain" },
            },
        }));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("and 2 more"), "{:?}", warnings);
    }
}
