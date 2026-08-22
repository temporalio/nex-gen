# shared-helpers — execution report

Owns `src/json_schema/{pattern,format,content_encoding,scalar,mod}.rs` plus the
`specs/json-schema/corpora/**` data files.

Every rule below was **measured** against Go 1.26 `regexp`, Node v26.5.0
`RegExp(..., "u")`, CPython 3.13 `re` + `re.ASCII`, and OpenJDK 21
`java.util.regex` — not reasoned about. The probe harness lives in
`/tmp/reprobe/` (`probe.py`, `verify.py`, one runner per language).

---

## Fixed

### `08#1` **P0** — the `pattern` gate is one-directional
`src/json_schema/pattern.rs:102-134` (`gate_and_normalize`) now runs a full
`regex-syntax` AST portability pass after the compile gate. Every construct
Rust accepts that a target engine either cannot compile or reads differently is
rejected with a fix-it naming the offending spelling and the portable
replacement.

| Rejected construct | Where it breaks | Code |
|---|---|---|
| Escaped punctuation outside the ECMA-262 `u` `IdentityEscape` set (`\-`, `\_`, `\"`, `\ `, `\#`, `\&`, `\~`, …) | JS `SyntaxError` | `pattern.rs:293-322` (`check_literal`, `PORTABLE_ESCAPES` at `:71`) |
| `\a` | JS | `pattern.rs:342-345` |
| `\v` | Java reads it as a *vertical whitespace class*, not U+000B | `pattern.rs:346-351` |
| `\0` / octal | Java `Illegal octal escape sequence` | `pattern.rs:323-326` |
| `￿` | Go `invalid escape sequence` | `pattern.rs:328-331` |
| `\UFFFFFFFF` | JS, Python, Java | `pattern.rs:332-335` |
| `\x{…}` | JS + Python | `pattern.rs:336-341` |
| `\p{…}` / `\pL` | Python (`\pL` also JS) | `pattern.rs:197-202`, `:468-471` |
| lone `{`, `}`, `]` outside a class | JS (`}`/`]`/`{`), Java (`{`) | `pattern.rs:300-311` |
| `\A`, `\z`, `\b{start}`/`\b{end}`/`\<`/`\>` | JS (`\z` also Python) | `pattern.rs:264-291` |
| named capture groups `(?P<n>…)` / `(?<n>…)` | Java / Python respectively | `pattern.rs:244-256` |
| POSIX classes `[[:alpha:]]` | JS `SyntaxError`; Go/Python/Java disagree on the match | `pattern.rs:463-467` |
| nested classes `[a[…]]` | JS `SyntaxError`; Go/Python/Java disagree | `pattern.rs:458-462` |
| class set ops `&&` / `--` / `~~` | JS `SyntaxError`; Go/Python/Java disagree | `pattern.rs:431-445` |

The flagship case `{"type":"string","pattern":"^\\d{3}\\-\\d{4}$"}` now fails
to load with
`pattern "^\d{3}\-\d{4}$" escapes '-' as '\-', which is a JavaScript SyntaxError
… write '-' unescaped` instead of emitting a TS module that throws on import.
**Verified** through the CLI (`nexgen typescript` exits 1 with that message).

### `08#2` **P0** — `.` diverges on line terminators
`.` is now normalized to `[^\n]` at load (`pattern.rs:57-64` `DOT_CLASS`,
`:186-193` the `Ast::Dot` arm), pinning all four engines to the RE2/Python
reading. Measured before: `a.b` vs `"a\rb"` → Go T, Python T, JS F, Java F.
After: T/T/T/T; `\n` stays F/F/F/F; U+0085, U+2028, U+2029 and an astral code
point all agree. **Verified** end-to-end: generated Go emits
`regexp.MustCompile("a[^\\n]b")`, TS emits `/a[^\n]b/u`, Python
`re.compile("a[^\\n]b", re.ASCII)`, Java `Pattern.compile("a[^\\n]b")`;
`go vet` clean and `go run` accepts `{"wildcard":"a\rb"}` / rejects `"a\nb"`.

### `08#3` **P0** — `\s`/`\S` escaping through nested classes and `&&`
`flatten` is replaced by `collect_class_set` / `collect_class_item`
(`pattern.rs:421-478`), which **cannot silently drop a subtree**: a nested
class, POSIX class, Unicode property or set binary operation is an error rather
than an opaque leaf or an empty vector. `[a[\s]]`, `[\w&&\s]` and `[[\S]]` are
now load rejects (all three are JS `SyntaxError`s anyway, and Go/Java disagreed
on the match). `[[\S]]` in particular no longer bypasses the
`\S`-in-multi-member-class reject.

### `08#9` P1 / decision **D7** — ReDoS
`check_repetition` (`pattern.rs:508-534`) rejects an **unbounded** repetition
(`*`, `+`, `{n,}`) whose body is ambiguous:
- nullable (`(a?)+`, `(a*)*`);
- reduces to another *inexact* repetition after stripping groups — including a
  concatenation whose every other element is nullable (`(a+)+`, `(a{1,2})+`,
  `(a+|b)*`, `(a+b*)+`, `(\w+\s*)+`);
- an alternation with two textually identical branches (`(a|a)*`).

An **exact-count** inner repetition stays accepted, which is what keeps the
generator's own pinned regexes inside the gate: `(?:[A-Za-z0-9+/]{4})*`, the
`hostname` / `email` / `uri` / `uri-reference` bodies and the whole `ipv6`
grammar all still pass (asserted by
`format.rs::pinned_patterns_pass_the_pattern_gate`, now extended to cover the
four temporal patterns too). Measured CPython times for the rejected shapes at
a 27-char input: `(a+)+` 2.2 s, `(a*)*` 3.4 s, `(a|a)*` 4.7 s, `([a-z]+)+`
2.6 s; the accepted shapes `(a+b)+`, `(\.a+)+`, `(a{2})+`,
`(?:[A-Za-z0-9+/]{4})*` are all 0.00 s.

**Known residual (documented, not fixed):** alternation ambiguity that is not a
textual duplicate — e.g. `^(a|ab)+$` — is still accepted. It measured 0.00 s in
CPython, and a sound first-set/ambiguity analysis needs HIR-level character-set
computation that would risk over-rejecting real schemas. Noted in the
`check_repetition` doc comment.

### `08#12` P2 — corpus drift
`specs/json-schema/corpora/pattern_conformance/corpus.json`: **83 → 140 pairs**.
- `case-inline-flag` now carries `expect_gate_reject: true`; the hard-coded
  `|| id == "case-inline-flag"` in the test is gone (the `conformance_corpus_gate_agrees` test).
- Added the placements `pattern.md` credits the corpus with pinning but that
  were absent: `[\s.]` (×3), `[^\s]` (×3, incl. NBSP), `[\S]` (×3), `[^\S]`
  (×3), and the `[\S.]` / `[\S\d]` rejects.
- Added a row for **every** case in the `08#1` table, the four `.`
  line-terminator instances, the ReDoS family, and the portable counter-cases
  (`[a\-z]`, `[{}]`, `a\/b`, `a\x41b`, `a\}b`, `(?:foo)+bar`,
  `(?:[A-Za-z0-9+/]{4})*`, `(\.a+)+`).
- **Every one of the 102 gate-accepted pairs now carries `expect_match`**, and
  the 38 gate-rejected pairs carry none. Two new tests enforce that shape
  (`conformance_corpus_gate_agrees`, `conformance_corpus_declares_expected_matches`).
- The `expect_match` values are not asserted from theory: each was produced by
  running the **gate-normalized** pattern (with the per-target `$`→`\Z`/`\z`
  rewrite) through all four real engines in the pinned configuration.
  **All 102 pairs agree 4/4 with zero divergences** — that run is the evidence
  that the gate + normalization actually closes `08#1`/`08#2`/`08#3`.
  The conformance agent's per-language runners can consume `expect_match`
  directly.

### `10#4` P1 / decision **D1** — base64 canonical form
`content_encoding.rs:69-80`. The final-quantum groups now constrain the unused
low bits of the last significant character:
- base64 `(?:[A-Za-z0-9+/][AQgw]==|[A-Za-z0-9+/]{2}[AEIMQUYcgkosw048]=)?`
- base64url `(?:[A-Za-z0-9_-][AQgw]|[A-Za-z0-9_-]{2}[AEIMQUYcgkosw048])?`

(Note: the brief's spelling kept the *original* `{2}`/`{3}` counts, which would
have lengthened the quantum by one character; the base64url spelling it gives
confirms the intent, so the total length is preserved.) `[AQgw]` are the
alphabet indices divisible by 16 (four unused bits), `[AEIMQUYcgkosw048]` those
divisible by 4 (two unused bits). No re-canonicalization step was added.

New test `rejects_non_canonical_trailing_bits` covers `aGl=`/`AB==`/`//9=` and
base64url `aGl`/`AB`, and **exhaustively** checks that every canonical encoding
of all 256 one-byte and 256 two-byte payloads is still accepted (via a small
reference encoder in the test module — the crate has no base64 dependency).

**Verified end-to-end in Go**: with the tightened regex,
`{"wildcard":"axb","payload":"aGl="}` is now a parse `Violation`
(`payload: must be base64-encoded, got "aGl="`) instead of round-tripping back
out as `"aGk="`.

### `09#2` / `09#7` **P0/P1 (my half)** — a real canonicalization entry point
`format.rs:257-352`:
- `pub fn canonicalize(kind: TemporalKind, value: &str) -> Option<String>` —
  covers **all four** temporal kinds, returning `None` for a value the
  materialized grammar rejects (so a caller validates + canonicalizes in one
  step).
- `pub fn canonicalize_for_format(format: &str, value: &str) -> Option<String>`
  — the same keyed by format name, `None` for non-temporal formats. Callers
  wanting "canonical wire, or the literal unchanged" write
  `.unwrap_or_else(|| v.to_string())`.
- `canonicalize_duration` is no longer dead code (it is `canonicalize`'s
  `Duration` arm) and its doc now points at `canonicalize`.

Rules implemented per `format.md` "Serialized form": `date` is already
canonical; `time`/`date-time` uppercase the RFC 3339 §5.6 lowercase `t`/`z`,
fold `+00:00` / `-00:00` → `Z`, and trim trailing zeros from the fractional
seconds (dropping an all-zero fraction); `duration` recomposes
(`PT90M`→`PT1H30M`). Two tests: an explicit 18-row table plus an idempotence
sweep over every row of `format_materialize_clock/corpus.json`.

### `09#10` P2 — the unknown-format fix-it hid the temporal names
`format.rs:31-62`. `SUPPORTED_FORMATS` is now the **11** formats the generator
actually accepts (7 string-shaped + 4 temporal); the regex-lowered subset moved
to a new `STRING_FORMATS`. The loader's message
(`parser/json_schema.rs:2188`) picks this up with no change on its side, so
`format: datetime` is now rejected with a list that contains `date-time`. New
test `supported_formats_names_every_accepted_format` asserts the constant can
never again advertise less (or more) than `classify` accepts. The two internal
uses were repointed at `STRING_FORMATS`, and
`pinned_patterns_pass_the_pattern_gate` was extended to also drive the four
temporal patterns through the gate.

### `09#13` P2 — stale doc comment
`format.rs:11-21` no longer claims the temporal formats are "rejected at load as
not yet supported (temporal, pending)"; it describes materialization, the
narrowed grammar and the canonicalization entry point. (The other half of the
finding, `parser/json_schema.rs:2100-2104`, is the loader agent's file — see
cross-file requests.)

---

## Not fixed

- **`08#6` P1** (loader runs Rust's *Unicode* `\d`/`\w` when checking a literal
  against `pattern`; needs `RegexBuilder::unicode(false)`) — the code is in
  `src/parser/json_schema.rs`, which I do not own. Listed below.
- **Alternation ambiguity that is not a textual duplicate** (`^(a|ab)+$`) is
  still gate-accepted — see the `08#9` residual above.

---

## Cross-file requests

1. **`src/generator/json_schema/java.rs` (java-emitter) — NEW P0 found while
   probing.** Java's static support files emit the pinned regexes with a raw
   `$` and never call `pattern::rewrite_end_anchor(p, "\\z")`. Java's `$`
   matches *before a single trailing `\n`*, so:
   - `Base64Support.java:13-14` → Java **accepts** `"aGk=\n"` as base64; Go,
     JS and Python all reject it (measured: go F, js F, py T→F once Python's
     `\Z` rewrite is applied — Python's `_definitions.py:487` already emits
     `\Z` correctly, Java does not).
   - `TemporalSupport.java:18-21` (both `definitions/temporal/` and
     `definitions/showcase/`) → same problem for all four temporal grammars:
     `"2021-06-15T12:30:45Z\n"` parses in Java only.

   Fix: route every pinned pattern the Java backend emits through
   `crate::json_schema::pattern::rewrite_end_anchor(p, "\\z")`, as the property-
   level `pattern` path already does. This is the exact shape of `09#4` but on
   the Java side.

2. **`src/parser/json_schema.rs` (loader)** — `08#6`: the literal-vs-`pattern`
   check must compile with `regex::RegexBuilder::new(p).unicode(false)`, so
   `{pattern: "^\\d+$", default: "٣"}` rejects at load instead of emitting a
   default all four runtimes reject.

3. **`src/parser/json_schema.rs` (loader)** — `09#13` second half: the doc
   comment at `:2100-2104` still says "the temporal formats (materialization
   pending)".

4. **`src/parser/json_schema.rs` (loader)** — `09#2`: the canonicalization
   entry point you asked for is ready. Use
   `crate::json_schema::format::canonicalize_for_format(format, literal)` (or
   `format::canonicalize(kind, literal)` when you already have the
   `TemporalKind`); it returns `None` exactly when `format::is_valid` would
   return `false`, so it can replace the current validate-only call.

5. **`src/generator/json_schema/java.rs` (java-emitter)** — `09#7`: for a
   temporal `const`/`default` literal, render
   `format::canonicalize(kind, literal)` rather than the authored string, so
   `OffsetDateTime.parse("2021-06-15t12:30:45z")` becomes
   `OffsetDateTime.parse("2021-06-15T12:30:45Z")`. Same helper serves the D10
   canonical-wire comparison in all four emitters.

6. **`specs/json-schema/features/pattern.md` (specs)** — the spec must gain:
   - the `.` → `[^\n]` normalization rule (currently there is none, and the
     "Character classes & the dot" section only says `.` must match one code
     point);
   - the new reject list (non-portable escapes, lone `{`/`}`/`]`, `\A`/`\z`,
     named capture groups, POSIX/nested/set-operation classes, ambiguous
     unbounded quantifiers) alongside the existing three rules;
   - the corpus is now **140 pairs**, not 83, and each accepted pair carries an
     `expect_match`;
   - the "Deferred, not excluded" and "Open questions" lists should name the
     newly rejected constructs as future-admission candidates;
   - the Prospective-targets table's claim that .NET/Ruby need "no new gate
     rules" should be re-read against the new rules (the .NET astral-`.` rewrite
     is now moot — `.` is already an explicit class).

7. **`specs/json-schema/features/contentEncoding.md` (specs)** — quote the new
   pinned regexes and state the trailing-bits rule explicitly (the byte-identity
   claim at `:87-95` is now actually true).

8. **`specs/json-schema/features/format.md` (specs)** — the unknown-format
   fix-it now lists 11 names; `format.md:116-119` should say so.

---

## Sample schema requests

None required for my changes (the three existing sample schemas still load
unchanged — verified by running the CLI over each). If a regression fixture is
wanted, the smallest useful pair is:

```yaml
  wildcard:
    description: A pattern with a bare dot, normalized to an explicit class.
    type: string
    pattern: "a.b"
```

added to `samples/schemas/showcase.nexusrpc.yaml`, which would pin the `.`
normalization in every golden sample.

---

## Snapshot shifts

The base64 pinned regex text changes, so these checked-in files move on the
consolidated regeneration pass (no test asserts the literal text; the
whole-file `*_matches_checked_in_output` comparisons will fail until then):

- `samples/go/showcase/showcase.go`
- `samples/python/{showcase,chat,kb,temporal}/_definitions.py`
- `samples/typescript/showcase/definitions.ts`
- `samples/java/src/main/java/json_schema/definitions/showcase/Base64Support.java`

No sample uses a bare `.` in a `pattern`, so the `.` normalization shifts
nothing today.

---

## Changelog — constructs that used to load and now reject

A previously-accepted schema will now fail to load if its `pattern` contains any
of:

1. an escaped punctuation character outside `^ $ \ . * + ? ( ) [ ] { } | /`
   (plus `-` inside a class) — most commonly **`\-`**;
2. `\a`, `\v`, `\0` or any octal escape, `￿`, `\UFFFFFFFF`, `\x{…}`;
3. `\p{…}` or `\pL`, anywhere;
4. a lone `{`, `}` or `]` outside a character class (write `\{`, `\}`, `\]`);
5. `\A`, `\z`, `\b{start}`, `\b{end}`, `\<`, `\>`;
6. a named capture group, in either spelling;
7. a POSIX class `[[:name:]]`, a nested class `[a[…]]`, or a class set
   operation `&&` / `--` / `~~`;
8. an ambiguous unbounded quantifier — `(a+)+`, `(a*)*`, `(a?)+`, `(a+b*)+`,
   `(a+|b)*`, `(a|a)*` and friends.

A `contentEncoding: base64` / `base64url` literal (in `const`, `enum`,
`default`, or on the wire) with non-zero unused trailing bits — `"aGl="`,
`"AB=="`, `"//9="`, base64url `"aGl"` — is now rejected instead of being
silently re-encoded to a different string.

Every rejection carries a fix-it naming the offending spelling, the engine(s)
it breaks, and the portable replacement.

---

## Test status

- `cargo build --all-features` clean and `cargo test --all-features --lib`
  **393 passed / 0 failed** at the point my last change landed.
- `cargo fmt --check` clean on all three files I touched.
- The **full** `cargo test --all-features` could not be run to completion: the
  shared working tree was mid-edit by the go-, python- and java-emitter agents
  (`cannot find type PlannedJsonType`, `no method adopt_tree_models`,
  `render_py_field_checks` arity) at every attempt. Nothing in those failures
  touches `src/json_schema/`.

---

# Addendum — `corpus:leap-second-rows` (coordinator follow-up)

## Verdict: the corpus data is wrong, not the runtimes. Rows flipped.

I agree with the proposed resolution, and the decisive evidence is inside the
corpora themselves — the two notes contradict each other:

- `format_conformance`'s note says *"leap second :60 **ACCEPTED**
  syntactically"* and its two rows say `expect_valid: true`.
- `format_materialize_clock`'s note describes the very same check as
  *"the pinned materialized-temporal format check (format_conformance,
  **:60-rejecting grammar**)"* — and then admits its own two `:60` rows *"are
  rejected by every native parser **(SKIP)**"*, while sitting in a set whose
  stated contract is "wire strings that are VALID".

So one corpus documents the pre-materialization syntactic grammar and the other
documents the post-materialization narrowed one. `format_conformance` is the
stale side:

1. **The implemented check rejects `:60` by construction.** `materialized_pattern`
   spells the seconds group `[0-5][0-9]`; `60` cannot match. There is no code
   path, flag or mode in the generator today that accepts it.
2. **Measured, not inferred** — the four emitted temporal regexes run through
   Go 1.26, Node v26.5.0 (`u`), CPython 3.13 (`re.ASCII`) and OpenJDK 21:

   | row | value | go | js | py | java |
   |---|---|---|---|---|---|
   | `dt-leap-second` | `2021-02-28T23:59:60Z` | F | F | F | F |
   | `dt-leap` | `2021-12-31T23:59:60Z` | F | F | F | F |
   | `t-leap` / `time-second-60-leap` | `23:59:60Z` | F | F | F | F |
   | *control* | `2021-12-31T23:59:59Z` | T | T | T | T |
   | *control* | `23:59:59Z` | T | T | T | T |

   Unanimous reject, unanimous accept on the controls — so the disagreement is
   purely corpus-vs-implementation, with no cross-runtime component.
3. **RFC 3339 does permit `:60`, and that is exactly why the rows were authored
   `true`** — but `format.md` deliberately narrows it away on a *materialized*
   node (no target's native temporal type can hold `:60`), and specifies the
   wider `:60`-accepting grammar behind the **`string` opt-out**
   (`format.md:323-332`, `:440`), which `09#8` found is entirely unimplemented.
   The rows describe behaviour that does not exist in any configuration.

Both corpora now say the same thing, and both record the RFC position and the
condition for flipping back, so nothing is lost.

## What changed

**`specs/json-schema/corpora/format_conformance/corpus.json`**
- `time-second-60-leap` and `dt-leap-second`: `expect_valid` **true → false**.
- The note's `"leap second :60 ACCEPTED syntactically"` clause is replaced with
  the narrowing, the measured 4-way verdict, the RFC caveat, and the explicit
  instruction to flip both rows back when the `string` opt-out ships.
- Dropped the dangling `see NOTES.md` citation — there is no `NOTES.md` in that
  directory (or anywhere under `specs/`).

**`specs/json-schema/corpora/format_materialize_clock/corpus.json`**
- `dt-leap` and `t-leap` now carry `"expect_valid": false`. The prose `(SKIP)`
  instruction in the note is gone: a runner should not have to parse English —
  or match on `":60"` — to know a row is a negative case. Absent field = valid
  (so every other row and every existing consumer is unchanged).
- The note's first sentence now states that rule, and records the flip-back
  condition.

**`src/json_schema/format.rs` — all three `:60` force-flips removed**
- `materialized_temporal_conformance_with_leap_narrowing` → renamed
  `materialized_temporal_matches_the_conformance_corpus`; the
  `if value.contains(":60") { expect = false }` compensation is gone and the
  assertion reads `expect_valid` verbatim (`format.rs:640-654`).
- `materialized_clock_roundtrip_values_are_valid`: `let expect =
  !wire.contains(":60")` → `clock_row_is_valid(row)` (`format.rs:828`).
- `canonicalize_is_idempotent_over_the_clock_corpus`: the
  `assert!(wire.contains(":60"))` escape hatch → `assert!(!clock_row_is_valid(row),
  "… is declared valid but does not canonicalize")` (`format.rs:779-783`).
- New shared reader `clock_row_is_valid` (`format.rs:756-762`), the single place
  the default-is-valid rule is spelled.

**Mutation-checked.** All three assertions previously passed no matter what the
data said. I flipped `dt-leap-second` back to `true` and removed `t-leap`'s
field: **all three tests now fail** (`format.rs:648`, `:780`, `:829`), then pass
again on restore. They have teeth for the first time.

`cargo test --lib json_schema::` — **407 passed / 0 failed**; `rustfmt --check`
clean.

## Harness state — one cross-file change needed to finish this

I ran `cargo test --test json_schema_corpus_runtime` against the fixed data.
Half of `corpus:leap-second-rows` is now closed and half is blocked on the
harness, which is the conformance agent's file:

- ✅ `format_conformance/time-second-60-leap` and `dt-leap-second` produce **no
  finding at all** any more. Their two `OpenRow` entries are now stale and the
  harness fails with *"the open-row entry … matches nothing any more — delete
  it"* (`tests/json_schema_corpus_runtime.rs:419`). **They must be deleted.**
- ⚠️ `format_materialize_clock/dt-leap` and `t-leap` still report
  `"<target> parse_rejected, corpus says accept"` ×4 each, because the clock
  loop hardcodes `Expectation::Agree` and does not read the new field.

**Cross-file request — `tests/json_schema_corpus_runtime.rs` (conformance).**
One change, then all four `corpus:leap-second-rows` entries go at once. At
`:390-402`, in the `format_materialize_clock` loop:

```rust
-                expected: Expectation::Agree,
+                // Absent `expect_valid` = a valid wire whose re-emitted string
+                // must agree across targets; `false` declares a wire every
+                // target must reject (the leap-second rows).
+                expected: match row.get("expect_valid").and_then(Value::as_bool) {
+                    Some(false) => Expectation::Rejected,
+                    _ => Expectation::Agree,
+                },
```

then delete all four `OpenRow` entries whose `finding` is
`"corpus:leap-second-rows"` (`:438-459`). `Expectation::Rejected` already
requires `outcome == "parse_rejected"` in every target and compares
`outcome_summary()` rather than the wire, which is exactly the contract for
these rows. I verified the observed verdicts are a clean 4-way
`parse_rejected`, so the entries will be stale immediately after that change.

## One more corpus row worth a decision (NOT changed — deliberate hold)

The same harness run flags `format_conformance/dt-frac-high-precision`
(`2021-01-15T12:30:45.123456789012Z`, 12 fractional digits) under a *different*
finding, `new:java-rejects-12-digit-fraction`: Go, Python and TypeScript accept,
**Java rejects**. That is a genuine P1 accept-set divergence, not a corpus
error — and the fix would land in **my** file, so flagging it rather than
silently leaving it:

`materialized_pattern` spells the fraction `(\.[0-9]+)?` — unbounded precision.
Java's `ISO_OFFSET_DATE_TIME` caps at 9 digits (nanosecond) and throws beyond
that; the other three accept and truncate. The uniform-cap precedent is already
in this file — `MAX_DURATION_NANOS` is documented as *"the uniform
cross-language overflow cap … (P1: identical accept/reject in every target)"* —
and the analogous one-token fix is `(\.[0-9]{1,9})?` in all three clock
patterns, after which all four reject 12 digits uniformly. The alternative
(teach Java to parse-and-truncate) would convert an accept-set divergence into a
*round-trip* divergence, which is strictly worse.

I did not make the change because the finding has a different id and owner, and
because it is a second silent accept-set narrowing that would also strand that
finding's `OpenRow` entry in the same harness file. **Say the word and it is a
one-line edit in `format.rs` plus deleting one more `OpenRow`** — batching it
with the leap-second deletions above costs nothing extra.

## Calibration noted

Point taken on the Base64/Temporal end-anchor finding: I graded it P0 from the
pattern text without checking the call sites. `Matcher.matches()` requires a
full-span match and `$` is zero-width, so the trailing `\n` was never consumed —
Java's accept set already agreed. I should have run the accept/reject through
the generated Java before assigning severity, exactly as I did for the Go
base64 and `.`-normalization claims. The grep-level evidence supported "the
pattern text differs from the other three", which is P2 latent coupling, not the
P1 accept-set claim I made. Applying that standard here: every verdict in this
addendum is a measured runtime result, not a reading of the regex.

---

# Addendum 2 — `new:java-rejects-12-digit-fraction` (authorized)

## Done. `(\.[0-9]+)?` → `(\.[0-9]{1,9})?`

I agree with the reasoning, and checked it rather than taking it: the cap is not
a common-denominator floor, it is the real capacity of three of the four
targets. Go `time.Time` and Java `java.time` are nanosecond; TS `Temporal` is
nanosecond; only Python's `datetime` is tighter at microseconds, and that
residual is a **round-trip** loss already tracked as `09#8` under P1 exception
(b) — a different axis from the **accept set**, which exception (b) does not
cover and which P1 does not bend on. Nine digits is therefore the widest value
every target can actually hold, which is exactly the `MAX_DURATION_NANOS`
pattern already in this file.

The counterfactual also holds up: teaching Java to parse-and-truncate 12 digits
would have moved the divergence from "one target rejects" to "one target
re-emits a different wire", i.e. from a loud accept-set split to a silent
round-trip split. Strictly worse.

**Changed** — `src/json_schema/format.rs:121-149`, the `Time` and `DateTime`
arms of `materialized_pattern` (`Date` and `Duration` have no fractional group).
The `materialized_pattern` doc now carries the capacity argument, the measured
evidence and the rejected alternative; `TemporalKind::pattern`'s narrowing list
gained "fractional seconds capped at nine digits".

**Measured across all four runtimes** (emitted regex, pinned per-target config),
every fractional width at the boundary:

| digits | 0 | 1 | 3 | 6 | 9 | 10 | 12 | 20 |
|---|---|---|---|---|---|---|---|---|
| `date-time` | T T T T | T T T T | T T T T | T T T T | T T T T | F F F F | F F F F | F F F F |
| `time` | T T T T | T T T T | T T T T | T T T T | T T T T | F F F F | F F F F | F F F F |

(go / js / py / java.) Unanimous at every width — the accept set is now
identical, where before 12 digits was accept / accept / accept / **reject**.

New test `materialized_fraction_is_capped_at_nanosecond_precision` sweeps
0–9 valid and 10/12/20 rejected for both kinds, plus the lone-`.` case.
`pinned_patterns_pass_the_pattern_gate` still passes, so `{1,9}` stays inside
the portability + ReDoS gate (it is a bounded repetition under a `?`, not a
loop).

## Corpus rows that change verdict

For the regeneration pass and the changelog — **three rows total across both
addenda**, all in `format_conformance`, all `expect_valid` **true → false**:

| corpus | row | value | was | now | why |
|---|---|---|---|---|---|
| `format_conformance` | `time-second-60-leap` | `23:59:60Z` | true | **false** | materialized grammar rejects `:60`; all four runtimes reject |
| `format_conformance` | `dt-leap-second` | `2021-02-28T23:59:60Z` | true | **false** | as above |
| `format_conformance` | `dt-frac-high-precision` | `2021-01-15T12:30:45.123456789012Z` | true | **false** | 12 fractional digits now exceed the pinned nine-digit cap |

Plus two rows that gained a field rather than changing an existing verdict:

| corpus | row | change |
|---|---|---|
| `format_materialize_clock` | `dt-leap` | added `"expect_valid": false` (was an implicit-valid row) |
| `format_materialize_clock` | `t-leap` | added `"expect_valid": false` (was an implicit-valid row) |

No row changed from `false` to `true`, and no row's `value`/`wire` text was
edited. Both corpus notes record the flip-back condition: the two `:60` pairs
return to `true` if the `string` opt-out (`09#8`) ships; the nine-digit cap is
permanent unless a target gains sub-nanosecond capacity.

## Snapshot shifts (updated)

The temporal fraction group is emitted into every target's support code, so this
change moves more golden files than the base64 one did. Adding to the earlier
list:

- `samples/go/{temporal,showcase}/*.go`
- `samples/python/{temporal,showcase,chat,kb}/_definitions.py`
- `samples/typescript/{temporal,temporal-date,temporal-temporal,showcase}/definitions.ts`
- `samples/java/src/main/java/json_schema/definitions/{temporal,showcase}/TemporalSupport.java`

(Plus the untracked `samples/typescript/__probe_*` scratch directories, which are
another agent's and will regenerate or be removed.) No test asserts the literal
regex text; the whole-file `*_matches_checked_in_output` comparisons will fail
until the consolidated regeneration pass.

## Harness state — fully reconciled, nothing left open on my side

The conformance agent's cleanup had already landed by the time I re-ran, and it
reconciles exactly:

- the clock loop now reads `expect_valid` (`tests/json_schema_corpus_runtime.rs:396-409`),
  matching the patch I specified;
- all four `corpus:leap-second-rows` entries **and** the
  `new:java-rejects-12-digit-fraction` entry are deleted;
- `OPEN_FORMAT_ROWS` is down to the two `09#8` Python-nanosecond round-trip rows.

`cargo test --test json_schema_corpus_runtime` — **2 passed, 0 failed**, so the
`pattern_conformance` (140 pairs) and `format_*` (293 rows) corpora both hold
across all four runtimes with no unexpected findings and no stale entries.

`cargo test --lib json_schema::` — **408 passed / 0 failed**. `rustfmt --check`
clean on all three of my files. All three corpora re-parse as valid JSON.

---

# Addendum 3 — the nanosecond cap is REVERTED

**Addendum 2 is wrong and is superseded by this section.** Read them together:
the change it describes was made, then backed out. Nothing from Addendum 2's
fractional-seconds analysis should be relied on; the leap-second work in
Addendum 1 is unaffected and stands.

## Reverted

- `src/json_schema/format.rs` — the `Time` and `DateTime` arms of
  `materialized_pattern` are back to `(\.[0-9]+)?`. The only `{1,9}` left in
  `src/` is in two doc comments that now warn against reintroducing it.
- `specs/json-schema/corpora/format_conformance/corpus.json` —
  `dt-frac-high-precision` is back to `expect_valid: true`, and the note's
  fractional clause now states the real contract (accept every width, truncate
  per target) and says not to cap the grammar to make a rejecting target agree.

## Where my analysis went wrong

The framing I handed up was a false dilemma — "cap at 9, or let Java diverge" —
and the coordinator's approval inherited it. The third option was to fix the
target, and it was the right one.

Two concrete errors on my side, both avoidable with the evidence already in the
repo:

1. **I never looked for an existing position.** `samples/python/tests/test_temporal.py:201-227`
   is named `test_sub_second_precision_is_accepted_at_every_width`, carries
   `(".1234567890", 123456, ".123456")` as an explicit parameterised case, and
   states the rationale outright: *"Past `datetime`'s own microsecond resolution
   the extra digits are dropped — the bounded loss P1 exception (b) allows,
   mirroring Go's truncation at nanoseconds — rather than the value being
   rejected."* A ten-digit fraction was already a decided, tested, documented
   accept. I proposed rejecting it without checking whether the question was
   already answered. Reproduced: with the cap in place that test fails
   (`1 failed, 7 passed`); with the revert it passes (`8 passed`, and `24 passed`
   for the whole file) — verified in a scratch copy at `/tmp/pyrevert`, touching
   no checked-in file.

2. **My counterfactual was weaker than I presented it.** I argued that teaching
   Java to truncate would convert an accept-set split into "a strictly worse"
   round-trip split. But that round-trip split **already exists and is already
   documented** — Python truncates to 6 while Go/Java/TS keep 9, which is `09#8`
   under exception (b). Truncating 10+ digits to each target's capacity is the
   *same* bounded loss already in force, not a new class of defect. And the
   exception-(b) test cuts the other way from how I applied it: it permits loss
   "only at the target type's genuine capacity limit", so per-target truncation
   satisfies it for all four (6 for Python, 9 for the rest), whereas a uniform
   nine-digit **reject** satisfies it for none of them at 10+ — nine is not
   Python's limit, and the other three can simply truncate.

   The `MAX_DURATION_NANOS` analogy I leaned on does not transfer, either.
   That cap exists because a duration beyond `i64` nanoseconds cannot be
   *represented at all* in Go — there is no truncated value to fall back to. A
   twelve-digit fraction is perfectly representable in every target once the
   sub-capacity digits are dropped. Overflow and precision are different
   failures and only one of them justifies a load reject.

The lesson generalises past this row: when a divergence is "three targets do X,
one does Y", narrowing the shared grammar makes all four do neither. Fixing the
outlier should be the default, and the shared grammar should move only when no
target can represent the value.

## Test kept, retargeted

`materialized_fraction_is_capped_at_nanosecond_precision` →
**`materialized_fraction_is_accepted_at_every_width`** (`format.rs:812-848`).
It now asserts the actual contract: widths 1, 2, 3, 6, 7, 9, 10, 12 and 20 are
all accepted for `time` and `date-time` (7+ exceeds Python's microseconds, 10+
exceeds nanoseconds — both accepted and truncated, never rejected), a bare value
is accepted, a lone `.` is not, and `2021-01-15T12:30:45.123456789012Z`
specifically is valid.

**Mutation-checked**: reapplying `{1,9}` makes it fail — together with
`materialized_temporal_matches_the_conformance_corpus` — in `format.rs` itself.
That is the guard that was missing: the cap's only tripwire was a Python sample
suite three layers away, which is why it reached an integration pass instead of
`cargo test`.

## `new:java-rejects-12-digit-fraction` — resolved by fixing Java, not by capping

Corrected record. The finding was a real P1 accept-set divergence
(`2021-01-15T12:30:45.123456789012Z`: Go, Python, TypeScript accept; Java's
`ISO_OFFSET_DATE_TIME` throws past nine digits). It is closed by
**`truncateFraction`** in `src/generator/json_schema/java.rs:6457-6482`, which
drops digits past the tenth so Java accepts and truncates like the other three —
the direction the repo had already documented. The shared grammar is unchanged
and must stay unchanged.

I confirmed both halves have landed and reconcile:
`cargo test --test json_schema_corpus_runtime` — **2 passed, 0 failed**, with no
`dt-frac-high-precision` finding and no stale `OpenRow`. `OPEN_FORMAT_ROWS` is
down to the two `09#8` Python-microsecond round-trip entries, which are the
correct remaining exception-(b) cases.

## Corpus rows that change verdict — corrected, final

Supersedes Addendum 2's table. **Two** rows change verdict, both leap-second,
both `expect_valid` **true → false**:

| corpus | row | value | was | now |
|---|---|---|---|---|
| `format_conformance` | `time-second-60-leap` | `23:59:60Z` | true | **false** |
| `format_conformance` | `dt-leap-second` | `2021-02-28T23:59:60Z` | true | **false** |

`dt-frac-high-precision` **does not change** — it is `expect_valid: true`, as it
always was.

Two clock rows gained a field without changing an existing verdict:
`format_materialize_clock` `dt-leap` and `t-leap` now carry
`"expect_valid": false`.

## Snapshot shifts — corrected, final

Addendum 2 listed the temporal support files as shifting because of the cap.
That is no longer a reason. **However, they are currently stale in the other
direction**: the integration pass regenerated them *with* `{1,9}`, and the
generator no longer produces it. These twelve checked-in files still contain
`[0-9]{1,9}` and need the next regeneration to put `[0-9]+` back:

- `samples/go/{showcase,temporal}/*.go`
- `samples/python/{showcase,temporal,chat,kb}/_definitions.py`
- `samples/typescript/{showcase,temporal,temporal-date,temporal-temporal}/definitions.ts`
- `samples/java/src/main/java/json_schema/definitions/{showcase,temporal}/TemporalSupport.java`

Until then `samples/python/tests/test_temporal.py::test_sub_second_precision_is_accepted_at_every_width[.1234567890-…]`
fails against the checked-in samples. I did **not** hand-patch them and did not
run `cargo build-json-examples`; the regeneration is yours.

The base64 snapshot shifts from the first report are unaffected and still apply.

## Final state

- `cargo test --lib json_schema::` — **414 passed / 0 failed**.
- `cargo test --test json_schema_corpus_runtime` — **2 passed / 0 failed**
  (140 pattern pairs + 293 format rows, four runtimes).
- `rustfmt --check` clean on all three of my files; all corpora valid JSON.
- No `{1,9}` anywhere in `src/` except the two warning comments.
