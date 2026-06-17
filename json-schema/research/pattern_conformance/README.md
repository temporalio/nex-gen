# `pattern` cross-language conformance

Empirical study answering **open question 2**: does the load-time gate (pure
Rust `regex` crate) plus the pinned per-target runtime recipe actually produce
**identical** `pattern` validation across Go, JS/TS, Python, and Java — or are
there residual divergences on the gate-accepted subset?

## The recipe under test

- **Gate (load time):** compile the pattern with the Rust `regex` crate. Reject
  the schema if it does not compile. `regex` is the same regular / RE2 family as
  Go, so lookaround and backreferences are rejected here.
- **Runtime match (pinned per target):**
  - Go: `regexp.Compile` then `MatchString` (unanchored search)
  - JS: `new RegExp(p, "u")` then `.test(v)`
  - Python: `re.compile(p, re.ASCII)` then `.search(v)`
  - Java: `Pattern.compile(p)` (default flags) then `.matcher(v).find()`

## Files

- `corpus.json` — 83 `{pattern, instance}` pairs; 5 flagged `expect_gate_reject`.
- `rust_runner/` — Rust gate runner (`cargo build --release`); reports compile
  accept/reject only. Run `cargo clean` to keep the tree light (only
  `Cargo.toml`, `Cargo.lock`, `src/` are kept in git).
- `runner.go`, `runner.mjs`, `runner.py`, `Runner.java` — one per runtime engine.
  Each reads `corpus.json` and emits JSON Lines
  `{"id","engine","compiled","matched"}` to stdout.
- `compare.py` — builds/runs all five, aligns by id, and reports (a) compile
  acceptance and (b) match agreement. Exits nonzero on any divergence.

## Run

```sh
cd json-schema/research/pattern_conformance
python3 compare.py
```

Each runner can also be run standalone, e.g. `go run runner.go corpus.json`,
`node runner.mjs corpus.json`, `python3 runner.py corpus.json`,
`java Runner.java corpus.json`, and (after `cargo build --release`)
`rust_runner/target/release/rust_runner corpus.json`.

## Findings (as of this study)

**The gate + pinned semantics are NOT sufficient on their own.** Of 78
gate-accepted pairs, 72 agreed; **1 compile-acceptance violation** and **5 match
divergences** were found. Three independent axes of divergence remain:

### 1. Inline flags — Rust accepts, JS rejects (compile-acceptance violation)

`(?i)^cat$` compiles in the Rust gate, Go, Python, and Java, but **JS throws**
(`Invalid group`) because JS `RegExp` has no inline `(?i)`/`(?flags)` syntax.
This breaks the "Rust-accepted subset of every runtime" property.

### 2. `\s` scope — JS is Unicode, the others are ASCII

JS `\s` always matches the full Unicode whitespace set and is **not**
flag-controllable, whereas Go/RE2, Python `re.ASCII`, and Java default flags all
restrict `\s`/`\S` to ASCII whitespace.

- `\s` vs NBSP `U+00A0` / ideographic space `U+3000`: JS = match, others = no match.
- `\S` vs NBSP `U+00A0`: JS = no match, others = match.

### 3. `$` and trailing newline — Python/Java differ from Go/JS

Python `$` and Java `$` match at end-of-input **or just before a single trailing
`\n`**; Go/RE2 `$` and JS `$` (no `m` flag) match end-of-input only.

- `^cat$` vs `"cat\n"`: Python/Java = match, Go/JS = no match.
- `foo$` vs `"foo\n"`: Python/Java = match, Go/JS = no match.

Note this only bites when the pattern's `$` is at the very end and the instance
ends in `\n`; an anchored `$` mid-alternation or followed by more pattern is
unaffected.

### Verdict / required additional gating

To make validation identical across all four runtimes, the gate must **also**
reject (or the runtimes must be re-pinned to neutralize) the following:

1. **Reject inline flag groups** `(?flags)` / `(?flags:...)` (at minimum `(?i)`,
   and any `(?...)` non-capturing-with-flags form). The Rust gate accepts them
   but JS cannot compile them. (Case-insensitivity, if wanted, would need a
   portable mechanism, not inline `(?i)`.)
2. **Reject `\s` and `\S`** in patterns, OR re-pin JS to a class-substituted
   whitespace definition. As written, JS's Unicode `\s` cannot be narrowed to
   ASCII by any flag, so the only way to keep `\s` and stay identical is to
   forbid non-ASCII whitespace in *instances* (not enforceable) — practically,
   gate out `\s`/`\S`.
3. **Reject an unescaped `$` anchor** (or forbid instances containing a trailing
   `\n`), OR re-pin: on Java, add `Pattern.DOTALL`? no — the fix is to pin Java
   to `find()` semantics that match Go, which requires rejecting the trailing
   `\n` case. Practically: reject patterns whose `$` can match end-of-string,
   or normalize/forbid trailing `\n` in string instances. The cleanest gate is
   to reject `$` unless it is escaped, since a trailing-newline-sensitive anchor
   has no portable meaning.

If instead the intent is to keep `\s`, `$`, and `(?i)` in the accepted language,
the runtime pinning is insufficient and would need per-engine rewriting of the
pattern (desugaring `(?i)`, substituting `\s`→`[ \t\n\r\f\v]`, and rejecting or
trimming trailing `\n`), which is a heavier translation layer than the current
"compile-as-is with pinned flags" recipe.
