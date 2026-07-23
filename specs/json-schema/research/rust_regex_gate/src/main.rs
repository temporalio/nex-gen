// Probe: can the pure-Rust `regex` crate serve as the load-time portability
// gate for `pattern`, replacing a Go/RE2 dependency? It must REJECT the
// non-regular Perl constructs (lookaround, backreferences) that JS/Python/Java
// accept but that have no portable linear-time semantics, and ACCEPT the
// portable subset. Mirrors the Go RE2 results in ../string_probe/main.go.
use regex::Regex;

fn main() {
    // Should reject (non-regular; Go RE2 rejects these too):
    let reject = [
        ("lookahead", "(?=foo)"),
        ("neg lookahead", "(?!foo)"),
        ("lookbehind", "(?<=x)y"),
        ("backreference", r"(a)\1"),
    ];
    // Should accept (portable regular subset):
    let accept = [
        ("anchored class", r"^[a-z]+$"),
        ("unanchored substring", "cat"),
        ("ascii digit class", r"^\d{3}-\d{4}$"),
        ("dot star", "a.*b"),
        ("alternation + groups", r"^(foo|bar)-\w+$"),
        ("empty (vacuous)", ""),
    ];
    println!("== should REJECT ==");
    for (name, p) in reject {
        match Regex::new(p) {
            Ok(_) => println!("  {name:16} {p:12} -> ACCEPTED  (UNEXPECTED)"),
            Err(_) => println!("  {name:16} {p:12} -> rejected  (ok)"),
        }
    }
    println!("== should ACCEPT ==");
    for (name, p) in accept {
        match Regex::new(p) {
            Ok(_) => println!("  {name:22} {p:14} -> accepted  (ok)"),
            Err(e) => println!("  {name:22} {p:14} -> REJECTED  (UNEXPECTED: {})", e.to_string().lines().next().unwrap_or("")),
        }
    }
    // Semantics spot-check: unanchored + ASCII \d + code-point '.', matching
    // the other four engines (see ../string_probe/).
    println!("== semantics ==");
    println!("  'cat' is_match 'the cat sat' (unanchored): {}", Regex::new("cat").unwrap().is_match("the cat sat"));
    println!("  '\\d' is_match Arabic digit U+0663 (ASCII): {}", Regex::new(r"\d").unwrap().is_match("\u{0663}"));
    println!("  'a.b' is_match 'a<emoji>b' (. = code point): {}", Regex::new("a.b").unwrap().is_match("a\u{1F600}b"));
}
