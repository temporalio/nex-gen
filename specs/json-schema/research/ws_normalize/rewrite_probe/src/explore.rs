// Exploratory: dump the regex-syntax AST for `\s`/`\S` in each placement, and
// check whether the AST carries byte SPANS so we can do a precise, span-driven
// lexical rewrite (vs. reconstructing the pattern from the AST). This informs
// the AST-vs-lexical decision documented in the README.
use regex_syntax::ast::{self, Ast};

fn kind_of(a: &Ast) -> &'static str {
    match a {
        Ast::Empty(_) => "Empty",
        Ast::Flags(_) => "Flags",
        Ast::Literal(_) => "Literal",
        Ast::Dot(_) => "Dot",
        Ast::Assertion(_) => "Assertion",
        Ast::ClassUnicode(_) => "ClassUnicode",
        Ast::ClassPerl(_) => "ClassPerl",
        Ast::ClassBracketed(_) => "ClassBracketed",
        Ast::Repetition(_) => "Repetition",
        Ast::Group(_) => "Group",
        Ast::Alternation(_) => "Alternation",
        Ast::Concat(_) => "Concat",
    }
}

fn perl_span(a: &Ast, src: &str, depth: usize) {
    let pad = "  ".repeat(depth);
    match a {
        Ast::ClassPerl(p) => {
            let negated = matches!(p.kind, ast::ClassPerlKind::Space) == false;
            let is_space = matches!(p.kind, ast::ClassPerlKind::Space);
            // p.negated distinguishes \s (false) from \S (true) for the SAME kind.
            let sp = p.span;
            let slice = &src[sp.start.offset..sp.end.offset];
            println!(
                "{pad}ClassPerl kind_is_space={is_space} negated={} span=[{}..{}] src={:?}",
                p.negated, sp.start.offset, sp.end.offset, slice
            );
            let _ = negated;
        }
        Ast::ClassBracketed(b) => {
            let sp = b.span;
            println!(
                "{pad}ClassBracketed negated={} span=[{}..{}] src={:?}",
                b.negated, sp.start.offset, sp.end.offset,
                &src[sp.start.offset..sp.end.offset]
            );
            walk_class(&b.kind, src, depth + 1);
        }
        Ast::Concat(c) => for x in &c.asts { perl_span(x, src, depth) },
        Ast::Alternation(al) => for x in &al.asts { perl_span(x, src, depth) },
        Ast::Repetition(r) => perl_span(&r.ast, src, depth),
        Ast::Group(g) => perl_span(&g.ast, src, depth),
        other => println!("{pad}{}", kind_of(other)),
    }
}

fn walk_class(set: &ast::ClassSet, src: &str, depth: usize) {
    let pad = "  ".repeat(depth);
    match set {
        ast::ClassSet::Item(item) => walk_class_item(item, src, depth),
        ast::ClassSet::BinaryOp(op) => {
            println!("{pad}BinaryOp");
            walk_class(&op.lhs, src, depth + 1);
            walk_class(&op.rhs, src, depth + 1);
        }
    }
}

fn walk_class_item(item: &ast::ClassSetItem, src: &str, depth: usize) {
    let pad = "  ".repeat(depth);
    match item {
        ast::ClassSetItem::Perl(p) => {
            let sp = p.span;
            println!(
                "{pad}[Perl] kind_is_space={} negated={} src={:?}",
                matches!(p.kind, ast::ClassPerlKind::Space),
                p.negated,
                &src[sp.start.offset..sp.end.offset]
            );
        }
        ast::ClassSetItem::Literal(l) => {
            let sp = l.span;
            println!("{pad}[Literal] {:?} src={:?}", l.c, &src[sp.start.offset..sp.end.offset]);
        }
        ast::ClassSetItem::Range(r) => {
            println!("{pad}[Range] {:?}-{:?}", r.start.c, r.end.c);
        }
        ast::ClassSetItem::Union(u) => {
            for it in &u.items { walk_class_item(it, src, depth) }
        }
        other => println!("{pad}[other class item {:?}]", std::mem::discriminant(other)),
    }
}

fn main() {
    let cases = [
        r"\s+",
        r"\S",
        r"[\s.]",
        r"[a-z\s]",
        r"[^\s]",
        r"[^\S]",
        r"a\sb\Sc",
        r"[\S\d]",
    ];
    for p in cases {
        println!("===== pattern {:?} =====", p);
        match ast::parse::Parser::new().parse(p) {
            Ok(ast) => perl_span(&ast, p, 1),
            Err(e) => println!("  PARSE ERR: {}", e),
        }
    }
}
