# String-constraint probes — findings

Backs `features/maxLength`, `features/minLength`, `features/pattern`.
Everything here is re-runnable:

```
go run .                    # main.go
node len.mjs                # length + pattern behavior (JS)
python3 len.py              # length + pattern behavior (Python)
java Len.java               # length + pattern behavior (Java, single-file mode)
node codepoint_count_bench.mjs   # JS code-point-count strategies
```

## Length unit (minLength / maxLength)

Spec length = Unicode **code points** (RFC 8259), no normalization. The
naive per-language "length" primitive counts the wrong unit in three of
four targets. For `"a😀b"` (a + astral U+1F600 + b):

| Language | naive primitive | value | correct code-point primitive | value |
|---|---|---|---|---|
| Go | `len(s)` (bytes) | 6 | `utf8.RuneCountInString(s)` | 3 |
| JS | `s.length` (UTF-16 units) | 4 | surrogate scan / `[...s].length` | 3 |
| Java | `s.length()` (UTF-16 units) | 4 | `s.codePointCount(0, s.length())` | 3 |
| Python | `len(s)` (code points) | 3 | `len(s)` (already correct) | 3 |

No normalization: precomposed `é` (NFC, U+00E9) is length **1**;
decomposed `é` (NFD, `e`+U+0301) is length **2**. All four agree on each
form because none normalizes — the apparent mismatch when first authoring
the probe was a source-file encoding artifact, which is why the probes now
use explicit `\u` escapes.

## JS code-point counting — how to count without allocating

100k-char string, 2000 iterations (`codepoint_count_bench.mjs`):

| Approach | Time | Notes |
|---|---|---|
| `[...s].length` (spread) | ~800ms | allocates a full code-point array |
| `for (const _ of s)` | ~830ms | no array, but iterator overhead ≈ baseline — **not** a win |
| surrogate-aware scan | ~230ms | allocation-free single pass, ~3.5× faster |
| early-exit vs bound 8 | ~0.4ms | stops once the bound is crossed; work bounded by the bound, not `s.length` |

Consequences for the emitted TS validator:
- Emit a shared `codePointLength(s)` helper: UTF-16 `.length` minus one per
  well-formed high+low surrogate pair (allocation-free).
- For `minLength`/`maxLength`, **early-exit** against the bound rather than
  taking a full count — bounds work on adversarially long input. `maxLength`
  recounts on the (rare) failure path for the exact `got N`; `minLength`
  already holds the full count when it fails short, so no recount.
- Lone surrogates count as one unit under both the scan and `[...s]`
  (verified), so they agree.

## Pattern — the load-time gate is pure Rust (no Go dependency)

The generator is Rust, so the portability gate compiles the pattern with
the **`regex` crate**, not Go's `regexp`. `../rust_regex_gate/` (run
`cargo run`) confirms `regex` is the same regular / no-backtracking family
as Go RE2: it **rejects** lookahead, negative lookahead, lookbehind, and
backreferences, and **accepts** the portable subset — matching
`string_probe/main.go` construct-for-construct.

Key distinction the probe also shows: the gate decides **compilability
only**, never runs a production match. The `regex` crate's *own* defaults
differ from our pinned runtime semantics — notably its `\d` is
**Unicode-aware** (matches Arabic-Indic `٣`), where every target runtime is
pinned to **ASCII** `\d`. That is why the Python runtime validator uses
`re` + `re.ASCII` rather than pydantic-core's Rust `regex` engine, yet the
loader still trusts the same crate for the accept/reject *structure*. The
residual risk (a pattern Rust `regex` accepts but some runtime engine
rejects) folds into the conformance corpus.

## Pattern — regex portability

Three cross-engine divergences, each pinned to the portable choice:

1. **Dialect.** Go RE2 rejects lookahead `(?=…)` and backreferences `\1`
   (`invalid or unsupported Perl syntax` / `invalid escape sequence`); JS,
   Python, Java all accept them. RE2 is the strictest → it is the load-time
   compile gate.
2. **Anchoring.** Spec = not implicitly anchored → unanchored search: Go
   `MatchString`, JS `RegExp.test`, Python `re.search`, Java
   `Matcher.find`. Footguns: Java `Matcher.matches()` anchors the whole
   input (`"cat"` fails on `"the cat sat"`); Python `re.match` anchors the
   start.
3. **Classes & dot.** `\d\w\s` must be ASCII, `.` one code point:
   - Python `\d` is Unicode-by-default (matches Arabic-Indic `٣`) → compile
     with `re.ASCII`.
   - JS `.` is a UTF-16 unit without the `u` flag (fails on astral) → emit
     with the `u` flag.
   - Java `\d` becomes Unicode under `UNICODE_CHARACTER_CLASS` → use default
     flags (Java's `.` already matches a code point; `\d` is ASCII by
     default).
   - Go RE2 is already ASCII-class + rune-`.`.

Those three are necessary but **not sufficient** — the conformance corpus
below found three more divergences that survive (search + ASCII +
code-point-`.`).

## Pattern — conformance corpus (`../pattern_conformance/`)

An 83-pair `(pattern, instance)` corpus run through the Rust gate + all four
runtimes (`python3 compare.py`) proved the compile-gate + pinned-flags
recipe is **not** enough. Three constructs compile in every engine yet
diverge, so each got an added gate rule:

1. **Inline flags `(?i)` / `(?flags:…)` — REJECT.** JS `RegExp` can't
   compile them ("Invalid group"); Rust gate + Go + Python + Java accept.
   Pure compile-acceptance gap. No portable rewrite → reject.
2. **`\s` / `\S` — NORMALIZE** (see `../ws_normalize/`). JS `\s` is the full
   Unicode whitespace set and is **not** flag-controllable (matches NBSP
   U+00A0, U+3000); Go/RE2, Python `re.ASCII`, Java-default are ASCII (and
   the ASCII sets even differ on `\v`). Rather than reject, rewrite
   `\s`→`[\t\n\x0B\f\r ]` / `\S`→`[^…]` (explicit, spliced in for every
   target) — 13 divergences → 0. Include `\v` (U+000B, only Go/RE2 omits
   it). All placements incl. `[^\S]` (double negation) work; **only `\S` in
   a multi-member class `[\S.]` stays a reject** (open complement). `\d`/`\w`
   untouched.
3. **`$` + trailing `\n` — NORMALIZE.** Python/Java `$` match at end OR
   before a final `\n`; Go/JS `$` = end-of-input only. Rewrite unescaped `$`
   → `\Z` (Python) / `\z` (Java); keep `$` (Go/JS). Rejecting isn't viable —
   JS has no `\z`/`\Z`, so there'd be no portable end-anchor.

All rules need an AST check (everything compiles): the gate walks the
`regex-syntax` AST — see `../rust_regex_gate/` (`ast_detect`) and
`../ws_normalize/` (the `\s`/`\S` span-splice rewrite) — flagging inline
flags, splicing `\s`/`\S`, and locating `$` (escaped `\$` and explicit ws
classes correctly pass). With these rules the corpus fully agrees.

## Pattern — prospective targets (.NET, Ruby)

Verified against the same corpus (`../pattern_conformance/dotnet_runner/`,
`runner.rb`) — both future-conformant with per-target emitter transforms
only, no new gate rules:

- **.NET** (`System.Text.RegularExpressions`): `Regex.IsMatch(v,p,
  RegexOptions.ECMAScript)` (ECMAScript → ASCII `\d\w\s`), `$`→`\z` (its
  `\Z` is the lenient one — reverse of Java). **Astral `.` diverges** — .NET
  `.` is one UTF-16 unit and there's no `u`-flag equivalent, so the emitter
  must rewrite `.`→`(?:[\uD800-\uDBFF][\uDC00-\uDFFF]|.)` (70/72 → 72/72).
- **Ruby** (Onigmo): `re.match?(v)`; `\d\w\s` ASCII and `.` code-point by
  default (good). But `^`/`$` are **always line anchors** → normalize
  `^`→`\A`, `$`→`\z` (never `\Z`). And `\b` is **Unicode** despite ASCII
  `\w` → inject a leading `(?a)` ASCII-mode flag. With both: 0 divergences.

## Pydantic

`pydantic_length_probe.py` (run in a throwaway venv):

- **Length unit — RESOLVED (pydantic 2.13.4): counts code points.** A
  single astral emoji (1 code point, 4 UTF-8 bytes, 2 UTF-16 units) passes
  `max_length=1`; two emoji (2 code points) fail; `min_length` symmetric.
  So `Field(min/max_length=…)`/`StringConstraints` is spec-correct with no
  custom validator — matters because pydantic-core checks length in Rust
  (`str.len()` is bytes, `.chars().count()` is code points), so it had to
  be verified, not assumed.
- **`pattern` keeps the explicit `re` + `re.ASCII` + `.search()`
  `AfterValidator` — RESOLVED (`pydantic_pattern_probe.py`, 2.13.4).**
  Native `StringConstraints(pattern=…)` uses pydantic-core's Rust `regex`
  engine, whose `\d\w\s` are **Unicode** (4/32 corpus disagreements vs our
  ASCII — `^\d+$` accepts `٣`, `\w` accepts accented letters, `\s` accepts
  NBSP). Its anchoring (unanchored) and dot (code point) *do* match, and it
  rejects lookaround/backref at build time — but the class divergence is a
  hard blocker, so the explicit `re` validator stays.
