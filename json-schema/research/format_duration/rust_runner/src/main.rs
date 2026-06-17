// Rust runner = the generator's own engine (the `regex` crate), which is ALSO
// the load-time gate. Compiles the SINGLE pinned duration regex (from
// corpus.json's `pinned_regex`) and, for each value, reports whether it matches.
//
// This proves two things at once: (a) the pinned regex is RE2-safe (compiles in
// the pure-Rust `regex` crate at all -> no backtracking construct), and (b) the
// generator's own runtime verdict per value, to line up against the six targets.
//
// The pinned regex is fully anchored (^...$); `is_match` on an anchored pattern
// gives the whole-string verdict.
//
// Emits JSON Lines to stdout: {"id","engine":"rust","compiled":bool,"matched":bool|null}
use regex::Regex;
use serde::Deserialize;
use std::io::Write;

#[derive(Deserialize)]
struct Corpus {
    pinned_regex: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    value: String,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../corpus.json".to_string());
    let data = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        std::process::exit(1);
    });
    let corpus: Corpus = serde_json::from_str(&data).expect("corpus.json parse");

    let compiled_re = Regex::new(&corpus.pinned_regex);
    let compiled = compiled_re.is_ok();

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for c in &corpus.cases {
        let matched = match &compiled_re {
            Ok(re) => serde_json::Value::Bool(re.is_match(&c.value)),
            Err(_) => serde_json::Value::Null,
        };
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "id": c.id,
                "engine": "rust",
                "compiled": compiled,
                "matched": matched,
            })
        )
        .unwrap();
    }
}
