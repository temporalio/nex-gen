# `\s` / `\S` normalization research

**Question.** The `pattern` spec currently REJECTS `\s`/`\S` at load because the
whitespace shorthand diverges across target engines (JS `\s` is full-Unicode and
not flag-controllable; Go/RE2 `\s` even omits U+000B). Can we instead NORMALIZE
`\s`/`\S` — rewrite them to an explicit canonical ASCII class in the emitted
pattern for every target — the same way we already rewrite the `$` anchor?

**Answer: YES**, for every placement except one narrow, detectable case
(`\S` inside a *multi-member* character class), which we flag and reject with a
fix-it. Everything else (standalone `\s`/`\S`, quantified, in `[...]`, in a
negated `[^...]`, and the double-negation `[^\S]`) normalizes cleanly, and all
four engines then AGREE.

## Canonical set

    WS = \t \n \x0B \f \r <space>          emitted as   [\t\n\x0B\f\r ]
    \s  ->  [WS]        \S  ->  [^WS]

- **Includes U+000B** (`\x0B`, vertical tab). ECMA-262 `\s` includes `\v`, and
  Python `re.ASCII` / Java-default / JS all include it; only Go/RE2's `\s` class
  omits it. Emitting it explicitly makes Go agree with the rest and matches
  author intent (`\s` "means" the ECMA whitespace controls). We write `\x0B`
  rather than `\v`: as a standalone escape `\v == U+000B` in all four engines,
  but `\x0B` is unambiguous and sidesteps the shorthand-class confusion.
- **Deliberately ASCII** — we drop ECMA-262's Unicode spaces (NBSP U+00A0,
  U+3000, …). This is the intended pinned semantics: `\d`/`\w` are already ASCII,
  so `\s` matches.

## Rewrite rules (by AST placement)

| Placement                         | Rewrite                    |
|-----------------------------------|----------------------------|
| standalone `\s`                   | `[\t\n\x0B\f\r ]`          |
| standalone `\S`                   | `[^\t\n\x0B\f\r ]`         |
| `\s` inside `[ … ]` or `[^ … ]`   | bare members `\t\n\x0B\f\r ` spliced in place |
| `[\s]` / `[^\s]`                  | `[…]` / `[^…]`             |
| `[\S]`                            | `[^…]` (reduces to standalone `\S`) |
| `[^\S]` (double negation)         | `[…]`  (reduces to standalone `\s`) |
| `\S` inside a **multi-member** class (e.g. `[\S.]`, `[\S\d]`) | **UNSUPPORTED** — reject with fix-it |

The multi-member `\S`-in-class case is genuinely inexpressible: `[\S.]` = "not-WS
OR '.'" = "everything except (WS minus '.')", an open-ended complement that RE2 /
JS / Python cannot spell as a positive member list (no nested negation or class
subtraction). It is rare and statically detectable, so we reject it precisely.

## AST vs lexical

**AST wins, decisively.** `regex-syntax` (already a dependency of the gate)
represents every `\s`/`\S` — standalone or in-class — as a `ClassPerl` /
`ClassSetItem::Perl` node with `kind == Space`, a `negated` flag (`\s`=false,
`\S`=true), and a precise **byte span**. The enclosing `ClassBracketed` carries
its own independent `negated` flag. So the rewrite is: walk the AST, collect the
spans of the space-perl nodes plus their placement context, and splice
replacements at those byte offsets. This is escape-safe for free — `foo\\sbar`
(escaped backslash + literal `s`) parses as a literal and produces no perl node,
so it is left untouched; a lexical scanner would have to reimplement all of
regex-syntax's escape/class bookkeeping to get this right.

## Proof of cross-engine agreement

`agree.py` normalizes `corpus.json` with the Rust probe and runs BOTH the
original and normalized patterns through the same Go/JS/Python/Java runners used
by `../pattern_conformance` (pinned semantics: Go RE2, JS `u`, Python
`re.ASCII`, Java default flags, unanchored search).

    original divergences:   13     (vtab: Go alone; NBSP & U+3000: JS alone)
    normalized divergences:  0
    VERDICT: PASS -- normalization makes all engines agree

The 13 divergences cover standalone `\s`/`\S`, quantified `\s+`, in-class
`[\s.]` / `[a-z\s]`, negated `[^\s]`, and the double-negation `[^\S]`. After
normalization all 29 pairs are unanimous.

## Edge cases / risks (`edge` binary)

- **`[\s-x]`** (shorthand as a range boundary): the `regex` gate already
  REJECTS this at parse ("invalid range boundary"), so it never reaches
  normalization. `[a\s-]` (trailing literal dash) normalizes fine.
- **Quantifiers** `\s{2,4}` → `[WS]{2,4}`: `\s` and `[WS]` are both single atoms,
  so the quantifier binds identically. Safe.
- **Idempotent**: an already-explicit `[\t\n\x0B\f\r ]` is a no-op.
- **No `^`/`$` interaction**: `\s` normalization only touches space-perl spans;
  anchors are untouched and compose (`^\s+$` → `^[WS]+$`, verified in corpus).
- **Performance**: emitted class is a 6-element set; negligible.

## Files

- `rewrite_probe/` — Rust (`regex-syntax`) probe.
  - `src/rewrite_lib.rs` — the `normalize()` routine + rewrite rules.
  - `src/rewrite.rs` — prints the rewrite for every placement (`cargo run --bin rewrite`).
  - `src/explore.rs` — dumps the AST/spans that make the rewrite feasible.
  - `src/edge.rs` — edge-case / risk probe.
  - `src/normalize_corpus.rs` — turns a corpus into its normalized form.
- `corpus.json` — `\s`/`\S` patterns + whitespace instances (vtab/NBSP/U+3000/letters).
- `agree.py` — builds the probe, normalizes the corpus, runs all four engines on
  both, and reports divergence before vs after. Reuses `../pattern_conformance`
  runners.

## Re-run

    cd rewrite_probe && cargo run --bin rewrite     # mapping table
    cd rewrite_probe && cargo run --bin explore     # AST feasibility
    cd rewrite_probe && cargo run --bin edge        # edge cases
    python3 agree.py                                # cross-engine proof (go/node/python3/java/cargo on PATH)
