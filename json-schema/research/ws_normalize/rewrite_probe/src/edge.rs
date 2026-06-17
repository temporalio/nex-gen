// Edge-case / risk probe: how does the parser (and our normalizer) treat the
// trickier placements the task flagged -- \s adjacent to a range dash, escaped
// backslashes, quantified \s, nested groups, alternation, and \s inside a group
// with flags.
#[path = "rewrite_lib.rs"]
mod rewrite_lib;
use rewrite_lib::normalize;
use regex_syntax::ast::parse::Parser;

fn main() {
    let cases: &[&str] = &[
        r"[\s-x]",     // dash after \s: is it a literal '-' or a range? (RE2 rejects ranges w/ class endpoints)
        r"[a\s-]",     // trailing dash literal
        r"\s{2,4}",    // quantified
        r"(\s|x)",     // alternation
        r"((\s))",     // nested groups
        r"a\\sb",      // escaped backslash + literal s (NOT a perl class)
        r"\\\s",       // escaped backslash then a REAL \s
        r"[\t\n\x0B\f\r ]", // already-explicit canonical class: must be a no-op
        r"\D\s\W",     // \s alongside other perl classes we keep as-is
    ];
    for p in cases {
        let parse = Parser::new().parse(p);
        let parsed = match &parse { Ok(_) => "parses", Err(_) => "PARSE-ERR" };
        match normalize(p) {
            Ok(out) => {
                let noop = out == *p;
                println!("{:22} [{}] -> {}{}", p, parsed, out, if noop { "   (no-op)" } else { "" });
            }
            Err(e) => println!("{:22} [{}] -> ERR: {}", p, parsed, e),
        }
    }
}
