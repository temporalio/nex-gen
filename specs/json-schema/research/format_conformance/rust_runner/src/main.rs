// Rust runner for the JSON-Schema `format` conformance corpus.
//
// Rust is the LOAD-TIME GATE for `format`'s regex-lowered check: the pinned
// patterns are the same portable, RE2-safe patterns emitted for every target,
// and they must compile in the pure-Rust `regex` crate. This runner therefore
// (a) compiles each pinned pattern with `regex::Regex` (proving gate-acceptance)
// and (b) implements the full PINNED CHECK -- the pinned regex PLUS the shared
// integer-arithmetic calendar predicate for the temporal formats -- so its
// verdict can be compared value-for-value against the four current targets plus
// Ruby and .NET.
//
// ANCHORING: the Rust `regex` crate's `$` matches only end-of-text (multi-line
// off by default, no trailing-\n exception), matching Go/JS. We nonetheless
// anchor with `\z` (end of text) for uniformity with the Python/Java/Ruby/.NET
// pinning; `regex` supports `\z`.
//
// Emits JSON Lines to stdout: {"id","engine":"rust","valid":bool,"native":null}
// (there is no `native` typed parser column for Rust -- the stdlib has no
// date/uuid parser, so it is reported as null.)
use regex::Regex;
use serde::Deserialize;
use std::io::Write;
use std::sync::LazyLock;

const OCTET: &str = "(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";
const H16: &str = "[0-9a-fA-F]{1,4}";

fn ipv6_pattern() -> String {
    let v4 = format!("({o}\\.{o}\\.{o}\\.{o})", o = OCTET);
    let ls32 = format!("({H16}:{H16}|{v4})");
    format!(
        "\\A(\
         ({H16}:){{6}}{ls32}|\
         ::({H16}:){{5}}{ls32}|\
         ({H16})?::({H16}:){{4}}{ls32}|\
         (({H16}:){{0,1}}{H16})?::({H16}:){{3}}{ls32}|\
         (({H16}:){{0,2}}{H16})?::({H16}:){{2}}{ls32}|\
         (({H16}:){{0,3}}{H16})?::({H16}:){ls32}|\
         (({H16}:){{0,4}}{H16})?::{ls32}|\
         (({H16}:){{0,5}}{H16})?::{H16}|\
         (({H16}:){{0,6}}{H16})?::\
         )\\z"
    )
}

static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        "\\A[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\\z",
    )
    .unwrap()
});
static IPV4_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("\\A{o}\\.{o}\\.{o}\\.{o}\\z", o = OCTET)).unwrap());
static IPV6_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&ipv6_pattern()).unwrap());
static DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("\\A([0-9]{4})-([0-9]{2})-([0-9]{2})\\z").unwrap());
static TIME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("\\A([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?\\z")
        .unwrap()
});
static DATE_TIME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        "\\A([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})\\z",
    )
    .unwrap()
});

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn valid_calendar_date(y: i64, m: i64, d: i64) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

fn valid_time_fields(hh: i64, mm: i64, ss: i64) -> bool {
    hh <= 23 && mm <= 59 && ss <= 60 // :60 leap second accepted
}

fn valid_offset(off: Option<&str>) -> bool {
    match off {
        None => true,
        Some(s) if s == "Z" || s == "z" => true,
        Some(s) => {
            let oh: i64 = s[1..3].parse().unwrap_or(99);
            let om: i64 = s[4..6].parse().unwrap_or(99);
            oh <= 23 && om <= 59
        }
    }
}

fn gi(caps: &regex::Captures, i: usize) -> i64 {
    caps.get(i).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0)
}

fn pinned_valid(format: &str, v: &str) -> bool {
    match format {
        "uuid" => UUID_RE.is_match(v),
        "ipv4" => IPV4_RE.is_match(v),
        "ipv6" => IPV6_RE.is_match(v),
        "date" => match DATE_RE.captures(v) {
            None => false,
            Some(c) => valid_calendar_date(gi(&c, 1), gi(&c, 2), gi(&c, 3)),
        },
        "time" => match TIME_RE.captures(v) {
            None => false,
            Some(c) => {
                valid_time_fields(gi(&c, 1), gi(&c, 2), gi(&c, 3))
                    && valid_offset(c.get(5).map(|m| m.as_str()))
            }
        },
        "date-time" => match DATE_TIME_RE.captures(v) {
            None => false,
            Some(c) => {
                valid_calendar_date(gi(&c, 1), gi(&c, 2), gi(&c, 3))
                    && valid_time_fields(gi(&c, 4), gi(&c, 5), gi(&c, 6))
                    && valid_offset(c.get(8).map(|m| m.as_str()))
            }
        },
        _ => false,
    }
}

#[derive(Deserialize)]
struct Corpus {
    pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct Pair {
    id: String,
    format: String,
    value: String,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "corpus.json".to_string());
    let data = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        std::process::exit(1);
    });
    let corpus: Corpus = serde_json::from_str(&data).expect("corpus.json parse");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for p in &corpus.pairs {
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "id": p.id,
                "engine": "rust",
                "valid": pinned_valid(&p.format, &p.value),
                "native": serde_json::Value::Null,
            })
        )
        .unwrap();
    }
}
