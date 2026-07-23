# Materializing the `duration` format as a language construct — findings

Backs `features/format` (Type mapping). Question the spec's Type-mapping left
open for `duration` specifically: instead of the current `string` field, can we
**materialize** the ISO 8601 duration as an idiomatic in-memory value (Go
`time.Duration`, Java `Duration`, Python `timedelta`, a TS type, …), with
**consistent** cross-language re-serialization (P1)? Truncation is acceptable
**only if it is identical across every materializing language**; materializing in
only *some* languages is acceptable.

This complements `research/format_typed_repr/` (which covered `uuid` / the
temporal *clock* formats but NOT `duration`) and reuses
`research/format_duration/native_parsers_probe/` (which proved native ISO-8601
duration *parsers* diverge). Validation is **unchanged** throughout: the pinned
regex still runs; materialization only changes the emitted field type + the
parse/serialize paths.

Everything is re-runnable:

```
cd go_full   && go run .        # Q1/Q2/Q3 walkthrough, Go
java Full.java                  # (in java_full) Java Duration vs Period
python3 full.py                 # (in py_full) Python stdlib gaps + timedelta
node full.mjs                   # (in ts_full) JS has no duration type
ruby full.rb                    # (in rb_full) Ruby has no stdlib duration
dotnet run                      # (in cs_full/DurRunner) .NET TimeSpan / XmlConvert
python3 compare.py              # cross-language byte-equality harness (the proof)
```

Toolchains as-run: go 1.26, node v25, python3 3.13, java 21, ruby 2.6,
dotnet 8, rustc 1.88.

> **Bottom line up front.** No stdlib fixed-duration type in ANY target can hold
> the full accepted grammar, because ISO 8601 durations carry **calendar-variable
> Years/Months** (and a distinct **week form** `PnW`) that a fixed nanosecond/tick
> count cannot represent without a reference date. So a *native* materialization
> is only possible if we **narrow the accepted grammar to pure time durations**
> `PTnHnMnS` (design C). If we keep the full grammar, the only faithful
> materialization is a **generated component struct** (design B) — which
> round-trips every accepted value byte-identically across all six languages
> (proven below), but is a generated type, not a stdlib "language construct."
> **Recommendation: keep `string` (A) as the shipped default; if materialization
> is demanded, ship design B (the component struct) — it is the only option that
> preserves the full grammar and P1. Do NOT narrow the grammar (C) just to reach
> a stdlib type.**

---

## The core problem, verified

ISO 8601 / RFC 3339 App. A durations we accept include `P1Y`, `P1M`, `P1Y6M4D`,
`P1YT1H`, and the mutually-exclusive week form `P4W`. A **year and a month are
calendar-variable** — a month is 28–31 days, a year 365 or 366 — so they have no
fixed second/nanosecond value without a reference date. Every stdlib
fixed-duration type is a scalar count of a fixed unit:

- Go `time.Duration` = `int64` nanoseconds.
- Python `timedelta` = days + seconds + microseconds (all fixed).
- Java `java.time.Duration` = seconds + nanos.
- .NET `System.TimeSpan` = `int64` ticks.
- Rust `std::time::Duration` = secs + nanos (Rust is the load gate, not a target).

None has a Y/M/W field or a reference date, so **none can store `P1Y` / `P1M` /
`P4W`**. Java `Period` holds Y/M/D but **not** H/M/S, so it can't store `PT1H`
or the combined `P1Y1DT1H` either. **There is no single stdlib type in any
target that holds the full grammar** — confirmed by the probes below.

---

## Matrix A — can a stdlib type hold the FULL accepted grammar? (Y/M/W + H/M/S)

| Lang | Candidate stdlib type | Holds full grammar? | Where it fails (probe evidence) |
|---|---|---|---|
| Go | `time.Duration` | **No** | int64 ns; no Y/M/W field, no reference date. Can't store `P1Y`/`P1M`; `P4W` only as 4·7·24h, losing the week form on re-emit. No ISO parser at all (`time.ParseDuration` is the `1h30m` grammar). |
| Java | `Duration` **or** `Period` | **No** | `Duration.parse` **rejects** `P1Y`/`P1M`/`P4W`/`P1Y1DT1H`, and **normalizes `P1D`→`PT24H`**. `Period.parse` **rejects** any time part (`P1YT1H`), **expands `P4W`→`P28D`** and `P1W`→`P7D`, and **accepts negatives** (`-P1Y`). Neither holds `P1Y1DT1H`. |
| Python | `timedelta` | **No** | days/seconds/µs only; no Y/M/W; **no ISO-8601 duration parser and no ISO serializer** in stdlib. |
| TypeScript/JS | *(none)* | **No** | **No stdlib duration type of any kind** — no `Duration`, no `timedelta`, no ISO parse/format. `Temporal.Duration` is the TC39 proposal, NOT in the Node/browser stdlib (would be a dependency / non-portable). |
| Ruby | *(none)* | **No** | **No stdlib ISO-8601 duration type or parser.** `Date._iso8601('P1Y')` → `{}`. `ActiveSupport::Duration` is Rails (dependency → P4). |
| .NET | `TimeSpan` (via `XmlConvert`) | **No** | `XmlConvert.ToTimeSpan` **collapses `P1Y`→365d, `P1M`→30d** (lossy value transform, not faithful) and **rejects the week form `P1W`/`P4W`**. `TimeSpan` has no Y/M/W concept. |
| Rust (std) | `std::time::Duration` | **No** | secs+nanos; no Y/M/W, no ISO parser. (Gate engine, not a target — listed for completeness.) |

**Conclusion: full-grammar materialization into a stdlib type is impossible in
every target.** This is strictly worse than the clock formats (`format_typed_repr`
Matrix A), where at least *some* languages had a faithful stdlib type.

---

## Matrix B — the narrowed time-only subset `PTnHnMnS` → native fixed-duration type

If we **reject Y/M/W at load** (narrow the grammar), the field becomes a pure
time duration and a native fixed type *can* hold it. Does it round-trip the
canonical `PTnHnMnS` byte-identically?

| Lang | Native type | Time-only round-trips canonical? | Notes |
|---|---|---|---|
| Go | `time.Duration` | **Yes** | We parse (no ISO parser in stdlib) and emit our canonical from the total. |
| Java | `java.time.Duration` | **Yes** | `Duration.parse` accepts `PTnHnMnS`; **`Duration.toString()` is byte-equal to our canonical** for all cases incl. `PT0S`, and keeps hours (never rolls to days: `PT48H`→`PT48H`). |
| Python | `timedelta` | **Yes** | No stdlib ISO parser; we parse + emit canonical from `total_seconds()`. `PT24H` stays `PT24H` even though `timedelta` prints "1 day". |
| .NET | `TimeSpan` | **Yes, with our emitter** | `XmlConvert.ToTimeSpan` parses `PTnHnMnS` fine, but **`XmlConvert.ToString(TimeSpan)` diverges: `PT24H`→`P1D`** (rolls 24h into a day). Must use the generator-owned canonical, NOT the BCL serializer. |
| TS/JS | *(none)* | n/a — no native type | Would still need design B (custom object) even for time-only. |
| Ruby | *(none)* | n/a — no native type | Same — no stdlib type even for time-only. |

**The single most important design-C finding:** even in the narrowed case, the
canonical re-serializer must be **generator-owned**, not the native `toString`.
Java's `Duration.toString()` happens to match, but **.NET's `XmlConvert.ToString`
rolls `PT24H`→`P1D`** and would break P1. And a native fixed-duration type
**cannot preserve a non-canonical input** — `PT90M` becomes `PT1H30M`, `PT3600S`
becomes `PT1H` (probe `go_full`/etc. "non-canonical" rows). That truncation is
*consistent* across the materializing languages (all collapse the same way), so
it satisfies the user's "consistent truncation" bar — but it is a real wire-shape
change from the input, and JS/Ruby have no native type at all, so design C is only
*partially* materializing (Go/Java/Python/.NET native; TS/Ruby fall back).

---

## Design options evaluated

### (A) Keep `string` — status quo, the safe default
Zero risk. Validation already owns the check; the field stays authoritative;
byte round-trip is trivially exact. The only cost is ergonomics (P2): the user
gets a validated `string`, not a duration object. This is what `format`'s
Type-mapping already prescribes.

### (B) Generated component struct — faithful, idiomatic-ish, no dependency
A generated type per language holding the seven integer components + a week flag:

```
struct ISODuration { years, months, weeks, days, hours, minutes, seconds: int; isWeek: bool }
```

- **Faithful:** stores Y/M/W verbatim; no calendar math, no reference date.
- **No dependency (P4):** plain integers + a hand-written serializer.
- **Round-trips exactly:** parse the validated string into components; canonical
  serialize reproduces the ISO string.
- **Canonical serialization** (proven byte-equal across all six languages):
  - Week form: `P{weeks}W`.
  - Otherwise ISO order `PnYnMnDTnHnMnS`, **omitting zero-valued components**;
    the `T` appears only if there is a time component.
  - **Whole-zero → `PT0S`** (so `P0Y` canonicalizes to `PT0S`). This is the only
    input that does not survive byte-identically; it is a semantic no-op
    (zero duration) and is canonicalized identically everywhere.
- **Caveat:** it is a *generated* type, not a stdlib "language construct." It is
  idiomatic-*ish* (a struct with named fields reads clearly) but does not give
  the user a `time.Duration` they can pass to `time.Sleep`. For calendar-variable
  Y/M values that is *correct* — there is no faithful arithmetic type — but for a
  pure `PT1H` the user might have preferred a real `time.Duration`.

### (C) Native type, but only by NARROWING the grammar to `PTnHnMnS`
Reject Y/M/W at load; the field becomes Go `time.Duration` / Java `Duration` /
Python `timedelta` / .NET `TimeSpan`. TS/Ruby have no native type so they fall
back to string-or-struct — meaning the **model shape varies per language**, one
half of P1's "identical model shape" intent.

- Requires **narrowing the accepted grammar** — a breaking change to what the
  `duration` format accepts (`P1Y`, `P4W`, etc. would now be load/validation
  rejects). The current spec explicitly accepts them and the corpus enshrines
  them.
- Native re-serialize **truncates non-canonical inputs consistently** (`PT90M`→
  `PT1H30M`) — acceptable per the user's bar, but still a wire change.
- Even so, the canonical emitter must be generator-owned (the .NET `XmlConvert`
  `PT24H`→`P1D` divergence), so we write the same serializer as design B's
  time-only arm anyway — the native type buys only the *field type*, not the
  serialization.
- **Verdict: not worth the grammar narrowing.** It sacrifices a chunk of the
  accepted grammar and P1's uniform model shape to reach a stdlib type in 4 of 6
  languages, while still needing an owned serializer. If native ergonomics for
  pure-time durations are wanted, expose them as an **opt-in derived accessor**
  on a design-B struct (`asTimeDuration() (time.Duration, ok bool)`), gated to the
  no-Y/M/W case, never the serialized field.

### (D) Normalize everything to a canonical total (e.g. total nanoseconds) — REJECT
Collapsing to a single scalar total **destroys Y/M fidelity**: `P1Y` and `P1M`
have no fixed nanosecond value (calendar-variable), so any fixed total is *wrong*
for at least some reference dates, and `P1Y` vs `P365D` vs `P12M` would all map to
indistinguishable (or arbitrarily-chosen) totals — a silent data corruption that
breaks the byte round-trip and P1. This is the same class of error the
`native_parsers_probe` flagged for .NET `XmlConvert` (`P1Y`→365d). **Rejected.**

---

## Recommendation

**Ship (A) keep `string` as the default.** It is already the spec's position and
carries zero P1 risk.

**If materialization is demanded, choose (B) the generated component struct** —
not (C). Rationale:

1. **(B) preserves the full accepted grammar**; (C) forces a breaking grammar
   narrowing (drop Y/M/W) to reach a stdlib type.
2. **(B) materializes in *all six* languages uniformly** (proven byte-equal);
   (C) reaches a native type in only 4 of 6 (JS/Ruby have none), so the model
   shape would vary per language — against P1's "identical model shape" intent.
3. **(B) round-trips every value byte-identically** (only the semantic no-op
   `P0Y`→`PT0S` canonicalizes); (C)'s native types truncate non-canonical inputs
   (consistently, but still a wire change).
4. Both need a **generator-owned serializer** anyway (the native `toString`s
   diverge — .NET `PT24H`→`P1D`), so (C)'s only real gain over (B) is handing the
   user a stdlib type — achievable as an **opt-in derived accessor** on (B)
   without narrowing the grammar or splitting the model shape.

**Truncation consistency (the user's bar):** with (B) there is essentially no
truncation — the only canonicalization is `P0Y`→`PT0S` (zero duration), applied
identically everywhere. With (C), truncation of non-canonical time inputs is
consistent across the four native languages but the two non-native languages
can't participate. (B) wins on the user's own criterion.

**No grammar narrowing is required for (B).** Narrowing is required *only* for
(C), and is the main reason to prefer (B).

---

## Consistency with the parallel clock-materialization work

`research/format_materialize_clock/` (date / time / date-time) is deciding the
same materialize-or-string question for the clock formats, using the identical
harness shape (parse → typed construct → CANONICAL serialize → cross-language
byte-equal compare; leap-second is its known conflict row). Whatever that work
lands on:

- If clock formats materialize into **native** types, `duration` still **cannot**
  follow for the full grammar (no native type exists) — so duration would be the
  odd one out and should either stay `string` or use a **custom struct**, which
  the clock work is unlikely to use for date/time. A mixed story ("clock formats
  native, duration struct-or-string") is coherent and should be stated explicitly.
- If clock formats stay **`string`** (likely, given `format_typed_repr`'s
  verdict), then `duration` staying `string` is the consistent, uniform outcome —
  the recommended default (A).
- A design-B **component struct for duration** would be the *most consistent* with
  a hypothetical component-struct approach for offset/clock values, and its
  generator-owned canonical serializer mirrors the clock harness's
  generator-owned canonical serializer. If the clock work introduces any
  generated temporal wrapper type, duration's design-B struct should share its
  naming/emit conventions.

---

## Residual risks

- **Design B is a generated type, not a stdlib construct.** Delivers named
  components + faithful round-trip, but not a `time.Duration` the user can do
  arithmetic with. For calendar-variable Y/M that is unavoidable; for pure-time
  values it is a minor ergonomic gap (closable via an opt-in accessor).
- **`P0Y`→`PT0S` canonicalization** is the one non-identity round-trip in (B).
  It is a zero-duration no-op, canonicalized identically across all languages,
  but a consumer expecting the exact input bytes for `P0Y` would see `PT0S`.
  (Also `P0Y0M0D` etc. would collapse — any all-zero duration → `PT0S`.) If exact
  byte preservation of zero forms is required, store the raw string alongside the
  struct, or keep (A).
- **(C) native serializers diverge** (.NET `XmlConvert.ToString` `PT24H`→`P1D`;
  and native fixed types collapse `PT90M`→`PT1H30M`). Mitigated by always using
  the generator-owned canonical, but it means the native type is a parse target
  only, never the serialize source of truth.
- **Leading-zero / redundant forms:** the pinned regex accepts `P01Y`? — No: the
  grammar uses `[0-9]+`, so `P01Y` matches and would canonicalize to `P1Y` in
  design B (integer parse drops the leading zero). This is a consistent
  canonicalization across languages but another non-identity round-trip for such
  inputs. (Not in the corpus; noted as a design-B canonicalization to decide on.)
- **Very large components:** `P100Y200M300DT400H500M600S` round-trips (fits in
  int64 per component); an adversarial multi-hundred-digit component would
  overflow int64 — the pinned regex has no digit-count cap, so design B needs a
  parse-side overflow guard (push a violation) to stay safe. Design A (string)
  has no such concern.
