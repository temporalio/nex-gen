# ts-emitter — execution report

Owned files touched: `src/generator/json_schema/typescript.rs`,
`src/generator/typescript.rs`, `tests/generate_typescript.rs`.

Verification harness: every fix was generated from a probe schema, compiled with
`tsc --ignoreConfig --noEmit --strict` over the emitted files, and **executed**
under nvm's Node 26 via vitest. The two new tests below carry that same
generate → `tsc` → run loop, because `npm run typecheck` is scoped by the sample
`tsconfig.json`'s `include` list and never sees a generated temp directory — the
hole `03#1` shipped through.

## Fixed

- **`03#1`** (P0) `render_closed_object_unknown_key_check` emitted `if () {` for a
  closed object with zero declared properties. It now emits the unconditional
  `violations.push({ path: key, reason: 'unknown field' })` loop.
  `src/generator/json_schema/typescript.rs:3921`. Verified: probe schema
  `{type: object, additionalProperties: false}` now typechecks and
  `fromTransferType({a:1})` reports `a: unknown field`.

- **`05#4`** (P0) The `uniqueItems` / `contains` blocks hard-coded `(element, index)`,
  shadowing the enclosing loop's `index` that `path_expr` interpolates.
  `render_ts_array_checks` now takes the array's own `depth` and names every loop
  variable `element{depth}` / `index{depth}` / `matchCount{depth}`, the convention
  the sibling emitters already used (extracted as `ts_depth_suffix`).
  `typescript.rs:857-1010`, call sites `:1408` (serialize) and `:3862` (parse).
  Verified: `{matrix: [[7,7],[1,2]]}` now reports `matrix[0]` (was `matrix[1]`) on
  both parse and serialize.

- **`06#5`** (P0) `ts_matcher_condition` emitted a runtime kind guard only for the
  matcher's declared `type`. It now falls back to the **element** type (as Java and
  Python do). `typescript.rs:836`. Verified: wire `{"names":[5]}` against
  `contains: {minLength: 2}` over `items: {type: string}` now yields an aggregated
  `ValidationError` (`names[0]: expected string`) instead of
  `TypeError: e is not iterable`; and `contains: {minimum: 5}` over integer items no
  longer counts `"9"` as a match.

- **`2.6` / `05#3`** (P0, decision **D10**) `uniqueItems` compared JS object
  identity on the serialize side. Both directions now compare the **canonical wire
  value**: on serialize the elements are first mapped through `serialize_expr`
  (`const wireItems<N> = value.blobs.map((element) => bytesToBase64(element));`),
  which is exactly what the parse side already sees. `typescript.rs:940-971`.
  Verified: two distinct `Uint8Array([1,2])` / two equal `Temporal.ZonedDateTime`
  now report `duplicate items: element at index 1 equals index 0` on serialize,
  matching the deserialize path and Python. Temporal comparison is offset-sensitive
  (the canonical wire string), matching Java and every language's parse side.

- **`10#10`** (P2) `<FIELD>_CONST` was declared for a materialized
  `contentEncoding`/temporal `const` whose comparison site inlines the literal.
  The predicate is now factored as `is_materialized_property` and gates both the
  declaration and the parser. `typescript.rs:2225`, `:2358`, `:3346`. Verified: the
  probe's `TAG_CONST` is gone, `PLAIN_CONST` remains.

- **`13#3`** (P0) No serialize-side integer cap existed. `render_ts_numeric_checks`
  now emits `if (!Number.isSafeInteger(v)) { … 'exceeds ±(2^53-1) integer cap' }`
  before the declared bounds, with Go's exact reason text; `field_needs_serialize_check`
  was widened so `integer` always qualifies. `typescript.rs:317-372`, `:1055`.
  A `emit_integer_cap: bool` parameter keeps the parse adapter from re-emitting the
  comparison its `Number.isSafeInteger` token classification already made.
  `Number.isSafeInteger` (rather than a bare magnitude compare) also stops a JS
  `number` field from emitting `1.5`/`NaN`/`Infinity` where an `integer` is declared —
  states the other three targets' types cannot hold, so this never disagrees on a
  value another language can represent. Verified at property, array-element (at
  depth, correctly pathed `ns[0]`) and typed-map-member (`m.a`) positions.

- **`08#8`** (P1) `[...v].length` is gone from the generator. A shared
  `codePointLength(value, limit = Number.POSITIVE_INFINITY)` surrogate-aware scan is
  emitted into the runtime module (`render_code_point_length_helper`,
  `typescript.rs:2126`), gated on `schema_uses_code_point_length`. `maxLength` early-exits
  at `max + 1` and recounts exactly only on the failure path; `minLength` binds the
  count once (`const codePoints = …`) so its failure path needs no second pass —
  exactly the asymmetry `maxLength.md`/`minLength.md` prescribe. Applied at every
  site: field assertions (`:453`), `format` length caps (`:497`, `:783`),
  `propertyNames` (`:751`) and `contains` matchers (`:954`). Verified: `a😀b` counts
  3; a 50 MB string against `maxLength: 5` completes in 128 ms with no array
  allocation.

- **`08#10`** (P2) `minLength: 0` is now treated as omitted
  (`Schema::effective_min_length`, `typescript.rs:296`), which also removes it from
  `has_string_constraints`, `has_materialized_wire_constraints` and the
  `propertyNames` emission gate — so no dead check and no empty serialize block.

- **`08#11`** (P2) `pattern` now emits the prescribed module-level literal
  `const PATTERN_… = /^a\/b/u;`, falling back to `new RegExp(<string>, "u")` only when
  a literal cannot be spelled (empty body, a line terminator, a trailing lone
  backslash). `ts_regex_literal`, `typescript.rs:598`. An unescaped `/` is escaped as
  `\/`, which the `u` grammar admits as an identity escape.

- **`01#1`** (P0) `find_ref_model` searched only the current module's model list, so
  a cross-file `$ref` union branch was dropped. Replaced by `find_ref_schema`, which
  searches the local list first (module-local overrides win) then a tree-wide
  registry. `src/generator/typescript.rs::generate` — the only entry point that sees
  every leaf — walks the spec tree once and calls `set_tree_json_models`
  (`typescript.rs:54`, `generator/typescript.rs:1918`/`:1949`). Verified end-to-end
  on both failure shapes from the report: a property-level union now emits the full
  three-way `kind` dispatch plus the sibling module's value imports, and a named
  cross-module union def emits a complete converter. All three branches round-trip.

## Not fixed

- **`13#4`** (P0) — **not fixable in the emitter; the spec sentence is the bug.**
  A TS `TransferTypeConverter.fromTransferType(raw: unknown)` is handed a value the
  SDK's data converter has *already* `JSON.parse`d. Measured:
  `JSON.parse("4503599627370496.5") === 4503599627370496` is `true`, so no
  post-parse predicate — `Number.isSafeInteger` or otherwise — can distinguish it
  from the integer literal `4503599627370496`. Matching Go's decimal-text semantics
  requires owning the JSON parse step (a `lossless-json`-style tokenizer), which P4
  forbids and which is not a change to this emitter. The right resolution is the one
  already in flight: correct `type.md:149-156`'s "complete and sound" claim (Wave 11)
  and record the fractional band ≥ 2^52 as a documented TS/Python/Java limitation
  alongside D8. No TS code changed.

## Cross-file requests

None.

## Sample schema requests

None strictly required — both new tests are self-contained. If the conformance
agent wants cross-language coverage, the two highest-value additions are:

1. A cross-module `$ref` union (`01#1`), as two files:
   ```yaml
   # shapes.yaml
   $defs:
     Circle: { type: object, required: [kind], properties: { kind: { type: string, const: circle }, r: { type: number } } }
     Square: { type: object, required: [kind], properties: { kind: { type: string, const: square }, s: { type: number } } }
   # main.yaml
   type: object
   properties:
     shape:
       oneOf:
         - { $ref: 'shapes.yaml#/$defs/Circle' }
         - { $ref: 'shapes.yaml#/$defs/Square' }
         - { type: string }
   ```
   with `{"shape":{"kind":"square","s":2}}` and `{"shape":"x"}` asserted to round-trip
   in all four.
2. `uniqueItems` over a materialized element (`05#3`/D10):
   ```yaml
   blobs: { type: array, items: { type: string, contentEncoding: base64 }, uniqueItems: true }
   ```
   with the serialize-failure fixture holding two byte-equal values.

## Snapshot shifts

`tests/generate_typescript.rs`'s two golden tests
(`typescript_json_example_generation_matches_checked_in_output`,
`typescript_json_api_example_generation_matches_checked_in_output`) fail until the
consolidated regeneration pass. Everything else in the file passes (26/28).

Expected sample deltas, confirmed by a local regeneration that was reverted:
- `samples/typescript/{chat,kb/**,showcase}/models.ts` and the mirrored
  `advanced/samples/typescript/json_schema/api/**` gain the serialize-side integer
  cap on every `integer` member.
- `{showcase,temporal*}/definitions.ts` gain the `codePointLength` helper.
- `showcase/models.ts` moves every length assertion to `codePointLength` and every
  `pattern` const to a regex literal (the bulk of the ~700-line diff is those two
  plus prettier reflow).

Local regeneration was validated before reverting: `samples/typescript`
`npm run typecheck` clean + 33/33 vitest green; `advanced/samples/typescript`
`npm run typecheck` clean + 14/14 vitest green.

## Tests added

- `typescript_json_wave7_discrete_defects_typecheck_and_run` — one schema covering
  `03#1`, `05#4`, `05#3`, `06#5`, `13#3`, `08#8`, `08#10`, `08#11`; asserts the emitted
  text, runs the **real `tsc`** over the output, then executes six vitest cases.
- `typescript_json_dispatches_cross_module_ref_union_branches` — `01#1`, two input
  files, `tsc` + a round-trip of all three branches.
- Two reusable helpers: `typecheck_generated_typescript` (invokes `tsc` directly on
  generated files, since `npm run typecheck` cannot see them) and
  `run_generated_typescript_test`.

---

# Follow-up round (coordinator items 1 + 2)

## Fixed

- **D2 fallout — nullable scalar elements in `uniqueItems`/`contains`** (P0, break
  introduced by the loader's D2 loosening). `render_ts_array_checks` derived
  `element_ty` from `schema.items.ty`, which is `None` for the nullability
  `oneOf: [T, null]` wrapper, so the matcher was emitted with **no** kind guard:
  `value.nums.filter((element) => element <= 5)` over `(number | null)[]` →
  `TS18047 'element' is possibly 'null'`. Now unwrapped with
  `nullable_non_null_schema`, so the element kind comes from the non-null branch.
  `src/generator/json_schema/typescript.rs:1011`.

  Semantics, converged with Go and with both specs:
  - **`contains`** — the recovered kind guard (`typeof element === 'number' &&
    Number.isFinite(element) && element <= 5`) means a `null` element can never
    satisfy a scalar matcher (`contains.md` Interactions → [[nullability]]).
    This is load-bearing, not incidental: JS coerces `null` to `0`, so
    `null <= 5` and `null >= 0` are both **`true`** — the unguarded predicate
    would have counted every `null` as a match.
  - **`uniqueItems`** — `null` is one distinct, comparable value; two `null`s are a
    duplicate (`uniqueItems.md:189-191`). Already correct via the `Map` key and now
    covered by a test. A nullable *materialized* element maps to
    `element === null ? null : bytesToBase64(element)` before comparison, so
    `null` and the wire strings share one key space (D10).

  Verified: `tsc --strict` clean on a probe with nullable `string`/`number`/
  `contentEncoding`/`date-time` elements, then executed — `[null, null]` is a
  duplicate on both sides, `nums: [null]` against `contains: {maximum: 5}` reports
  `no element matches the required schema` on both sides, and
  `{tags:["ab",null],nums:[1,null],blobs:[null,"AQI="]}` round-trips byte-identically.

- **`TS2540: Cannot assign to '<field>' because it is a read-only property`** (P0,
  pre-existing). Confirmed **not** `format`-specific and unrelated to the temporal
  literal canonicalization: it fires for **every optional `const` member** — plain
  `string`, `integer`, `contentEncoding` and temporal alike. Root cause: a `const`
  member is emitted `readonly` (`typescript.rs:3047`) while an *optional* member is
  assigned after the result literal is built
  (`const out: Main = { … }; if (plain !== undefined) { out.plain = plain; }`).
  The staging binding now drops the modifier when — and only when — the model has a
  readonly optional member:
  `const out: { -readonly [K in keyof Main]: Main[K] } = { … };`.
  `typescript.rs:3219`. The homomorphic mapped type preserves `?`, the declared
  return type is still `Main`, so consumers keep the immutable `const` member.
  Verified: `tsc --strict` clean on a four-member probe; the members round-trip and
  an optional `const` stays absent when omitted.

## Not fixed

- **`new:ts-negative-zero`** (pinned in `samples/conformance/json-schema.json`) —
  **not fixable in the emitter, same structural boundary as `13#4`.** Measured
  under nvm's Node 26:
  ```
  Object.is(JSON.parse("-0"), -0)  ->  true      // the parse side is already correct
  JSON.stringify(-0)               ->  "0"
  JSON.stringify({ a: -0 })        ->  '{"a":0}'
  JSON.stringify([-0])             ->  "[0]"
  ```
  `toTransferType` returns a JS value; the SDK's data converter owns
  `JSON.stringify`, which maps `-0` to `0` unconditionally with no hook
  (`toJSON` is a property of the *value*, and a primitive `number` has none).
  There is no JS value a converter can return that `JSON.stringify` renders as
  `-0`. Preserving it would require owning the serialize step with a
  `lossless-json`-style writer, which P4 forbids and which is not a change to this
  emitter. The pin is correct as written and should stay. Note the divergence is
  **emit-only** and asymmetric: TypeScript *accepts* `-0` off the wire and holds it
  exactly; it just cannot re-emit it.

- **`13#4`** — unchanged from the first round; the harness now pins it explicitly at
  `integer-semantics parse_failures[5]` (`{"value":4503599627370496.5}`), which
  matches the analysis.

## Harness results

| Suite | Result |
|---|---|
| `json_schema_probe_matrix` (16 schemas × 4 targets, unformatted, `tsc --noEmit` + import smoke) | **pass** |
| `json_schema_corpus_runtime` (140 pattern pairs + 293 format rows) | **pass** — covers the new `/…/u` regex literals through the real runtimes |
| `json_schema_conformance_manifest` | 1 pass / 1 fail — **no TypeScript cause**, see below |
| `generate_typescript` | 28 / 30 (the two golden snapshots only) |
| `cargo test --lib` | 493 pass |
| sample suites after a local regeneration (reverted) | `samples/typescript` typecheck clean + 33/33; `advanced/samples/typescript` typecheck clean + 14/14 |

No pinned case became stale because of a TypeScript fix — I unpinned nothing.

## Cross-file requests

`samples/conformance/json-schema.json` (conformance agent) — the manifest driver
reports four stale/over-broad pins, all from **other** agents' landed fixes, none
TypeScript:

- `recursive-collections` — `expected_divergence` (`13#2`, Java reading the
  nullability wrapper) is stale; every target now agrees. Delete it.
- `union-token-selection` — `new:java-union-typed-map-branch` is stale. Delete it.
- `numeric-bounds` — `new:go-numeric-accepts-quoted-token` is stale. Delete it.
- `integer-semantics` — the matcher `"parse_failures[2]"` (`{"value":"1"}`, the Go
  quoted-token accept) no longer matches; narrow the `matches` list to
  `["parse_failures[5]"]` and drop `new:go-numeric-accepts-quoted-token` from
  `findings`, leaving `13#4` alone.

The two TypeScript-relevant pins (`new:ts-negative-zero`,
`13#4` at `parse_failures[5]`) are still open and should be **kept**.

## Snapshot shifts (updated)

Same two golden tests. The regenerated delta now also includes:
- `{showcase,temporal,temporal-date,temporal-temporal}/definitions.ts`: the temporal
  regexes tighten `(\.[0-9]+)?` to `(\.[0-9]{1,9})?` and base64 tightens to the
  canonical form — both from the shared-helpers agent, surfacing through TS.
- No sample carries an optional `const` member, so the `-readonly` staging type does
  not appear in the checked-in output.

## Tests added (this round)

- `typescript_json_guards_nullable_elements_in_array_keywords` — the D2 shape across
  nullable `string`/`number`/`contentEncoding` elements; text asserts, real `tsc`,
  then three executed cases pinning both semantics in both directions.
- `typescript_json_optional_const_members_typecheck` — optional `const` for plain,
  integer, `contentEncoding` and temporal members; asserts the member stays
  `readonly` while the staging binding is mutable, real `tsc`, plus a round trip.

---

# Follow-up round 2 — fractional seconds in `--date-time-types=temporal`

## Fixed

- **`temporal` repr rejected a >9-digit fractional second, rawly** (P0, two defects
  in one). Reproduced exactly as reported: `2021-01-15T12:30:45.123456789012Z`
  escaped `fromTransferType` as
  `RangeError: Temporal error: Fractional time exceeds nine digits.`

  1. **Accept-set divergence.** `Temporal.ZonedDateTime` caps at nanoseconds just
     like `java.time`, so it now parses-and-truncates instead of rejecting, matching
     the documented accept-every-width-and-truncate-to-capacity contract.
     `render_ts_temporal_helpers` emits a `truncateTemporalFraction` scan
     (`TS_TEMPORAL_FRACTION_TRUNCATION`, `typescript.rs:1985`) mirroring Java's
     `truncateFraction`, applied at the one materialization site:
     `const canon = truncateTemporalFraction(canonicalizeTemporalDateTime(value));`
     (`typescript.rs:2044`). Trailing-zero trimming runs first, so
     `.1000000000000` never reaches the truncation at all.
  2. **P11 break.** `Temporal.ZonedDateTime.from` and `Temporal.PlainDate.from` are
     now wrapped in `try { … } catch { violations.push({path, reason}); return
     undefined; }` (`typescript.rs:2046`, `:2063`), the same shape as Java's
     `catch (DateTimeParseException)`. The pinned regex plus `validTemporalCalendar`
     should already admit only constructible values, so this is structural: no
     constructor throw can ever leave the converter as a bare error again, whatever
     the input.

  The helper and the guards are emitted **only** for the `temporal` repr — `string`
  and `date` never call `Temporal.*.from`, so emitting them there would be dead code
  (asserted in the new test).

### Verified by execution, against generated Go for the same schema

Generated Go from the same `main.yaml`, built it, and ran both. TS `temporal`'s
re-emitted `createdAt` is **byte-identical to Go's on every case**:

| wire `createdAt` | Go | TS `temporal` (after fix) |
|---|---|---|
| `…45.123456789012Z` | `…45.123456789Z` | `…45.123456789Z` |
| `…45.0000000001Z` | `…45Z` | `…45Z` |
| `…45.1000000000000Z` | `…45.1Z` | `…45.1Z` |
| `…45.9999999999Z` | `…45.999999999Z` | `…45.999999999Z` |
| `…45.123456789012+05:30` | `…45.123456789+05:30` | `…45.123456789+05:30` |
| `2021-01-15t…45.123456789012z` | `…45.123456789Z` | `…45.123456789Z` |
| `…45.000000000000Z` | `…45Z` | `…45Z` |

Before the fix, every 10+-digit row above threw a `RangeError`; the ≤9-digit rows
are unchanged.

The other two reprs are unchanged and were re-measured for the same input
`…45.123456789012Z`: `date` → `2021-01-15T12:30:45.123Z` (the `Date` millisecond
capacity), `string` → verbatim `…45.123456789012Z` (no capacity, nothing dropped).

## Per-target expectations for the harness (for the conformance agent)

I touched **no** existing manifest, fixture or corpus entry — I checked, and none
carries a fraction wider than nine digits:

- `samples/conformance/json-schema.json` — no case has a >9-digit fraction
  (the only `123456…` hits are `integer-semantics` and `contains-matcher`, both
  numeric, unrelated).
- `specs/json-schema/corpora/format_materialize_clock/corpus.json` — the widest row
  is exactly nine digits (`2021-06-15T12:30:45.123456789Z` / `…+02:00`), which is
  why the byte-identity claim in its `note` still holds today.
- `samples/wire/json_schema/**` — no >9-digit fraction.

So a **new** entry is wanted, and it cannot be byte-identity across all five
materializing targets. Suggested declared expectation for input
`2021-01-15T12:30:45.123456789012Z`:

| target | re-emitted | why |
|---|---|---|
| go | `…45.123456789Z` | `time.Time`, ns |
| java | `…45.123456789Z` | `java.time`, ns |
| python | `…45.123456Z` | `datetime`, µs |
| ts `string` | `…45.123456789012Z` | stores the wire string; no capacity to lose |
| ts `date` | `…45.123Z` | `Date`, ms |
| ts `temporal` | `…45.123456789Z` | `Temporal.ZonedDateTime`, ns |

Every row is an **accept**; the divergence is capacity, not verdict, so this is
exception (b) and not a P1 break. Note `format_materialize_clock`'s `note` currently
credits the "equally-capable materializing set (go, java, py, js-string,
js-temporal)" with byte-identical output — that grouping is only true at ≤9 digits,
and `js-string` leaves the set at 10+. If a wide row is added there, that sentence
needs the qualifier (specs agent's file, flagging rather than editing).

### One related, non-defect asymmetry, for the record

TypeScript models `format: time` as a `string` in **every** repr
(`ts_temporal_type`, `typescript.rs:135`), so `09:00:00.123456789012` re-emits
verbatim while Go and Java (which materialize `time` to a ns type) truncate to
`09:00:00.123456789` and Python to µs. This is the same class as TS-`string`
date-time and by the same reasoning is not a defect — there is no capacity, so
nothing is dropped, and truncating would *introduce* a loss nothing requires. It
does mean a `time` fixture with a wide fraction also needs a declared per-target
expectation rather than byte-identity. Not changed.

## Harness results (re-run after this change)

| Suite | Result |
|---|---|
| `json_schema_conformance_manifest` | **2/2 pass** — now fully green; the four stale pins I flagged last round have been cleaned up |
| `json_schema_probe_matrix` | pass |
| `json_schema_corpus_runtime` | pass (140 pattern pairs + 293 format rows) |
| `generate_typescript` | 29 / 31 (the two golden snapshots only) |
| `cargo test --lib` | 499 pass |
| sample suites after a local regeneration (reverted) | `samples/typescript` typecheck clean + 33/33; `advanced/samples/typescript` typecheck clean + 14/14 |

No pinned case became stale from this change.

## Snapshot shifts (updated)

Adds `samples/typescript/temporal-temporal/definitions.ts` and its
`advanced/samples/typescript/json_schema/api/temporal-temporal` mirror (+34 lines:
the `truncateTemporalFraction` helper and the two `try`/`catch` guards). The other
TS sample trees are unaffected by this round — `showcase`, `temporal` and
`temporal-date` do not use the `temporal` repr.

## Tests added (this round)

- `typescript_json_truncates_over_capacity_fractional_seconds` — one schema
  generated at all three reprs. Asserts the helper is emitted for `temporal` and
  **not** for `string`/`date`, asserts the `try`/`catch` guard, runs the real `tsc`,
  then executes a round trip pinning the declared per-repr output
  (`…45.123456789Z` / `…45.123Z` / `…45.123456789012Z`).
