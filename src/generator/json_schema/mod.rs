pub(crate) mod dotnet;
pub(crate) mod go;
pub(crate) mod java;
pub(crate) mod python;
pub(crate) mod typescript;

pub(in crate::generator) use crate::planning::build_json_name_manifest;

use std::collections::BTreeMap;

use crate::planning::{PlannedJsonType, PlannedSpec};

/// Returns the target of a named JSON Schema alias.
///
/// Alias declarations are deliberately recognized from the exact authored
/// shape: a model containing only `$ref`. A reference with annotations or
/// other siblings is governed by the ordinary reference/merge rules instead.
pub(in crate::generator) fn bare_ref_target(model: &PlannedJsonType) -> Option<&str> {
    let object = model.schema.as_object()?;
    object
        .iter()
        .all(|(key, value)| key == "$ref" || value.is_null())
        .then(|| object.get("$ref")?.as_str())
        .flatten()
}

/// Registers the emitted identifier of every model declared in *another* module
/// in a backend's `$ref` registry, under both key forms a backend looks a
/// reference up by: the bare model full name and the resolved `$ref` text
/// `#/$defs/<full name>`.
///
/// A leaf's own name manifest covers only the models it declares, so without
/// this a cross-module `$ref` would be recased from the reference text and drop
/// the `x-<lang>-name` override the other input file declares. The identifiers
/// themselves are resolved once, tree-wide, by `EmittedNameResolutionPass`.
pub(in crate::generator) fn register_cross_module_ref_names(
    api_plan: &PlannedSpec,
    ref_names: &mut BTreeMap<String, String>,
) {
    for (full_name, model_name) in &api_plan.data.cross_module_model_names {
        ref_names.insert(full_name.clone(), model_name.clone());
        ref_names.insert(format!("#/$defs/{full_name}"), model_name.clone());
    }
}

/// Returns one member segment in the public P11 violation-path grammar.
/// Identifier-like wire keys use dot notation; every other key is bracket
/// quoted so dots, brackets, quotes, and backslashes remain unambiguous.
pub(in crate::generator) fn violation_member_segment(key: &str) -> String {
    let mut bytes = key.bytes();
    let identifier = bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric());
    if identifier {
        return key.to_string();
    }
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("[\"{escaped}\"]")
}

#[cfg(test)]
mod tests {
    use super::violation_member_segment;

    #[test]
    fn violation_member_segments_follow_the_public_path_grammar() {
        assert_eq!(violation_member_segment("plain_name9"), "plain_name9");
        assert_eq!(violation_member_segment("9lives"), "[\"9lives\"]");
        assert_eq!(violation_member_segment("a.b"), "[\"a.b\"]");
        assert_eq!(violation_member_segment("[0]"), "[\"[0]\"]");
        assert_eq!(
            violation_member_segment("quote\"slash\\"),
            "[\"quote\\\"slash\\\\\"]"
        );
        assert_eq!(violation_member_segment(""), "[\"\"]");
        assert_eq!(violation_member_segment("café"), "[\"café\"]");
    }
}
