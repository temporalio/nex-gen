# `duration` format cross-language conformance

Empirical study answering **`format` open question 1** (widen the asserted
subset to `duration`) and **open question 3** (the `PnW`-vs-`PnDT…`
mutual-exclusion rule). Question: can the `duration` format
(JSON Schema 2020-12 §7.3.1 -> RFC 3339 Appendix A -> ISO 8601 duration) be
supported by a **single generator-owned pinned regex**, lowered through the
[[pattern]] RE2-safe gate, to **identical** verdicts across all seven targets
(Rust, Go, JS/TS, Python, Java, Ruby, .NET) — with **no native duration
parser** and **no new dependency**?

**Answer: yes.** One pinned, RE2-safe regex gives identical *and* correct
verdicts on a 68-value corpus across all seven engines.

## The grammar under test (RFC 3339 Appendix A, verbatim)

```
dur-second = 1*DIGIT "S"
dur-minute = 1*DIGIT "M" [dur-second]
dur-hour   = 1*DIGIT "H" [dur-minute]
dur-time   = "T" (dur-hour / dur-minute / dur-second)
dur-day    = 1*DIGIT "D"
dur-week   = 1*DIGIT "W"
dur-month  = 1*DIGIT "M" [dur-day]
dur-year   = 1*DIGIT "Y" [dur-month]
dur-date   = (dur-day / dur-month / dur-year) [dur-time]
duration   = "P" (dur-date / dur-time / dur-week)
```

Consequences pinned by this grammar (all in the corpus):
- **`P` prefix** mandatory; **>=1 component** required (`P`, `PT` invalid).
- **`T`** required before any time component (`P1H` invalid; `PT1H` valid).
- **`PnW`** (week form) is a **separate alternative**, mutually exclusive with
  the Y/M/D/H/M/S form (`P1Y1W`, `P1W1D`, `P1WT1H`, `PT1W` all invalid).
- **No fractions** — the ABNF is `1*DIGIT` only (`PT1.5S`, `PT1,5S`, `P1.5W`
  invalid). This is the tightest point vs. most native parsers.
- **No sign** (`-P1Y`, `P-1Y` invalid).
- **Strict nesting / ordering**: a higher unit may be followed only by the units
  *below* it, in order. `dur-year`'s optional tail is `dur-month`, whose tail is
  `dur-day` — so **`P1Y4D` (year then day, skipping month) is INVALID**, while
  `P1Y6M4D` is valid. Likewise `PT1H5S` (hour then second, skipping minute) is
  invalid. `P1YT1H` (date then time) is valid because `dur-date` carries an
  optional trailing `dur-time`.

## The pinned regex

Direct transcription of the ABNF as a single anchored alternation — pure
concatenation + alternation + `+` on a digit class, **no backtracking
construct**, so it compiles in the pure-Rust `regex` crate (the [[pattern]] gate)
and is RE2-safe by construction:

```
^P(?:(?:[0-9]+Y(?:[0-9]+M(?:[0-9]+D)?)?|[0-9]+M(?:[0-9]+D)?|[0-9]+D)(?:T(?:[0-9]+H(?:[0-9]+M(?:[0-9]+S)?)?|[0-9]+M(?:[0-9]+S)?|[0-9]+S))?|T(?:[0-9]+H(?:[0-9]+M(?:[0-9]+S)?)?|[0-9]+M(?:[0-9]+S)?|[0-9]+S)|[0-9]+W)$
```

The week-vs-rest mutual exclusion and the strict nesting are expressed **purely
by the alternation structure** — no lookaround, no counting, no auxiliary
predicate. The three nested date branches (`Y[M[D]]` / `M[D]` / `D`) and the
three time branches (`H[M[S]]` / `M[S]` / `S`) encode the "higher unit followed
only by lower units" rule directly. **A pure regex is a complete fit** — unlike
the RFC 3339 date/time formats, `duration` needs **no** calendar predicate
(there is no month/day-in-month/leap-year validity to check).

**`[0-9]` not `\d`.** Written with the explicit ASCII class so that even the
Rust `regex` crate — whose `\d` is Unicode-aware and matches `٣`/`３` — agrees
ASCII-only, without relying on a per-engine ASCII flag. (The probe caught this:
with `\d`, Rust-as-runtime accepted `P٣Y`/`P３Y` while the six runtimes rejected.)

## Per-target anchor normalization (reused from [[pattern]])

The regex's trailing `$` needs the same per-target end-anchor treatment
[[pattern]] already pins, because `$` accepts a trailing `\n` in some engines
(the `newline-tail` case, `"P1Y\n"`):

| target | end anchor emitted | why |
|---|---|---|
| Go, JS | keep `$` | already end-of-input only |
| Python | `\Z` | Python `$` matches before a trailing `\n` |
| Java | `\z` | Java `$` matches before a trailing `\n` |
| .NET | `\z` | .NET `$` lenient; `\z` is strict (also `RegexOptions.ECMAScript` for ASCII) |
| Ruby | `\A`/`\z` | `^`/`$` are always line anchors in Onigmo |

These are **exactly** the transforms the `pattern` spec already specifies — no
new gate rule is introduced. The runners apply them so each tests the *form the
generator would emit*.

## Files

- `corpus.json` — the pinned regex + 68 `{value, expect}` cases (strict-ABNF
  verdicts). Positive set covers every branch; negatives cover bare `P`/`PT`,
  missing `T`, fractions (dot and comma), signs, the week-mutual-exclusion set,
  ordering/nesting violations, whitespace/newline/embedding, and Unicode digits.
- `runner.go`, `runner.mjs`, `runner.py`, `Runner.java`, `runner.rb` — one per
  runtime engine; each compiles the pinned regex (with its per-target anchor
  normalization) and emits JSON Lines `{"id","engine","compiled","matched"}`.
- `rust_runner/` — the `regex`-crate runner = the load-time gate *and* the
  generator's own engine. `cargo build --release`.
- `dotnet_runner/DurationRunner/` — the C# runner (`net8.0`).
- `compare.py` — builds/runs all seven, checks (a) the regex compiles
  everywhere, (b) all seven agree per value, (c) the shared verdict equals the
  ABNF `expect`. Exits nonzero on any divergence or wrong verdict.
- `native_parsers_probe/` — `probe.py` + `NOTES.md`: evidence that native
  duration parsers are unusable as the source of truth.

## Run

```sh
cd json-schema/research/format_duration
python3 compare.py
python3 native_parsers_probe/probe.py
```

## Findings

**PASS — 68/68 values agree across all seven engines and match the strict ABNF.**
`compare.py`:

```
(a) compile-acceptance: OK   -- the pinned regex compiled in all seven engines.
(b) cross-engine agreement:  OK   -- all seven returned the same verdict per value.
(c) correctness:             OK   -- the shared verdict equals `expect` for every value.
VERDICT: PASS - one pinned regex, identical & correct across all 7 targets
```

The probe found (and the corpus now guards) three real issues while getting
there — each resolved without weakening the grammar:
1. `\d` is Unicode in the Rust crate -> switched the pinned regex to `[0-9]`.
2. `$` accepts a trailing `\n` in Python/Java/.NET/Ruby -> the [[pattern]]
   per-target end-anchor normalization (`\Z`/`\z`/`\A`) applies unchanged.
3. A drafting error in the corpus (`P1Y4D` marked valid) — the regex correctly
   rejects it per strict ABNF nesting; the corpus was corrected. This is
   *evidence the pinned regex enforces the nesting rule* the native parsers get
   wrong (`Period.parse` and `XmlConvert` both ACCEPT `P1Y4D`).

### Native parsers are unusable (see `native_parsers_probe/NOTES.md`)

Verified live: **Go** `time.ParseDuration` parses a different grammar (`1h30m`)
and rejects every `P...`; **Python**/**Ruby** have no stdlib ISO-8601 duration
parser; **Java** splits across `Duration.parse` (no Y/M, accepts fractions) and
`Period.parse` (accepts negatives, accepts `P1Y4D`, expands `P1W`->`P7D`,
rejects `P1YT1H`) — the two disagree with each other and the ABNF; **.NET**
`XmlConvert.ToTimeSpan` accepts fractions/signs/`P1Y4D` and **rejects the week
form `P1W`**. Delegating to any of these would break P1 — the owned pinned regex
is the only portable route.

## Residual risks

- **Semantic (not syntactic) checks are out of scope**, matching the RFC 3339
  temporal formats' stance: no bound on component magnitude (`P999999999Y` is
  syntactically valid), no cross-field arithmetic. The spec asserts *syntax*.
- **`$`/anchor normalization is a dependency on the [[pattern]] recipe.** It is
  already pinned and corpus-proven there; `duration` adds no new rule, but the
  regression guard is this corpus (run it if the anchor recipe ever changes).
- **The corpus is the proof, not an exhaustive grammar oracle.** New edge cases
  (e.g. very long digit runs, additional embedding attacks) should be added here
  rather than assumed covered.
- **ISO 8601 has richer forms** (`PYYYY-MM-DDThh:mm:ss`, fractions, comma
  decimal separator, negative/overflow) that RFC 3339 App.A deliberately
  excludes; the corpus's `iso8601-alt-form`, `fractional-*`, and sign cases
  confirm the pinned regex rejects them, matching the JSON Schema profile.
