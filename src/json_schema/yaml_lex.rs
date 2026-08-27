//! Lossless YAML scalar preflight for authored-number rules.
//!
//! `serde_yaml` necessarily hands a floating scalar to Serde as `f64`. That
//! loses the written fractional part before a `serde_json::Number` field sees
//! it (`4503599627370496.5` becomes `4503599627370496.0`). Most schema rules are
//! defined over the binary64 value and belong in the ordinary loader. The one
//! exception here is JSON Schema's written-number definition of `integer`.
//! This small event-tree pass retains scalar text just long enough to reject a
//! fractional `const`/`default`/`enum` member on a directly typed integer node.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::mem::MaybeUninit;
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
    Scalar { text: String, plain: bool },
    Sequence(Vec<Node>),
    Mapping(Vec<(Node, Node)>),
    Alias(String),
}

#[derive(Debug)]
enum Event {
    Scalar {
        text: String,
        plain: bool,
        anchor: Option<String>,
    },
    Alias(String),
    SequenceStart(Option<String>),
    SequenceEnd,
    MappingStart(Option<String>),
    MappingEnd,
}

/// Returns the first authored fractional numeric literal on a directly typed
/// integer schema node. Syntax failures are intentionally left to `serde_yaml`,
/// which owns the repository's established located parse diagnostic.
pub(crate) fn fractional_integer_literal(input: &str) -> Option<String> {
    let events = parse_events(input)?;
    let mut index = 0;
    let mut anchors = BTreeMap::new();
    let root = parse_node(&events, &mut index, &mut anchors)?;
    find_fractional_integer_literal(&root)
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
                    Some(Event::Scalar {
                        text: String::from_utf8_lossy(bytes).into_owned(),
                        plain: scalar.style == YAML_PLAIN_SCALAR_STYLE,
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
        Event::Scalar {
            text,
            plain,
            anchor,
        } => {
            let node = Node::Scalar {
                text: text.clone(),
                plain: *plain,
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

fn scalar(node: &Node) -> Option<&str> {
    match node {
        Node::Scalar { text, .. } => Some(text),
        Node::Alias(anchor) => {
            let _ = anchor;
            None
        }
        Node::Sequence(_) | Node::Mapping(_) => None,
    }
}

fn mapping_value<'a>(entries: &'a [(Node, Node)], key: &str) -> Option<&'a Node> {
    entries
        .iter()
        .find_map(|(candidate, value)| (scalar(candidate) == Some(key)).then_some(value))
}

fn find_fractional_integer_literal(node: &Node) -> Option<String> {
    let Node::Mapping(entries) = node else {
        return None;
    };
    if mapping_value(entries, "type").and_then(scalar) == Some("integer") {
        for keyword in ["const", "default"] {
            if let Some(value) = mapping_value(entries, keyword)
                && let Some(literal) = fractional_numeric_scalar(value)
            {
                return Some(format!("`{keyword}` value {literal}"));
            }
        }
        if let Some(Node::Sequence(values)) = mapping_value(entries, "enum") {
            for value in values {
                if let Some(literal) = fractional_numeric_scalar(value) {
                    return Some(format!("`enum` value {literal}"));
                }
            }
        }
    }

    // Recurse through schema positions only. Annotation payloads and arbitrary
    // foreign objects may themselves contain keys named `type`/`const`; they
    // are data, not schemas, and must never acquire schema diagnostics.
    for keyword in [
        "items",
        "additionalProperties",
        "contains",
        "propertyNames",
        "not",
    ] {
        if let Some(child) = mapping_value(entries, keyword)
            && let Some(found) = find_fractional_integer_literal(child)
        {
            return Some(found);
        }
    }
    for keyword in ["oneOf", "allOf"] {
        if let Some(Node::Sequence(branches)) = mapping_value(entries, keyword) {
            for branch in branches {
                if let Some(found) = find_fractional_integer_literal(branch) {
                    return Some(found);
                }
            }
        }
    }
    for keyword in ["properties", "$defs"] {
        if let Some(Node::Mapping(children)) = mapping_value(entries, keyword) {
            for (_, child) in children {
                if let Some(found) = find_fractional_integer_literal(child) {
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
                        && let Some(found) = find_fractional_integer_literal(child)
                    {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn fractional_numeric_scalar(node: &Node) -> Option<String> {
    let Node::Scalar { text, plain } = node else {
        return None;
    };
    if !plain
        || !matches!(
            serde_yaml::from_str::<serde_yaml::Value>(text),
            Ok(serde_yaml::Value::Number(_))
        )
        || written_number_is_integer(text)
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
        assert_eq!(
            fractional_integer_literal("type: integer\nconst: 4503599627370496.5"),
            Some("`const` value 4503599627370496.5".to_string())
        );
        assert_eq!(
            fractional_integer_literal("type: integer\nconst: 4503599627370496.0"),
            None
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
    }
}
