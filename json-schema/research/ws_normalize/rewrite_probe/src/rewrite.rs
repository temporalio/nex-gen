// Demonstration binary for the AST-driven \s/\S normalizer. Prints the rewrite
// for every placement (standalone / in-class / negated-in-class / escaped) so
// the mapping table in the README can be verified by eye.
#[path = "rewrite_lib.rs"]
mod rewrite_lib;

use rewrite_lib::normalize;

fn main() {
    let cases = [
        r"\s+",
        r"\S",
        r"a\sb\Sc",
        r"[\s.]",
        r"[a-z\s]",
        r"[^\s]",
        r"[^\S]",
        r"[\s]",
        r"[\S]",
        r"[\S\d]", // \S in multi-member class -> should be flagged UNSAFE
        r"[\s\S]", // \s + \S in class -> \S unsafe
        r"^[\s]+$",
        r"\d{3}\s\d{4}",
        r"foo\\sbar", // ESCAPED backslash then literal s: NOT a perl class
    ];
    for p in cases {
        match normalize(p) {
            Ok(out) => println!("{:14} -> {}", p, out),
            Err(e) => println!("{:14} -> ERR: {}", p, e),
        }
    }
}
