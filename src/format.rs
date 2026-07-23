//! The asserted string `format` subset (JSON Schema 2020-12 §7). Each supported
//! format lowers to a **generator-owned** check: a pinned, portable, RE2-safe
//! regex (proven compilable by the [[pattern]] gate, `crate::pattern`) plus —
//! where a regex alone is insufficient — a shared **length guard**. The
//! generators emit the same regex the loader validates literals against, so a
//! value accepted (or rejected) by one language is accepted (or rejected) by all
//! (P1). See `specs/json-schema/features/format.md` for the authoritative rules and
//! the `research/format_{conformance,email,hostname,uri}/` corpora that pinned
//! every pattern and length below.
//!
//! Only the string-shaped subset is asserted here: `uuid`, `ipv4`, `ipv6`,
//! `hostname`, `email`, `uri`, `uri-reference`. The temporal formats
//! (`date-time`, `date`, `time`, `duration`) are recognized but **rejected at
//! load** as "not yet supported (temporal, pending)" — materialization is a
//! separate follow-up task, and nothing must silently no-op (P10). Every other
//! standard format is **deferred** (rejected "not yet supported"); anything else
//! is an **unknown** format (rejected with a fix-it listing the supported names).

/// The RFC 3986 dotted-quad octet (`0-255`, no leading zeros), shared by `ipv4`
/// and the IPv4-tail of `ipv6`. Pinned verbatim from
/// `research/format_conformance/`.
const OCTET: &str = "(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";

/// An RFC 4291 IPv6 16-bit group (1-4 hex digits).
const H16: &str = "[0-9a-fA-F]{1,4}";

/// The names asserted here, in canonical order, for the unknown-format fix-it.
pub const SUPPORTED_FORMATS: [&str; 7] = [
    "uuid",
    "ipv4",
    "ipv6",
    "hostname",
    "email",
    "uri",
    "uri-reference",
];

/// The RFC-3339 temporal formats. These are **materialized** as idiomatic
/// native typed model fields (Go `time.Time`, Java `OffsetDateTime`, Python
/// `datetime`, …) rather than a bare `string`, and asserted with a **narrowed**
/// grammar (leap second `:60` rejected; `duration` is time-only). See
/// `specs/json-schema/features/format.md` (Materialization) and `TemporalKind`.
pub const TEMPORAL_FORMATS: [&str; 4] = ["date-time", "date", "time", "duration"];

/// The four materialized temporal formats. Each carries a language-native typed
/// field; the wire is produced by re-serializing it through a generator-owned
/// serializer. The asserted grammar is the **narrowed** materialized grammar
/// (see the `materialized_pattern` docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalKind {
    /// RFC 3339 `date-time`, **offset required**, `:60` rejected.
    DateTime,
    /// RFC 3339 full-date `YYYY-MM-DD`.
    Date,
    /// RFC 3339 `partial-time` with optional offset, `:60` rejected.
    Time,
    /// ISO 8601 duration narrowed to **time-only** `PT…H…M…S`.
    Duration,
}

impl TemporalKind {
    /// The `TemporalKind` for a format name, or `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "date-time" => Some(Self::DateTime),
            "date" => Some(Self::Date),
            "time" => Some(Self::Time),
            "duration" => Some(Self::Duration),
            _ => None,
        }
    }

    /// The canonical format name.
    pub fn name(self) -> &'static str {
        match self {
            Self::DateTime => "date-time",
            Self::Date => "date",
            Self::Time => "time",
            Self::Duration => "duration",
        }
    }

    /// The pinned, anchored (`^…$`) **materialized** regex for this kind — the
    /// narrowed grammar (leap second `:60` excluded by the `[0-5][0-9]` seconds
    /// group; `date-time` offset required; `duration` time-only). `T`/`Z`
    /// separators are accepted in either case. Emitted (with the per-target
    /// end-anchor rewrite) into each generator's parse adapter, where the wire
    /// string's `:60` / offset / precision are still observable.
    pub fn pattern(self) -> &'static str {
        materialized_pattern(self)
    }
}

/// The pinned materialized regex for a temporal kind. See `TemporalKind::pattern`.
pub fn materialized_pattern(kind: TemporalKind) -> &'static str {
    match kind {
        TemporalKind::Date => "^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$",
        TemporalKind::Time => {
            "^([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\\.[0-9]+)?([Zz]|[+-]([01][0-9]|2[0-3]):[0-5][0-9])?$"
        }
        TemporalKind::DateTime => {
            "^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])[Tt]([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\\.[0-9]+)?([Zz]|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$"
        }
        TemporalKind::Duration => {
            "^PT(?:[0-9]+H(?:[0-9]+M(?:[0-9]+S)?)?|[0-9]+M(?:[0-9]+S)?|[0-9]+S)$"
        }
    }
}

/// The Gregorian leap-year rule.
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in a (1-based) month of `year`, or `None` for an out-of-range month.
pub fn days_in_month(year: i64, month: u32) -> Option<u32> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => return None,
    })
}

/// The `time.Duration` (Go int64-nanosecond) capacity — the uniform cross-language
/// overflow cap for a materialized `duration` (P1: identical accept/reject in
/// every target). A time-only duration whose total nanoseconds exceed this is a
/// load/parse `Violation`.
pub const MAX_DURATION_NANOS: i128 = i64::MAX as i128;

/// The date/date-time calendar predicate over `YYYY-MM-DD…`: month `01–12` and
/// day within the month's Gregorian length. The regex has already guaranteed the
/// digit shape and the month/day *ranges*; this adds day-in-month + leap-year.
fn valid_calendar_prefix(value: &str) -> bool {
    // value starts with `YYYY-MM-DD`; slice fixed ASCII positions.
    let bytes = value.as_bytes();
    if bytes.len() < 10 {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        value[0..4].parse::<i64>(),
        value[5..7].parse::<u32>(),
        value[8..10].parse::<u32>(),
    ) else {
        return false;
    };
    match days_in_month(year, month) {
        Some(max) => (1..=max).contains(&day),
        None => false,
    }
}

/// The total-nanoseconds of a materialized (time-only) `duration`, or `None` if
/// it overflows the uniform `MAX_DURATION_NANOS` cap. The input must already have
/// matched the materialized duration regex.
pub fn duration_total_nanos(value: &str) -> Option<i128> {
    // Strip leading "PT".
    let body = value.strip_prefix("PT")?;
    let mut total_seconds: i128 = 0;
    let mut number = String::new();
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let magnitude: i128 = number.parse().ok()?;
        number.clear();
        let unit_seconds: i128 = match ch {
            'H' => 3600,
            'M' => 60,
            'S' => 1,
            _ => return None,
        };
        total_seconds = total_seconds.checked_add(magnitude.checked_mul(unit_seconds)?)?;
        if total_seconds > MAX_DURATION_NANOS {
            return None;
        }
    }
    let total_nanos = total_seconds.checked_mul(1_000_000_000)?;
    if total_nanos > MAX_DURATION_NANOS {
        return None;
    }
    Some(total_nanos)
}

/// The generator-owned canonical serialization of a materialized (time-only)
/// `duration`: decompose the total seconds into `PT…H…M…S`, omitting zero
/// components; the whole-zero duration is `PT0S`. Non-canonical inputs
/// canonicalize (`PT90M` → `PT1H30M`, `PT3600S` → `PT1H`). Returns `None` on
/// overflow. Used at load to echo `const`/`default`/`enum` literals in their
/// serialized form.
pub fn canonicalize_duration(value: &str) -> Option<String> {
    let total_nanos = duration_total_nanos(value)?;
    let total_seconds = (total_nanos / 1_000_000_000) as i128;
    Some(format_duration_seconds(total_seconds))
}

/// Formats a whole-second total into the canonical `PT…H…M…S` form.
pub fn format_duration_seconds(total_seconds: i128) -> String {
    if total_seconds == 0 {
        return "PT0S".to_string();
    }
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    let mut out = String::from("PT");
    if hours != 0 {
        out.push_str(&format!("{hours}H"));
    }
    if minutes != 0 {
        out.push_str(&format!("{minutes}M"));
    }
    if seconds != 0 {
        out.push_str(&format!("{seconds}S"));
    }
    out
}

/// The runtime-equivalent verdict for a value under a **materialized** temporal
/// format: the pinned narrowed regex plus the calendar predicate (`date` /
/// `date-time`) or the overflow guard (`duration`). Used at load to validate
/// `const`/`default`/`enum` literals and as the oracle for regression tests.
pub fn is_valid_materialized(kind: TemporalKind, value: &str) -> bool {
    let matched = regex::Regex::new(materialized_pattern(kind))
        .expect("pinned materialized pattern compiles")
        .is_match(value);
    if !matched {
        return false;
    }
    match kind {
        TemporalKind::Date | TemporalKind::DateTime => valid_calendar_prefix(value),
        TemporalKind::Time => true,
        TemporalKind::Duration => duration_total_nanos(value).is_some(),
    }
}

/// Standard 2020-12 formats we do not (yet) assert: they need IDNA/Unicode
/// handling or are niche. Rejected at load as "not yet supported" (distinct from
/// an unknown/typo format).
pub const DEFERRED_FORMATS: [&str; 8] = [
    "idn-email",
    "idn-hostname",
    "iri",
    "iri-reference",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// The pinned, generator-owned check for a supported string format: the anchored
/// (`^…$`) RE2-safe regex plus an optional total-length guard (in Unicode code
/// points). The regex is emitted verbatim to Go/JS and with the [[pattern]]
/// per-target end-anchor rewrite (`$`→`\Z`/`\z`) to Python/Java.
#[derive(Debug, Clone)]
pub struct FormatCheck {
    /// The canonical format name (for the `must be a valid <name>` reason).
    pub name: &'static str,
    /// The anchored `^…$` pinned regex (pre per-target end-anchor rewrite).
    pub pattern: String,
    /// Total-length guard in code points, run **before** the regex where present
    /// (`hostname` ≤253, `email` ≤254; the email order neutralizes a Java
    /// matcher StackOverflow hazard on adversarial inputs).
    pub max_code_points: Option<usize>,
}

/// The load-time classification of a `format` value.
pub enum FormatClass {
    /// An asserted string format lowering to a pinned check.
    Supported(FormatCheck),
    /// A recognized RFC-3339 temporal format, **materialized** into a native
    /// typed field with the narrowed grammar.
    Temporal(TemporalKind),
    /// A recognized-but-deferred standard format.
    Deferred,
    /// An unrecognized / non-standard format name.
    Unknown,
}

fn ipv6_body() -> String {
    let v4 = format!("({OCTET}\\.{OCTET}\\.{OCTET}\\.{OCTET})");
    let ls32 = format!("({H16}:{H16}|{v4})");
    format!(
        "({H16}:){{6}}{ls32}|\
         ::({H16}:){{5}}{ls32}|\
         ({H16})?::({H16}:){{4}}{ls32}|\
         (({H16}:){{0,1}}{H16})?::({H16}:){{3}}{ls32}|\
         (({H16}:){{0,2}}{H16})?::({H16}:){{2}}{ls32}|\
         (({H16}:){{0,3}}{H16})?::({H16}:){ls32}|\
         (({H16}:){{0,4}}{H16})?::{ls32}|\
         (({H16}:){{0,5}}{H16})?::{H16}|\
         (({H16}:){{0,6}}{H16})?::"
    )
}

/// The pinned URI (`uri`, scheme required) / URI-reference RFC 3986 ASCII body,
/// pinned verbatim from `research/format_uri/pinned_body{,_uriref}.json`. The
/// IP-literal host `[…]` splices the `ipv6` grammar so `http://[1::2::3]`
/// rejects. Wrapped in `^(?:…)$` for anchored matching.
fn uri_body(reference: bool) -> &'static str {
    if reference {
        include_str!("../specs/json-schema/research/format_uri/pinned_body_uriref.body")
    } else {
        include_str!("../specs/json-schema/research/format_uri/pinned_body.body")
    }
}

/// Returns the pinned check for a supported string format, or `None`.
pub fn check_for(name: &str) -> Option<FormatCheck> {
    let anchored = |body: String| format!("^(?:{body})$");
    match name {
        "uuid" => Some(FormatCheck {
            name: "uuid",
            pattern:
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
                    .to_string(),
            max_code_points: None,
        }),
        "ipv4" => Some(FormatCheck {
            name: "ipv4",
            pattern: format!("^{OCTET}\\.{OCTET}\\.{OCTET}\\.{OCTET}$"),
            max_code_points: None,
        }),
        "ipv6" => Some(FormatCheck {
            name: "ipv6",
            pattern: anchored(ipv6_body()),
            max_code_points: None,
        }),
        "hostname" => Some(FormatCheck {
            name: "hostname",
            pattern: "^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$"
                .to_string(),
            max_code_points: Some(253),
        }),
        "email" => Some(FormatCheck {
            name: "email",
            pattern: "^[a-zA-Z0-9!#$%&'*+/=?^_`{|}~-]+(?:\\.[a-zA-Z0-9!#$%&'*+/=?^_`{|}~-]+)*@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+$"
                .to_string(),
            max_code_points: Some(254),
        }),
        "uri" => Some(FormatCheck {
            name: "uri",
            pattern: anchored(uri_body(false).trim().to_string()),
            max_code_points: None,
        }),
        "uri-reference" => Some(FormatCheck {
            name: "uri-reference",
            pattern: anchored(uri_body(true).trim().to_string()),
            max_code_points: None,
        }),
        _ => None,
    }
}

/// Classifies a `format` name for the load gate.
pub fn classify(name: &str) -> FormatClass {
    if let Some(check) = check_for(name) {
        FormatClass::Supported(check)
    } else if let Some(kind) = TemporalKind::from_name(name) {
        FormatClass::Temporal(kind)
    } else if DEFERRED_FORMATS.contains(&name) {
        FormatClass::Deferred
    } else {
        FormatClass::Unknown
    }
}

/// Runtime-equivalent verdict for a value under a supported format: the shared
/// predicate the generators emit (length guard first, then the pinned regex).
/// Used at load to validate `const`/`default`/`enum` literals and as the oracle
/// for the corpus regression tests. Returns `true` for any non-supported format
/// (the load gate rejects those separately).
pub fn is_valid(name: &str, value: &str) -> bool {
    if let Some(kind) = TemporalKind::from_name(name) {
        return is_valid_materialized(kind, value);
    }
    let Some(check) = check_for(name) else {
        return true;
    };
    if let Some(max) = check.max_code_points
        && value.chars().count() > max
    {
        return false;
    }
    // The load gate proves the pinned pattern compiles; recompiling here (load
    // path only) is fine.
    regex::Regex::new(&check.pattern)
        .expect("pinned format pattern compiles")
        .is_match(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_patterns_pass_the_pattern_gate() {
        for name in SUPPORTED_FORMATS {
            let check = check_for(name).expect("supported");
            crate::pattern::gate_and_normalize(&check.pattern).unwrap_or_else(|error| {
                panic!("{name} pinned pattern rejected by gate: {error:?}")
            });
        }
    }

    #[test]
    fn uuid_accepts_canonical_rejects_malformed() {
        assert!(is_valid("uuid", "de305d54-75b4-431b-adb2-eb6b9e546013"));
        assert!(!is_valid("uuid", "not-a-uuid"));
        assert!(!is_valid("uuid", "de305d54-75b4-431b-adb2-eb6b9e54601")); // 11 in last
    }

    #[test]
    fn ipv4_rejects_out_of_range_and_leading_zero() {
        assert!(is_valid("ipv4", "192.168.0.1"));
        assert!(!is_valid("ipv4", "256.0.0.1"));
        assert!(!is_valid("ipv4", "01.2.3.4"));
    }

    #[test]
    fn ipv6_rejects_double_colon() {
        assert!(is_valid("ipv6", "2001:db8::1"));
        assert!(is_valid("ipv6", "::1"));
        assert!(is_valid("ipv6", "::ffff:192.0.2.1"));
        assert!(!is_valid("ipv6", "1::2::3"));
        assert!(!is_valid("ipv6", "zzzz"));
    }

    #[test]
    fn email_rejects_single_label_domain_and_enforces_length() {
        assert!(is_valid("email", "user@example.com"));
        assert!(!is_valid("email", "user@localhost"));
        let huge = format!("{}@example.com", "a".repeat(300));
        assert!(!is_valid("email", &huge));
    }

    #[test]
    fn hostname_enforces_length_and_rejects_trailing_dot() {
        assert!(is_valid("hostname", "example.com"));
        assert!(!is_valid("hostname", "example.com."));
        let long_label = "a".repeat(64);
        assert!(!is_valid("hostname", &long_label));
        let over = (0..64).map(|_| "abc").collect::<Vec<_>>().join(".");
        assert!(over.chars().count() > 253);
        assert!(!is_valid("hostname", &over));
    }

    #[test]
    fn uri_splices_ipv6_and_requires_scheme() {
        assert!(is_valid("uri", "https://example.com/path?q=1#frag"));
        assert!(is_valid("uri", "http://[2001:db8::1]/"));
        assert!(is_valid("uri", "http://[::1]"));
        assert!(!is_valid("uri", "http://[1::2::3]"));
        assert!(!is_valid("uri", "//example.com/no-scheme"));
        assert!(!is_valid("uri", "http://example.com/%2"));
    }

    #[test]
    fn uri_reference_allows_relative() {
        assert!(is_valid("uri-reference", "/relative/path"));
        assert!(is_valid("uri-reference", "https://example.com"));
        assert!(!is_valid("uri-reference", "http://[1::2::3]"));
    }

    #[test]
    fn classify_partitions_names() {
        assert!(matches!(classify("uuid"), FormatClass::Supported(_)));
        assert!(matches!(
            classify("date-time"),
            FormatClass::Temporal(TemporalKind::DateTime)
        ));
        assert!(matches!(
            classify("duration"),
            FormatClass::Temporal(TemporalKind::Duration)
        ));
        assert!(matches!(classify("iri"), FormatClass::Deferred));
        assert!(matches!(classify("phone"), FormatClass::Unknown));
        assert!(matches!(classify("datetime"), FormatClass::Unknown));
    }

    /// The materialized temporal grammar agrees with `format_conformance`,
    /// **except** the two leap-second rows (`:60`), which the narrowed
    /// materialized grammar rejects (native types cannot hold `:60`).
    #[test]
    fn materialized_temporal_conformance_with_leap_narrowing() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../specs/json-schema/research/format_conformance/corpus.json"
        ))
        .expect("corpus parses");
        for pair in corpus["pairs"].as_array().expect("pairs") {
            let format = pair["format"].as_str().unwrap_or_default();
            let Some(kind) = TemporalKind::from_name(format) else {
                continue;
            };
            let value = pair["value"].as_str().expect("value string");
            let mut expect = pair["expect_valid"].as_bool().expect("expect_valid");
            // Materialized narrowing: leap second `:60` is rejected.
            if value.contains(":60") {
                expect = false;
            }
            assert_eq!(
                is_valid_materialized(kind, value),
                expect,
                "materialized {format} {value:?}"
            );
        }
    }

    #[test]
    fn materialized_duration_time_only_and_canonicalizes() {
        // Time-only accepted.
        assert!(is_valid_materialized(TemporalKind::Duration, "PT1H30M15S"));
        assert!(is_valid_materialized(TemporalKind::Duration, "PT0S"));
        assert!(is_valid_materialized(TemporalKind::Duration, "PT90M"));
        // Calendar components rejected (narrowed).
        assert!(!is_valid_materialized(TemporalKind::Duration, "P1Y"));
        assert!(!is_valid_materialized(TemporalKind::Duration, "P4W"));
        assert!(!is_valid_materialized(TemporalKind::Duration, "P1D"));
        assert!(!is_valid_materialized(TemporalKind::Duration, "P1YT1H"));
        // Canonicalization.
        assert_eq!(canonicalize_duration("PT90M").as_deref(), Some("PT1H30M"));
        assert_eq!(canonicalize_duration("PT3600S").as_deref(), Some("PT1H"));
        assert_eq!(
            canonicalize_duration("PT1H30M15S").as_deref(),
            Some("PT1H30M15S")
        );
        assert_eq!(canonicalize_duration("PT0S").as_deref(), Some("PT0S"));
        // Overflow.
        assert!(!is_valid_materialized(
            TemporalKind::Duration,
            "PT999999999999H"
        ));
    }

    #[test]
    fn materialized_clock_roundtrip_values_are_valid() {
        // Every VALID wire in the clock corpus (minus `:60`) must pass the
        // materialized check so the parse adapter never rejects a round-trip case.
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../specs/json-schema/research/format_materialize_clock/corpus.json"
        ))
        .expect("corpus parses");
        for (key, kind) in [
            ("date-time", TemporalKind::DateTime),
            ("date", TemporalKind::Date),
            ("time", TemporalKind::Time),
        ] {
            for row in corpus[key].as_array().expect("rows") {
                let wire = row["wire"].as_str().expect("wire");
                let expect = !wire.contains(":60");
                assert_eq!(
                    is_valid_materialized(kind, wire),
                    expect,
                    "clock {key} {wire:?}"
                );
            }
        }
    }

    /// Drive the conformance corpora through the shared predicate as a
    /// regression against every research accept/reject pair.
    #[test]
    fn conformance_corpora_agree() {
        // format_conformance: uuid / ipv4 / ipv6 (skip temporal rows).
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../specs/json-schema/research/format_conformance/corpus.json"
        ))
        .expect("corpus parses");
        for pair in corpus["pairs"].as_array().expect("pairs") {
            let format = pair["format"].as_str().unwrap_or_default();
            if !SUPPORTED_FORMATS.contains(&format) {
                continue;
            }
            let value = pair["value"].as_str().expect("value string");
            let expect = pair["expect_valid"].as_bool().expect("expect_valid");
            assert_eq!(
                is_valid(format, value),
                expect,
                "format_conformance {format} {value:?}"
            );
        }
    }

    #[test]
    fn hostname_corpus_agrees() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../specs/json-schema/research/format_hostname/corpus.json"
        ))
        .expect("corpus parses");
        for case in corpus["cases"].as_array().expect("cases") {
            let instance = case["instance"].as_str().expect("instance string");
            let expect = case["valid"].as_bool().expect("valid");
            assert_eq!(
                is_valid("hostname", instance),
                expect,
                "hostname {instance:?}"
            );
        }
    }

    #[test]
    fn uri_corpus_all_uri() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../specs/json-schema/research/format_uri/corpus.json"
        ))
        .expect("corpus parses");
        for pair in corpus["pairs"].as_array().expect("pairs") {
            let value = pair["value"].as_str().expect("value string");
            let expect = pair["expect"].as_bool().expect("expect");
            assert_eq!(is_valid("uri", value), expect, "uri {value:?}");
        }
    }

    #[test]
    fn email_corpus_agrees() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../specs/json-schema/research/format_email/corpus.json"
        ))
        .expect("corpus parses");
        for pair in corpus["pairs"].as_array().expect("pairs") {
            let instance = pair["instance"].as_str().expect("instance string");
            let expect = pair["expect_valid"].as_bool().expect("expect_valid");
            assert_eq!(is_valid("email", instance), expect, "email {instance:?}");
        }
    }
}
