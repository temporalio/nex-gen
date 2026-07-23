// Rust runner = the load-time GATE *and* a runtime engine.
//
// It plays two roles mirroring the `pattern` keyword recipe:
//   1. GATE: does the pure-Rust `regex` crate compile the pinned email regex?
//      (RE2-safe check -- if it compiles here, it is in the regular subset every
//       target's runtime engine accepts.)
//   2. RUNTIME: apply the compiled regex to each corpus instance. The `regex`
//      crate matches by Unicode code point and the pinned regex uses only
//      explicit ASCII classes (no \d \w \s \b, no bare `.`), so no ASCII flag
//      is needed -- a Unicode letter simply is not in [a-zA-Z].
//
// Emits JSON Lines to stdout:
//   {"id","engine":"rust","compiled":bool,"matched":bool|null}
use regex::Regex;
use serde::Deserialize;
use std::io::Write;

#[derive(Deserialize)]
struct Corpus {
    pinned_regex: String,
    pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct Pair {
    id: String,
    instance: String,
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
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for p in &corpus.pairs {
        let (compiled, matched) = match &compiled_re {
            Ok(re) => (true, serde_json::Value::Bool(re.is_match(&p.instance))),
            Err(_) => (false, serde_json::Value::Null),
        };
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "id": p.id,
                "engine": "rust",
                "compiled": compiled,
                "matched": matched,
            })
        )
        .unwrap();
    }
}
