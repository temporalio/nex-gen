# java-emitter — execution report

Owned files touched: `src/generator/json_schema/java.rs`, `tests/generate_java.rs`.
`src/generator/java.rs` needed no change.

## Verification setup

The main working tree does not build mid-rollout (concurrent `go.rs` edits), so
every build/probe ran in a HEAD snapshot at `/tmp/jbuild` with my three owned
files copied in. Probes were generated with the real CLI
(`nexgen java --output … --package-name …`) into `/tmp/jprobe/**`, compiled with
**`javac 21`** against jackson-databind 2.15.4 + jspecify 1.0.0, and **executed**
(parse + serialize round trips) — not just read.

End-to-end: `cargo build-json-examples` in the snapshot, then
`samples/java && ./gradlew test` and `advanced/samples/java && ./gradlew test`
both green (the Java round-trip suites run against the regenerated output).
Full `cargo test --all-features` in the snapshot: **green, 0 failures**
(with the samples regenerated).

---

## Fixed

### Wave 1.3 — nullability unwrap (`13#2`, `07#1`, `11#1`, `06#4`) — P0
`src/generator/json_schema/java.rs:1899-1996` (`resolve_model_kind`). Every read
of a property's **shape** now goes through `nullable_non_null_schema` via a
single `shape` binding: `const`/`enum` (`:1910-1916`), `ref_union` (`:1938`),
`ClosedNameOverrides` (`:1982`), `NumericConstraints` (`:1986`),
`StringLengthConstraints` (`:1987`), `ArrayConstraints` (`:1988`),
`schema` (`:1989`), `items` (`:1961`, `:1990`). Presence/annotation keywords
(`default`, `title`, `description`, `deprecated`, `x-java-name`, `nullable`)
stay on the authored wrapper.

Verified by running the plan's exact probe through generated+compiled Java:
`{"nullName":"A","nullEnum":"zzz","nullEmail":"nope","nullInt":99,"nullTags":["x"]}`
→ **6 violations** (was: silently accepted). `{"…":null}` still accepts, and a
valid payload still round-trips. A nullable `enum` now also produces its closed
value class (`NullEnum`), and a nullable array's element type comes from the
branch.
Test: `java_json_nullable_property_keeps_the_branch_constraints`.

### 2.6 / `05#3` + 4.3 / `05#2` — `uniqueItems` compared by reference / signed zero — P0
`java.rs:951-1006` (`java_needs_unique_key_mapping`, `java_unique_key_expr`) and
`:1082-1096`. The serialize-side map is now `Map<Object, Integer>` keyed by a
**derived** key per decision **D10**:
- `Temporal(k)` → `TemporalSupport.format<K>(e)` (canonical wire string),
- `Bytes(enc)` → `Base64Support.format<Enc>(e)`,
- `Double` → `SpecNumbers.numberKey(e)` (new helper; `-0.0` folds onto `0.0`),
- `List<…>` of any of those → the same key mapped elementwise.

Runtime-verified: two byte-equal `byte[]`, two equal `OffsetDateTime`, and
`[-0.0, 0.0]` all now report `duplicate items: element at index 1 equals index 0`
on serialize (all three were accepted before, then rejected by their own parser
and by Python).

### 2.3 / `09#3` — no serialize-side predicate for `duration`/`time`/offsets — P0
`java.rs:283-307` (`java_type_needs_temporal_check`, `java_temporal_check_fn`),
`:450-492` (`render_java_temporal_checks`, replacing the date-only
`render_java_calendar_year_checks`), predicates in `TEMPORAL_SUPPORT_BODY`
(`java.rs:5951-6006`). `TemporalSupport.checkDateTime/checkDate/checkTime/
checkDuration` mirror Python's `_check_date_time`/`_check_time`/`_check_duration`:
year `< 1` / `> 9999`, UTC offset not a whole number of minutes, negative
duration, sub-second duration, over-cap duration, and (for `time`) the pinned
grammar re-asserted over the stored canonical string.

Runtime-verified, all previously silent:
`PT-1H-30M` → `a duration cannot be negative`; `PT0.5S` → `a duration cannot
carry a fraction of a second`; 30-second offset → `the UTC offset +00:00:30 is
not a whole number of minutes`; year 10000 → `year must be <= 9999`;
`tod="not-a-time"` → `must be a valid time`.
Note the loop variables renamed `calendarIndex*`/`calendarValue*` →
`temporalIndex*`/`temporalValue*`, and the check is now a call rather than an
inline `if` (compile-once by construction).

### `09#7` — temporal `const`/`default` literal rendered raw — P1
`java.rs:5583-5591` (`default_expr`). The literal is `to_ascii_uppercase()`d
before `OffsetDateTime.parse(…)` / `Duration.parse(…)` / the stored `time`
string, as Go's `mustParseDateTime` does. `OffsetDateTime.parse("2021-06-15t12:30:45z")`
no longer throws `DateTimeParseException` the first time the default is read.

### 4.2 / `01#3` — discriminated union gated on `disc.isTextual()` — P0
`java.rs:2371-2389` (`java_node_equals_literal`) and `:2418-2452`. The
`switch (disc.textValue())` is gone; dispatch is now an if/else chain over
**JSON value equality** (`disc.isNumber() && disc.doubleValue() == 1.0`,
`disc.isBoolean() && …`, `"cat".equals(disc.textValue())`). The required-check
became `disc == null || disc.isNull() || !disc.isValueNode()` and the unknown-tag
reason uses `disc.asText()`. Braced branches also removed a latent scope
collision between two `owned_object` cases in the old switch.

Runtime-verified: `{"kind":2,…}` and `{"kind":1.0,…}` both select their branch;
`{"kind":9}` → `unknown discriminator kind 9: expected one of [1, 2]`.
Test: `java_json_dispatches_a_non_string_discriminant_by_value`.

### 4.1 / `11#2` — integer `const: 1.0` emitted `0L` — P0
`java.rs:204-226` (`java_bound_literal`), `:5537-5548` (`java_closed_literal`),
`:5567-5570` (`default_expr`). `as_i64()` is no longer the classifier; the shared
`java_bound_literal` handles integral `f64` spellings.
Runtime-verified: `const: 1.0` now emits `new Score(1L)`, accepts wire `1`/`1.0`
and rejects `0` (it was the exact opposite).

### 4.1 / `13#3` — serialize-side ±(2^53−1) integer cap missing — P0
`java.rs:249-281` (`java_type_needs_integer_cap_check`,
`render_java_integer_cap_checks`), wired at `:3392` (`field_has_serialize_check`),
`:3438` (per-field serialize) and `:4890` (typed-map member). Same reason text as
Go: `exceeds ±(2^53-1) integer cap`.
Runtime-verified: `count = 9007199254740993L` now fails serialize (was emitted,
then rejected by its own parser).

### 4.1 / `13#4` — `specLong` is not the `BigDecimal` helper `type.md` prints — P0
`java.rs:5788-5806` (`render_spec_numbers_file`) plus a new
`SpecNumbers.readExactTree` (`java.rs:5860-5930`, `SPEC_NUMBERS_EXACT_TREE_BODY`),
substituted for `parser.readValueAsTree()` at `java.rs:3966` and `:5236`.

`specLong`/`isSpecLong` now classify on `node.decimalValue().stripTrailingZeros()
.scale()` and compare the cap as a `BigDecimal`, exactly as the spec prints.
**That alone was not enough**: Jackson's default tree folds every floating token
into a `double`, so `4503599627370496.5` had already rounded to an integral value
before `decimalValue()` could see it. `readExactTree` walks the token stream and
keeps the exact decimal **only when the `double` is lossy** — for every other
token it produces the node Jackson would have, so signed zero, the re-emitted
lexeme of a pass-through extra, `1e2` → `100.0`, and `1e400` → the same
non-finite handling all stay byte-identical (measured side by side against
`ObjectMapper.readTree`). Runtime-verified: `{"count":4503599627370496.5}` now
fails with `not an integer` (was accepted as `4503599627370496`).

### 4.4 / `06#2` — fractional matcher bound truncated on the typed path — P0
`java.rs:204-226`. A fractional bound over an `integer` position keeps its
`double` spelling; Java widens the `long` operand. Generated output for
`items:{type:integer}, contains:{type:number, minimum:1.5}` is now
`element >= 1.5` on **both** sides (was `element >= 1L` serialize /
`rawElement.doubleValue() >= 1.5` deserialize — Java disagreeing with itself
across the P12 boundary). Runtime-verified: `{"bumps":[1]}` rejects, `[2]` accepts.

### Wave 7 / `03#8` — nested violations not re-pathed or merged on serialize — P1
`java.rs:3858-3928` (`field_serialize_may_nest`, `render_capturing_value_write`,
`render_field_serialize`), `:3742-3752` + `:3820-3826` (`render_object_serializer`),
`:5346-5360` (`write_map_value`), `:5273-5296` (typed-map serializer).
A member whose value is another validating model is written inside a
`try`/`catch (ValidationException)`; each violation is `withPathPrefix`ed and
merged into the parent's list, then thrown (the nested serializer aborted
mid-value, so nothing further can be written to the generator). Arrays of models
are written elementwise so the index survives. The parent's own early throw moves
to after `writeEndObject()` whenever the object can nest, so parent + nested
violations arrive as one aggregated failure.

Runtime-verified: serialize now reports
`label: must have length >= 4, got 2; address.zip: must have length >= 5, got 1`
(was: a bare `zip:` violation escaping alone) and `addresses[1].zip: …` for the
array case. Also covers the catch-all/typed-map path.

### Wave 7 / `10#5` — `byte[]` uses `Objects.equals`/`Objects.hash`/concat — P1
`java.rs:3382-3416` (`java_member_equality`) + `render_equals_hashcode_tostring`,
and three new `Base64Support.listEquals`/`listHashCode`/`listToString` helpers
(`java.rs:5745-5789`).
Runtime-verified: two models parsed from the same payload are now `equals` with
equal hash codes, and `toString()` prints `payload=[104, 105]` instead of
`[B@1b6d…`.

### Wave 7 / `08#7`, `09#11` — `contains` matcher regex recompiled per element — P1
`java.rs:680-737` (`java_contains_pattern_field_name`,
`java_contains_format_field_name`, `render_contains_pattern_statics`),
`java_matcher_condition` now takes a `scope`, threaded through
`render_java_array_checks` / `render_java_raw_array_checks` to the field,
typed-map-member, additionalProperties and union-wrapper positions. Statics are
emitted next to the existing `<FIELD>_PATTERN`/`<FIELD>_FORMAT` fields.
Generated output: `NAMES_CONTAINS_PATTERN.matcher(element).find()` with one
class-init `Pattern.compile`. The recursively-nested positions
(`render_java_inline_string_checks`, nested array levels) keep the documented
inline compile — they have no stable class member to hang a static on.
Test: `java_json_compiles_contains_matcher_regexes_once`.

### Wave 7 / `12#4` — Javadoc on the private field, orphan `@deprecated` tag — P1
`java.rs:3323-3327` (field loop) and `:3350-3369` (getter loop). The authored
summary/body now render as **one** Javadoc block on the public getter with the
generated `@deprecated` tag as its trailer, then `@Deprecated`, matching
`description.md`'s "Javadoc above the class/getter/method". The dead
`render_javadoc` wrapper was removed.
Test: `java_json_member_javadoc_lands_on_the_getter`.

### `06#gap-8` — no test for the serialize-side `contains` assertion — P1
This is a **testing** gap, not an implementation gap: `render_java_array_checks`
(the serialize emitter) has always included `contains`, verified in generated
output and at runtime. Added the missing assertion in
`java_json_serialize_side_guards_match_the_parse_side`.

### `2.2` / `09#2`, `10#2` — verified (no Java change)
Java compares temporal and `contentEncoding` `const`/`enum` on the **wire
string**, which is the D10-correct side:
serialize builds `<field>Wire` (`TemporalSupport.format*` / `Base64Support.format*`)
and compares that; deserialize compares `field.textValue()`. Confirmed on a probe
(`const: "PT90M"` → `must equal "PT90M", got " + whenWire`). The remaining defect
is that the **literal** is not canonicalized at load — see Cross-file requests.

---

## Not fixed

### `09#6` — Java materializes `time` as `String`, not `OffsetTime`/`LocalTime` — P1
**Half fixed, half disagreed with the finding.**

*Fixed*: the real consequence the finding names — "because the field is a plain
`String` it receives no serialize validation at all" — is closed.
`TemporalSupport.checkTime` now re-asserts the pinned `time` grammar and the
whole-minute offset on the way out (see `09#3` above), and `time` participates in
the `uniqueItems` canonical-wire key.

*Not done*: changing the materialized type. Java has **no single `java.time`
type** that carries both an offset-bearing and an offset-less time-of-day —
`format.md:275-282` says so itself ("Java falls back to `LocalTime`"). A field
declared as the union of the two would have to be typed
`java.time.temporal.TemporalAccessor` or `Object`, which:
- breaks P13.1 (the in-memory type stops being a compile-time closed contract),
- forces every reader to `instanceof`-dispatch and cast,
- and buys nothing on the wire: the stored value is already the **canonical**
  wire string, offset preserved, so the round trip is byte-identical and lossless
  in both directions today.

The `format.md:224` table row (`OffsetTime` / `LocalTime`) is the thing that is
wrong, not the emitter. Recommend amending that cell to `String (canonical wire
form)` with the reason, and dropping the `format.md:387` "`OffsetTime.parse` …
or `LocalTime.parse`" clause from the Java strategy row. **Spec owner call.**

### `11#4` — Java closed-value constant naming
Untouched per decision **D3**, as instructed.

### `11#6` — `get<Field>OrDefault`
Kept per decision **D9**. Its P15 registration is the loader agent's item.

---

## Cross-file requests

1. **`src/parser/json_schema.rs` (loader) — canonicalize a materialized
   temporal `const`/`default`/`enum` literal at load** (`09#2`, plan item 2.2).
   Java compares the canonical wire string on serialize but the literal verbatim,
   so `{type: string, format: duration, const: "PT90M"}` emits
   `must equal "PT90M", got " + whenWire` where `whenWire` is always `"PT1H30M"` —
   the field is **unsatisfiable on serialize** and the model that parsed from
   `"PT90M"` can never be written back. `format::canonicalize_duration`
   (`src/json_schema/format.rs:204`) is the helper and is currently dead code.
   Same for `date-time`/`time` case and offset normalization
   (`2021-06-15t12:30:45z` → `2021-06-15T12:30:45Z`, `+00:00` → `Z`).
   Once the literal is canonical, Java is correct in both directions with no
   further emitter change.

2. **`src/parser/json_schema.rs` — `validate_member_scope` should register the
   Java `get<Field>OrDefault` accessor** (`11#6`, D9) — noted for completeness;
   already the loader agent's item.

---

## Sample schema requests

To `samples/schemas/showcase.nexusrpc.yaml`, under the showcase root
`properties:` (all four of these are silently mis-handled by at least one target
today and none of them appear anywhere in the corpus):

```yaml
  nullableName:
    oneOf:
      - { type: string, minLength: 3, pattern: "^[a-z]+$" }
      - { type: "null" }
  nullableCount:
    oneOf:
      - { type: integer, minimum: 1, maximum: 10 }
      - { type: "null" }
  nullableTags:
    oneOf:
      - type: array
        items: { type: string }
        minItems: 2
        uniqueItems: true
        contains: { type: string, minLength: 2 }
      - { type: "null" }
  nullableTier:
    oneOf:
      - { type: string, enum: [gold, silver] }
      - { type: "null" }
  fractionalMatch:
    type: array
    items: { type: integer }
    contains: { type: number, minimum: 1.5 }
  uniqueStamps:
    type: array
    items: { type: string, format: date-time }
    uniqueItems: true
  uniqueBlobs:
    type: array
    items: { type: string, contentEncoding: base64 }
    uniqueItems: true
  uniqueRatios:
    type: array
    items: { type: number }
    uniqueItems: true
```

Suggested conformance manifest rows to go with them:
- `{"nullableName":"A"}`, `{"nullableCount":99}`, `{"nullableTier":"zzz"}`,
  `{"nullableTags":["x"]}` → `parse_failures` in **all four**
  (Java accepted every one of these before this change).
- `{"nullableName":null,…}` → accepted in all four.
- `{"fractionalMatch":[1]}` → `parse_failures`; `[2]` → accepted.
- `serialize_failures`: an over-cap integer, `-0.0`+`0.0` in `uniqueRatios`,
  two byte-equal `uniqueBlobs`, two equal `uniqueStamps`, a negative /
  sub-second / over-cap duration, a 30-second UTC offset, year 10000.
- `{"count": 4503599627370496.5}` against an `integer` member →
  `parse_failures` (Go and Java reject after this change; TS and Python still
  accept — that pair is still open).

---

## Snapshot shifts

`tests/generate_java.rs::java_json_example_generation_matches_checked_in_output`
and `…api_example_generation_matches_checked_in_output` will stay red until the
consolidated regeneration pass. Both go green immediately after
`cargo build-json-examples` (verified in the snapshot tree). Expected diff in
`samples/java/**` and `advanced/samples/java/**`:

- every `Deserializer`: `parser.readValueAsTree()` → `SpecNumbers.readExactTree(parser)`
- `SpecNumbers.java`: `BigDecimal` classification + `INTEGER_CAP_DECIMAL` +
  `isFiniteNode` + `numberKey` + `readExactTree`
- `TemporalSupport.java`: `checkDateTime`/`checkDate`/`checkTime`/`checkDuration`
  + `checkOffset`/`checkYear` + `import java.time.ZoneOffset`
- `Base64Support.java`: `listEquals`/`listHashCode`/`listToString`
- member Javadoc moves from the private field to the getter (every model)
- serialize-side integer cap on every `integer` member (adds a `Serializer`
  validation block to models that had none)
- temporal serialize checks become `TemporalSupport.check*(…)` calls, loop
  variables `calendarIndex*`/`calendarValue*` → `temporalIndex*`/`temporalValue*`
- nested-model writes wrapped in `try`/`catch (ValidationException nested0)`,
  and the parent's `throw` moves after `writeEndObject()` in those models

I updated `java_json_emits_runtime_support_for_nested_materialized_values` for
the renamed loop variables and the new `TemporalSupport` surface.

## New tests

`tests/generate_java.rs` (+6):
`java_json_nullable_property_keeps_the_branch_constraints`,
`java_json_serialize_side_guards_match_the_parse_side`,
`java_json_dispatches_a_non_string_discriminant_by_value`,
`java_json_repaths_nested_violations_on_serialize`,
`java_json_compiles_contains_matcher_regexes_once`,
`java_json_member_javadoc_lands_on_the_getter`.

These are text assertions, matching the file's existing style. The real
compile-and-run gate for Java lives in Wave 0.2's probe-matrix harness
(conformance agent); I ran `javac` + execution manually on every probe listed
above and the Gradle round-trip suites on the regenerated samples.

---

# Addendum — runtime support classes and the `\z` end-anchor rewrite

Raised by the coordinator (shared-helpers finding) after the main pass.

## Changed

`src/generator/json_schema/java.rs:689-700` — new `java_pinned_pattern(p)`
wrapping `pattern::rewrite_end_anchor(p, r"\z")`, applied to all six pinned
regexes baked into the runtime support classes:
`render_base64_support_file` (`BASE64`, `BASE64URL`, `java.rs:6113-6127`) and
`render_temporal_support_file` (`DATE_TIME`, `DATE`, `TIME`, `DURATION`,
`java.rs:6238-6259`). Those were the last Java regexes reaching a
`Pattern.compile` without the rewrite every other emitted regex already goes
through.

Generated output now reads, e.g.:

```java
private static final Pattern DATE = Pattern.compile("^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])\\z");
private static final Pattern BASE64 = Pattern.compile("^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/][AQgw]==|[A-Za-z0-9+/]{2}[AEIMQUYcgkosw048]=)?\\z");
```

Regression assertion added to
`java_json_emits_runtime_support_for_nested_materialized_values`: every
`private static final Pattern` line in `TemporalSupport.java` and
`Base64Support.java` must end in `\z` and must not end in a bare `$`.

## Verification, and a correction to the finding's severity

Generated from the **main tree**, compiled with `javac 21`, and executed. The two
payloads named in the finding, plus the other three pinned kinds:

| payload | after |
|---|---|
| `{"blobs":["aGk=\n"]}` | `blobs[0]: must be base64-encoded, got "aGk=\n"` |
| `{"ts":"2021-06-15T12:30:45Z\n"}` | `ts: must be a valid date-time, got "…\n"` |
| `{"day":"2021-06-15\n"}` | `day: must be a valid date, got "…\n"` |
| `{"tod":"12:30:45Z\n"}` | `tod: must be a valid time, got "…\n"` |
| `{"dur":"PT1H\n"}` | `dur: must be a valid duration, got "…\n"` |
| all five clean values | accepted, round-trip unchanged |

**However — the claimed accept-set divergence did not reproduce before the fix.**
I rebuilt the pre-fix support classes byte-exactly (took the generated output and
replaced only `\\z");` back to `$");` on the six pattern lines), compiled, and ran
the identical payloads: **all five already rejected**, with identical messages.

The reason is the use site, not the pattern. All seven uses of these constants
call `Matcher.matches()`, never `find()`:

```
TemporalSupport: DATE_TIME/DATE/TIME/DURATION .matcher(value).matches()   (x5, incl. checkTime)
Base64Support:   BASE64/BASE64URL             .matcher(value).matches()   (x2)
```

`matches()` requires the match to span the entire input, and `$` is zero-width,
so a trailing `\n` is left unconsumed and the match fails. Measured directly:

```
pattern with $   , "aGk=\n"                 -> matches=false, find=true
pattern with $   , "2021-06-15T12:30:45Z\n" -> matches=false, find=true
pattern with \z  , same two inputs          -> matches=false, find=false
```

So Java's accept set already agreed with Go/JS/Python here; the divergence would
have appeared the moment any of these seven sites moved to `find()` — which is
exactly what the value-level `pattern`/`format` path uses, so the two halves of
the emitter disagreed on which anchor discipline they relied on.

The fix is landed regardless and I'd keep it: it removes the hidden coupling
between the oracle's correctness and each call site's anchoring mode, and makes
Java consistent with the other three targets and with its own value-level path.
But it should be re-graded **P2 (latent)** rather than P0 — no wire value changes
verdict in either direction. Python's analogous `09#4` is genuinely live for a
different reason (`re.search` + `$`), so the two are not the same severity.

## Follow-up on the queued cross-file request

Noted that shared-helpers landed `format::canonicalize(kind, value)` /
`canonicalize_for_format(name, value)`. My cross-file request stands unchanged
and is now cheap for the loader agent: canonicalize a materialized temporal
`const`/`default`/`enum` literal at load with those entry points. No further Java
emitter change is needed once the literal is canonical — Java already compares the
canonical wire string on serialize and the wire string on parse.

## Re-verification after this change

- `cargo build --all-features` clean; `cargo fmt --check` clean on both my files.
- Snapshot tree: `cargo build-json-examples`, then `cargo test --all-features`
  **green, 0 failures** (18/18 in `generate_java`).
- `samples/java && ./gradlew test` and `advanced/samples/java && ./gradlew test`
  both exit 0 against the regenerated output.
- Main tree: the same two golden-snapshot tests remain the only failures,
  pending the consolidated regeneration pass.

**Additional snapshot shift:** `TemporalSupport.java` and `Base64Support.java` in
`samples/java/**` and `advanced/samples/java/**` — six pattern literals change
their trailing `$` to `\z`.

---

# Addendum 2 — conformance-harness findings

Three items routed after the Wave 0 harness went live. All three reproduced;
all three are fixed and confirmed against the harness, not only local probes.

## Fixed

### `new:java-union-typed-map-branch` — P0, generated Java does not compile
`src/generator/json_schema/java.rs:5126-5162` (`render_typed_map_class` now takes
`implements`) and `:2325-2336` (`render_model_file` passes it).

`render_model_file` computes the interfaces a model implements and handed them to
`render_object_class` only — `render_typed_map_class` never received them. A
typed-map branch of a typeless `oneOf` (`{type: object, additionalProperties:
{type: string}}`) is a `ModelKind::TypedMap`, so it was emitted with **no**
`implements` clause while the union's dispatcher still read it with
`readTreeAsValue(node, <Branch>.class)`. That is not a cast error but an
unsatisfiable inference constraint — `T` is pinned to the branch by the class
literal and bounded above by the interface:

```
error: incompatible types: inference variable T has incompatible bounds
    equality constraints: UtmScalarUnionObject
    upper bounds: ScalarUnion,Object
```

After: `public final class UtmScalarUnionObject implements Utm.ScalarUnion {`.
Verified with `javac 21` (compiles) and executed: all five token shapes of the
harness's `scalarUnion` select their branch and round-trip
(`{"k":"v"}`, `"t"`, `7`, `["x","y"]`), and `true` is rejected with
`expected one of: string, integer, array, UtmScalarUnionObject`.
Harness: `union-token-selection` now agrees across all four targets;
its `expected_divergence` **unpinned**.
Test: `java_json_typed_map_union_branch_implements_the_interface`.

### `13#2` at the element level — P0
`src/generator/json_schema/java.rs:1806-1820` — new `element_shape(schema)`
helper (`schema.items` with the nullability wrapper stripped), applied at all
nine sites that read an element's **shape**:
`schema_has_recursive_value_checks` (`:381`), `render_java_recursive_value_checks`
(`:443`), the union-variant array parse (`:2601`), `field_has_serialize_check`
(`:3383`), the serialize-side field element checks (`:3457`), the field element
parse (`:4420`), the nested-array level parse (`:4660`), the typed-map member
array parse (`:4867`) and the map-member serialize element checks (`:5026`).

The coordinator was right that this was a second site: my earlier fix unwrapped
the **property**, but `items: {oneOf: [T, null]}` is the *element-level* spelling
of nullability and the element's own wrapper was still read raw. Measured before
the fix on a probe carrying `slots` (`string, minLength: 2`), `counts`
(`integer, minimum: 5`), `grid` (nested) and a typed-map member: the generated
`Elem.java` contained **zero** occurrences of `must have length` or `must be >=`.
After, both directions carry them at every depth, and executed:

```
{"slots":["x"]}    -> slots[0]: must have length >= 2, got 1
{"counts":[1]}     -> counts[0]: must be >= 5, got 1
{"grid":[["x"]]}   -> grid[0][0]: must have length >= 2, got 1
```

`element_shape` deliberately does not also fold in the element's *nullability* —
that is read from the wrapper separately (`allows_null`), and collapsing the two
is what would reintroduce the bug in the other direction.
Harness: `recursive-collections` now agrees across all four targets;
its `expected_divergence` **unpinned**.
Test: `java_json_nullable_element_keeps_the_branch_constraints`.

### Temporal collection elements bypass jsr310 — P0
`src/generator/json_schema/java.rs:4033-4038` + `:4046-4128`
(`java_list_needs_owned_write`, `render_owned_value_write`) and `:5426-5432`
(`write_map_value`).

A scalar `format`/`contentEncoding` member was already written through
`TemporalSupport.format*` / `Base64Support.format*`, but the same value inside a
`List` fell into the generic `JavaType::List(_)` arm and reached
`serializers.defaultSerializeValue`. Measured with a **stock** `ObjectMapper`:

```
SER-THROW com.fasterxml.jackson.databind.exc.InvalidDefinitionException:
  Java 8 date/time type `java.time.OffsetDateTime` not supported by default:
  add Module "com.fasterxml.jackson.datatype:jackson-datatype-jsr310"
```

**It was two bugs, not one.** `byte[]` does not throw — Jackson silently writes
its own base64 variant. For `contentEncoding: base64url` that is wire
corruption the generated parser itself then rejects:

```
jackson default for List<byte[]>: ["aGk="]
canonical base64url for "hi":     ["aGk"]
```

The fix writes the array elementwise with the generator-owned encoder, recursing
through nesting levels and writing `gen.writeNull()` for a null element. Verified
end-to-end with a bare `ObjectMapper` over arrays of `date-time`, `base64`,
`base64url`, a nested array of `date`, and a typed-map member of `duration`:

```
PARSE-OK ...
SER-OK   {"stamps":["2021-06-15T12:30:45Z"],"blobs":["aGk="],"urls":["aGk"],
          "days":[["2021-06-15","2022-01-02"]],"byKey":{"a":["PT1H30M"]}}
ROUNDTRIP-IDENTICAL true
```

Test: `java_json_writes_materialized_collection_elements_itself`.

## Manifest edit (outside my ownership — flagging explicitly)

Per the coordinator's instruction to unpin what I fix, I deleted exactly two
`expected_divergence` blocks from `samples/conformance/json-schema.json`:
`recursive-collections` (`13#2`) and `union-token-selection`
(`new:java-union-typed-map-branch`). I touched nothing else in that file. The
run also reported `numeric-bounds` and `integer-semantics` as stale for
`new:go-numeric-accepts-quoted-token` — **not mine**, so I left them; they were
narrowed concurrently by the Go side and the harness is green now.

`integer-semantics` still pins `13#4`, correctly: Java is now on the rejecting
side with Go, and the remaining divergence is Python and TypeScript accepting
`4503599627370496.5`.

## Newly found while verifying — reported, not fixed

### A member named `index` makes the generated Java uncompilable — P0
Two properties are enough:

```yaml
type: object
properties:
  index: { type: string }
  tags: { type: array, items: { type: string } }
```

```
error: variable index is already defined in method deserialize(JsonParser,DeserializationContext)
    for (int index = 0; index < field.size(); index++) {
```

The field locals are declared at method scope (`String index = null;`) and the
array parse loop declares `int index` in a nested block; Java forbids a local
shadowing an enclosing local. The same surface covers the other generated
deserializer locals — `field`, `node`, `element`, `elementPath`, `items`,
`violations`, `length`.

I did **not** fix this unilaterally. A rename inside my emitter (e.g.
`<field>Index`, or a reserved `nexgen` prefix) shrinks the surface but cannot
close it, and it would churn every generated deserializer a second time in this
rollout. The complete fix is Wave 3's: `validate_member_scope` has to know Java's
generated **local** namespace, not just its method/nested-class namespace — the
plan already notes the loader ignores Java's synthesized names entirely
(`collect_synthesized_top_level` returns early for every language but Go).
Say the word and I will land the emitter-side prefix as well.

### A nullable element at array depth >= 1 rejects `null` — P1
```
{"grid":[["ab",null]]}  ->  grid[0][1]: expected string
```
with `grid: {type: array, items: {type: array, items: {oneOf: [{type: string,
minLength: 2}, {type: "null"}]}}}`. Depth 0 (`slots`) is correct; the nested
level is not, because `render_parse_element`'s `List` arm passes `false` for the
element's nullability (`java.rs:4667`) and the declared type is `List<List<String>>`
with no `@Nullable` on the inner element.

This is a *representation* gap rather than the constraint drop the finding named:
`FieldPlan` carries a single `nullable_items` flag, so element nullability only
exists for one level. Fixing it properly means moving element nullability into
`JavaType::List` itself, which changes the type model and every declared
collection type. Out of scope for this pass and worth its own finding; the other
three targets should be measured on the same input first, since I could not
establish from my side whether they accept it.

## Re-verification

- `cargo build --all-features` clean; `cargo fmt --check` clean on both my files.
- `tests/json_schema_conformance_manifest.rs`: **2/2 pass** (real `javac --release 8`
  plus execution across all four targets).
- `tests/json_schema_probe_matrix.rs`: **1/1 pass**.
- `tests/json_schema_corpus_runtime.rs`: **2/2 pass**.
- `tests/generate_java.rs`: 19 pass, only the two golden-snapshot tests red
  pending the consolidated regeneration.
- Fresh sandbox from the working tree, `cargo build-json-examples`, then
  `samples/java && ./gradlew test` and `advanced/samples/java && ./gradlew test`
  both exit 0, and `generate_java` goes 21/21.

**Additional snapshot shifts** beyond those already listed: element-level
constraint checks appear for nullable-element arrays (e.g. `Showcase.slots`),
materialized collections are written with an explicit
`writeStartArray`/`writeString(...)`/`writeEndArray` loop instead of
`defaultSerializeValue`, and a typed-map union branch gains an `implements`
clause.

## New tests (+3, 21 total in `tests/generate_java.rs`)

`java_json_typed_map_union_branch_implements_the_interface`,
`java_json_nullable_element_keeps_the_branch_constraints`,
`java_json_writes_materialized_collection_elements_itself`.

---

# Addendum 3 — `11#11`, the Java half

## Fixed

### `11#11` — `x-java-enum-names` ignored numeric and boolean members — P1
`src/generator/json_schema/java.rs:1821-1856`. `ClosedNameOverrides::get`
destructured `(&self.enum_names, Value::String(key))`, so only a string member
could be renamed. It now derives the map key from the member's canonical wire
spelling through a local `enum_names_lookup_key`, matching the loader's
`pub(crate) enum_names_lookup_key` (`src/parser/json_schema.rs:7197`) and the
Go emitter's copy (`src/generator/json_schema/go.rs:6091`) **character for
character**:

```rust
Value::String(text) => Some(text.clone()),
Value::Bool(flag)   => Some(flag.to_string()),
Value::Number(n)    => Some(n.to_string()),
_ => None,
```

I copied it locally rather than re-export it: the loader's copy is `pub(crate)`
inside `mod json_schema`, which `src/parser/mod.rs` does not re-export, so using
it would mean editing a file I do not own. That is the same call the go-emitter
agent made; if the loader agent decides to share it, both emitters collapse onto
it in one edit and the doc comment says so.

## Verification

Probe renaming an integer, a boolean, a number **and** a string member, compiled
with `javac 21` and executed. Measured before (`/tmp/jbuild2`, pre-fix build) and
after, same schema:

| member | override | before | after |
|---|---|---|---|
| `tier` `enum: [1,2]` | `"1": TIER_BRONZE`, `"2": TIER_SILVER` | `TIER_1`, `TIER_2` | `TIER_BRONZE`, `TIER_SILVER` |
| `toggle` `enum: [true,false]` | `"true": TOGGLE_ON`, `"false": TOGGLE_OFF` | `TOGGLE_TRUE`, `TOGGLE_FALSE` | `TOGGLE_ON`, `TOGGLE_OFF` |
| `scale` `enum: [1.5,2.5]` | `"1.5": SCALE_HALF`, `"2.5": SCALE_TWO_HALF` | `SCALE_1_5`, `SCALE_2_5` | `SCALE_HALF`, `SCALE_TWO_HALF` |
| `mode` `enum: [fast,slow]` | `fast: MODE_FAST` | `MODE_FAST`, `MODE_SLOW` | unchanged |

So before the fix, three of the four members silently ignored their override —
exactly the finding.

**Go cross-check, same schema, same file**: Go emits `TIER_BRONZE`,
`TIER_SILVER`, `TOGGLE_ON`, `TOGGLE_OFF`, `SCALE_HALF`, `SCALE_TWO_HALF`,
`MODE_FAST` — identical to Java for every overridden member, and they reach the
defined type, the parse `switch` and `Validate`. The one difference is the
**un-overridden** member (`slow`): Go derives `EnumnamesModeSlow`, Java derives
`MODE_SLOW`. That is decision **D3** (Java keeps member-derived value-constant
naming), deliberately untouched, and it is not what this override controls.

**Runtime**, bare `ObjectMapper`:
```
{"tier":2,"toggle":false,"scale":1.5,"mode":"fast"}
  -> PARSE-OK  Enumnames{tier=Tier[2], toggle=Toggle[false], scale=Scale[1.5], mode=Mode[fast]}
  -> SER-OK    {"tier":2,"toggle":false,"scale":1.5,"mode":"fast"}
{"tier":9} -> tier: must be one of [1, 2], got 9
```

**The escape hatch is now actually an escape hatch (P15).** The loader's
collision pass keys the map the same way, so the pass and emission agree on
which member a rename applies to — verified by the failure mode, not just the
success one:

```
x-java-enum-names: { "1": SAME, "2": SAME }
-> identifier collision in java output: `Collide2.tier` value constant for 1 and
   `Collide2.tier` value constant for 2 both map to `SAME`; disambiguate with an
   `x-java-name` override (P15 — the generator never auto-mangles)
```

Before the fix the pass could not see numeric overrides at all, so this schema
would have emitted two constants named `SAME`.

Incidental observation while probing: the loader now normalises `enum` members
by numeric value (`[1, 1.0]` and `[1e1, 10]` are both rejected as duplicates), so
`11#7` looks closed on the loader side too — numeric token collisions are largely
prevented upstream now, which makes this override a deliberate-renaming tool more
than a collision rescue. It still has to work, for the reason P15 gives.

## Snapshot impact

**None from this change.** The only `x-java-enum-names` in the corpus is
`showcase.nexusrpc.yaml:97` — `{ active: ACTIVE_JAVA }`, a string key, which
already worked. The Java sample diff is entirely the earlier addenda's shifts.

## Re-verification

- `cargo build --all-features` clean; `rustfmt --check` clean on both my files.
  (`cargo fmt --check` reports a diff in `src/parser/json_schema.rs`, the loader
  agent's file — not mine, left alone.)
- `tests/json_schema_conformance_manifest.rs` 2/2, `json_schema_probe_matrix.rs`
  1/1, `json_schema_corpus_runtime.rs` 2/2.
- `tests/generate_java.rs`: 21 pass in the working tree, the two golden snapshots
  red pending regeneration; **23/23** in a fresh sandbox after
  `cargo build-json-examples`.
- `samples/java && ./gradlew test` and `advanced/samples/java && ./gradlew test`
  both exit 0 against the regenerated output.

## New tests (+2, 23 total in `tests/generate_java.rs`)

`java_json_enum_name_override_reaches_non_string_members` (asserts the six
non-string overrides land and that the old derived fallbacks `TIER_1` /
`TOGGLE_TRUE` / `SCALE_1_5` are gone), and
`java_json_rejects_colliding_numeric_enum_name_overrides` (the P15 pass sees
numeric overrides).

## Acknowledged

Not landing the emitter-side `index`-collision prefix — routed to the loader
agent for `validate_member_scope`, per your note and my own assessment. The
nullable-element-at-depth≥1 representation gap stays deferred.

---

# Addendum 4 — `new:java-rejects-12-digit-fraction`, closed by fixing Java

Supersedes the nine-digit regex cap. The shared-helpers revert to `(\.[0-9]+)?`
had already landed in the tree when I picked this up, so everything below is
measured against the real post-revert generator, not a simulation.

## Fixed

`src/generator/json_schema/java.rs` — new `truncateFraction` in
`TEMPORAL_SUPPORT_BODY`, applied in `parseDateTime`
(`OffsetDateTime.parse(truncateFraction(value).toUpperCase())`) and `parseTime`
(`String upper = truncateFraction(value).toUpperCase();`). `parseDate` is
untouched — a date carries no fraction — and the `duration` grammar is
whole-second.

It walks the digit run after the first `.` and keeps at most nine, splicing the
remainder (offset, `Z`) back on. `java.time`'s ISO parser is the only thing that
needed convincing; the pinned grammar keeps admitting any width.

## Measurement, all four targets, same schema and same inputs

`{ts: {format: date-time}, tod: {format: time}}`, wire
`2021-01-15T12:30:45<f>Z` / `12:30:45<f>Z`. Each target generated from this tree
and **executed** (Java `javac 21` + run; Go `go run`; Python the generated
`TransferTypeConverter` under `samples/python/.venv`; TypeScript the generated
converter under the sample vitest project).

| input | Go | Java (before) | Java (after) | Python | TS `string` (default) | TS `temporal` |
|---|---|---|---|---|---|---|
| `.1` | `.1` | `.1` | `.1` | `.1` | `.1` | `.1` |
| `.123456` | `.123456` | `.123456` | `.123456` | `.123456` | `.123456` | `.123456` |
| `.123456789` | `.123456789` | `.123456789` | `.123456789` | `.123456` | `.123456789` | `.123456789` |
| `.123456789012` | `.123456789` | **REJECT** | `.123456789` | `.123456` | `.123456789012` | **REJECT** |

The pre-fix Java reject is reproduced exactly (a pre-truncation build with the
reverted regex), and it was worse than a reject: for `time` it escaped as a raw

```
[java.time.format.DateTimeParseException] Text '12:30:45.123456789012Z' could not be parsed,
unparsed text found at index 18
```

— an uncaught exception from `parseTime`'s `LocalTime.parse` fallback, i.e. a P11
break as well as an accept-set break. Both are gone.

**Java's re-emitted canonical form matches Go's byte for byte on all four
inputs**, which is the specific confirmation you asked for.

## Two things the measurement contradicts in the target state as stated

You wrote the target as "`.1`, `.123456`, `.123456789` and `.123456789012` all
parse in Go/TS/Python/Java, with each re-emitting at its own capacity (Java/Go/TS
to 9 significant digits, Python to 6)". Java, Go and Python land exactly there.
TypeScript does not, in either mode, and neither deviation is mine to fix:

1. **TS default (`string`) does not truncate — it re-emits all twelve digits
   verbatim.** That mode stores the wire string, so it has no capacity limit at
   all. I read this as *consistent* with P1 exception (b) rather than a defect:
   the accept sets agree (the P1 requirement), and a target with no capacity
   limit loses nothing. But it means the re-emission row is Go 9 / Java 9 /
   Python 6 / TS-string verbatim, not "TS to 9", and any round-trip fixture that
   carries a >9-digit fraction will differ between TS-string and the other three.

2. **TS `--date-time-types=temporal` still rejects**, and raw:
   `REJECT [RangeError] Temporal error: Fractional time exceeds nine digits.`
   `Temporal.ZonedDateTime` caps at nanoseconds like `java.time`, so it needs the
   same parse-and-truncate my fix applies — plus the throw is a bare `RangeError`
   escaping instead of an aggregated `ValidationError`, the same P11 shape Java
   had. **Routing to the ts-emitter agent**: truncate the fraction to nine digits
   before `Temporal.ZonedDateTime.from` / `PlainTime.from`, and make the residual
   failure a `Violation`.

So `new:java-rejects-12-digit-fraction` is closed on the Java side, but the
accept set is not yet uniform across all *modes* until (2) lands. In the default
TS mode it is uniform today.

## Harness entries that move

For the conformance agent:

- `new:java-rejects-12-digit-fraction` → **closed by fixing Java**
  (parse-and-truncate in `TemporalSupport`), not by capping the shared regex.
  Nothing in `samples/conformance/json-schema.json` pins it today, so there is no
  `expected_divergence` block to delete — it needs recording as closed in
  whatever ledger tracks the `new:` findings.
- A new entry is warranted for the TS `temporal`-mode reject in (2). I did not
  add it: `samples/conformance/**` is the conformance agent's, and unlike the two
  I unpinned earlier I am not the one fixing this one, so inventing the pin text
  is not mine to do.
- If a temporal round-trip fixture ever carries a fraction wider than nine
  digits, TS-`string` will re-emit more digits than the other three; that is
  exception (b) territory and would need a declared per-target expectation rather
  than a byte-identity assertion.

I touched no manifest file in this pass.

## Verification

- `cargo build --all-features` clean; `rustfmt --check` clean on both my files.
- `tests/generate_java.rs`: **24/24**, including both golden-snapshot tests — the
  samples on disk already carry `truncateFraction`, so the regeneration you own
  has run since my edit. I did not run `cargo build-json-examples`.
- `json_schema_conformance_manifest` 2/2, `json_schema_probe_matrix` 1/1,
  `json_schema_corpus_runtime` 2/2.
- `samples/java && ./gradlew test` and `advanced/samples/java && ./gradlew test`
  both exit 0.
- `samples/python/tests/test_temporal.py` — **24 passed**, including the
  `(".1234567890", 123456, ".123456")` case whose comment documents the
  accept-and-truncate contract. The cap would have broken it; this does not.
- Probe artifacts I created under `samples/typescript/` to run the TS
  measurement were removed; `git status --untracked-files=all` on that tree is
  clean.

## New test (+1, 24 total in `tests/generate_java.rs`)

`java_json_truncates_an_over_long_fractional_second` — asserts the pinned
fraction stays unbounded (`(\.[0-9]+)?`, so the cap cannot come back through the
regex), that both fraction-bearing parsers truncate before `java.time` sees the
value, and that `parseDate` does not.
