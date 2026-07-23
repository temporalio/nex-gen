// Rust runner = the load-time GATE plus a match pass.
// Reads the pinned body (arg 2, default ../pinned_body.json) and corpus (arg 1,
// default ../corpus.json). Reports (a) whether the pure-Rust `regex` crate
// COMPILES the anchored pinned pattern (the gate's job: proves RE2-safety), and
// (b) the match verdict per corpus value (^...$ anchored; Rust `$` is end of
// text, no trailing-\n exception).
//
// Emits JSON Lines: {"id","engine":"rust","compiled":bool,"matched":bool|null}
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
    value: String,
}
#[derive(Deserialize)]
struct Body {
    body: String,
}

fn main() {
    let corpus_path = std::env::args().nth(1).unwrap_or_else(|| "../corpus.json".to_string());
    let body_path = std::env::args().nth(2).unwrap_or_else(|| "../pinned_body.json".to_string());

    let cdata = std::fs::read_to_string(&corpus_path).expect("read corpus");
    let bdata = std::fs::read_to_string(&body_path).expect("read body");
    let corpus: Corpus = serde_json::from_str(&cdata).expect("parse corpus");
    let body: Body = serde_json::from_str(&bdata).expect("parse body");

    let anchored = format!("^{}$", body.body);
    let compiled_re = Regex::new(&anchored);
    if let Err(e) = &compiled_re {
        eprintln!("RUST GATE COMPILE ERROR: {e}");
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for p in &corpus.pairs {
        let (compiled, matched) = match &compiled_re {
            Ok(re) => (true, serde_json::Value::Bool(re.is_match(&p.value))),
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
