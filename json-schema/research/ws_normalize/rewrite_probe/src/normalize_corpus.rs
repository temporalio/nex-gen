// Reads a corpus.json ({pairs:[{id,pattern,instance},...]}), rewrites each
// pattern's \s/\S to the canonical explicit ASCII whitespace class using the
// same `normalize` routine as the `rewrite` binary, and writes a new corpus to
// stdout with the normalized patterns. Pairs whose pattern is UNSAFE to
// normalize (e.g. `\S` inside a multi-member class) are dropped with a note on
// stderr so the agreement check only covers normalizable patterns.
//
// Run: normalize_corpus <corpus.json>
#[path = "rewrite_lib.rs"]
mod rewrite_lib;

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Deserialize)]
struct InCorpus {
    #[serde(default)]
    note: String,
    pairs: Vec<Pair>,
}

#[derive(Deserialize, Serialize, Clone)]
struct Pair {
    id: String,
    pattern: String,
    instance: String,
}

#[derive(Serialize)]
struct OutCorpus {
    note: String,
    pairs: Vec<Pair>,
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: normalize_corpus <corpus.json>");
    let text = fs::read_to_string(&path).expect("read corpus");
    let corpus: InCorpus = serde_json::from_str(&text).expect("parse corpus");

    let mut out = Vec::new();
    for p in &corpus.pairs {
        match rewrite_lib::normalize(&p.pattern) {
            Ok(norm) => {
                eprintln!("normalize {:20} {:12} -> {}", p.id, p.pattern, norm);
                out.push(Pair { id: p.id.clone(), pattern: norm, instance: p.instance.clone() });
            }
            Err(e) => {
                eprintln!("DROP      {:20} {:12} :: {}", p.id, p.pattern, e);
            }
        }
    }

    let out_corpus = OutCorpus {
        note: format!("NORMALIZED from: {}", corpus.note),
        pairs: out,
    };
    println!("{}", serde_json::to_string_pretty(&out_corpus).unwrap());
}
