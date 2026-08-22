# String constraints (minLength / maxLength / pattern) — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/minLength.md` — inclusive lower bound on the Unicode **code-point** count of a string; pure runtime assertion, mirror of `maxLength`.
- `specs/json-schema/features/maxLength.md` — inclusive upper bound on the code-point count; owns the shared string-length machinery (P1 "count code points, never the native length"), the exact-length pin, the literal-vs-bound load check, and the materialized-node (wire-string) rule.
- `specs/json-schema/features/pattern.md` — ECMA-262 regex assertion narrowed to a portable "RE2-safe" subset, matched unanchored with ASCII class semantics; a Rust-`regex` compile gate + `regex-syntax` AST rules (inline-flag reject, `\s`/`\S` normalization, `$`→`\Z`/`\z` rewrite) claimed to make all four runtimes agree value-for-value.
- `specs/json-schema/corpora/pattern_conformance/corpus.json` — the 83-pair `(pattern, instance)` corpus the spec designates as this keyword's regression suite.
- `specs/json-schema/PRINCIPLES.md` — P1 (identical cross-language validation), P7/P7.1 (reject ambiguity loudly), P10/P11/P12 (enforced, aggregated, shared bidirectional `Validate`), P2 (idiomatic output).

## Summary

- **The length half is in good shape.** All four emitters use the correct code-point primitive (`utf8.RuneCountInString` / `[...s].length` / `len` / `codePointCount(0,length())`), both directions, and the astral crux (`"a😀b"` = 3, `"😀"×6` = 6) is covered by real round-trip tests in Go, TS, Python and Java. Load-time rejects (non-string type, negative, `min > max`, `const`/`enum` literal vs bound) are all implemented and unit-tested.
- **The `pattern` portability gate does not do what the spec claims it does.** It is a *one-directional* gate: it only rejects what Rust's `regex` **cannot** compile. It does nothing about the far larger set of patterns Rust *can* compile but JS, Python or Java cannot — or compile with different meaning. Verified failures include the entirely ordinary `^\d{3}\-\d{4}$`, which emits TypeScript that throws `SyntaxError` at module import.
- **`.` (dot) silently disagrees across the four engines** on `\r`, U+0085, U+2028 and U+2029: Go and Python match, JS and Java do not. The spec pinned only the astral axis of `.`; the corpus only tests `a.b` vs `a\nb`. This is a P1-mandate (identical accept/reject set) violation on the single most common regex construct.
- **The `\s`/`\S` normalization has holes**: nested character classes and class binary operations (`[a[\s]]`, `[\w&&\s]`, `[[\S]]`) are gate-accepted with a raw `\s`/`\S` passed through to every target — reintroducing exactly the JS-Unicode-whitespace divergence the rewrite exists to eliminate, and bypassing the `\S`-in-multi-member-class reject.
- **A concrete Go build break**: `contentEncoding` + `minLength`/`maxLength`/`pattern` on a top-level property emits `utf8.RuneCountInString(m.Blob)` where `Blob` is `[]byte`. Verified with `go build`. TS/Python/Java all get this combination right.
- **`minLength: 1.0` / `maxLength: 10.0` are rejected by every backend** with a serde error, although both are explicit rows in the specs' *Accepted* matrices.
- **Compile-once is violated in Go (`contains`, `propertyNames`) and Java (`contains`, nested array/map positions)** — `regexp.MustCompile` / `Pattern.compile` inside the per-element / per-key loop, on every (de)serialize.
- **ReDoS is unaddressed.** The gate admits nested quantifiers (`^(a+)+$`) that are linear in Go/Rust and exponential in JS/Python/Java — measured **39 s** for a 31-character input in Python. A gate-accepted schema is a remote DoS in three of the four targets.
- **The conformance corpus is not actually run against the four runtimes anywhere in the repo** — only against the Rust gate (`src/json_schema/pattern.rs:319`). The corpus also lacks entries for every rule the spec says it pinned (`[\s.]`, `[^\s]`, `[\S]`, `[^\S]`, `[\S.]`), and its one inline-flag pair is special-cased *by id* in the test rather than flagged in the data.
- **`default` is the one literal not properly checked against `pattern`**: the loader matches with Rust's Unicode-by-default classes, so `{pattern: "^\\d+$", default: "٣"}` loads and emits a default that all four runtimes reject.

## Implementation divergences

### 1. The `pattern` gate is compile-only against Rust `regex`; it does not verify the target engines can compile (or agree on) the pattern
**Severity: P0**
Spec cite: `pattern.md` — "the **load-time gate compiles the pattern with the Rust `regex` crate** … success ⟹ the regular subset, which every target's runtime engine accepts"; "every emitted pattern compiles in its target"; and P7.1 "A pattern that matches in three languages and fails to compile as a regular expression … is exactly the silently-inconsistent output the mission forbids."
Code cite: `src/json_schema/pattern.rs:41-69` (`gate_and_normalize`: `regex::Regex::new` + three AST rules only — inline flags, `\s`/`\S`, `\S`-in-multi-member-class); `src/parser/json_schema.rs:5139`.
What the spec requires: the gate is the guarantee that every emitted pattern compiles, with identical semantics, in Go/JS/Python/Java.
What the code does: it rejects only what the Rust `regex` crate refuses. Rust's accepted language is a **superset** of ECMA-262-with-`u`, of Python `re`, and of `java.util.regex` in several directions, and the gate has no rule for any of them.

Verified matrix (all rows gate-**accept**; `nexgen go/typescript/python/java` on `{type: string, pattern: P}`; engines probed directly with Go 1.26 / Node / CPython `re.ASCII` / OpenJDK 21):

| Pattern | Go | JS (`u`) | Python | Java |
|---|---|---|---|---|
| `\-` (escaped hyphen outside a class) | ok | **SyntaxError** | ok | ok |
| `a}b` | ok | **SyntaxError** ("Lone quantifier brackets") | ok | ok |
| `a]b` | ok | **SyntaxError** | ok | ok |
| `\p{L}+` / `\pL` | ok | ok / **SyntaxError** | **PatternError** | ok |
| `\x{1F600}` | ok | **SyntaxError** | **PatternError** | ok |
| `\Aabc\z` | ok | **SyntaxError** | ok only because `rewrite_end_anchor` accidentally turns `\z`→`\Z` | ok |
| `(?P<n>a)b` | ok | **SyntaxError** | ok | **PatternSyntaxException** |
| `(?<name>a)b` | ok | ok | **PatternError** | ok |
| `[[:alpha:]]+` vs `"x"` | **true** | **SyntaxError** | false | **false** |
| `[a-z&&[^aeiou]]` vs `"x"` | false | **SyntaxError** | false | **true** |
| `[\w&&\s]` vs `"a"` | **true** | **true** | false | **false** |

Concrete failing input: `{"type":"string","pattern":"^\\d{3}\\-\\d{4}$"}` — an ordinary phone pattern. Loads cleanly; `nexgen typescript` emits
`const PATTERN_309E74EEBDFCDE27 = new RegExp("^\\d{3}\\-\\d{4}$", "u");`
which throws `SyntaxError: Invalid regular expression: /^\d{3}\-\d{4}$/u: Invalid escape` the moment the generated module is imported. (The mandatory `u` flag — itself spec-required — is what makes `\-` illegal.) The bottom four rows are worse: they compile in ≥2 targets and **match differently**, which is the silent cross-language wire disagreement P1 forbids.
Confidence: **high** (reproduced end-to-end through the CLI and each runtime).

### 2. `.` diverges across the four engines on `\r`, U+0085, U+2028, U+2029
**Severity: P0**
Spec cite: `pattern.md` §"Character classes & the dot" — "`.` must match one **code point**"; and P1 "the same `(pattern, instance)` must produce the same accept/reject in Go, TS, Python, and Java."
Code cite: no handling anywhere. `src/json_schema/pattern.rs:118-161` (`walk_normalize`) has no `Ast::Dot` arm; no emitter rewrites `.`.
What the spec requires: identical accept/reject for every `(pattern, instance)`.
What the code does: emits `.` verbatim to all four engines. Their "any character except a line terminator" sets differ: Go/RE2 and Python `re` exclude only `\n`; JS excludes `\n \r    `; Java excludes `\n \r     `.

Concrete failing input: `{type:"string", pattern:"a.b"}` with instance `"a\rb"` → **Go accepts, Python accepts, JS rejects, Java rejects**. Same 2-2 split for U+2028/U+2029; for U+0085 it is Go/Python/JS accept vs Java reject.
Fix direction: the spec's own `\s`/`$` playbook — normalize `.` to an explicit negated class (`[^\n]`, or a per-target class) at emit time.
Confidence: **high** (probed all four engines directly).

### 3. `\s` / `\S` normalization escapes through nested character classes and class binary operations
**Severity: P0**
Spec cite: `pattern.md` — "the emitter **rewrites `\s`→`[\t\n\x0B\f\r ]` and `\S`→`[^\t\n\x0B\f\r ]`** (the explicit set spliced in for *every* target)"; "Placement rules (all verified — 13 original divergences → 0 after rewrite)"; and the `\S`-in-multi-member-class reject.
Code cite: `src/json_schema/pattern.rs:224-240` — `flatten` recurses only into `ClassSetItem::Union`, pushes `ClassSetItem::Bracketed` as an opaque leaf, and returns **an empty vector** for `ClassSet::BinaryOp` (`&&`); `src/json_schema/pattern.rs:163-211` (`handle_class`) therefore never sees the inner `\s`/`\S`.
What the spec requires: every `\s`/`\S` occurrence is rewritten (or, for `\S` in a multi-member class, rejected).
What the code does: leaves them verbatim.

Verified gate-accepted, un-normalized output (`nexgen go`, `MustCompile("…")` shown as-is):
- `[a[\s]]` → emitted verbatim. JS: **SyntaxError**. Go/Java disagree on `"a"` (Go false, Java true).
- `[\w&&\s]` → emitted verbatim. `"a"`: JS **true**, Go **true**, Java **false**. NBSP: JS **true**, Go **false**, Java **false**.
- `[[\S]]` → emitted verbatim, **bypassing** the `\S`-in-class reject. NBSP: Java **true**, Go **false**; JS **SyntaxError**.
Confidence: **high** (reproduced through the CLI plus direct engine probes).

### 4. Go emits non-compiling code for `contentEncoding` + `minLength`/`maxLength`/`pattern` on a top-level property
**Severity: P0**
Spec cite: `maxLength.md` §Serialize-side — "**On a materialized node** ([[format]] temporal / [[contentEncoding]] bytes) the decoded value is not a `string`, so the bound is a predicate over the **canonical wire string** … the encode adapter re-serializes the native value to, **before emit**."
Code cite: `src/generator/json_schema/go.rs:2810-2813` — the `Validate()` string-check guard is
```rust
if property.has_string_constraints()
    && property.ty.as_ref().and_then(Value::as_str) == Some("string")
    && temporal_kind(property).is_none()
```
It excludes the temporal-materialized case (handled correctly at `go.rs:2686-2715`, which projects to a `wire…` variable) but **not** `content_encoding_kind(property)`. The Go field is `[]byte`, so `render_go_string_checks` (`go.rs:233-273`) emits `utf8.RuneCountInString(*m.Blob)` / `utf8.RuneCountInString(m.Blob)`.
What the spec requires: the bound/pattern checks the base64 wire string in both directions (which `UnmarshalJSON`/`MarshalJSON` do correctly).
What the code does: the standalone exported `Validate()` method dereferences/passes the `[]byte` directly.
Concrete failing input:
```yaml
type: object
properties:
  blob: { type: string, contentEncoding: base64, minLength: 2, maxLength: 8, pattern: "^[A-Za-z0-9+/=]+$" }
```
`go build` on the generated package:
```
./out.go:80:35: invalid operation: cannot indirect m.Blob (variable of type []byte)
./out.go:86:34: invalid operation: cannot indirect m.Blob (variable of type []byte)
```
(and, with `required: [blob]`, `cannot use m.Blob (variable of type []byte) as string value`). TS (`o_ce_typescript/models.ts:71`), Python (`models.py:74`) and Java (`Ce.java:72`) all correctly project the wire string first.
Confidence: **high** (verified with `go build`).

### 5. `.0`-valued length bounds are rejected by every backend
**Severity: P1**
Spec cite: `maxLength.md` Accepted matrix row "`.0`-valued bound | `{type:"string", maxLength:10.0}`"; loader rule "`maxLength:5.0` is accepted (≡ `5`, honoring the `1.0`-as-integer rule from [[type]])". Same row in `minLength.md`.
Code cite: the loader accepts it — `src/parser/json_schema.rs:2027-2044` reads the bound via `number.as_f64()` and checks `fract() == 0.0` — but leaves the raw `10.0` in `schema.extra`. Every backend then deserializes it into `Option<u64>`: `src/generator/json_schema/go.rs:58-61`, `typescript.rs:218-221`, `python.rs:64-67`, `java.rs:58-61`.
What the spec requires: `minLength: 1.0` ≡ `minLength: 1`, accepted.
What the code does: fails after the loader with a leaked serde message.
Concrete failing input: `{type: string, minLength: 1.0, maxLength: 10.0}` →
`invalid JSON schema in '<go-json-generator>': failed to read planned JSON schema 'A': invalid type: floating point '1.0', expected u64` (identical for python/typescript; java shares the field type). Same failure for `maxLength: 1e30`.
Fix: normalize the bound to an integer `Value::Number` in `validate_string_constraints`, or make the backend fields tolerant.
Confidence: **high** (reproduced for all backends).

### 6. The loader's literal-vs-`pattern` check runs a Rust-semantics match (Unicode `\d`/`\w`), not the pinned ASCII runtime semantics
**Severity: P1**
Spec cite: `pattern.md` — "It never runs a production match, so the `regex` crate's *own* default semantics (e.g. Unicode-aware `\d` — it matches U+0663, verified) are irrelevant here"; and "A `const`/`default`/`enum` string literal on the **same node** that does **not** match the pattern → reject at load."
Code cite: `src/parser/json_schema.rs:5145-5152` — `regex::Regex::new(&normalized)` then `matcher.is_match(literal)`. The Rust engine is Unicode-aware for `\d`/`\w`/`\b` by default; all four runtimes are pinned to ASCII (`re.ASCII`, RE2, ECMA-262, Java default).
What the spec requires: the literal check must agree with the runtime check.
What the code does: accepts literals no runtime will accept. `const` and `enum` are shielded by an unrelated "string value must be ASCII" rule, so the exposure is `default`.
Concrete failing input: `{type: "string", pattern: "^\\d+$", default: "٣"}` loads; Go emits
```go
var uni5APattern = regexp.MustCompile("^\\d+$")
func (m Uni5) AOrDefault() string { … return "٣" }
```
`"٣"` (U+0663) fails `^\d+$` in Go, JS, Python (`re.ASCII`) and Java. Likewise `{pattern: "^\\w+$", default: "café"}` loads.
Fix: build the load-time matcher with `regex::RegexBuilder::new(..).unicode(false)` (or an explicitly ASCII-configured builder).
Confidence: **high** (reproduced end-to-end).

### 7. Compile-once is violated: Go (`contains`, `propertyNames`) and Java (`contains`, nested positions) recompile the regex inside the validation loop
**Severity: P1**
Spec cite: `pattern.md` Validator mapping — "Each language compiles the pattern **once** (module/package init) and reuses it"; §"Why compile-once. Recompiling a regex per (de)serialize call is a needless cost … a package-level compiled pattern *is* the idiom in all four."
Code cite: `src/generator/json_schema/go.rs:574-579` (`contains` matcher condition), `go.rs:886-889` and `go.rs:925-930` (`propertyNames`, inside `for k := range …`); `src/generator/json_schema/java.rs:560-566` and `java.rs:784`, `java.rs:800`.
What the spec requires: one module/package-level compiled constant.
What the code does: inline `regexp.MustCompile(...)` / `java.util.regex.Pattern.compile(...)` evaluated per element / per key, per call, in both directions.
Concrete output (verified):
- Go, `contains: {type: string, pattern: "^api\\."}` → `if regexp.MustCompile("^api\\.").MatchString(e) {` inside the element loop (this exact string is *asserted* by `tests/generate_go.rs:2186`).
- Go, `propertyNames: {type: string, pattern: "^[a-z]+$"}` → `if !regexp.MustCompile("^[a-z]+$").MatchString(k) {` inside `for k := range`.
- Java, `items: {items: {type: string, pattern: "^[a-z]+$"}}` → `if (!java.util.regex.Pattern.compile("^[a-z]+\\z").matcher(validationValue1).find())` inside a doubly-nested loop. Go hoists the same case correctly (`var nestGridItemItemPattern = …`).
Python and TypeScript hoist correctly in all of these positions (`_PATTERN_<HEX>` / `PATTERN_<HEX>`). `java.rs:536-538` documents the compromise ("recursively nested positions that do not have a stable class member name"), so this is a known-but-undeclared deviation; the `contains` and Go-`propertyNames` cases have no such excuse.
Confidence: **high**.

### 8. TypeScript uses `[...v].length` — the exact idiom the spec rejects — instead of the mandated `codePointLength` early-exit scan
**Severity: P1** (no semantic divergence; missing mandated behavior + unbounded allocation)
Spec cite: `maxLength.md` TS row — "An **allocation-free** surrogate-aware pass that **early-exits** the moment the bound is crossed … This beats the obvious `[...v].length` (which allocates a full code-point array) ~3.5×, and early-exit bounds work on adversarially long input regardless of `max`." `minLength.md` TS row — "the shared `codePointLength` surrogate-aware scan … with **early-exit**"; "the failure path needs **no second pass**".
Code cite: `src/generator/json_schema/typescript.rs:407` (`let length = format!("[...{value_expr}].length")`), also `:448`, `:625`, `:686`, `:825-837`. No `codePointLength` helper exists anywhere in the generator or the emitted runtime.
What the spec requires: a shared `codePointLength` helper, early-exit against the bound, single pass on the failure path.
What the code does: emits `[...v].length` — twice per predicate (once in the condition, once interpolated into the reason), so a min+max pair spreads the string **four** times:
```ts
if ([...raw.a].length < 2) {
  violations.push({ path: "a", reason: `must have length >= 2, got ${[...raw.a].length}` });
}
if ([...raw.a].length > 5) { … }
```
Semantics are correct (spread iterates code points). The cost is real: with `maxLength: 5`, a 100 MB input string allocates a 100M-element array four times before the bound can fire — the adversarial case the spec explicitly designed the early-exit for.
Confidence: **high**.

### 9. ReDoS: the gate admits nested quantifiers that are linear in Go but exponential in JS/Python/Java
**Severity: P1** (spec gap + implementation gap)
Spec cite: `pattern.md` — the whole gate is justified as "the **regular** (no-backtracking) subset … the rejected non-portable constructs are exactly the ones with no portable linear-time semantics anyway", and P1 requires the four targets to agree.
Code cite: `src/json_schema/pattern.rs:41-69` — no complexity analysis; `regex::Regex::new` succeeds for `^(a+)+$`.
What the spec requires (implicitly): the accepted subset is safe and behaves the same everywhere.
What the code does: "regular" is a property of the *language*, not of the *engine*. Go and Rust run these in linear time; JS, Python and Java are backtracking engines and blow up.
Concrete failing input: `{type: "string", pattern: "^(a+)+$"}` gate-accepts and emits `regexp.MustCompile("^(a+)+$")` / `re.compile("^(a+)+\\Z", re.ASCII)` / `new RegExp("^(a+)+$","u")` / `Pattern.compile("^(a+)+\\z")`. Measured on the emitted Python regex with the 31-byte input `"a"*30 + "!"`: **39.06 s**. A Nexus handler generated in Python/TS/Java from such a schema is a remote DoS; the Go handler generated from the *same* schema is fine.
Confidence: **high** for the divergence; the right remedy (reject nested quantifiers? document? per-call timeout?) is a spec decision.

### 10. `minLength: 0` emits a dead comparison instead of being treated as omitted
**Severity: P2**
Spec cite: `minLength.md` — "**`minLength:0`** → accepted, treated as **omitted** (the spec's explicit equivalence); it constrains nothing."
Code cite: `src/generator/json_schema/go.rs:261-266`, `typescript.rs:408-412`, `python.rs` / `java.rs` equivalents — the bound is emitted whenever `min_length.is_some()`, with no `> 0` filter.
What the code does:
```go
if n := utf8.RuneCountInString(*m.B); n < 0 { … }        // Go: unreachable
```
```ts
if ([...raw.b].length < 0) { … }                          // TS: spreads the whole string for nothing
```
```java
int length = value.b.codePointCount(0, value.b.length()); // Java: full scan for nothing
if (length < 0) { … }
```
Dead code in all four (P2 "hand-written-feeling output"), and in TS/Java it is a full string scan/allocation per field per call.
Confidence: **high**.

### 11. TypeScript never emits a regex literal
**Severity: P2**
Spec cite: `pattern.md` TS row — "Module-level ``const PAT_RE = /<pattern>/u;`` (or `new RegExp(<pattern>, "u")` when the literal can't be spelled)."
Code cite: `src/generator/json_schema/typescript.rs:526` — always `new RegExp({}, "u")`.
Cosmetic only (P2 idiom), but it is the stated default form.
Confidence: **high**.

### 12. Corpus drift: the inline-flag rule is encoded in the test, not the data; the corpus is missing the placements it is credited with pinning
**Severity: P2**
Spec cite: `pattern.md` — "Placement rules (all verified — 13 original divergences → 0 after rewrite): standalone `\s`/`\S`; `\s` inside a class (`[\s.]`→…, `[^\s]`→…); sole-member `[\S]`→`[^…]` and the double-negation `[^\S]`→`[…]`" and the `[\S.]`/`[\S\d]` reject — all attributed to the corpus.
Code cite: `specs/json-schema/corpora/pattern_conformance/corpus.json` contains **none** of `[\s.]`, `[^\s]`, `[\S]`, `[^\S]`, `[\S.]`, `[\S\d]`. Its only inline-flag pair (`case-inline-flag`) has **no** `expect_gate_reject`, and `src/json_schema/pattern.rs:328` compensates with `|| id == "case-inline-flag"` — a hard-coded id in the test.
Confidence: **high**.

## Testing gaps

### 1. No cross-runtime driver for the `pattern` conformance corpus
**Severity: P0**
Untested: the corpus's whole stated purpose. `pattern.md` says the 83 pairs were "run through all four runtime engines plus the Rust gate" and that the corpus "stays as the regression guard". The only consumer in the repo is `src/json_schema/pattern.rs:319`, which drives it through the **Rust gate alone** and never performs a match.
Spec line mandating it: `pattern.md` §"Runtime fixtures (validator)" — "is exercised by the **83-pair conformance corpus** …, run through all four runtime engines plus the Rust gate. That corpus is this keyword's regression suite."
Where the test should go: a Go/TS/Python/Java corpus runner per language (e.g. `samples/go/tests/pattern_corpus_test.go`, `samples/typescript/tests/pattern-corpus.test.ts`, `samples/python/tests/test_pattern_corpus.py`, `samples/java/src/test/java/jsonschema/PatternCorpusTest.java`) each reading `corpus.json`, applying the target's emit transform, and asserting against a **new `expect_match` field** pinned in the corpus data (today the pairs carry no expected result at all, so even a runner could only check pairwise agreement).
Suggested case: add `expect_match` to all 83 pairs; add `{pattern:"a.b", instance:"a\rb"}`, `{pattern:"a.b", instance:"a b"}`, `[\s.]`, `[^\s]`, `[\S]`, `[^\S]`, `[\S.]` (`expect_gate_reject`), `[a[\s]]`, `[\w&&\s]`, `\-`, `a}b`, `a]b`, `\p{L}`, `\x{1F600}`, `\Aabc\z`, `(?P<n>a)b`, `(?<n>a)b`, `[[:alpha:]]`, `[a-z&&[^aeiou]]`, `^(a+)+$`.

### 2. No test compiles/imports generated output containing a hostile pattern
**Severity: P0**
Untested: that an emitted pattern actually compiles in its target. `tests/generate_typescript.rs` only string-matches the rendered text for pattern cases (`:1248`, `:1897`); `tests/generate_python.rs` never runs the interpreter; `tests/generate_java.rs` never invokes `javac`. `tests/generate_go.rs` does run `go test` on a few outputs but never on a non-portable-regex case.
Spec line: `pattern.md` — "the one gate-accepted-but-runtime-uncompilable case the corpus found (JS and inline flags) is now a gate reject, so **every emitted pattern compiles in its target**."
Where: extend `tests/generate_typescript.rs` (npm typecheck + a smoke `import`) and add a Python import check, driven by a small list of adversarial patterns.
Suggested case: `{type: string, pattern: "^\\d{3}\\-\\d{4}$"}` → assert either a load reject or a TS module that imports without throwing (today it throws).

### 3. No test for `contentEncoding` (or any materialized node) combined with `minLength`/`maxLength`/`pattern` in Go
**Severity: P0**
Untested: the entire divergence #4. `tests/generate_go.rs` covers `contentEncoding` + `enum` (`:2542`) and + `default` (`:2544`) only; `samples/schemas/showcase.nexusrpc.yaml:177-190` has `blob`/`urlBlob` with no length or pattern. `tests/generate_python.rs:157` *does* cover `contentEncoding` + `pattern` — which is why Python is correct and Go is not.
Spec line: `maxLength.md` §Serialize-side, "On a materialized node ([[format]] temporal / [[contentEncoding]] bytes) … the bound is a predicate over the **canonical wire string**".
Where: `tests/generate_go.rs` (with the existing `go build`/`go test` harness), plus a `blob`-with-bounds field in `samples/schemas/showcase.nexusrpc.yaml` so all four sample suites exercise it.
Suggested case: `{type: string, contentEncoding: base64, minLength: 2, maxLength: 8, pattern: "^[A-Za-z0-9+/=]+$"}`, required and optional, asserting the check runs against the base64 wire string in both directions.

### 4. `.0`-valued bounds are never generated end-to-end
**Severity: P1**
Untested: the spec's own Accepted-matrix row. `src/parser/json_schema.rs:8160-8180` tests `minLength: 0` and integral bounds, and there is a `.0` acceptance path in the loader, but no test drives `minLength: 1.0` / `maxLength: 10.0` **through a backend**.
Spec line: `maxLength.md` Accepted matrix, "`.0`-valued bound | `{type:"string", maxLength:10.0}`"; `minLength.md`, "`{type:"string", minLength:1.0}`".
Where: `tests/generate_go.rs` (or one shared case across all four `tests/generate_*.rs`).
Suggested case: `{type: string, minLength: 1.0, maxLength: 10.0}` → assert the emitted checks read `< 1` / `> 10`.

### 5. No P11 aggregation test for two *string* violations on one value
**Severity: P1**
Untested: the mandated "all reported in one shot" for the string family. The showcase suites assert exactly one violation per string case (`samples/python/tests/test_showcase.py:1015-1016`: `[path for path,_ in token_violations] == ["tokens.primary"]`; Go `:989`; TS `:1175`). The only multi-violation-per-value test is numeric (`test_showcase.py:317-318`, `ratio` below `minimum` **and** off `multipleOf`).
Spec line: `maxLength.md` — "Combined with other failing assertions ([[minLength]], [[pattern]], a failing sibling field) → **all** reported in one shot (**P11**)"; identical in `minLength.md` and `pattern.md`.
Where: all four sample suites, over `showcase.Tokens` (its member already has `minLength: 2` + `maxLength: 8` + `pattern: ^[a-z]+$`).
Suggested case: `{"tokens": {"primary": "A"}}` → expect **two** violations on `tokens.primary` (`must have length >= 2, got 1` and `must match pattern …`), in every language.

### 6. NFC/NFD normalization fixtures absent
**Severity: P1**
Untested: `maxLength.md` names it as a runtime fixture — "NFC `"é"` counts as **1** and NFD `"e"+U+0301` as **2** — every language agrees on each because none normalizes." No test anywhere uses a combining mark (grep for U+0301 across `samples/` and `tests/` returns nothing).
Spec line: `maxLength.md` §"Runtime fixtures (validator)".
Where: alongside the existing astral assertions in `samples/{go,typescript,python}/tests/*showcase*` and `samples/java/.../JsonSchemaShowcaseRoundTripTest.java`.
Suggested case: against `code` (`minLength: 2, maxLength: 5`) — `"éé"` (NFC, 2 code points) accepts; `"éé"` (NFD, 4) accepts; `"ééé"` (NFD, 6) rejects with `got 6` in all four.

### 7. No Java `matches()`-vs-`find()` anchoring regression test at the sample level
**Severity: P1**
Untested cross-language: the spec calls `Matcher.matches()` the "verified footgun". `tests/generate_java.rs:490` asserts the rendered text contains `.find()`, which is a good guard, but no *runtime* test in any language asserts that an **unanchored substring** pattern accepts a longer instance — the corpus's `literal-substring-hit` (`"cat"` vs `"the cat sat"`) is never executed against a runtime.
Spec line: `pattern.md` §Anchoring — "`"cat"` fails to match `"the cat sat"` — verified".
Where: `samples/schemas/showcase.nexusrpc.yaml` (add an unanchored `pattern: "cat"` field) + all four suites; or fold into gap #1's corpus runner.
Suggested case: `{type: string, pattern: "cat"}` accepts `"the cat sat"` and rejects `"the dog sat"` in Go/TS/Python/Java.

### 8. Non-number and fractional bound rejects only partially covered
**Severity: P2**
Untested: `minLength: "1"`, `minLength: false`, `maxLength: true`, `maxLength: null`, `maxLength: 5.5`, `minLength: 0.5` — all explicit Rejected-matrix rows. Only `maxLength: -1` (`src/parser/json_schema.rs:8142`) and the non-string-type case (`:8124`) are tested. (I verified by hand that the loader *does* reject them all correctly at `src/parser/json_schema.rs:2027-2044`; they are simply untested.)
Spec line: `maxLength.md` Rejected matrix — "Value not a number | `maxLength:"5"`, `maxLength:true`, `maxLength:null`"; "Fractional value | `maxLength:5.5`".
Where: `src/parser/json_schema.rs` `#[cfg(test)] mod tests`, next to `rejects_negative_max_length`.

### 9. `default` literal vs `pattern` / vs bounds untested
**Severity: P2**
Untested: the `default` arm of the literal-vs-constraint obligation. `rejects_const_violating_pattern` (`:8213`) and `rejects_enum_violating_pattern` (`:8225`) exist; `rejects_const_string_violating_max_length` (`:8136`) and `rejects_const_below_min_length` (`:8148`) exist. There is **no** `default` case for either keyword — which is why divergence #6 (Unicode `\d` in the loader matcher) is invisible to the suite.
Spec line: `pattern.md` Rejected matrix — "Literal fails pattern | `{type:"string", pattern:"^[a-z]+$", const:"AB"}`, `{…, default:"9"}`"; `minLength.md` — "`{…, default:""}`".
Where: `src/parser/json_schema.rs` tests.
Suggested case: `type: string\npattern: "^[a-z]+$"\ndefault: "9"` and `type: string\npattern: "^\\d+$"\ndefault: "٣"` (the second currently **passes** and must not).

### 10. `\S` sole-member / in-class placements never exercised at runtime
**Severity: P2**
Untested: `[\s.]`, `[^\s]`, `[\S]`, `[^\S]` are unit-tested at the *rewrite* level (`src/json_schema/pattern.rs:275-283`) but never generated or executed. Only standalone `\s`/`\S` reaches a runtime, via `showcase.nexusrpc.yaml:145` (`^\S+\s\S+$`).
Spec line: `pattern.md` Accepted matrix — "`\s`/`\S` (normalized to ASCII class) | `{…, pattern:"^\\s+$"}`, `{…, pattern:"\\S"}`, `{…, pattern:"[^\\s]"}`".
Where: add a field to `samples/schemas/showcase.nexusrpc.yaml` using `[^\s]` and one using `[\s.]`, asserted against NBSP in all four suites.

### 11. `maxLength: 0` (empty-string-only) and `minLength == maxLength` never exercised at runtime
**Severity: P2**
Untested: both are Accepted-matrix rows. `accepts_valid_string_bounds` (`:8165-8180`) loads a `fixed: {minLength: 3, maxLength: 3}` field but nothing generates or runs it, and `maxLength: 0` appears nowhere.
Spec line: `maxLength.md` Accepted matrix — "Zero max (empty string only) | `{type:"string", maxLength:0}`"; "Exact length (min==max)".
Where: `tests/generate_*.rs` and/or the showcase schema.
Suggested case: `maxLength: 0` accepts `""` and rejects `"a"` with `must have length <= 0, got 1`; `minLength: 3, maxLength: 3` accepts exactly 3 code points including `"a😀b"`.

### 12. No conformance-manifest case for string constraints
**Severity: P2**
Untested: `samples/conformance/json-schema.json` has 4 cases (recursive collections, number equality, year-zero, null collapse) and none for `minLength`/`maxLength`/`pattern`, even though the astral count and the `\s`/`$` normalization are the flagship P1 cross-language claims and each language already has an equivalent local test.
Spec line: `maxLength.md` P1 crux — "the count must be *identical* across all four"; `pattern.md` — "all four agree value-for-value".
Where: `samples/conformance/json-schema.json`, checked by `tests/json_schema_conformance_manifest.rs`, wired to the existing per-language anchors.
Suggested case: id `string-length-code-points` (astral `code` accept/reject) and id `pattern-ascii-class-and-end-anchor` (NBSP and trailing-`\n` rejected via `phrase`).

## Combination gaps

| Feature A × Feature B | Spec says | Tested? | Risk |
|---|---|---|---|
| `minLength` × `maxLength` (`min > max`) | load reject, unsatisfiable (`minLength.md`, `maxLength.md`) | Yes — `src/parser/json_schema.rs:8130` | — |
| `minLength` × `maxLength` (`min == max`) | accepted, pins an exact length | Load only (`:8165`); never generated or run | Low |
| `minLength` × `pattern` (both fail) | independent; **both** aggregate in one `ValidationError` (P11) | **No** — every sample asserts exactly one violation | **Medium** — a real P11 claim with zero coverage |
| length × `pattern` satisfiability | explicitly **not** cross-checked | N/A (correctly absent) | — |
| length × `const`/`enum` | literal must satisfy the bound at load | Yes — `:8136`, `:8148`, `:8154` | — |
| length × `default` | literal must satisfy the bound at load | **No** test (code path exists at `:2079`) | Low |
| `pattern` × `const`/`enum` | literal must match at load | Yes — `:8213`, `:8225` | — |
| `pattern` × `default` | literal must match at load | **No** — and the check uses Unicode classes (div. #6) | **Medium** |
| length/`pattern` × `contentEncoding` | bound/regex applies to the **encoded wire string**, both directions | Python only (`tests/generate_python.rs:154-157`); **Go emits non-compiling code** (div. #4) | **High** |
| length/`pattern` × `format` (temporal) | bound/regex applies to the canonical wire string | Yes — `tests/generate_go.rs:2452`, and Go handles it (`go.rs:2686-2715`) | — |
| `pattern` × `format` (regex-lowered) | both apply; both compile-once | Yes — showcase `requestId`/`host`/`homepage` | — |
| length/`pattern` × `type` mismatch (P7.1) | load reject | Yes — `:8124`, `:8207` | — |
| length/`pattern` × nullable (`oneOf [T, null]`) | branch constraints apply to the non-null branch | Yes — showcase `Nicknames`, `idOrName` | — |
| `pattern` × `propertyNames` | key-space regex, compile-once | Behavior yes; **Go recompiles per key** (div. #7) | Medium |
| `pattern` × `contains` | matcher must not drop the predicate | Yes (`tests/generate_go.rs:2186`) — but the test *asserts* the per-element recompile | Medium |
| `pattern` × `allOf` merge | distinct patterns on merged conjuncts reject | Yes — `rejects_all_of_distinct_patterns` (`:9804`) | — |
| `pattern` × astral instance | `u` flag / rune `.` so `.` is one code point | TS `u` flag emitted; corpus has `dot-astral-emoji`/`dotstar-astral` but no runtime driver | Medium |
| `pattern` (`.`) × line terminators | not addressed | **No** — corpus only has `a.b` vs `a\nb` | **High** (div. #2) |
| `pattern` × nested char class / `&&` | `\s`/`\S` must be normalized everywhere | **No** | **High** (div. #3) |
| `pattern` × backtracking blowup | gate premised on the "linear-time" subset | **No** | **High** (div. #9) |
| `minLength: 0` × anything | treated as omitted | Load accept only (`:8160`); emitted code not asserted | Low (div. #10) |
| empty `pattern: ""` | vacuous, accepted | Load only (`:8231`); `MustCompile("")`/`new RegExp("","u")` never executed | Low |

## Verified-good

- **Code-point counting in all four emitters.** Go `utf8.RuneCountInString` (`go.rs:243`), TS surrogate-aware spread (`typescript.rs:407`), Python `len` on `str`, Java `codePointCount(0, length())` — never `len`/`.length`. Verified in generated output for properties, array items, map members, `contains` matchers, and `propertyNames` keys.
- **Astral fixtures actually run.** `"a😀b"` (3 code points, 6 bytes, 4 UTF-16 units) passes `maxLength: 5` and `"😀"×6` is rejected with `got 6` — asserted in Go (`json_schema_showcase_test.go:140-167`), TS (`json-schema-showcase.test.ts:133-283`), Python (`test_showcase.py:427-446`) and Java (`JsonSchemaShowcaseRoundTripTest.java:162-269`).
- **Unanchored search everywhere.** Generated code uses `MatchString` / `RegExp.test` / `re.search` / `Matcher.find()` — never `matches()`, `re.match`, or `fullmatch`. Guarded at the text level by `tests/generate_java.rs:490`.
- **`$` end-anchor rewrite is correct per target.** `^[a-z]+$` → `^[a-z]+\Z` (Python), `^[a-z]+\z` (Java), unchanged (Go/JS); escaped `\$` and `^` untouched (`pattern.rs:286-291`). The trailing-`\n` divergence is asserted at runtime in the Python and Go showcase suites.
- **Standalone `\s`/`\S` normalization.** `^\S+\s\S+$` → `^[^\t\n\x0B\f\r ]+[\t\n\x0B\f\r ][^\t\n\x0B\f\r ]+$` in all four; the NBSP rejection is asserted at runtime in Go/TS/Python/Java.
- **Python `re.ASCII` and the TS `u` flag** are emitted unconditionally (`re.compile(..., re.ASCII)`, `new RegExp(..., "u")`), and Java/Go use default (ASCII-class) flags.
- **Backtracking rejects work**: lookahead, negative lookahead, lookbehind, negative lookbehind, backreference, inline `(?i)`/`(?i:…)`, and `[\S.]`/`[\S\d]` all reject at load with named diagnostics (`pattern.rs:294-312`, `src/parser/json_schema.rs:8183-8240`); `(?:ab)+` correctly accepted.
- **Length load-time rejects**: non-string type, negative, `min > max`, and `const`/`enum` literal over/under the bound — all implemented (`src/parser/json_schema.rs:2003-2093`) and unit-tested; the literal length uses `chars().count()` (code points), correctly.
- **P12 serialize-side re-check** is genuinely present in all four for length and pattern (Go `Validate()` called from `MarshalJSON`, TS `toTransferType`, Python `to_transfer_type`, Java `Serializer`), and the "in-memory over-length string fails to marshal" case is asserted in Go/TS/Python/Java.
- **`contentEncoding` + length/pattern is correct in TypeScript, Python and Java** — each projects the base64 wire string (`bytesToBase64` / `_format_base64` / `Base64Support.formatBase64`) before checking, on the serialize side. Only Go is broken.
- **Format-pinned patterns pass the gate** — `src/json_schema/format.rs:410-415` asserts every pinned `format` regex survives `gate_and_normalize`.
- **Reason strings** match the spec convention in all four (`must have length >= 2, got 1`, `must have length <= 5, got 6`, `must match pattern …, got …`).
