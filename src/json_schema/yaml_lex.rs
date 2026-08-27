//! Lossless YAML scalar preflight for authored-number rules.
//!
//! `serde_yaml` necessarily hands a floating scalar to Serde as `f64`. That
//! loses the written fractional part before a `serde_json::Number` field sees
//! it (`4503599627370496.5` becomes `4503599627370496.0`). Most schema rules are
//! defined over the binary64 value and belong in the ordinary loader. The one
//! exception here is JSON Schema's written-number definition of `integer`.
//! This small event-tree pass retains scalar text just long enough to reject a
//! fractional `const`/`default`/`enum` member on an effectively integer-typed
//! node, including a type contributed through `allOf` or `$ref`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::fs;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::slice;

use unsafe_libyaml::{
    YAML_ALIAS_EVENT, YAML_DOCUMENT_END_EVENT, YAML_DOCUMENT_START_EVENT, YAML_MAPPING_END_EVENT,
    YAML_MAPPING_START_EVENT, YAML_PLAIN_SCALAR_STYLE, YAML_SCALAR_EVENT, YAML_SEQUENCE_END_EVENT,
    YAML_SEQUENCE_START_EVENT, YAML_STREAM_END_EVENT, YAML_STREAM_START_EVENT, yaml_event_delete,
    yaml_event_t, yaml_parser_delete, yaml_parser_initialize, yaml_parser_parse,
    yaml_parser_set_input_string, yaml_parser_t,
};

#[derive(Clone, Debug)]
enum Node {
    Scalar { text: String, kind: ScalarKind },
    Sequence(Vec<Node>),
    Mapping(Vec<(Node, Node)>),
    Alias(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarKind {
    String,
    Number,
    ExplicitInteger,
    Other,
}

#[derive(Debug)]
enum Event {
    Scalar {
        text: String,
        kind: ScalarKind,
        anchor: Option<String>,
    },
    Alias(String),
    SequenceStart(Option<String>),
    SequenceEnd,
    MappingStart(Option<String>),
    MappingEnd,
}

/// Returns the first authored fractional numeric literal whose normalized
/// schema is integer-typed. Syntax failures are intentionally left to
/// `serde_yaml`, which owns the repository's established located parse
/// diagnostic.
///
/// The source set matters because an `allOf` or `$ref` sibling can contribute
/// the integer type while another branch/file contributes the literal. Keeping
/// this inference in the lossless tree avoids teaching normalization about YAML
/// spellings while still following exactly the schema-valued conjunction edges.
pub(crate) fn fractional_integer_literal_in_sources(
    sources: &[(PathBuf, String)],
) -> Option<(PathBuf, String)> {
    let documents = Documents::parse(sources)?;
    documents.find_fractional_integer_literal()
}

#[cfg(test)]
fn fractional_integer_literal(input: &str) -> Option<String> {
    fractional_integer_literal_in_sources(&[(PathBuf::from("api.yaml"), input.to_string())])
        .map(|(_, literal)| literal)
}

struct Documents {
    roots: BTreeMap<PathBuf, (PathBuf, Node)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SchemaKind {
    Integer,
    Number,
    String,
    Boolean,
    Object,
    Array,
    Null,
    Conflict,
}

impl Documents {
    fn parse(sources: &[(PathBuf, String)]) -> Option<Self> {
        let mut roots = BTreeMap::new();
        for (path, input) in sources {
            let events = parse_events(input)?;
            let mut index = 0;
            let mut anchors = BTreeMap::new();
            let root = parse_node(&events, &mut index, &mut anchors)?;
            roots.insert(canonical(path), (path.clone(), root));
        }
        Some(Self { roots })
    }

    fn find_fractional_integer_literal(&self) -> Option<(PathBuf, String)> {
        self.roots
            .iter()
            .find_map(|(canonical_path, (path, root))| {
                self.find_in_schema(path, canonical_path, root, None, false)
                    .map(|literal| (path.clone(), literal))
            })
    }

    fn find_in_schema(
        &self,
        source_path: &Path,
        canonical_path: &Path,
        node: &Node,
        inherited_kind: Option<SchemaKind>,
        conjunction_branch: bool,
    ) -> Option<String> {
        let Node::Mapping(entries) = node else {
            return None;
        };
        let local_kind =
            self.effective_kind(source_path, canonical_path, node, &mut BTreeSet::new());
        let effective_kind = intersect_kinds(inherited_kind, local_kind);
        let nullable_projection = nullable_non_null_node(entries).is_some();
        if effective_kind == Some(SchemaKind::Integer) {
            // `const`/`enum` are illegal siblings of every `oneOf`, including
            // nullability; leave those to the owning sibling diagnostic. The
            // wrapper-level scalar keyword that legitimately projects is
            // `default`.
            if !nullable_projection && let Some(literal) = fractional_assertion_keyword(entries) {
                return Some(literal);
            }
            // `default` is an annotation with last-wins merge semantics. Check
            // the surviving value once at the conjunction root rather than
            // rejecting an earlier branch that normalization overwrites.
            if !conjunction_branch
                && let Some(default) =
                    self.merged_default(source_path, canonical_path, node, &mut BTreeSet::new())
                && let Some(literal) = fractional_numeric_scalar(&default)
            {
                return Some(format!("`default` value {literal}"));
            }
        }

        // Ordinary child schema positions start a fresh instance context. An
        // `allOf` branch is different: it constrains the same instance, so the
        // effective kind of the whole conjunction is inherited by every branch.
        for keyword in [
            "items",
            "additionalProperties",
            "contains",
            "propertyNames",
            "not",
        ] {
            if let Some(child) = mapping_value(entries, keyword) {
                let inherited = if keyword == "contains" {
                    mapping_value(entries, "items")
                        .and_then(|items| {
                            self.effective_kind(
                                source_path,
                                canonical_path,
                                items,
                                &mut BTreeSet::new(),
                            )
                        })
                        .filter(|kind| *kind == SchemaKind::Integer)
                } else {
                    None
                };
                if let Some(found) =
                    self.find_in_schema(source_path, canonical_path, child, inherited, false)
                {
                    return Some(found);
                }
            }
        }
        if let Some(Node::Sequence(branches)) = mapping_value(entries, "oneOf") {
            for branch in branches {
                if let Some(found) =
                    self.find_in_schema(source_path, canonical_path, branch, None, false)
                {
                    return Some(found);
                }
            }
        }
        if let Some(Node::Sequence(branches)) = mapping_value(entries, "allOf") {
            for branch in branches {
                if let Some(found) =
                    self.find_in_schema(source_path, canonical_path, branch, effective_kind, true)
                {
                    return Some(found);
                }
            }
        }
        for keyword in ["properties", "$defs"] {
            if let Some(Node::Mapping(children)) = mapping_value(entries, keyword) {
                for (_, child) in children {
                    if let Some(found) =
                        self.find_in_schema(source_path, canonical_path, child, None, false)
                    {
                        return Some(found);
                    }
                }
            }
        }

        // Nexus envelope operation input/output positions are schemas too.
        if let Some(Node::Mapping(services)) = mapping_value(entries, "services") {
            for (_, service) in services {
                let Node::Mapping(service) = service else {
                    continue;
                };
                let Some(Node::Mapping(operations)) = mapping_value(service, "operations") else {
                    continue;
                };
                for (_, operation) in operations {
                    let Node::Mapping(operation) = operation else {
                        continue;
                    };
                    for keyword in ["input", "output"] {
                        if let Some(child) = mapping_value(operation, keyword)
                            && let Some(found) =
                                self.find_in_schema(source_path, canonical_path, child, None, false)
                        {
                            return Some(found);
                        }
                    }
                }
            }
        }
        None
    }

    fn effective_kind(
        &self,
        source_path: &Path,
        canonical_path: &Path,
        node: &Node,
        visiting_refs: &mut BTreeSet<(PathBuf, String)>,
    ) -> Option<SchemaKind> {
        let Node::Mapping(entries) = node else {
            return None;
        };
        let mut kind = mapping_value(entries, "type")
            .and_then(yaml_string_scalar)
            .and_then(schema_kind);
        if let Some(reference) = mapping_value(entries, "$ref").and_then(yaml_string_scalar)
            && let Some((target_source_path, target_canonical_path, target, identity)) =
                self.resolve_reference(source_path, canonical_path, reference)
            && visiting_refs.insert(identity.clone())
        {
            kind = intersect_kinds(
                kind,
                self.effective_kind(
                    target_source_path,
                    target_canonical_path,
                    target,
                    visiting_refs,
                ),
            );
            visiting_refs.remove(&identity);
        }
        if let Some(Node::Sequence(branches)) = mapping_value(entries, "allOf") {
            for branch in branches {
                kind = intersect_kinds(
                    kind,
                    self.effective_kind(source_path, canonical_path, branch, visiting_refs),
                );
            }
        }
        // The only `oneOf` that projects to one effective kind is the exact
        // nullability wrapper: one exact `{type: "null"}` branch and one non-null
        // branch, in either order. General unions remain unclassified.
        if let Some(non_null) = nullable_non_null_node(entries) {
            kind = intersect_kinds(
                kind,
                self.effective_kind(source_path, canonical_path, non_null, visiting_refs),
            );
        }
        kind
    }

    fn merged_default(
        &self,
        source_path: &Path,
        canonical_path: &Path,
        node: &Node,
        visiting_refs: &mut BTreeSet<(PathBuf, String)>,
    ) -> Option<Node> {
        let Node::Mapping(entries) = node else {
            return None;
        };
        let mut default = None;
        if let Some(reference) = mapping_value(entries, "$ref").and_then(yaml_string_scalar)
            && let Some((target_source_path, target_canonical_path, target, identity)) =
                self.resolve_reference(source_path, canonical_path, reference)
            && visiting_refs.insert(identity.clone())
        {
            default = self.merged_default(
                target_source_path,
                target_canonical_path,
                target,
                visiting_refs,
            );
            visiting_refs.remove(&identity);
        }
        if let Some(Node::Sequence(branches)) = mapping_value(entries, "allOf") {
            for branch in branches {
                if let Some(branch_default) =
                    self.merged_default(source_path, canonical_path, branch, visiting_refs)
                {
                    default = Some(branch_default);
                }
            }
        }
        // A node's own keywords are the final branch in normalization, so its
        // annotation wins over both its `$ref` target and explicit `allOf`.
        if let Some(own_default) = mapping_value(entries, "default") {
            default = Some(own_default.clone());
        }
        default
    }

    fn resolve_reference<'a>(
        &'a self,
        source_path: &Path,
        canonical_path: &Path,
        reference: &str,
    ) -> Option<(&'a Path, &'a Path, &'a Node, (PathBuf, String))> {
        let (file_part, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        let target_path = if file_part.is_empty() {
            canonical_path.to_path_buf()
        } else {
            if Path::new(file_part).is_absolute() {
                return None;
            }
            canonical(
                &source_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(file_part),
            )
        };
        let (target_canonical_path, (target_source_path, root)) =
            self.roots.get_key_value(&target_path)?;
        let target = resolve_pointer(root, pointer)?;
        Some((
            target_source_path.as_path(),
            target_canonical_path.as_path(),
            target,
            (target_canonical_path.clone(), pointer.to_string()),
        ))
    }
}

fn schema_kind(kind: &str) -> Option<SchemaKind> {
    match kind {
        "integer" => Some(SchemaKind::Integer),
        "number" => Some(SchemaKind::Number),
        "string" => Some(SchemaKind::String),
        "boolean" => Some(SchemaKind::Boolean),
        "object" => Some(SchemaKind::Object),
        "array" => Some(SchemaKind::Array),
        "null" => Some(SchemaKind::Null),
        _ => None,
    }
}

fn intersect_kinds(left: Option<SchemaKind>, right: Option<SchemaKind>) -> Option<SchemaKind> {
    match (left, right) {
        (None, kind) | (kind, None) => kind,
        (Some(left), Some(right)) if left == right => Some(left),
        (Some(SchemaKind::Integer), Some(SchemaKind::Number))
        | (Some(SchemaKind::Number), Some(SchemaKind::Integer)) => Some(SchemaKind::Integer),
        _ => Some(SchemaKind::Conflict),
    }
}

fn resolve_pointer<'a>(root: &'a Node, pointer: &str) -> Option<&'a Node> {
    if pointer.is_empty() {
        return Some(root);
    }
    let tokens = pointer
        .strip_prefix('/')?
        .split('/')
        .map(decode_pointer_token)
        .collect::<Option<Vec<_>>>()?;
    // Match the loader's schema-position-only reference grammar: a fragment
    // may name the root or a (possibly nested) `$defs` entry, never arbitrary
    // annotation/data positions that merely look schema-shaped.
    if tokens.len() < 2
        || tokens.len() % 2 != 0
        || tokens.iter().step_by(2).any(|token| token != "$defs")
    {
        return None;
    }
    let mut current = root;
    for token in tokens {
        let Node::Mapping(entries) = current else {
            return None;
        };
        current = mapping_value(entries, &token)?;
    }
    Some(current)
}

fn decode_pointer_token(token: &str) -> Option<String> {
    let mut decoded = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        match chars.next()? {
            '0' => decoded.push('~'),
            '1' => decoded.push('/'),
            _ => return None,
        }
    }
    Some(decoded)
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize(path))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if out.file_name().is_some() {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn parse_events(input: &str) -> Option<Vec<Event>> {
    let mut parser = MaybeUninit::<yaml_parser_t>::uninit();
    // SAFETY: libyaml's parser is initialized before use, borrows `input` only
    // for this function's duration, and every produced event/parser is deleted
    // exactly once on both success and failure paths.
    unsafe {
        if !yaml_parser_initialize(parser.as_mut_ptr()).ok {
            return None;
        }
        let mut parser = parser.assume_init();
        yaml_parser_set_input_string(&mut parser, input.as_ptr(), input.len() as u64);
        let mut out = Vec::new();
        loop {
            let mut raw = MaybeUninit::<yaml_event_t>::uninit();
            if !yaml_parser_parse(&mut parser, raw.as_mut_ptr()).ok {
                yaml_parser_delete(&mut parser);
                return None;
            }
            let mut raw = raw.assume_init();
            let finished = raw.type_ == YAML_STREAM_END_EVENT;
            let event = match raw.type_ {
                YAML_SCALAR_EVENT => {
                    let scalar = raw.data.scalar;
                    let bytes = slice::from_raw_parts(scalar.value, scalar.length as usize);
                    let text = String::from_utf8_lossy(bytes).into_owned();
                    let plain = scalar.style == YAML_PLAIN_SCALAR_STYLE;
                    let tag = c_string(scalar.tag);
                    Some(Event::Scalar {
                        kind: scalar_kind(&text, plain, tag.as_deref()),
                        text,
                        anchor: c_string(scalar.anchor),
                    })
                }
                YAML_ALIAS_EVENT => Some(Event::Alias(
                    c_string(raw.data.alias.anchor).unwrap_or_default(),
                )),
                YAML_SEQUENCE_START_EVENT => Some(Event::SequenceStart(c_string(
                    raw.data.sequence_start.anchor,
                ))),
                YAML_SEQUENCE_END_EVENT => Some(Event::SequenceEnd),
                YAML_MAPPING_START_EVENT => {
                    Some(Event::MappingStart(c_string(raw.data.mapping_start.anchor)))
                }
                YAML_MAPPING_END_EVENT => Some(Event::MappingEnd),
                YAML_STREAM_START_EVENT
                | YAML_STREAM_END_EVENT
                | YAML_DOCUMENT_START_EVENT
                | YAML_DOCUMENT_END_EVENT => None,
                _ => None,
            };
            if let Some(event) = event {
                out.push(event);
            }
            yaml_event_delete(&mut raw);
            if finished {
                yaml_parser_delete(&mut parser);
                return Some(out);
            }
        }
    }
}

unsafe fn c_string(pointer: *mut u8) -> Option<String> {
    if pointer.is_null() {
        None
    } else {
        // SAFETY: anchor pointers in a live libyaml event are NUL-terminated;
        // the caller copies the string before deleting the event.
        Some(
            unsafe { CStr::from_ptr(pointer.cast()) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn parse_node(
    events: &[Event],
    index: &mut usize,
    anchors: &mut BTreeMap<String, Node>,
) -> Option<Node> {
    let event = events.get(*index)?;
    *index += 1;
    match event {
        Event::Scalar { text, kind, anchor } => {
            let node = Node::Scalar {
                text: text.clone(),
                kind: *kind,
            };
            remember_anchor(anchors, anchor, &node);
            Some(node)
        }
        Event::Alias(anchor) => anchors
            .get(anchor)
            .cloned()
            .or_else(|| Some(Node::Alias(anchor.clone()))),
        Event::SequenceStart(anchor) => {
            let mut values = Vec::new();
            while !matches!(events.get(*index), Some(Event::SequenceEnd)) {
                values.push(parse_node(events, index, anchors)?);
            }
            *index += 1;
            let node = Node::Sequence(values);
            remember_anchor(anchors, anchor, &node);
            Some(node)
        }
        Event::MappingStart(anchor) => {
            let mut entries = Vec::new();
            while !matches!(events.get(*index), Some(Event::MappingEnd)) {
                let key = parse_node(events, index, anchors)?;
                let value = parse_node(events, index, anchors)?;
                entries.push((key, value));
            }
            *index += 1;
            let node = Node::Mapping(entries);
            remember_anchor(anchors, anchor, &node);
            Some(node)
        }
        Event::SequenceEnd | Event::MappingEnd => None,
    }
}

fn remember_anchor(anchors: &mut BTreeMap<String, Node>, anchor: &Option<String>, node: &Node) {
    if let Some(anchor) = anchor {
        anchors.insert(anchor.clone(), node.clone());
    }
}

fn scalar_kind(text: &str, plain: bool, tag: Option<&str>) -> ScalarKind {
    // An explicit core tag defines the YAML value kind even when the scalar's
    // presentation style would ordinarily imply a string (for example
    // `!!float '1.5'`). Style and implicit resolution apply only without a tag.
    match tag {
        Some("tag:yaml.org,2002:str") => ScalarKind::String,
        Some("tag:yaml.org,2002:float") => ScalarKind::Number,
        Some("tag:yaml.org,2002:int") => ScalarKind::ExplicitInteger,
        Some(_) => ScalarKind::Other,
        None if !plain => ScalarKind::String,
        None => match serde_yaml::from_str::<serde_yaml::Value>(text) {
            Ok(serde_yaml::Value::String(_)) => ScalarKind::String,
            Ok(serde_yaml::Value::Number(_)) => ScalarKind::Number,
            _ => ScalarKind::Other,
        },
    }
}

fn yaml_string_scalar(node: &Node) -> Option<&str> {
    match node {
        Node::Scalar {
            text,
            kind: ScalarKind::String,
        } => Some(text),
        Node::Alias(anchor) => {
            let _ = anchor;
            None
        }
        Node::Scalar { .. } | Node::Sequence(_) | Node::Mapping(_) => None,
    }
}

fn mapping_value<'a>(entries: &'a [(Node, Node)], key: &str) -> Option<&'a Node> {
    entries.iter().find_map(|(candidate, value)| {
        (yaml_string_scalar(candidate) == Some(key)).then_some(value)
    })
}

fn nullable_non_null_node(entries: &[(Node, Node)]) -> Option<&Node> {
    let Node::Sequence(branches) = mapping_value(entries, "oneOf")? else {
        return None;
    };
    let [first, second] = branches.as_slice() else {
        return None;
    };
    match (is_exact_null_schema(first), is_exact_null_schema(second)) {
        (true, false) => Some(second),
        (false, true) => Some(first),
        _ => None,
    }
}

fn is_exact_null_schema(node: &Node) -> bool {
    let Node::Mapping(entries) = node else {
        return false;
    };
    entries.len() == 1
        && mapping_value(entries, "type").and_then(yaml_string_scalar) == Some("null")
}

fn fractional_assertion_keyword(entries: &[(Node, Node)]) -> Option<String> {
    if let Some(value) = mapping_value(entries, "const")
        && let Some(literal) = fractional_numeric_scalar(value)
    {
        return Some(format!("`const` value {literal}"));
    }
    if let Some(Node::Sequence(values)) = mapping_value(entries, "enum") {
        for value in values {
            if let Some(literal) = fractional_numeric_scalar(value) {
                return Some(format!("`enum` value {literal}"));
            }
        }
    }
    None
}

fn fractional_numeric_scalar(node: &Node) -> Option<String> {
    let Node::Scalar {
        text,
        kind: ScalarKind::Number,
    } = node
    else {
        return None;
    };
    if !matches!(
        serde_yaml::from_str::<serde_yaml::Value>(text),
        Ok(serde_yaml::Value::Number(_))
    ) || written_number_is_integer(text)
    {
        return None;
    }
    Some(text.clone())
}

fn written_number_is_integer(value: &str) -> bool {
    let value = value.replace('_', "");
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(&value);
    if unsigned.starts_with("0x") || unsigned.starts_with("0o") || unsigned.starts_with("0b") {
        return true;
    }
    let (mantissa, exponent) =
        unsigned
            .split_once(['e', 'E'])
            .map_or((unsigned, 0_i64), |(mantissa, exponent)| {
                let exponent = exponent.parse::<i64>().unwrap_or_else(|_| {
                    if exponent.starts_with('-') {
                        i64::MIN
                    } else {
                        i64::MAX
                    }
                });
                (mantissa, exponent)
            });
    let Some((whole, fraction)) = mantissa.split_once('.') else {
        return true;
    };
    let digits = format!("{whole}{fraction}");
    if digits.bytes().all(|byte| byte == b'0') {
        return true;
    }
    let fractional_places = fraction.len() as i64;
    if exponent >= fractional_places {
        return true;
    }
    if exponent < 0 {
        return false;
    }
    let retained_fraction = (fractional_places - exponent) as usize;
    digits[digits.len() - retained_fraction..]
        .bytes()
        .all(|byte| byte == b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_fraction_that_binary64_rounds_away() {
        for literal in [
            "4503599627370496.5",
            "!!float '4503599627370496.5'",
            "!<tag:yaml.org,2002:float> '4503599627370496.5'",
        ] {
            assert_eq!(
                fractional_integer_literal(&format!("type: integer\nconst: {literal}")),
                Some("`const` value 4503599627370496.5".to_string()),
                "{literal}"
            );
        }
        for literal in [
            "4503599627370496.0",
            "!!float '4503599627370496.0'",
            "!!int '4503599627370496'",
            "!!str '4503599627370496.5'",
            "!!null 'null'",
        ] {
            assert_eq!(
                fractional_integer_literal(&format!("type: integer\nconst: {literal}")),
                None,
                "{literal}"
            );
        }
    }

    #[test]
    fn follows_effective_integer_type_across_conjunctions() {
        for keyword in [
            "const: 4503599627370496.5",
            "default: 4503599627370496.5",
            "enum: [1, 4503599627370496.5]",
        ] {
            let schema = format!("allOf:\n  - {{ type: integer }}\n  - {{ {keyword} }}");
            assert!(fractional_integer_literal(&schema).is_some(), "{keyword}");
        }
        assert_eq!(
            fractional_integer_literal(
                "allOf:\n  - { type: integer }\n  - { const: 4503599627370496.0 }"
            ),
            None
        );
        assert_eq!(
            fractional_integer_literal(
                "allOf:\n  - { type: integer }\n  - { default: 4503599627370496.5 }\n  - { default: 4503599627370496.0 }"
            ),
            None
        );
        assert!(
            fractional_integer_literal(
                "allOf:\n  - { type: integer }\n  - { default: 4503599627370496.0 }\n  - { default: 4503599627370496.5 }"
            )
            .is_some()
        );
    }

    #[test]
    fn follows_effective_integer_type_across_external_ref_siblings() {
        let sources = [
            (
                PathBuf::from("defs.yaml"),
                "$defs:\n  Whole: { type: integer }".to_string(),
            ),
            (
                PathBuf::from("api.yaml"),
                "$ref: defs.yaml#/$defs/Whole\nconst: 4503599627370496.5".to_string(),
            ),
        ];
        assert_eq!(
            fractional_integer_literal_in_sources(&sources),
            Some((
                PathBuf::from("api.yaml"),
                "`const` value 4503599627370496.5".to_string()
            ))
        );
    }

    #[test]
    fn projects_only_the_exact_nullable_integer_union() {
        for one_of in [
            "oneOf:\n  - { type: integer }\n  - { type: \"null\" }",
            "oneOf:\n  - { type: \"null\" }\n  - { type: integer }",
        ] {
            assert!(
                fractional_integer_literal(&format!("{one_of}\ndefault: 4503599627370496.5"))
                    .is_some()
            );
            assert_eq!(
                fractional_integer_literal(&format!(
                    "type: array\nitems:\n{}\ncontains: {{ const: 4503599627370496.5 }}",
                    one_of
                        .lines()
                        .map(|line| format!("  {line}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )),
                Some("`const` value 4503599627370496.5".to_string())
            );
            assert_eq!(
                fractional_integer_literal(&format!("{one_of}\ndefault: 4503599627370496.0")),
                None
            );
            for illegal_sibling in ["const: 4503599627370496.5", "enum: [4503599627370496.5]"] {
                assert_eq!(
                    fractional_integer_literal(&format!("{one_of}\n{illegal_sibling}")),
                    None
                );
            }
        }

        // Neither a genuine sum type nor a malformed null branch projects a
        // scalar kind into the wrapper.
        for one_of in [
            "oneOf:\n  - { type: integer }\n  - { type: number }",
            "oneOf:\n  - { type: integer }\n  - { type: \"null\", description: not-exact }",
            "oneOf:\n  - { type: integer }\n  - { type: null }",
            "oneOf:\n  - { type: integer }\n  - { type: !!null 'null' }",
        ] {
            assert_eq!(
                fractional_integer_literal(&format!("{one_of}\ndefault: 4503599627370496.5")),
                None
            );
        }
        assert!(
            fractional_integer_literal(
                "oneOf:\n  - { type: integer }\n  - { type: !!str 'null' }\ndefault: 4503599627370496.5"
            )
            .is_some()
        );
    }

    #[test]
    fn follows_nested_and_aliased_schema_nodes() {
        assert!(
            fractional_integer_literal(
                "$defs:\n  Integer: &integer\n    type: integer\n    enum: [1, 9007199254740991.1]\n  Copy: *integer"
            )
            .is_some()
        );
    }

    #[test]
    fn ignores_type_and_const_keys_inside_annotation_data() {
        assert_eq!(
            fractional_integer_literal(
                "type: object\nproperties: {}\nexamples:\n  - type: integer\n    const: 4503599627370496.5"
            ),
            None
        );
        assert_eq!(
            fractional_integer_literal(
                "allOf:\n  - { type: integer }\n  - examples:\n      - type: integer\n        const: 4503599627370496.5"
            ),
            None
        );
        assert_eq!(
            fractional_integer_literal(
                "oneOf:\n  - { type: integer }\n  - { type: \"null\" }\ndefault: 4503599627370496.0\nexamples:\n  - { type: integer, const: 4503599627370496.5 }"
            ),
            None
        );
        assert_eq!(
            fractional_integer_literal(
                "examples:\n  schemaish: { type: integer }\n$ref: '#/examples/schemaish'\nconst: 4503599627370496.5"
            ),
            None
        );
    }
}
