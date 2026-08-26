# `pattern`

Source: JSON Schema 2020-12, Validation vocabulary, §6.3.3
"Validation Keywords for Strings → pattern".

Asserts that a string instance **matches a regular expression**. A pure
runtime assertion — no type impact. The string keyword with the richest
cross-language hazard: the four targets ship **different regex engines**
whose dialect, anchoring, and character-class semantics diverge, so the
supported form is narrowed to the **portable (RE2-safe) subset** matched
with **unanchored search** and **ASCII** class semantics. That
configuration was validated against the **conformance corpus** of
`(pattern, instance)` pairs run through all four runtime engines plus the
Rust gate (`json-schema/corpora/pattern_conformance/`), which proved the compile
gate + pinned flags alone is *not* enough — three further constructs
(inline flags, `\s`/`\S`, and the `$` anchor) compile everywhere yet match
differently. Each gets an explicit rule: inline flags are **rejected**, and
`\s`/`\S` and `$` are **normalized** to a portable form in the emitted
pattern (**Conformance-verified gate rules**, below). With those, all four
agree value-for-value.

## Spec summary

Verbatim (2020-12 validation, §6.3.3):

> The value of this keyword MUST be a string. This string SHOULD be a
> valid regular expression, according to the ECMA-262 regular expression
> dialect.

> A string instance is considered valid if the regular expression matches
> the instance successfully. Recall: regular expressions are not
> implicitly anchored.

Distilled:
- Value MUST be a **string** (a regular expression, ECMA-262 dialect).
- Instance valid iff the regex **matches somewhere** in the string —
  **not implicitly anchored** (a substring match suffices unless the
  pattern itself uses `^`/`$`).
- Applies only to string instances; a `pattern` on a non-string [[type]]
  is rejected at load (**P7.1**).
- Pure assertion; no annotation behavior.

## Support decision

**Support:** partial — **portable (RE2-safe) subset only**, matched with
**unanchored, ASCII-class, code-point** semantics. A pattern that the
pure-Rust **`regex` crate** (the generator's own engine) cannot compile —
lookahead `(?=…)`/`(?!…)`, lookbehind, backreferences `\1`, and other
backtracking-only Perl constructs — is **rejected at load** (deferred,
*not* a categorical P6 exclusion). Additional syntax categories are rejected
or normalized where the runtime engines diverge (the complete gate is below):
inline flags are rejected; `.` / `\s` / `\S` and the `$` end-anchor are
normalized; non-portable escapes, assertion spellings, class operations,
named captures, and ambiguous unbounded repetitions are rejected. Applies only
to `string` fields. "RE2-safe" below names the
**regular (no-backtracking) subset** — the algorithm family that Rust's
`regex` and Go's `regexp` both implement — not a dependency on Go.

Rationale (citing [[PRINCIPLES.md]]):
- **P1 (identical cross-language validation).** The same
  `(pattern, instance)` must produce the same accept/reject in Go, TS,
  Python, and Java. The four engines diverge on the axes below; each is
  pinned to the portable choice, and the
  conformance corpus then caught three more (**Conformance-verified gate
  rules**, further below):
  1. **Dialect.** The *regular* engines — Rust's `regex` crate (what the
     generator itself uses) and Go's `regexp` (RE2) — have **no
     backtracking** and reject lookaround/backreferences; ECMA-262, Python
     `re`, and `java.util.regex` all **accept** them. The regular engines
     are therefore the **strictest** — a pattern they compile is compilable
     by the permissive three — so the **load-time gate compiles the pattern
     with the Rust `regex` crate** (below; pure Rust, no Go toolchain), and
     the rejected non-regular constructs are exactly the ones with no
     portable linear-time semantics anyway. Rust `regex` and Go RE2 are the
     same family and reject the identical construct set — verified
     directly against each other.
  2. **Anchoring.** The spec says regexes are *not* implicitly anchored,
     so we use each engine's **unanchored search**: Go `MatchString`, JS
     `RegExp.test`, Python `re.search`, Java `Matcher.find`. The footgun
     is **Java `Matcher.matches()`**, which anchors the *whole* input
     (`"cat"` fails to match `"the cat sat"` — verified) and Python
     `re.match`, which anchors the *start*; using either would silently
     diverge from the spec and the other three.
  3. **Character classes & the dot.** `\d`/`\w`/`\s` must be **ASCII**
     (ECMA-262: `\d` ≡ `[0-9]`), and `.` must match one **code point**.
     Verified divergences, each pinned: Python `re` makes `\d`
     **Unicode-aware by default** (matches Arabic-Indic `٣`) → compile
     with **`re.ASCII`**; JS `.` is a **UTF-16 code unit** without the
     `u` flag (fails on astral input) → emit with the **`u` flag**; Java
     `\d` becomes Unicode under `UNICODE_CHARACTER_CLASS` → use
     **default flags** (Java's `.` already matches a code point, and `\d`
     is ASCII by default). Go RE2 is already ASCII-class + rune-`.`.
- **P4 (minimal runtime deps).** Every target's regex engine is in its
  standard library / language runtime (Go `regexp`, JS `RegExp`, Python
  `re`, `java.util.regex`) — no third-party dependency, unlike a shared
  decimal library. The constraint is purely *which subset and flags*, not
  *whether* an engine exists.
- **P7 / P7.1 (reject ambiguity loudly).** A pattern that matches in three
  languages and fails to compile as a regular expression (or matches an
  astral character in one and not another) is exactly the
  silently-inconsistent output the mission forbids. Reject the non-portable form at load with a clear
  diagnostic naming the offending construct.

**Conformance-verified gate rules (beyond no-backtracking).** The
`(pattern, instance)` corpus run through all four runtime engines + the
Rust gate (`json-schema/corpora/pattern_conformance/`) showed the compile gate +
pinned flags is **not** sufficient, so the gate applies these explicit rules:
1. **Inline flag groups `(?i)` / `(?flags:…)` → reject.** JS `RegExp`
   cannot compile them (they are not ECMA-262 syntax); Rust/Go/Python/Java
   all do — a pure compile-acceptance gap (`(?i)^cat$` fails only in JS).
   No portable rewrite (case-folding a whole pattern is out of scope), so
   this one is a reject, not a normalization.
2. **`\s` / `\S` → normalize to a canonical ASCII class.** JS whitespace
   `\s` is the full **Unicode** set and is **not** flag-controllable
   (matches NBSP U+00A0, ideographic space U+3000, …), whereas Go/RE2,
   Python `re.ASCII`, and Java-default are ASCII — *and even the ASCII sets
   disagree on `\v`* (only Go/RE2 omits it). So rather than lean on any
   engine's `\s`, the emitter **rewrites `\s`→`[\t\n\x0B\f\r ]` and
   `\S`→`[^\t\n\x0B\f\r ]`** (the explicit set spliced in for *every*
   target), which every engine then matches identically. The set is
   deliberately **ASCII** (dropping ECMA-262's Unicode spaces, consistent
   with the ASCII `\d`/`\w` pinning) and **includes `\v` (U+000B)** —
   written `\x0B`, not `\v`, to avoid shorthand ambiguity — because
   ECMA-262 / Python / Java / JS all include it and only Go/RE2 omits it, so
   spelling it explicitly makes Go agree and honors author intent.
   The rewrite is defined for every placement, and the corpus pins each
   one: standalone `\s`/`\S`; `\s` inside a class
   (`[\s.]`→`[\t\n\x0B\f\r .]`, `[^\s]`→`[^\t\n\x0B\f\r ]`); sole-member
   `[\S]`→`[^…]` and the double-negation `[^\S]`→`[…]`. **`\d`/`\w` are
   untouched** (identical ASCII across all four already). *One narrow
   reject remains:* **`\S` inside a multi-member class** (`[\S.]`, `[\S\d]`)
   — "not-whitespace OR something" is an open-ended complement RE2/JS/Python
   cannot spell as a positive member list (no nested negation / class
   subtraction), so it is statically detected and rejected with the
   explicit-class fix-it.
3. **`$` end-anchor → normalize.** Python and Java `$` match at end-of-input
   *or before a single trailing `\n`*; Go and JS `$` match end-of-input
   only. Rejecting `$` is untenable — `^…$` is the most common pattern shape
   and there is **no** portable end-anchor to switch to (JS has no
   `\z`/`\Z`). So the emitter **rewrites the unescaped `$` assertion** to an
   end-of-input anchor without the newline exception: `\Z` for Python, `\z`
   for Java; Go and JS keep `$` (already exception-free). Provably
   semantics-aligning (multiline is never enabled), so it *eliminates* the
   divergence rather than hiding it; `^` needs no change. **Watch the `\Z`
   vs `\z` letter flip:** Python's strict end-of-string anchor is `\Z`,
   whereas Java's (and .NET's / Ruby's — see Prospective targets) is `\z`,
   with `\Z` being the *lenient* one there. The generator emits the right
   letter per target.
4. **`.` → `[^\n]`.** Runtime dot semantics disagree on which line terminators
   are excluded. The explicit class pins the project's one-code-point,
   not-newline rule and is the spelling used in diagnostics.
5. **Non-portable escapes → reject.** Only the shared escape vocabulary is
   admitted. In particular escaped punctuation such as `\-`, `\_`, `\"`,
   `\ `, `\#`, `\&`, and `\~`; `\a`/`\v`; octal/`\0`; `\uFFFF`;
   `\UFFFFFFFF`; `\x{…}`; and Unicode property escapes `\p{…}` are rejected.
6. **Non-portable assertions and captures → reject.** `\A`, `\z`,
   `\b{start}`/`\b{end}`, `\<`/`\>`, and both named-capture syntaxes are
   outside the shared grammar.
7. **Ambiguous punctuation/classes → reject.** Lone `{`, `}`, or `]`; POSIX
   classes; nested classes; and class set operations (`&&`, `--`, `~~`) are
   rejected rather than interpreted differently per engine.
8. **Ambiguous unbounded repetition → reject (D7).** Nested/unbounded
   quantifier shapes such as `^([a-z]+)+$` are rejected to keep evaluation
   linear and avoid target-specific backtracking failures.

All of these compile under `regex::Regex::new`, so the gate additionally
walks the pattern's `regex-syntax` **AST** — rejecting inline-flag groups,
locating each `\s`/`\S` Perl node (with its `negated` flag, enclosing-class
context, and byte span) to splice the rewrite, and locating the `$`
assertion for the anchor rewrite. The AST is escape-safe for free: an
escaped `\$` or a literal `s` from `\\s` produces no assertion / Perl node
and is left untouched. Still pure Rust, no Go toolchain.

This is the same "support the portable subset, reject the hazardous form
at load, deferred not excluded" posture as [[multipleOf]] (fractional
divisors) and the [[patternProperties]] carve-out — which already flagged
this exact RE2-vs-ECMA-262 dialect gap for the *key* space; `pattern`
manages it for the *value*.

Loader behavior:
- `pattern` not a string → reject.
- `pattern` on a non-string [[type]] → reject (**P7.1**).
- **`pattern` does not compile under the Rust `regex` crate → reject** with
  a "not portable / not yet supported" diagnostic that names the construct
  (lookahead, lookbehind, backreference, …) and notes the generator
  supports the regular (no-backtracking) subset. This is the portability
  gate: the loader (pure Rust) compiles the pattern once with the `regex`
  crate — **no Go toolchain dependency** — and success ⟹ the regular
  subset, which every target's runtime engine accepts. The gate then walks
  the `regex-syntax` **AST** for the three conformance rules below. It never
  runs a production match, so the `regex` crate's *own* default semantics
  (e.g. Unicode-aware `\d` — it matches U+0663, verified) are irrelevant
  here; runtime matching is pinned per target (ASCII classes, code-point
  `.`, unanchored) in the Validator mapping.
- **Inline flag group `(?i)` / `(?flags:…)` → reject** — not ECMA-262; JS
  cannot compile it (Conformance-verified gate rules).
- **`\s` / `\S` → normalized** to `[\t\n\x0B\f\r ]` / `[^\t\n\x0B\f\r ]` in
  the emitted pattern (all placements: standalone, in-class, and
  sole-member `[\S]`/`[^\S]`). **Exception:** `\S` inside a *multi-member*
  class (`[\S.]`) → reject with a fix-it (spell the set explicitly) — an
  open-ended complement with no portable positive form. `\d`/`\w` untouched.
- **`$` end-anchor → normalized, not rejected** — emitted as `\Z` (Python)
  / `\z` (Java), kept as `$` (Go/JS), so it means end-of-input in every
  target (no trailing-`\n` exception).
- **Empty pattern `""`** → matches every string (vacuous no-op) → accepted
  but constrains nothing (mirrors [[minLength]]`:0`).
- A `const`/`default`/`enum` string literal on the **same node** that does
  **not** match the pattern → reject at load (e.g. `{type:"string",
  pattern:"^[a-z]+$", const:"AB"}`). The string-regex half of the deferred
  literal-vs-constraint obligation ([[const]] / [[default]]).

**Deferred, not excluded.** The remaining rejects are the conservative v1
line, not a permanent boundary. A future release could **admit `(?i)`** via
a case-fold rewrite or a real flag channel, admit the **backtracking
constructs** where a portable rewrite exists / by shipping a shared engine,
or admit **`\S` in a multi-member class** if a portable positive form is
found — each gated on the conformance corpus agreeing. Tracked in the open
question below; mirrors [[multipleOf]]'s fractional-divisor deferral and
[[patternProperties]]' single-pattern carve-out.

## Type mapping

None. `pattern` never changes the emitted field type — [[type]]'s
`string`, unless a materializing sibling ([[format]] / [[contentEncoding]])
governs it; the regex lives only
in the validator, emitted as a **compiled constant** (below) rather than
recompiled per call.

## Validator mapping

Per **P10**/**P11**. A single "does the regex match?" predicate against a
compiled-once pattern, identical in both directions (shared `Validate`,
**P12**). Each language compiles the pattern **once** (module/package
init) and reuses it; the flags/method are the P1-pinned choices above, and
`<pattern>` denotes the **gate-normalized** form — `\s`/`\S` already
expanded to the explicit ASCII class and `$` rewritten per target
(`\Z`/`\z`), inline flags already rejected — so no runtime row has to cope
with them.

| Language | Strategy |
|---|---|
| Go | Package-level `var patRe = regexp.MustCompile(<pattern>)` (compiled once at init; the load-time gate already proved it compiles). The shared `Validate` checks `if !patRe.MatchString(v) { push(Violation{Path, Reason: fmt.Sprintf("must match pattern %q, got %q", <pattern>, v)}) }` — `MatchString` is unanchored; RE2 is ASCII-class + rune-`.`. Collected into one `PayloadValidationError` application failure. |
| TypeScript | Module-level ``const PAT_RE = /<pattern>/u;`` (or `new RegExp(<pattern>, "u")` when the literal can't be spelled). **The `u` flag is mandatory** (code-point `.`; verified). ``if (!PAT_RE.test(v)) push(Violation{path, reason: `must match pattern ${PAT_RE}, got ${JSON.stringify(v)}`})``. `test` is unanchored and — with no `g` flag — stateless. Throw one `PayloadValidationError` application failure. |
| Python | A module-level `_PATTERN_<HEX> = re.compile(<pattern>, re.ASCII)` (with the `$`→`\Z` normalization applied), keyed by the pattern text so identical patterns share one compiled instance per module. Both directions of the model's `_<Model>TransferTypeConverter` inline the check — `if _PATTERN_<HEX>.search(value) is None: violations.append(Violation(path=…, reason=f"must match pattern <pattern>, got {_quote(value)}"))` — collected into the single `PayloadValidationError` application failure (**PRINCIPLES Python §2/§3**). The comparison is emitted inline rather than behind a runtime helper, the same way TypeScript emits it. **`re.search` (unanchored — never `re.match`, which anchors the start, or `fullmatch`), `re.ASCII` (ASCII `\d\w\s`).** |
| Java | Static `private static final Pattern PAT_RE = Pattern.compile(<pattern>);` (**default flags** — ASCII `\d\w\s`, code-point `.`; with the `$`→`\z` normalization applied). The per-POJO collecting deserializer (PRINCIPLES Java §5) reads the `String` and checks `if (!PAT_RE.matcher(v).find())`, pushing a `Violation{path, "must match pattern " + <pattern> + ", got " + v}` into the single `PayloadValidationError` application failure. **`Matcher.find` (unanchored), never `matches()`** (which anchors the whole input — verified footgun). Not bean-validation `@Pattern`. |

**Informative `reason` strings.** The `Violation` `reason` names the
**pattern and the offending value** (`must match pattern "^[a-z]+$", got
"AB1"`), per the [[maximum]] convention. The pattern is an emitted
compile-time constant; the value is interpolated at runtime.

**Why compile-once.** Recompiling a regex per (de)serialize call is a
needless cost (P2 favors ergonomics/idiom, and a package-level compiled
pattern *is* the idiom in all four); the load-time gate is what lets the
emitted `MustCompile`/`Pattern.compile` be unconditional — its job is to
turn any runtime compile failure into a load-time reject. The one
gate-accepted-but-runtime-uncompilable case the corpus found (JS and inline
flags) is now a gate reject, so every emitted pattern compiles in its
target; the corpus (`json-schema/corpora/pattern_conformance/`) stays as the
regression guard against any future-discovered edge.

### Serialize-side (P12)

The match is a shared-`Validate` predicate, so it **re-runs before emit**
over the decoded value — a model constructed with a non-matching string
(a Go `string` / Java `String` / Python `str` set to an off-pattern value
in memory) fails serialize with the same aggregated primitive rather than
emitting an invalid value. Real teeth in every target, since constructing a
value in memory is unchecked in all four — a Go struct literal, a TS object
literal, a Java setter, and an inert Python dataclass alike bypass the parse
adapter, so the only place the match can be re-asserted is before emit. No
parse-adapter-only or encode-adapter-only logic: the match is pure and
direction-agnostic.

**On a materialized node** ([[format]] temporal / [[contentEncoding]]
bytes) the decoded value is not a `string`, so the regex matches the
**canonical wire string** instead: the incoming wire string on parse, and
the encode adapter's re-serialized wire string on serialize, **before
emit**. Still one predicate, identical in both directions; the wire string
is projected from the native value on the encode side.

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Anchored pattern (`$` normalized per target) | `{type:"string", pattern:"^[a-z]+$"}` |
| Unanchored substring | `{type:"string", pattern:"cat"}` (matches `"the cat sat"`) |
| ASCII digit/word class | `{type:"string", pattern:"^\\d{3}-\\w{4}$"}` |
| `\s`/`\S` (normalized to ASCII class) | `{type:"string", pattern:"^\\s+$"}`, `{…, pattern:"\\S"}`, `{…, pattern:"[^\\s]"}` |
| Explicit whitespace class (unchanged) | `{type:"string", pattern:"^[ \\t]+$"}` |
| Empty pattern (no-op) | `{type:"string", pattern:""}` |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Value not a string | `pattern:5`, `pattern:true`, `pattern:["a"]` |
| Type mismatch (P7.1) | `{type:"integer", pattern:"^\\d+$"}` |
| Non-portable: lookahead | `{type:"string", pattern:"(?=.*[A-Z]).+"}` |
| Non-portable: lookbehind | `{type:"string", pattern:"(?<=x)y"}` |
| Non-portable: backreference | `{type:"string", pattern:"(a)\\1"}` |
| Inline flag group (JS can't compile) | `{type:"string", pattern:"(?i)^cat$"}` |
| `\S` in a multi-member class (open complement) | `{type:"string", pattern:"[\\S.]"}`, `{…, pattern:"[\\S\\d]"}` |
| Non-portable escape | escaped punctuation, octal, Unicode-property, or engine-specific escape syntax |
| Non-portable assertion/capture | `\Acat`, `(?<word>cat)` |
| Class operation / POSIX class | `[a-z&&[^x]]`, `[[:alpha:]]` |
| Ambiguous unbounded repetition (D7) | `^([a-z]+)+$` |
| Literal fails pattern | `{type:"string", pattern:"^[a-z]+$", const:"AB"}`, `{…, default:"9"}` |

### Runtime fixtures (validator)

The per-`(pattern, instance)` match behavior — unanchored search, ASCII
class semantics, code-point `.`, and the `\s`/`\S` and `$` normalizations —
is specified by the **conformance corpus**
(`json-schema/corpora/pattern_conformance/corpus.json`). Each row states the
pattern, the instance, and the expected disposition, including which patterns
the gate must reject; a row's expectation is data, never a special case
carried in a test's source. That corpus is this keyword's regression suite —
new edge cases are added there, not enumerated here.

`tests/json_schema_corpus_runtime.rs` is the executable consumer: it generates
one member per accepted pattern and runs all 102 runtime rows through Go,
TypeScript, Python, and Java. The 38 `expect_gate_reject` rows exercise the gate
helper directly; they are not end-to-end loader coverage because no runtime
pattern is emitted for a rejected schema.
`tests/json_schema_conformance_manifest.rs` adds the integration/serialize
case and records `pattern.parse` in its fixed coverage ledger.

Fixtures outside the corpus (validator integration, not pure matching):
- Combined with a failing [[minLength]]/[[maxLength]] or sibling field →
  **all** reported in one shot (**P11**); serialize of an off-pattern
  in-memory value → rejected before emit (**P12**).

## Interactions

- **[[minLength]] / [[maxLength]]**: independent string assertions; all
  present keywords apply and aggregate. We do **not** attempt
  regex↔length satisfiability (a pattern like `^.{5,}$` implies a minimum
  length, but general regex-length reasoning is undecidable and out of
  scope) — each is checked on its own.
- **[[type]]**: gates applicability — `pattern` is meaningful only for
  `string`; a mismatch is a load reject (**P7.1**). `pattern` does not
  force the emitted type to `string`: a materializing sibling ([[format]] /
  [[contentEncoding]]) may replace it with a native construct while the
  regex still checks the wire string.
- **[[const]] / [[default]] / [[enum]]**: a supplied string literal MUST
  match `pattern` at load (rule above) — the regex half of the deferred
  literal-vs-constraint obligation.
- **[[patternProperties]]**: the *key-space* regex keyword (temporarily
  unsupported). It faces the **same** RE2-vs-ECMA-262 dialect gap this
  keyword manages for values — see [[patternProperties]], which points
  here for the confined, managed case.
- **[[format]]**: the named-shape string keyword. Its regex-lowered
  formats (`uuid`, `ipv4`, `ipv6`, and the syntactic pass of the temporal
  formats) reuse this keyword's RE2-safe gate and compile-once mechanism;
  temporal formats add a shared calendar predicate on top. Both may appear
  on one node — the value must satisfy both.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native (ECMA-262 dialect). Portable subset accepted; non-portable constructs rejected (deferred). |
| OpenAPI 3.1 | Adopts 2020-12 — same. |
| OpenAPI 3.0 / draft-4 | `pattern` present since draft-4, same ECMA-262 intent. Patterns using backtracking constructs (lookaround/backreferences) or inline flags need a rewrite to the regular subset or await wider support; `\s`/`\S` and `$` are handled automatically by normalization. |
| Swagger 2.0 | Same as OAS 3.0. |

The known cross-engine divergences are now all handled — the compile gate
+ `regex-syntax` AST checks **reject** the non-portable constructs
(lookaround/backref, inline flags, and the narrow `\S`-in-multi-member-class
case) and **normalize** `\s`/`\S` (→ explicit ASCII class) and `$` (→ per-
target end-anchor), which is what makes the four runtimes agree
value-for-value. The **residual risk** is an edge the corpus does not yet cover
(for example a `\b` word-boundary or `.`-newline corner). Every accepted row the
corpus does contain is executed in all four current runtimes; every rejected
row is rechecked by the Rust loader gate. New acceptance edges belong in that
corpus before the gate is widened.

### Prospective targets (.NET, Ruby)

Not current targets, but both are planned. Conformance was verified against
the same corpus (`json-schema/corpora/pattern_conformance/`), and both are feasible
with **per-target emission transforms only — no new gate rules**. The
findings (record them here so the recipe survives to implementation time):

| Target | Engine | Runtime config + transforms to match the pinned semantics |
|---|---|---|
| **.NET** | `System.Text.RegularExpressions` | `Regex.IsMatch(v, p, RegexOptions.ECMAScript)` — `IsMatch` is unanchored; **`ECMAScript`** makes `\d\w\s` ASCII (Unicode by default otherwise). `$`→**`\z`** (its `\Z` is the *lenient* one, reverse of Java). **Astral `.` is a divergence:** .NET `.` matches one UTF-16 unit and there is **no `u`-flag equivalent**, so `^a.b$` misses `"a😀b"` — the emitter must rewrite each `.`→`(?:[\uD800-\uDBFF][\uDC00-\uDFFF]|.)` (verified 70/72 → 72/72 with the rewrite). This is the same problem JS has, solved by an explicit rewrite instead of a flag. |
| **Ruby** | Onigmo (`Regexp`) | `re.match?(v)` — unanchored. `\d`/`\w`/`\s` are ASCII by default (good), and `.` is a code point (good). But `^`/`$` are **always line anchors** (no non-multiline mode), so normalize **`^`→`\A`, `$`→`\z`** (never `\Z` — that is the lenient one). And `\b` is **Unicode** even though `\w` is ASCII (an Onigmo quirk), so **inject a leading `(?a)`** ASCII-mode flag into the emitted pattern to force ASCII `\b` (verified 0 divergences with both transforms). `\s`/`\S` normalization and the `$`/`\S`-class rules carry over unchanged. |

Neither needs a new *gate* rule — the existing rejects (backtracking, inline
flags, `\S`-in-multi-member-class) already cover them, and both accept the
`\s`/`\S`- and `$`-normalized output. The `.NET` astral-`.` rewrite and the
Ruby `(?a)`-inject + `^`/`$`→`\A`/`\z` are additions to those targets'
*emitters*, mirroring how each current target already applies its own
per-engine flag/anchor treatment.

## Open questions

1. **Widen the accepted subset.** The v1 gate still rejects backtracking
   constructs (lookaround/backreferences), inline flag groups, and `\S`
   inside a multi-member class. Each is a candidate for later admission via
   a semantics-preserving rewrite — `(?i)` → case-fold expansion or a real
   flag channel, backtracking → a portable rewrite where one exists or a
   shared engine, multi-member `[\S…]` → a positive form if one is found —
   each gated on the conformance corpus (`json-schema/corpora/pattern_conformance/`)
   still agreeing across all targets (incl. the prospective .NET/Ruby).
   Revisit on demand (mirrors [[multipleOf]]'s fractional-divisor and
   [[patternProperties]]' single-pattern carve-outs). `\s`/`\S` and `$` were
   on this list and are now **resolved** by normalization.

## See also

- [[minLength]] / [[maxLength]] — the other string assertions (length);
  independent of `pattern`.
- [[patternProperties]] — the key-space regex keyword; same dialect gap,
  points here for the managed value-level case.
- [[type]] — supplies the emitted `string`; gates applicability; owns the
  cross-language conformance-suite open question.
- [[const]], [[default]], [[enum]] — supplied string literals validated
  against `pattern` at load.
- [[multipleOf]] — the sibling "support the portable subset, reject the
  hazardous form, deferred not excluded" decision posture.
- [[maximum]] — the `reason`-string convention.
