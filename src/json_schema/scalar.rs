//! Language-neutral scalar matcher descriptors.
//!
//! JSON Schema uses the same scalar assertion vocabulary in several places:
//! ordinary values (including `const`, `enum`, and `default` validation), the
//! `contains` applicator, and `propertyNames`. The loader proves that a matcher
//! is in the supported subset; this descriptor is the normalized hand-off used
//! by target backends so they do not independently decide which assertions a
//! matcher contains.

use serde_json::{Number, Value};

/// A JSON scalar kind accepted by the generated model surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarKind {
    String,
    Number,
    Integer,
    Boolean,
}

impl ScalarKind {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "string" => Some(Self::String),
            "number" => Some(Self::Number),
            "integer" => Some(Self::Integer),
            "boolean" => Some(Self::Boolean),
            _ => None,
        }
    }
}

/// The supported, normalized scalar assertions on one schema node.
///
/// Fields intentionally retain JSON numbers rather than converting to `f64`:
/// each backend must render the authored decimal without losing precision, and
/// integer-valued number literals are normalized by the loader before this
/// descriptor is constructed.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ScalarMatcher {
    pub(crate) kind: Option<ScalarKind>,
    pub(crate) const_value: Option<Value>,
    pub(crate) enum_values: Vec<Value>,
    pub(crate) minimum: Option<Number>,
    pub(crate) maximum: Option<Number>,
    pub(crate) exclusive_minimum: Option<Number>,
    pub(crate) exclusive_maximum: Option<Number>,
    pub(crate) multiple_of: Option<Number>,
    pub(crate) min_length: Option<u64>,
    pub(crate) max_length: Option<u64>,
    pub(crate) pattern: Option<String>,
    pub(crate) format: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_kind_classifies_only_supported_scalar_types() {
        assert_eq!(ScalarKind::from_name("string"), Some(ScalarKind::String));
        assert_eq!(ScalarKind::from_name("integer"), Some(ScalarKind::Integer));
        assert_eq!(ScalarKind::from_name("object"), None);
        assert_eq!(ScalarKind::from_name("null"), None);
    }
}

/// The canonical decimal a numeric value contributes to a synthesized
/// identifier (a Go value constant's suffix, a Java value class's constant).
///
/// P1 makes `1`, `1.0` and `1e0` one mathematical number, so they must yield one
/// identifier: deriving the token from `Number`'s own spelling instead gave
/// `const: 1.0` on a `type: integer` the constant `Score1_0` / `V_1_0` while
/// `const: 1` gave `Score1` / `V_1`. Re-spelling a `const` is a no-op on the
/// wire, so that made a no-op schema edit rename a public constant — the
/// cross-revision instability [[const]] promises the encoding does not have
/// ("because a number token derives from the canonical round-trippable decimal,
/// the name is stable across schema revisions (P13)").
///
/// Integral values collapse to their integer spelling; everything else keeps the
/// shortest round-trippable decimal. The sign is left in place for the caller to
/// recase (`Neg` in Go, `NEG_` in Java).
pub(crate) fn value_token_decimal(number: &Number) -> String {
    // `as_i64`/`as_u64` return `None` for the `1.0` spelling, so ask the float.
    if let Some(float) = number.as_f64()
        && float.fract() == 0.0
        && float.is_finite()
        // Beyond 2^53 the `f64` no longer names a unique integer; the loader caps
        // integers there anyway, so fall through to the authored spelling.
        && float.abs() <= 9_007_199_254_740_991.0
    {
        return format!("{}", float as i64);
    }
    number.to_string()
}

#[cfg(test)]
mod value_token_decimal_tests {
    use super::value_token_decimal;
    use serde_json::json;

    fn token(value: serde_json::Value) -> String {
        value_token_decimal(value.as_number().expect("a number"))
    }

    /// Every spelling of one mathematical number yields one token, so re-spelling
    /// a `const`/`enum` member never renames its Go or Java constant.
    #[test]
    fn integral_spellings_collapse_to_one_token() {
        for spelling in [json!(1), json!(1.0)] {
            assert_eq!(token(spelling), "1");
        }
        assert_eq!(token(json!(-3.0)), "-3");
        assert_eq!(token(json!(0.0)), "0");
        assert_eq!(token(json!(-0.0)), "0", "P1 compares ±0 equal");
    }

    /// A genuine fraction keeps its decimal — the `.` is what the callers turn
    /// into `_` to keep `3_14` distinct from `314`.
    #[test]
    fn fractions_keep_their_decimal() {
        assert_eq!(token(json!(3.14)), "3.14");
        assert_eq!(token(json!(-3.14)), "-3.14");
        assert_eq!(token(json!(1.50)), "1.5", "shortest round-trippable");
    }
}
