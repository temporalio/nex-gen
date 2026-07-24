//! The RE2-safe portable subset for the JSON-Schema `pattern` keyword: a
//! load-time compile gate + `\s`/`\S` normalization (target-agnostic) plus the
//! per-target `$` end-anchor rewrite applied by the Python/Java backends. See
//! `specs/json-schema/features/pattern.md` for the authoritative rules and
//! `specs/json-schema/corpora/pattern_conformance/corpus.json` for the corpus
//! that pinned every rule below.
//!
//! The gate is pure Rust — it compiles the pattern with the `regex` crate (the
//! same no-backtracking family as Go's RE2, so a pattern it accepts is
//! compilable by the permissive JS/Python/Java engines) and walks the
//! `regex-syntax` AST for the three conformance rules the compile gate alone
//! does not cover. No Go (or other) toolchain is involved.

use regex_syntax::ast::{self, Ast};

/// The canonical ASCII whitespace member list (no surrounding brackets), spliced
/// in for `\s`/`\S`. `\x0B` (U+000B vertical tab) is written as a hex escape,
/// not `\v`, to avoid the shorthand-class ambiguity where Go/RE2's `\s` omits
/// U+000B while JS/Python/Java include it — spelling it makes every engine
/// agree. Deliberately ASCII (ECMA-262's Unicode spaces dropped), consistent
/// with the ASCII `\d`/`\w` pinning.
const WS: &str = r"\t\n\x0B\f\r ";

#[derive(Debug, Clone)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

/// A load-time rejection of a non-portable `pattern`, carrying a fix-it message.
#[derive(Debug)]
pub struct PatternError(pub String);

/// The load-time compile gate + `\s`/`\S` normalization. Returns the normalized
/// pattern (Perl whitespace classes expanded to the explicit ASCII class; the
/// `$` end-anchor kept canonical for the per-target backend rewrite). Rejects:
/// backtracking constructs the `regex` crate cannot compile (lookaround,
/// backreferences, …), inline flag groups (`(?i)` / `(?flags:…)`), and `\S`
/// inside a multi-member character class.
pub fn gate_and_normalize(pattern: &str) -> Result<String, PatternError> {
    // 1. Compile gate. Success ⟹ the regular (no-backtracking) subset, which
    //    every target runtime engine accepts. Failure names the offending
    //    construct via the `regex` diagnostic.
    if let Err(error) = regex::Regex::new(pattern) {
        let detail = error
            .to_string()
            .lines()
            .find(|line| line.contains("error:"))
            .map(str::trim)
            .unwrap_or("not a regular expression")
            .to_string();
        return Err(PatternError(format!(
            "pattern {pattern:?} is not portable across the supported engines ({detail}); \
             only the regular (no-backtracking) subset is supported \
             (no lookahead/lookbehind/backreferences)"
        )));
    }

    // 2. Walk the AST (guaranteed to parse, since it just compiled) for the
    //    three conformance rules the compile gate alone does not cover.
    let ast = ast::parse::Parser::new()
        .parse(pattern)
        .map_err(|error| PatternError(format!("pattern {pattern:?} failed to parse: {error}")))?;

    let mut edits = Vec::new();
    walk_normalize(&ast, pattern, &mut edits)?;
    Ok(apply_edits(pattern, edits))
}

/// Rewrite every unescaped `$` end-anchor assertion to `replacement` (Python
/// `\Z`, Java `\z`), leaving `^`, escaped `\$`, and literals untouched. Applied
/// by the Python/Java backends at emit time; Go/JS keep `$` (already
/// exception-free). The input is the loader-normalized pattern, so it is known
/// to parse. On the unexpected event it does not, the pattern is returned
/// unchanged (the load gate already accepted it).
pub fn rewrite_end_anchor(pattern: &str, replacement: &str) -> String {
    let Ok(ast) = ast::parse::Parser::new().parse(pattern) else {
        return pattern.to_string();
    };
    let mut edits = Vec::new();
    collect_end_anchors(&ast, &mut edits, replacement);
    apply_edits(pattern, edits)
}

fn collect_end_anchors(ast: &Ast, edits: &mut Vec<Edit>, replacement: &str) {
    match ast {
        Ast::Assertion(assertion) => {
            if matches!(
                assertion.kind,
                ast::AssertionKind::EndLine | ast::AssertionKind::EndText
            ) {
                edits.push(Edit {
                    start: assertion.span.start.offset,
                    end: assertion.span.end.offset,
                    replacement: replacement.to_string(),
                });
            }
        }
        Ast::Concat(concat) => {
            for child in &concat.asts {
                collect_end_anchors(child, edits, replacement);
            }
        }
        Ast::Alternation(alternation) => {
            for child in &alternation.asts {
                collect_end_anchors(child, edits, replacement);
            }
        }
        Ast::Repetition(repetition) => collect_end_anchors(&repetition.ast, edits, replacement),
        Ast::Group(group) => collect_end_anchors(&group.ast, edits, replacement),
        _ => {}
    }
}

/// Walk the AST collecting `\s`/`\S` rewrite edits, rejecting inline-flag groups
/// and the open-complement `\S`-in-multi-member-class case.
fn walk_normalize(ast: &Ast, pattern: &str, edits: &mut Vec<Edit>) -> Result<(), PatternError> {
    match ast {
        // Bare inline flag directive `(?i)` / `(?m)` … — JS cannot compile it.
        Ast::Flags(_) => Err(inline_flag_error(pattern)),
        Ast::ClassPerl(perl) if matches!(perl.kind, ast::ClassPerlKind::Space) => {
            let replacement = if perl.negated {
                format!("[^{WS}]") // \S -> [^WS]
            } else {
                format!("[{WS}]") // \s -> [WS]
            };
            edits.push(Edit {
                start: perl.span.start.offset,
                end: perl.span.end.offset,
                replacement,
            });
            Ok(())
        }
        Ast::ClassBracketed(class) => handle_class(class, pattern, edits),
        Ast::Concat(concat) => {
            for child in &concat.asts {
                walk_normalize(child, pattern, edits)?;
            }
            Ok(())
        }
        Ast::Alternation(alternation) => {
            for child in &alternation.asts {
                walk_normalize(child, pattern, edits)?;
            }
            Ok(())
        }
        Ast::Repetition(repetition) => walk_normalize(&repetition.ast, pattern, edits),
        Ast::Group(group) => {
            // `(?flags:…)` sets flags on a group — reject (JS cannot compile it).
            // `(?:…)` is a bare non-capturing group (empty flag list) — fine.
            if let ast::GroupKind::NonCapturing(flags) = &group.kind
                && !flags.items.is_empty()
            {
                return Err(inline_flag_error(pattern));
            }
            walk_normalize(&group.ast, pattern, edits)
        }
        _ => Ok(()),
    }
}

fn handle_class(
    class: &ast::ClassBracketed,
    pattern: &str,
    edits: &mut Vec<Edit>,
) -> Result<(), PatternError> {
    let items = flatten(&class.kind);

    // Sole-member reductions: `[\s]`→`[WS]`, `[^\s]`→`[^WS]`, `[\S]`→`[^WS]`,
    // `[^\S]`→`[WS]` (the class reduces to a standalone `\s`/`\S`).
    if items.len() == 1
        && let ast::ClassSetItem::Perl(perl) = items[0]
        && matches!(perl.kind, ast::ClassPerlKind::Space)
    {
        let effective_negated = class.negated ^ perl.negated;
        let replacement = if effective_negated {
            format!("[^{WS}]")
        } else {
            format!("[{WS}]")
        };
        edits.push(Edit {
            start: class.span.start.offset,
            end: class.span.end.offset,
            replacement,
        });
        return Ok(());
    }

    // Multi-member class: `\s` becomes the bare member list; `\S` is an
    // open-ended complement with no portable positive form — reject.
    for item in &items {
        if let ast::ClassSetItem::Perl(perl) = item
            && matches!(perl.kind, ast::ClassPerlKind::Space)
        {
            if perl.negated {
                return Err(PatternError(format!(
                    "pattern {pattern:?} uses `\\S` inside a multi-member character class \
                     (an open-ended complement with no portable positive form); \
                     spell the intended set explicitly (e.g. `[^\\t\\n\\x0B\\f\\r ]`)"
                )));
            }
            edits.push(Edit {
                start: perl.span.start.offset,
                end: perl.span.end.offset,
                replacement: WS.to_string(),
            });
        }
    }
    Ok(())
}

fn inline_flag_error(pattern: &str) -> PatternError {
    PatternError(format!(
        "pattern {pattern:?} uses an inline flag group (e.g. `(?i)` / `(?flags:…)`) \
         which is not ECMA-262 syntax and cannot compile in JavaScript; remove it \
         (per-pattern case-insensitivity and friends are not portable)"
    ))
}

/// Flatten a `ClassSet` into its leaf items (handles `Union` nesting produced by
/// a plain `[…]`). `BinaryOp` (`&&`, `--`) is not valid in the accepted subset
/// and yields no items.
fn flatten(set: &ast::ClassSet) -> Vec<&ast::ClassSetItem> {
    fn item<'a>(it: &'a ast::ClassSetItem, out: &mut Vec<&'a ast::ClassSetItem>) {
        match it {
            ast::ClassSetItem::Union(union) => {
                for child in &union.items {
                    item(child, out);
                }
            }
            other => out.push(other),
        }
    }
    let mut out = Vec::new();
    if let ast::ClassSet::Item(it) = set {
        item(it, &mut out);
    }
    out
}

/// Apply non-overlapping `edits` to `src` (left to right by start offset).
fn apply_edits(src: &str, mut edits: Vec<Edit>) -> String {
    if edits.is_empty() {
        return src.to_string();
    }
    edits.sort_by_key(|edit| edit.start);
    let mut out = String::with_capacity(src.len() + 16);
    let mut cursor = 0;
    for edit in edits {
        out.push_str(&src[cursor..edit.start]);
        out.push_str(&edit.replacement);
        cursor = edit.end;
    }
    out.push_str(&src[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(pattern: &str) -> String {
        gate_and_normalize(pattern).expect("should accept")
    }

    #[test]
    fn keeps_plain_patterns_unchanged() {
        assert_eq!(norm("^[A-Z]{2,4}$"), "^[A-Z]{2,4}$");
        assert_eq!(norm(r"^\d{3}-\w{4}$"), r"^\d{3}-\w{4}$");
        assert_eq!(norm(""), "");
    }

    #[test]
    fn normalizes_perl_space() {
        assert_eq!(norm(r"\s"), r"[\t\n\x0B\f\r ]");
        assert_eq!(norm(r"\S"), r"[^\t\n\x0B\f\r ]");
        assert_eq!(norm(r"^\s+$"), r"^[\t\n\x0B\f\r ]+$");
        assert_eq!(norm(r"[\s.]"), r"[\t\n\x0B\f\r .]");
        assert_eq!(norm(r"[^\s]"), r"[^\t\n\x0B\f\r ]");
        assert_eq!(norm(r"[\S]"), r"[^\t\n\x0B\f\r ]");
        assert_eq!(norm(r"[^\S]"), r"[\t\n\x0B\f\r ]");
    }

    #[test]
    fn rewrites_end_anchor_per_target() {
        assert_eq!(rewrite_end_anchor("^[a-z]+$", r"\Z"), r"^[a-z]+\Z");
        assert_eq!(rewrite_end_anchor("^[a-z]+$", r"\z"), r"^[a-z]+\z");
        // Escaped `\$` and `^` are untouched.
        assert_eq!(rewrite_end_anchor(r"^a\$", r"\z"), r"^a\$");
    }

    #[test]
    fn rejects_backtracking() {
        assert!(gate_and_normalize("(?=.*[A-Z]).+").is_err());
        assert!(gate_and_normalize("(?<=x)y").is_err());
        assert!(gate_and_normalize(r"(a)\1").is_err());
    }

    #[test]
    fn rejects_inline_flags() {
        assert!(gate_and_normalize("(?i)^cat$").is_err());
        assert!(gate_and_normalize("(?i:ab)").is_err());
        // A bare non-capturing group is fine.
        assert!(gate_and_normalize("(?:ab)+").is_ok());
    }

    #[test]
    fn rejects_open_complement_class() {
        assert!(gate_and_normalize(r"[\S.]").is_err());
        assert!(gate_and_normalize(r"[\S\d]").is_err());
    }

    /// Regression: drive the shared 83-pair conformance corpus through the gate.
    /// Every `expect_gate_reject` pair (backtracking constructs) must reject; the
    /// lone inline-flag pair rejects too (the gate is stricter than the corpus's
    /// original flag, per the spec's inline-flag rule); every other pair accepts.
    #[test]
    fn conformance_corpus_gate_agrees() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../specs/json-schema/corpora/pattern_conformance/corpus.json"
        ))
        .expect("corpus parses");
        for pair in corpus["pairs"].as_array().expect("pairs array") {
            let id = pair["id"].as_str().unwrap_or("<no id>");
            let pattern = pair["pattern"].as_str().expect("pattern string");
            let expect_reject =
                pair["expect_gate_reject"].as_bool().unwrap_or(false) || id == "case-inline-flag";
            let result = gate_and_normalize(pattern);
            if expect_reject {
                assert!(
                    result.is_err(),
                    "pair `{id}` ({pattern:?}) should gate-reject"
                );
            } else {
                assert!(
                    result.is_ok(),
                    "pair `{id}` ({pattern:?}) should gate-accept: {:?}",
                    result.err()
                );
            }
        }
    }
}
