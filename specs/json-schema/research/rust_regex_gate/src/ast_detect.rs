// Feasibility check: can the pure-Rust gate detect, from the parsed AST,
// the three non-portable constructs the conformance corpus found — so it can
// REJECT inline flags + \s/\S and NORMALIZE the `$` anchor? Uses regex-syntax
// (a dependency of the `regex` crate we already use for the gate).
use regex_syntax::ast::{self, Ast};

#[derive(Default, Debug)]
struct Findings { inline_flags: bool, perl_space: bool, dollar: bool, caret: bool }

fn walk(a: &Ast, f: &mut Findings) {
    match a {
        Ast::Flags(_) => f.inline_flags = true, // bare (?i) flag directive
        Ast::ClassPerl(p) => {
            if matches!(p.kind, ast::ClassPerlKind::Space) { f.perl_space = true; }
        }
        Ast::Assertion(asrt) => match asrt.kind {
            ast::AssertionKind::EndLine | ast::AssertionKind::EndText => f.dollar = true,
            ast::AssertionKind::StartLine | ast::AssertionKind::StartText => f.caret = true,
            _ => {}
        },
        Ast::Group(g) => {
            // (?flags:...) sets flags on a group
            if matches!(g.kind, ast::GroupKind::NonCapturing(_)) { /* may carry flags */ }
            walk(&g.ast, f);
        }
        Ast::Concat(c) => for x in &c.asts { walk(x, f) },
        Ast::Alternation(al) => for x in &al.asts { walk(x, f) },
        Ast::Repetition(r) => walk(&r.ast, f),
        _ => {}
    }
}

fn analyze(p: &str) -> Result<Findings, String> {
    let ast = ast::parse::Parser::new().parse(p).map_err(|e| e.to_string())?;
    let mut f = Findings::default();
    walk(&ast, &mut f);
    Ok(f)
}

fn main() {
    let cases = [
        r"(?i)^cat$",      // inline flags + caret + dollar
        r"^[a-z]+$",        // caret + dollar, no perl-space, no flags
        r"\s+",             // perl space
        r"\S",              // perl space (negated)
        r"[ \t\n\r\f]",     // explicit ws class -> should NOT flag perl_space
        r"\d{3}-\d{4}",     // perl digit only -> no space, no dollar
        r"foo\$bar",        // escaped dollar -> should NOT flag dollar
        r"(?:ab)+",         // non-capturing, no flags
    ];
    for p in cases {
        match analyze(p) {
            Ok(f) => println!("{p:16} -> flags={} space={} $={} ^={}", f.inline_flags, f.perl_space, f.dollar, f.caret),
            Err(e) => println!("{p:16} -> PARSE ERR: {}", e.lines().next().unwrap_or("")),
        }
    }
}
