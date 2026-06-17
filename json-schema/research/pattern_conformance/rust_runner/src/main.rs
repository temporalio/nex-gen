// Rust runner = the load-time GATE.
// Reads corpus.json (path as argv[1], or ../corpus.json by default) and, for
// each pair, reports ONLY whether the pure-Rust `regex` crate compiles the
// pattern. The gate does no runtime matching, so `matched` is always null.
//
// Emits JSON Lines to stdout: {"id","engine":"rust","compiled":bool,"matched":null}
use regex::Regex;
use serde::Deserialize;
use std::io::Write;

#[derive(Deserialize)]
struct Corpus {
    pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct Pair {
    id: String,
    pattern: String,
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "../corpus.json".to_string());
    let data = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        std::process::exit(1);
    });
    let corpus: Corpus = serde_json::from_str(&data).expect("corpus.json parse");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for p in &corpus.pairs {
        let compiled = Regex::new(&p.pattern).is_ok();
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "id": p.id,
                "engine": "rust",
                "compiled": compiled,
                "matched": serde_json::Value::Null,
            })
        )
        .unwrap();
    }
}
