// AST-driven normalization of the Perl whitespace classes `\s` / `\S` into an
// explicit, canonical ASCII whitespace set, so every target engine agrees.
//
// Canonical set (matches ECMA-262 `\s` minus its Unicode spaces):
//     WS = \t \n \v \f \r <space>   ==  [\t\n\x0B\f\r ]
// (\x0B = U+000B vertical tab. We emit it as the hex escape \x0B, not \v: as a
//  standalone escape \v == U+000B in all four engines, but \x0B is unambiguous
//  and avoids the shorthand-class confusion where Go/RE2's `\s` class OMITS
//  U+000B while JS/Python/Java's include it -- that omission is exactly the
//  divergence this normalization removes.)
//
// Rewrite rules, keyed off the regex-syntax AST (ClassPerlKind::Space nodes):
//   Placement                         Rewrite
//   ---------                         -------
//   standalone  \s                    [WS]
//   standalone  \S                    [^WS]
//   inside [ ... ]      \s            WS   (bare members, no brackets)
//   inside [ ... ]      \S            requires complement of an ASCII set;
//                                     NOT directly expressible as bare members,
//                                     because "\S" = "any char except WS" and a
//                                     positive class cannot spell an open-ended
//                                     complement.  See handling below.
//   inside [^ ... ]     \s            same bare members WS (the outer ^ negates)
//   inside [^ ... ]     \S            double negation.
//
// The \S-inside-a-MULTI-member-class case is the only hard one. A bracketed
// class is a UNION of its members, so `[\S.]` means "(not-WS) OR '.'", i.e.
// "everything except (WS minus '.')". That complement cannot be written as a
// finite positive member list, and RE2/JS/Python have no nested-negation or
// class-subtraction syntax to express it portably. So we treat `\S` inside a
// multi-member class as UNSUPPORTED and report it (the author should write the
// explicit class themselves). The degenerate single-member classes `[\S]` and
// `[^\S]` are NOT hard: they reduce to standalone `\S` / `\s`, which we do
// rewrite (see the single-member reduction in handle_class).
//
// This probe implements the span rewrite for every SAFE placement and flags the
// genuinely-unsafe one, then prints the rewritten pattern for inspection.

use regex_syntax::ast::{self, Ast};

const WS: &str = r"\t\n\x0B\f\r "; // members, no surrounding brackets

#[derive(Debug, Clone)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

#[derive(Debug)]
enum RewriteError {
    // `\S` inside a bracketed class that also has other members: not expressible
    // as a portable positive/negative member list.
    UnsafeNotSInClass { span: (usize, usize) },
}

/// Collect edits from the AST. `in_class` tracks whether we are inside `[...]`.
fn collect(a: &Ast, edits: &mut Vec<Edit>, errs: &mut Vec<RewriteError>) {
    match a {
        Ast::ClassPerl(p) if matches!(p.kind, ast::ClassPerlKind::Space) => {
            // standalone \s / \S
            let (s, e) = (p.span.start.offset, p.span.end.offset);
            let repl = if p.negated {
                format!("[^{WS}]") // \S -> [^WS]
            } else {
                format!("[{WS}]") // \s -> [WS]
            };
            edits.push(Edit { start: s, end: e, replacement: repl });
        }
        Ast::ClassBracketed(b) => {
            handle_class(b, edits, errs);
        }
        Ast::Concat(c) => for x in &c.asts { collect(x, edits, errs) },
        Ast::Alternation(al) => for x in &al.asts { collect(x, edits, errs) },
        Ast::Repetition(r) => collect(&r.ast, edits, errs),
        Ast::Group(g) => collect(&g.ast, edits, errs),
        _ => {}
    }
}

fn handle_class(b: &ast::ClassBracketed, edits: &mut Vec<Edit>, errs: &mut Vec<RewriteError>) {
    // Count members and detect the sole-\S special case.
    let items = flatten(&b.kind);
    let space_perls: Vec<&ast::ClassSetItem> = items
        .iter()
        .copied()
        .filter(|it| matches!(it, ast::ClassSetItem::Perl(p) if matches!(p.kind, ast::ClassPerlKind::Space)))
        .collect();

    // Special reductions when the class body is EXACTLY one perl-space item.
    if items.len() == 1 {
        if let ast::ClassSetItem::Perl(p) = items[0] {
            if matches!(p.kind, ast::ClassPerlKind::Space) {
                // [\s] -> [WS] ; [^\s] -> [^WS] ; [\S] -> [^WS] ; [^\S] -> [WS]
                let outer_neg = b.negated;
                let s_neg = p.negated;
                let effective_negated = outer_neg ^ s_neg;
                let repl = if effective_negated {
                    format!("[^{WS}]")
                } else {
                    format!("[{WS}]")
                };
                edits.push(Edit {
                    start: b.span.start.offset,
                    end: b.span.end.offset,
                    replacement: repl,
                });
                return;
            }
        }
    }

    // Multi-member class: rewrite each \s in place to bare members; a \S here is
    // unsafe (complement can't join a union of positive members portably).
    for it in &space_perls {
        if let ast::ClassSetItem::Perl(p) = it {
            let (s, e) = (p.span.start.offset, p.span.end.offset);
            if p.negated {
                errs.push(RewriteError::UnsafeNotSInClass { span: (s, e) });
            } else {
                // \s -> bare members WS (works whether class is [..] or [^..]).
                edits.push(Edit { start: s, end: e, replacement: WS.to_string() });
            }
        }
    }
}

/// Flatten a ClassSet into its leaf items (only handles Union nesting, which is
/// what a plain `[...]` produces). BinaryOp (&&, --) is out of scope for JSON
/// Schema patterns and reported as-is by callers if needed.
fn flatten<'a>(set: &'a ast::ClassSet) -> Vec<&'a ast::ClassSetItem> {
    let mut out = Vec::new();
    fn item<'a>(it: &'a ast::ClassSetItem, out: &mut Vec<&'a ast::ClassSetItem>) {
        match it {
            ast::ClassSetItem::Union(u) => for x in &u.items { item(x, out) },
            other => out.push(other),
        }
    }
    match set {
        ast::ClassSet::Item(it) => item(it, &mut out),
        ast::ClassSet::BinaryOp(_) => { /* not expected in JSON Schema patterns */ }
    }
    out
}

fn apply_edits(src: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| e.start);
    let mut out = String::with_capacity(src.len() + 16);
    let mut cursor = 0;
    for e in edits {
        out.push_str(&src[cursor..e.start]);
        out.push_str(&e.replacement);
        cursor = e.end;
    }
    out.push_str(&src[cursor..]);
    out
}

pub fn normalize(pattern: &str) -> Result<String, String> {
    let ast = ast::parse::Parser::new()
        .parse(pattern)
        .map_err(|e| format!("parse error: {}", e))?;
    let mut edits = Vec::new();
    let mut errs = Vec::new();
    collect(&ast, &mut edits, &mut errs);
    if let Some(RewriteError::UnsafeNotSInClass { span }) = errs.first() {
        return Err(format!(
            "`\\S` inside a multi-member character class at [{}..{}] is not \
             portably normalizable; author should write an explicit class",
            span.0, span.1
        ));
    }
    Ok(apply_edits(pattern, edits))
}

// binary entry point lives in rewrite.rs (which includes this module)
#[allow(dead_code)]
fn _lib_only() {}
