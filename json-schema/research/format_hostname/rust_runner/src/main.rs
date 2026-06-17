// Rust runner = both the generator's OWN engine and (implicitly) the load-time
// GATE proof: the pinned hostname regex must COMPILE under the pure-Rust `regex`
// crate (RE2-safe by construction) -- which it does, so it is portable to every
// target's runtime engine. This runner also EVALUATES the pinned check so the
// Rust verdict can be compared value-for-value against the six runtimes.
//
// The PINNED check:
//   1. compile-once anchored regex (RE2-safe; no lookahead/backtracking)
//   2. total-length guard (1..=253 CHARS = code points) OUTSIDE the regex,
//      because RE2 has no whole-input length lookahead.
//   Verdict = regex matches AND length in range.
//
// Emits JSON Lines: {"id","engine":"rust","valid","regex","len_ok"}
use regex::Regex;
use serde::Deserialize;
use std::io::Write;

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    instance: String,
}

const MAX_TOTAL_LEN: usize = 253;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../corpus.json".to_string());
    let data = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        std::process::exit(1);
    });
    let corpus: Corpus = serde_json::from_str(&data).expect("corpus.json parse");

    // The pinned regex MUST compile under the pure-Rust regex crate (the gate).
    // `regex` is anchored with ^...$; the crate matches a full haystack with
    // is_match when the pattern is anchored. Its default `$` is end-of-text
    // (with an optional trailing \n only under multi-line, which is off), but to
    // be unambiguous the pattern is written with explicit ^ and $ and the crate
    // treats them as text boundaries here.
    let host_re = Regex::new(
        r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$",
    )
    .expect("PINNED hostname regex must compile under the Rust regex crate (gate)");

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for c in &corpus.cases {
        let n = c.instance.chars().count();
        let len_ok = n >= 1 && n <= MAX_TOTAL_LEN;
        let regex = host_re.is_match(&c.instance);
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "id": c.id,
                "engine": "rust",
                "valid": regex && len_ok,
                "regex": regex,
                "len_ok": len_ok,
            })
        )
        .unwrap();
    }
}
