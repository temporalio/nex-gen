# format — gap analysis

## Scope

- Spec: `specs/json-schema/features/format.md` (573 lines) + `specs/json-schema/PRINCIPLES.md`
  (P1 incl. bounded exception (b), P2, P4, P7.1, P10, P11, P12, P16).
- Corpora: `specs/json-schema/corpora/format_conformance/` (124 pairs),
  `format_email/` (56), `format_hostname/` (41), `format_uri/` (72 + two pinned
  `.body` grammars), `format_materialize_clock/` (17 date-time / 4 date / 11 time rows).
- Implementation: `src/json_schema/format.rs`, the load gate
  `src/parser/json_schema.rs::validate_format`, and the four emitters
  `src/generator/json_schema/{go,typescript,python,java}.rs`.
- Tests: inline `#[cfg(test)]` in `src/json_schema/format.rs` (17) and
  `src/parser/json_schema.rs` (~16 format-specific), `tests/generate_go.rs`,
  `tests/json_schema_conformance_manifest.rs`, and the four sample round-trip suites
  (`samples/{go,python,typescript,java}` + `samples/schemas/temporal.yaml`,
  `samples/schemas/showcase.nexusrpc.yaml`).

Note: `tests/generate_format.rs` is about the `--format` (code-formatter) CLI flag,
**not** the `format` keyword. It contributes nothing to this feature's coverage.

All findings below were reproduced by generating code from probe schemas and
compiling/running it (Go `go vet`/`go test`, Python via `samples/python/.venv`).

## Summary

- **The Go emitter does not compile** for an array of, a typed map of, or a
  `propertyNames` constrained by, `format: time` or `format: duration` — the loop
  scaffold is emitted but the body is empty, producing `declared and not used`.
  Every existing test/sample dodges this by using `format: date` (which emits a
  year check) or by adding a sibling `pattern`.
- **`const`/`enum` on a materialized temporal is compared against the *authored*
  literal, never its canonical serialized form.** Go compares native values;
  TS/Python/Java compare wire strings. For `format: duration, const: "PT90M"` the
  wire `"PT1H30M"` is accepted by Go and rejected by the other three, and a model
  parsed from `"PT90M"` **cannot be serialized at all** in TS/Python/Java. The
  shared helper that would fix this (`canonicalize_duration`) exists and is dead code.
- **Go and Java have no serialize-side predicate for `duration`, `time`, or offset
  representability.** A negative `time.Duration`/`java.time.Duration` serializes to
  the ill-formed `"PT-1H-30M"`; a sub-second duration silently truncates to `"PT0S"`;
  a sub-minute UTC offset silently rounds to `"+00:00"` (and is not normalized to
  `Z`); year > 9999 emits a 5-digit year. Python raises a `ValidationError` for all
  of these and TS re-validates the serialized wire. Direct P1 violation.
- **Python's materialized-temporal regexes skip the `\Z` end-anchor rewrite** that
  the string-format path applies, so a trailing `\n` passes the regex and escapes as
  a raw `ValueError`/`KeyError` instead of an aggregated `ValidationError` (P11).
- **`format` inside a `contains` matcher or a `propertyNames` subschema silently
  no-ops for all four temporal formats** in all four languages — the exact
  accepted-but-unenforced footgun the spec's opening paragraph says P10 forbids.
- **Java materializes `time` as `String`**, not `OffsetTime`/`LocalTime` as the spec's
  Materialization table and Validator-mapping row both state.
- **The per-node `string` opt-out (authority model A) is entirely unimplemented** —
  no keyword, no generator-wide mode, no derived accessor. P1's bounded exception (b)
  is conditioned on that opt-out existing, so Python's sub-µs truncation is currently
  unrecoverable.
- **The corpora are executed only by Rust.** No Go/TS/Python/Java test reads any
  `corpora/format_*` file, and `tests/json_schema_conformance_manifest.rs` is a purely
  structural manifest lint (it greps for anchor strings; it never runs a wire value).
  The `format_materialize_clock` corpus note describes a "compare harness" that does
  not exist in this repo; its nanosecond/`-00:00`/trailing-zero rows are never
  round-tripped in any language.
- `ipv6` and `uri-reference` appear in no sample schema, so they have **zero**
  per-language runtime coverage. The `format_uri` corpus is run only as `uri`; the
  separate `pinned_body_uriref.body` grammar has 3 inline assertions and no corpus.
- The **doc-comment mandate** (naming the format and its round-trip loss on the
  materialized field) is unimplemented in all four languages.

## Implementation divergences

### 1. Array / map / propertyNames of `time` or `duration` emits non-compiling Go
**Severity: P0**
- Spec cite: `format.md:198-235` (Materialization type mapping), `format.md:488-500`
  (Interactions → `[[type]]`: "the emitted field type is the native construct").
  Generated code that does not compile is a categorical failure of P2/P1.
- Code cite: `src/generator/json_schema/go.rs:106-111` (`has_string_constraints`
  returns `true` merely because `format.is_some()`) → `go.rs:3332`
  (`schema_requires_go_validation` gate) → `go.rs:3341-3343` emits
  `for i0, v0 := range … { p0 := fmt.Sprintf(...) }` unconditionally, but the
  temporal arm at `go.rs:3450-3471` emits a body **only** when `minLength`/`maxLength`/
  `pattern` is also present (the `DateTime|Date` arm at `go.rs:3419-3427` always emits
  a `Year() < 1` check, which is why `date` happens to work). Same shape for typed
  maps at `go.rs:4119-4122` and for `propertyNames` at `go.rs:843-858` + `go.rs:920-940`.
- What the spec requires: a materialized temporal in a collection position is a
  native-typed element and the generated module must build.
- What the code does: emits `for i0, v0 := range m.Times { p0 := … }` /
  `for k, v := range m.AdditionalProperties { }` / `for k := range m.AdditionalProperties { }`
  with an empty body.
- Concrete failing input:
  ```yaml
  type: object
  properties:
    times: { type: array, items: { type: string, format: time } }
  ```
  → `go vet`: `declared and not used: v0`. Same with `format: duration`, with
  `additionalProperties: { type: string, format: duration }`, and with
  `propertyNames: { type: string, format: date-time }` (`declared and not used: k`).
- Confidence: **verified** (generated + `go vet` + `go test`).

### 2. `const`/`enum` on a materialized temporal is not canonicalized — Go disagrees with TS/Python/Java, and TS/Python/Java models become unserializable
**Severity: P0**
- Spec cite: `format.md:501-505` — "a supplied string literal MUST satisfy the format
  at load; on a **materialized** node it must also be **materializable** … and is
  stored/echoed in its **serialized** form"; `format.md:283` — "`duration`
  canonicalizes value-preserving non-canonical inputs (`PT90M` → `PT1H30M`) …
  byte-identically across languages"; P1 (identical accept/reject in every language).
- Code cite: `src/parser/json_schema.rs:2150-2169` — `check_literal` only calls
  `format::is_valid`; it never rewrites the literal.
  `src/json_schema/format.rs:204-208` `canonicalize_duration` exists and is **dead**
  (no caller outside `format.rs`; verified by `rg`).
  Go compares the native value: `go.rs` closed-value path emits
  `if (*m.B) != ProbeBPt90m` where `ProbeBPt90m = mustParseDuration("PT90M")`.
  TS/Python/Java compare the wire string: `typescript.rs` emits `if (raw.b !== "PT90M")`
  and, on serialize, `if (wire !== "PT90M")`; `python.rs` emits
  `if _format_duration(value.b) not in ("PT90M",)`; `java.rs` emits
  `if (!("PT90M".equals(bWire)))`.
- What the code does, for `{type: string, format: duration, const: "PT90M"}`:
  | wire | Go | TS | Python | Java |
  |---|---|---|---|---|
  | `"PT90M"` parse | accept | accept | accept | accept |
  | `"PT1H30M"` parse | **accept** | **reject** | **reject** | **reject** |
  | serialize that model | emits `"PT1H30M"` | **throws** `must equal "PT90M"` | **throws** | **throws** |
- Concrete failing input: schema above; wire `{"b":"PT1H30M"}` (Go accepts, others
  reject) and wire `{"b":"PT90M"}` (round-trip is impossible in TS/Python/Java).
  The same asymmetry applies to a lowercase `date-time` literal
  (`const: "2021-06-15t12:30:45z"`), which the loader accepts per `format.md:167-171`.
- Confidence: **verified** (Go compiled + `go test` shows both accepted and re-emitted
  as `PT1H30M`; TS/Python/Java generated source inspected line-by-line).

### 3. Go and Java serialize native temporal values the wire grammar cannot spell — no predicate at all
**Severity: P0**
- Spec cite: `format.md:405-424` (Serialize-side P12): the serializer "still enforces
  the shared calendar floor" and "Other native values are valid by construction
  (`time.Duration` always represents a supported time-only duration, for example)".
  That parenthetical is **factually false** — `time.Duration` is signed and
  nanosecond-resolution — and Go/Java rely on it. P1 forbids excepting the
  accepted/rejected value set.
- Code cite: `src/generator/json_schema/go.rs:2656-2718` — the temporal branch of
  `render_validate` emits **only** a `Year() < 1` check, and only for `DateTime|Date`;
  `duration` and `time` get nothing. `src/generator/json_schema/java.rs:443-447` —
  identical (`getYear() < 1` for Date/DateTime only). Serializers:
  `go.rs:4863-4879` `formatDuration`, `go.rs:4738-4748` `temporalOffset`,
  `java.rs` `TEMPORAL_SUPPORT_BODY` `formatDuration` / `offset`.
  Python by contrast has `_check_date_time` / `_check_time` / `_check_duration`
  (`python.rs:2231-2244`), and TS re-runs the pinned regex over the serialized wire
  (`typescript.rs:1856-1885`).
- Observed Go output (`samples/go/temporal`, real run):
  | in-memory value | Go wire | Python |
  |---|---|---|
  | `Timeout = -90*time.Minute` | `"PT-1H-30M"` (its own parser rejects it) | `ValidationError: a duration cannot be negative` |
  | `Timeout = 500*time.Millisecond` | `"PT0S"` (silent loss) | `ValidationError: … cannot carry a fraction of a second` |
  | `CreatedAt` in `FixedZone("",30)` | `"…T12:30:45+00:00"` (offset lost **and** not normalized to `Z`) | `ValidationError: the UTC offset 0:00:30 is not a whole number of minutes` |
  | `CreatedAt` year 10000 | `"10000-01-01T00:00:00Z"` | not representable |
- Java additionally writes its `time` field (a `String`) **verbatim with no check** —
  see the generated `Temporal.Serializer`, which validates only the four
  `getYear() < 1` cases and then does `gen.writeStringField("alarm", value.alarm)`.
- Confidence: **verified** for Go (executed); **verified by code reading** for Java.

### 4. Python's materialized-temporal regexes are missing the `\Z` end-anchor rewrite
**Severity: P0**
- Spec cite: `format.md:342-351` — "the pinned regex compiled once ([[pattern]]'s
  machinery — the ASCII-class rule and the per-target end-anchor `$`→`\Z`/`\z`
  normalization apply)"; `format.md:369-380` (the temporal patterns are pinned the
  same way); P1 + P11.
- Code cite: `src/generator/json_schema/python.rs:965-985` `render_temporal_helpers`
  interpolates `TemporalKind::*.pattern()` **verbatim**, unlike
  `python.rs:1643` / `python.rs:1817` / `python.rs:5028`, which all call
  `pattern::rewrite_end_anchor(&check.pattern, r"\Z")` for the string formats.
  Verified in the checked-in sample: `samples/python/temporal/_definitions.py:214`
  is `re.compile(r"^[0-9]{4}-…-(0[1-9]|[12][0-9]|3[01])$")` while
  `samples/python/showcase/models.py:37` correctly ends in `\\Z`.
- What the code does: Python's `$` also matches immediately before a trailing
  newline, so `"PT1H\n"` / `"2021-06-15T12:30:45Z\n"` pass the regex; the value then
  reaches `fromisoformat` / the duration unit map and escapes as a raw
  `ValueError: Invalid isoformat string` or `KeyError: '\n'`. Go, TS and Java all
  reject the same value cleanly as `must be a valid …`.
- Concrete failing input:
  `{"createdAt":"2021-06-15T12:30:45Z","birthday":"2000-01-01","alarm":"09:00:00","timeout":"PT1H\n"}`
  → Python raises an uncaught `KeyError('\n')` (not a `ValidationError`, no path, no
  aggregation); Go/TS/Java raise one aggregated `timeout: must be a valid duration`.
- Confidence: **verified** (executed against `samples/python`).

### 5. `format` in a `contains` matcher or a `propertyNames` subschema silently no-ops for the temporal formats
**Severity: P0**
- Spec cite: `format.md:8-19` ("an accepted-but-unenforced `format` is exactly the
  'looks constrained, silently isn't' footgun **P10** forbids"), P7.1.
- Code cite: the load gate accepts it — `src/parser/json_schema.rs:2618-2632`
  explicitly infers `type: string` for a `contains` matcher carrying `format`, then
  runs `validate_format`. But every emitter's matcher/key path goes through
  `format::check_for`, which returns `None` for the four temporal names
  (`src/json_schema/format.rs:322-366`): `go.rs:580-590` (contains matcher),
  `go.rs:920-940` (propertyNames), `python.rs:1640-1642` and
  `typescript.rs:441-443` (early `return` on `check_for(...) == None`).
- What the code does, for
  `{type: array, items: {type: string}, contains: {type: string, format: date-time}}`:
  Go emits `if true { matchCount++ }`; TS emits
  `raw.arr.filter((element) => typeof element === 'string').length`. The assertion is
  gone. `propertyNames: {type: string, format: date-time}` emits an empty
  `for k := range … { }` in Go (which also fails to compile, see #1) and nothing at
  all in TS/Python/Java. `format: uuid` in the same positions works correctly.
- Confidence: **verified** (generated all three; inspected output).

### 6. Java materializes `time` as `String`, not `OffsetTime`/`LocalTime`
**Severity: P1**
- Spec cite: `format.md:224` (table: Java `time` → `OffsetTime` / `LocalTime`),
  `format.md:275-282` ("the offset-bearing types are used only when an offset is
  present (Java falls back to `LocalTime`…)"), `format.md:387` ("`OffsetTime.parse`
  **retaining the offset** (or `LocalTime.parse` when the wire omits it)").
- Code cite: `src/generator/json_schema/java.rs:1235-1242`
  (`TemporalKind::Time => "String"`), `java.rs:1256-1263`
  (`java_temporal_format_fn(Time) => None`), and the runtime `parseTime` in
  `TEMPORAL_SUPPORT_BODY`, which parses to `OffsetTime`/`LocalTime` only to
  immediately re-render a canonical `String`. Visible in the checked-in sample:
  `samples/java/.../Temporal.java` has `private final String alarm;`.
- What the code does: correct on the wire, but the field is not the idiomatic typed
  construct the spec promises, and — because the field is a plain `String` — it
  receives **no serialize-side validation** (see #3).
- Confidence: **verified**.

### 7. Java renders a temporal `const`/`default` literal with the raw authored string → `DateTimeParseException`
**Severity: P1**
- Spec cite: `format.md:167-171` (RFC 3339 case-insensitive `T`/`Z` accepted;
  "Materialized nodes **uppercase on the parse path** before the native parse"),
  `format.md:501-505` (the literal is echoed in its serialized form).
- Code cite: `src/generator/json_schema/java.rs:5140-5154` — the temporal literal
  arm emits `OffsetDateTime.parse(<raw literal>)` / `LocalDate.parse(...)` /
  `Duration.parse(...)` with no `.toUpperCase()` and no canonicalization. Go's
  equivalent goes through `mustParseDateTime`, which uppercases (`go.rs:4881-4886`);
  TS goes through `parseTemporalDateTime`; Python through `_parse_date_time`.
- Concrete failing input:
  `{type: string, format: date-time, default: "2021-06-15t12:30:45z"}` (accepted by
  the loader) → Java emits
  `return a != null ? a : OffsetDateTime.parse("2021-06-15t12:30:45z");`
  which throws `java.time.format.DateTimeParseException` the first time
  `getAOrDefault()` is called. Go/TS/Python all resolve the same default fine.
- Confidence: **verified** (generated `Probe.java:55`).

### 8. The `string` opt-out (authority model A) is entirely unimplemented
**Severity: P1**
- Spec cite: `format.md:323-332` (per-node opt-out, generator-wide mode, derived
  accessor `asDateTime()` / `AsOffsetDateTime()` / `.as_datetime()`, wider grammar
  with `:60` and calendar durations), `format.md:378-380`, `format.md:440`
  (Property-testing matrix: "String opt-out keeps the wider grammar"), and
  **PRINCIPLES P1 exception (b)**, which makes the exception conditional on the loss
  being "recoverable through a per-field `string` opt-out".
- Code cite: no keyword, flag, or mode exists. `rg` for `opt-out|opt_out|as_datetime|
  AsOffsetDateTime|asDateTime` over `src/` returns nothing; the only `x-*` extensions
  the loader knows are the four `x-<lang>-name`, `x-go-enum-names` and
  `x-<lang>-const-name` families (`src/parser/json_schema.rs:10874-10877`).
  `src/json_schema/format.rs` has only one grammar per kind
  (`materialized_pattern`, lines 95-108) — the wider `|60` / `PnYnMnDTnHnMnS`
  alternative is not encoded anywhere.
- Consequence: the Python sub-microsecond truncation and the legacy TS `date`
  UTC-instant fold have no escape hatch, so P1's exception (b) is not satisfied on
  its own terms.
- Confidence: **verified** (exhaustive search).

### 9. No doc comment names the format or its round-trip behavior on a materialized field
**Severity: P2**
- Spec cite: `format.md:334-340` — "The materialized field's doc comment names the
  format and its round-trip behavior (`// format: date-time — offset & precision
  preserved; round-trip may lose precision beyond this type's resolution`) so any loss
  is visible in the generated source (**P2**). The only lossy TS mode is legacy
  `--date-time-types=date`, whose comment names the UTC-instant fold…"
- Code cite: `rg "round-trip may lose|precision preserved|offset & precision"` over
  `src/generator/json_schema/*.rs` returns nothing. Generated Go for an undocumented
  temporal property is `// A corresponds to the "a" JSON property.`; documented ones
  carry only the schema's own `description` (see `samples/go/temporal/temporal.go`).
  Go's P2 §1 name-led fallback is satisfied; the format-specific sentence is not.
- Confidence: **verified**.

### 10. The unknown-format fix-it omits the four supported temporal formats
**Severity: P2**
- Spec cite: `format.md:116-119` — "reject with a fix-it listing the supported names".
- Code cite: `src/parser/json_schema.rs:2140-2145` joins
  `format::SUPPORTED_FORMATS`, which is declared as the **seven string formats only**
  (`src/json_schema/format.rs:28-36`); `TEMPORAL_FORMATS`
  (`format.rs:43`) is never spliced in.
- What the code does: `format: datetime` (the typo the spec's own matrix calls out at
  `format.md:448`) is rejected with "supported formats are uuid, ipv4, ipv6, hostname,
  email, uri, uri-reference" — the fix-it hides the very name the user meant.
- Confidence: **verified** (and the existing test `rejects_typo_format_as_unknown`,
  `src/parser/json_schema.rs:8277`, asserts nothing about the list).

### 11. `format` regexes are recompiled per evaluation in two nested positions
**Severity: P2**
- Spec cite: `format.md:400-402` — "the pinned pattern is a package-level compiled
  constant; the load gate proves it compiles, so the emitted `MustCompile` /
  `Pattern.compile` is unconditional."
- Code cite: `src/generator/json_schema/go.rs:580-590` emits
  `regexp.MustCompile("<pinned>")` **inside** the `contains` element loop;
  `src/generator/json_schema/java.rs:576-582`
  (`render_java_inline_string_checks`) emits `java.util.regex.Pattern.compile(...)`
  at each nested call site. The property-position paths correctly hoist
  (`go_format_var_name`, `java_format_field_name`).
- Confidence: **verified** (see generated `p6-go/p6go.go:28`).

### 12. The `$vocabulary: {format-assertion: true}` IDE-support schema does not exist
**Severity: P2**
- Spec cite: `format.md:78-81` — "the `*.nexusrpc.yaml` IDE-support schema declares
  the `format-assertion` vocabulary in its `$vocabulary` (mapped to `true`)".
- Code cite: no meta-schema artifact exists in the repo (`find` for `*.json`
  matching meta/vocab/nexusrpc returns nothing); the only `$vocabulary` handling is
  the loader **rejecting** it in a type schema
  (`src/parser/json_schema.rs:1526-1528`).
- Confidence: **verified**.

### 13. Stale doc comments claim the temporal formats are rejected at load
**Severity: P2**
- Code cite: `src/json_schema/format.rs:11-17` — "The temporal formats (`date-time`,
  `date`, `time`, `duration`) are recognized but **rejected at load** as 'not yet
  supported (temporal, pending)' — materialization is a separate follow-up task".
  `src/parser/json_schema.rs:2100-2104` — "Rejects (P7 / P7.1): … the temporal
  formats (materialization pending)". Both are false; materialization shipped.
- Related stale naming: the flag is `--date-time-types` (`src/main.rs:86`) but
  `samples/schemas/temporal.yaml:6`, `samples/typescript/tests/json-schema-temporal.test.ts:23,82,112`
  and `xtask/src/build_examples.rs` comments still say `--js-temporal-repr`.
- Confidence: **verified**.

### 14. Go's `enum`-on-materialized-temporal reason prints the Go value, not the wire form; the year-floor reason names neither format nor value
**Severity: P2**
- Spec cite: `format.md:396-398` — "The `Violation` `reason` names the **format and
  the offending value** (`must be a valid date-time, got "…"`)"; the repo convention
  is that a reason names the concrete bound and the offending value.
- Code cite: `src/generator/json_schema/go.rs:2680-2682` emits
  `Violation{"c", "year must be >= 1"}` (no format name, no value), whereas Java
  (`java.rs:446`) and Python (`_temporal_reason`) both name both. The enum message
  emits `fmt.Sprintf("must be one of […], got %q", (*m.C))` over a `time.Time`,
  which prints `"2021-05-05 00:00:00 +0000 UTC"` instead of the wire form
  `"2021-05-05"`.
- Confidence: **verified** (executed).

### 15. The spec cites corpora that do not exist
**Severity: P2**
- Spec cite: `format.md:456-465` lists "duration 68" among the runtime fixtures and
  refers to "the `format_materialize_clock/` and materialize-duration corpora".
- Reality: `specs/json-schema/corpora/` contains only `format_conformance`,
  `format_email`, `format_hostname`, `format_uri`, `format_materialize_clock`,
  `pattern_conformance`. `format_conformance` covers six formats
  (date 25 / time 22 / date-time 22 / ipv6 20 / uuid 18 / ipv4 17) — **no `duration`
  rows at all**; `format_materialize_clock` has no `duration` section.
- Confidence: **verified**.

## Per-format coverage matrix

"Validated" = the pinned check actually runs in that language's generated code for an
ordinary property position. "Tested" = a test asserts accept **and** reject.

| format | supported? | Go | TS | Py | Java | native type | tested? |
|---|---|---|---|---|---|---|---|
| `uuid` | asserted | ✓ | ✓ | ✓ | ✓ | `string` | Rust inline + 18 corpus pairs + 4-language runtime (showcase `requestId`) |
| `ipv4` | asserted | ✓ | ✓ | ✓ | ✓ | `string` | Rust + 17 corpus pairs + 4-language runtime (`gateway`) |
| `ipv6` | asserted | ✓ | ✓ | ✓ | ✓ | `string` | Rust only (20 corpus pairs) — **no sample field, no per-language runtime test** |
| `hostname` | asserted (≤253) | ✓ | ✓ | ✓ | ✓ | `string` | Rust + 41-case corpus + 4-language runtime (`host`) |
| `email` | asserted (≤254, guard first) | ✓ | ✓ | ✓ | ✓ | `string` | Rust + 56-pair corpus + 4-language runtime (`contactEmail`) |
| `uri` | asserted | ✓ | ✓ | ✓ | ✓ | `string` | Rust + 72-pair corpus + 4-language runtime (`homepage`, `links[]`) |
| `uri-reference` | asserted | ✓ | ✓ | ✓ | ✓ | `string` | **3 inline Rust assertions only** — separate pinned grammar, no corpus, no sample |
| `date-time` | materialized | ✓ | ✓ | ✓ | ✓ | `time.Time` / `string`\|`Date`\|`ZonedDateTime` / `datetime` / `OffsetDateTime` | 22 corpus pairs (Rust) + 4-language round-trip; **ns precision & `-00:00` untested** |
| `date` | materialized | ✓ | ✓ | ✓ | ✓ | `time.Time`† / `string`\|`PlainDate` / `date` / `LocalDate` | 25 corpus pairs (Rust) + 4-language round-trip + arrays/maps (showcase) |
| `time` | materialized | ✓ | ✓ | ✓ | ✓ (as **`String`**, spec says `OffsetTime`/`LocalTime`) | `time.Time`† / `string` / `time` / `String` | 22 corpus pairs (Rust) + 4-language round-trip; **arrays/maps break Go build** |
| `duration` | materialized (time-only) | ✓ parse / **✗ serialize** | ✓ | ✓ | ✓ parse / **✗ serialize** | `time.Duration` / `string`\|`Duration` / `timedelta` / `Duration` | **no corpus**; 4-language round-trip; **arrays/maps break Go build** |
| `idn-email`, `idn-hostname`, `iri`, `iri-reference`, `uri-template`, `json-pointer`, `relative-json-pointer`, `regex` | deferred → load reject | n/a | n/a | n/a | n/a | n/a | one test (`iri`) covers the whole list |
| OAS `int32`/`int64`/`float`/`double`/`password`/`byte`/`binary` | unknown → load reject | n/a | n/a | n/a | n/a | n/a | **untested** (spec calls them out at `format.md:527`) |

† Go has no date-only / time-of-day type; the serializer ignores the phantom
component. For `time`, Go uses an **undocumented sentinel year 1** to mean
"offset-less" (`go.rs:4789`, `parseTime`/`formatTime`) — a user-constructed
`time.Time` from `time.Now()` therefore silently gains an offset on the wire.

## Testing gaps

### 1. No language test executes any conformance corpus
**Severity: P0 (test-only, but it is the sole proof of P1 for `format`)**
- Untested: that Go's RE2, JS's `/u` RegExp, Python's `re` (with the `\Z` rewrite) and
  `java.util.regex` (with the `\z` rewrite) agree value-for-value on the pinned
  patterns. Divergence #4 is exactly the class of bug this would catch.
- Spec line: `format.md:29-33` ("Every rule below was verified value-for-value across
  all four runtime targets **plus** the Rust gate … by conformance corpora"),
  `format.md:456-465`.
- Where: a corpus-driven test per language, e.g.
  `samples/go/tests/json_schema_format_corpus_test.go`,
  `samples/python/tests/test_format_corpus.py`,
  `samples/typescript/tests/json-schema-format-corpus.test.ts`,
  `samples/java/.../JsonSchemaFormatCorpusTest.java`, each reading
  `specs/json-schema/corpora/format_*/corpus.json` and driving a generated model
  whose field carries each format.
- Suggested case: for every pair, assert `expect_valid == parses`.

### 2. `format_materialize_clock` is never round-tripped
**Severity: P0 (test-only)**
- Untested: the corpus's own stated contract — "the compare harness checks the
  equally-capable materializing set (go, java, py, js-string, js-temporal) emits
  byte-identical output for each id". The only consumer is
  `src/json_schema/format.rs:562-585`, which asserts *validity*, never the
  re-serialized bytes.
- Spec line: `format.md:461-465`, `format.md:252-270`.
- Where: the same four per-language corpus runners, plus a Rust cross-language
  compare in `xtask`.
- Suggested case: `dt-frac-9-offset` (`2021-06-15T12:30:45.123456789+02:00`) must
  re-emit verbatim in Go / Java / TS `string` / TS `temporal`, as
  `…123456+02:00` in Python, and as `…10:30:45.123Z` in TS `date`.

### 3. Nanosecond precision and `-00:00` are absent from every wire fixture
**Severity: P1**
- Untested: the headline P1-exception-(b) claim. `samples/wire/json_schema/temporal/*`
  contains only `.123456` (microsecond) and `+00:00`; the corpus rows `dt-frac-9`,
  `dt-frac-9-offset`, `dt-offset-neg0000`, `dt-frac-trailzero` and `t-neg0000` are
  never round-tripped. (`-00:00` → `Z` does work — I verified it manually in Go and
  Python — but nothing asserts it.)
- Spec line: `format.md:263-267`, `format.md:237-240`.
- Where: a new `samples/wire/json_schema/temporal/temporal-precision.json` consumed
  by all four suites, plus a new conformance-manifest case.
- Suggested case: `{"createdAt":"2021-06-15T12:30:45.123456789+02:00", …, "reminder":"12:30:45.120-00:00"}`
  → Go/Java/TS keep `…789+02:00`; Python emits `…123456+02:00`; `reminder` re-emits
  as `12:30:45.12Z`.

### 4. No test builds a collection of `time` or `duration`
**Severity: P0**
- Untested: an array item / typed-map value / `propertyNames` carrying `time` or
  `duration`. `tests/generate_go.rs:2441`
  (`go_json_temporal_constraints_cover_original_and_canonical_wire_values`) uses
  `format: date` **with a sibling `pattern`** in all three positions, which is exactly
  the combination that hides divergence #1; `samples/schemas/showcase.nexusrpc.yaml`
  uses `format: date` only (`dates`, `DateIndex`).
- Spec line: `format.md:220-226` (the type table applies at every position),
  Interactions → `[[type]]`.
- Where: extend `tests/generate_go.rs:2441` with unconstrained `time`/`duration`
  members, and add them to `samples/schemas/temporal.yaml` so all four suites cover them.
- Suggested case: `{ durs: {type: array, items: {type: string, format: duration}},
  byName: {type: object, additionalProperties: {type: string, format: time}} }`
  → must compile and round-trip in all four languages.

### 5. No test covers `const`/`enum`/`default` on a materialized temporal beyond load acceptance
**Severity: P0**
- Untested: the runtime semantics. `accepts_materializable_temporal_const_literals`
  (`src/parser/json_schema.rs:8318`) only asserts the schema loads. Nothing generates
  or runs code for a temporal `const`, so divergence #2 and #7 are invisible.
- Spec line: `format.md:501-505`.
- Where: a per-language runtime test plus a conformance-manifest case.
- Suggested case: `{type: string, format: duration, const: "PT90M"}` — all four
  languages must accept both `"PT90M"` and `"PT1H30M"` on the wire and re-emit
  `"PT1H30M"`; and `{format: date-time, default: "2021-06-15t12:30:45z"}` must resolve
  its default to `2021-06-15T12:30:45Z` in all four.

### 6. No serialize-side test for a native temporal value the wire cannot spell, outside Python
**Severity: P0**
- Untested: negative / sub-second / over-cap `Duration`, sub-minute offsets, and
  year > 9999 on the serialize path in Go, Java and TS.
  `samples/python/tests/test_temporal.py:308`
  (`test_serialize_rejects_temporal_values_the_wire_form_cannot_carry`) is the *only*
  such test in the repo, and no manifest case mirrors it to the other three.
- Spec line: `format.md:405-424`.
- Where: `samples/go/tests/json_schema_temporal_test.go`,
  `samples/java/.../JsonSchemaTemporalRoundTripTest.java`,
  `samples/typescript/tests/json-schema-temporal.test.ts`, plus a manifest
  `serialize_failures` case (the manifest supports `native_value` already).
- Suggested case: set `Timeout = -90m` / `500ms`, `CreatedAt` in a 30-second
  fixed zone → one aggregated violation per field in every language.

### 7. `ipv6` and `uri-reference` have no per-language runtime coverage
**Severity: P1**
- Untested: whether the 9-alternative spliced `ipv6` grammar and the
  `pinned_body_uriref.body` grammar survive the per-target end-anchor rewrite and the
  four regex engines. The `format_uri` corpus is driven only as `uri`
  (`src/json_schema/format.rs:628-639`).
- Spec line: `format.md:86-92`, `format.md:355-360`.
- Where: add `ipv6` and `uri-reference` properties to
  `samples/schemas/showcase.nexusrpc.yaml`; add a `uri-reference` pass over the
  `format_uri` corpus in `format.rs` (relative pairs `expect` `true` for
  `uri-reference` even where `uri` rejects them).
- Suggested case: `http://[1::2::3]` rejected; `//example.com/x` and `../a?b#c`
  accepted as `uri-reference`, rejected as `uri`.

### 8. The `format-assertion` opt-in and the string opt-out have no tests because they have no implementation
**Severity: P1**
- Untested: everything in `format.md:78-81` and `format.md:323-332`, including the
  wider opt-out grammar row in the Property-testing matrix (`format.md:440`:
  "opt-out `date-time` accepts `…T23:59:60Z`; opt-out `duration` accepts `P1Y`").
  The two `:60`-accepting rows in `format_conformance` (`time-second-60-leap`,
  `dt-leap-second`) are force-flipped to `false` by
  `src/json_schema/format.rs:511-513`, so nothing exercises the wider grammar.
- Where: with the feature; until then the spec sections should be marked deferred.

### 9. Load-gate matrix rows without a test
**Severity: P2**
- `format: true` / `format: ["uuid"]` (only `format: 5` is tested,
  `src/parser/json_schema.rs:8264`).
- `{type: "boolean", format: "date"}` (only `type: integer` is tested, line 8258).
- Each of the 8 deferred names individually (only `iri`, line 8283).
- The OAS-specific names `int32`/`int64`/`float`/`double`/`password`/`byte`/`binary`
  rejecting as unknown (`format.md:527`).
- That the unknown-format fix-it lists the temporal names (divergence #10).
- `format: duration, const: "P1YT1H"` mixed calendar+time form (covered in
  `format.rs` but not in the loader tests).
- Where: `src/parser/json_schema.rs` inline tests next to lines 8241-8390.

### 10. `format` on a `contains` matcher / `propertyNames` is untested for every format
**Severity: P1**
- Untested: even the working `uuid` case has no test; the silently-ignored temporal
  case (divergence #5) has none either.
- Spec line: `format.md:8-19` (P10), `format.md:342-351`.
- Where: `src/parser/json_schema.rs` load tests + `tests/generate_go.rs`.
- Suggested case: `contains: {type: string, format: date-time}` must either be a load
  reject or emit a real predicate; today it emits `if true`.

### 11. Python-vs-others reason-shape divergence at the year floor is asserted only in Python
**Severity: P2**
- `samples/python/tests/test_temporal.py:173` pins Python's
  `…: year 0000 is not representable (datetime.MINYEAR is 1)`; Go's is
  `year must be >= 1`, Java's `…: year must be >= 0001`, TS's has no suffix. P11 says
  the text is not contractual, so this is polish — but the `year-zero-rejection`
  manifest case should at least pin that all four report the violation **at the same
  path**, which nothing verifies mechanically.

## Combination gaps

| Feature A × Feature B | spec says | tested? | risk |
|---|---|---|---|
| `format` × `pattern` (string format) | both apply, aggregated independently (`format.md:437-438`, 490-493) | ✓ load (`accepts_constrained_non_object_union_branches`) + generated Go/TS/Py/Java inspected | low |
| `format` × `pattern` (materialized temporal) | pattern constrains the **wire**, re-run before emit (`format.md:417-421`) | partial — `tests/generate_go.rs:2441` covers Go with `format: date`; no TS/Py/Java test | medium |
| `format` × `minLength`/`maxLength` (materialized) | independent; re-run over the re-serialized wire (`format.md:506-509`, 417-421) | ✗ no test in any language (I verified all four emitters do it correctly) | low (works) |
| `format` × `const` (materialized temporal) | literal must be materializable and **stored/echoed in serialized form** (`format.md:501-505`) | ✗ load-only | **P0 — divergence #2** |
| `format` × `enum` (materialized temporal) | same | ✗ load-only | **P0 — divergence #2**; Go compares native values, others compare wire strings |
| `format` × `default` (materialized temporal) | same | ✗ load-only | **P1 — divergence #7** (Java throws on a lowercase literal) |
| `format` × `const`/`enum`/`default` (string format) | literal validated at load | ✓ (`rejects_const/default/enum_violating_format`) | low |
| `format` × nullability (`oneOf[T, null]`) | orthogonal; `null` skipped, not materialized (`format.md:510-511`) | ✓ 4-language (`deletedAt`, `archivedOn` in `temporal.yaml`) — but only `date-time`/`date` | low; `time`/`duration` nullable untested |
| `format` × `oneOf` sum-type branch, asserted format | rides along, branch stays `string` (`format.md:512-519`) | ✓ load (`accepts_constrained_non_object_union_branches`, uuid) | medium — no runtime test that the branch check actually fires |
| `format` × `oneOf` sum-type branch, materialized temporal | **deferred / reject** (`format.md:514-518`) | ✓ (`rejects_materialized_temporal_format_on_a_sum_type_branch`) | low |
| `format` × array `items` | element keeps its assertion | ✓ for `uri` and `date`; ✗ for `time`/`duration` | **P0 — divergence #1** |
| `format` × typed map (`additionalProperties`) | same | ✓ for `date` (`DateIndex`); ✗ for `time`/`duration` | **P0 — divergence #1** |
| `format` × `propertyNames` | not addressed by the spec; P10 forbids a silent no-op | ✗ | **P0 — divergence #5** (silent no-op + Go build break) |
| `format` × `contains` | not addressed; P10 applies | ✗ | **P0 — divergence #5** |
| `format` × `required` | orthogonal (`format.md:520`) | ✓ implicitly (`temporal.yaml` required members) | low |
| `format` × `allOf` merge | merged schemas must agree | ✓ (`rejects_all_of_differing_format`, `all_of_and_ref_siblings_reject_malformed_keywords_before_merge`) | low |
| `format` × `--date-time-types` × arrays/maps | flag applies to every materialized TS field | partial — only scalar properties in `temporal.yaml`; `temporal`-mode array of `Temporal.Duration` untested | medium |
| `format` × `--date-time-types=temporal` × `const` | const compares the serialized wire | ✗ | medium — TS `temporal` mode stores a `Temporal.Duration`; the const check runs on `serializeTemporalDuration(value)`, unverified |
| `format` × unknown-key preservation (P13) | orthogonal | ✓ (open models in `temporal.yaml`) | low |
| `format` × `contentEncoding` | mutually exclusive in practice (both replace the `string` field type) | ✗ no test that both on one node rejects | low-medium — `validate_format` and `validate_content_encoding` are independent; unverified whether a node with both is caught |

## Verified-good

- The load gate (`src/parser/json_schema.rs:2105-2172`) correctly rejects a non-string
  `format`, a `format` on a non-`string` node, deferred names, and unknown names, and
  validates `const`/`default`/`enum` string literals against the format — including
  the materialized narrowings (`:60`, calendar durations, missing offset, year zero).
  All of these have tests at `src/parser/json_schema.rs:8241-8390`.
- The pinned patterns all pass the `[[pattern]]` RE2 gate
  (`format.rs:409-417`) and are shared verbatim between the loader oracle and all four
  emitters — one source of truth, no per-language regex authoring.
- The email/hostname length guard runs **before** the regex in all four emitters
  (short-circuit `||` / `or`): `go.rs:400-405`, `typescript.rs:446-448`,
  `python.rs:1644-1648`, `java.rs:517-527`. Code-point counting is used everywhere
  (`utf8.RuneCountInString`, `[...s].length`, `len()`, `codePointCount`).
- The `\Z`/`\z` end-anchor rewrite is correctly applied to **string** formats in
  Python and Java (verified in the checked-in samples).
- The four corpora that are executed agree with the shared predicate, pair for pair
  (`format.rs:589-652`), and the counts match the spec exactly (124/56/41/72).
- `+00:00` and `-00:00` → `Z`, lowercase `t`/`z` → uppercase, and `PT90M` → `PT1H30M`
  all round-trip correctly in Go, TS (all three reprs) and Python — I executed the
  `-00:00` case in Go and Python.
- `--date-time-types` maps exactly to the spec's table
  (`typescript.rs:110-120`), including `time` staying a `string` in `temporal` mode,
  and it is rejected on the non-TS subcommands (`tests/generate_format.rs:131`).
- The Go/Java/Python/TS duration overflow caps are all `i64::MAX / 1e9` seconds and
  agree in verdict across every digit width I traced (`format.rs:131-196`,
  `go.rs:4827`, `java.rs` `MAX_DURATION_SECONDS`, `python.rs` `_TEMPORAL_MAX_DURATION_SECONDS`).
- Materialized `date-time`/`date` carry a serialize-side year-floor check in Go
  (`go.rs:2670-2685`), Java (`java.rs:443-447`), TS (`validTemporalCalendar` inside
  `validateTemporal*`) and Python — the one serialize-side rule the spec spells out
  is implemented in all four.
- A materialized temporal on a `oneOf` sum-type branch is rejected at load, per
  `format.md:514-518` (`rejects_materialized_temporal_format_on_a_sum_type_branch`).
- `minLength`/`maxLength` on a materialized temporal correctly re-run over the
  **re-serialized** wire before emit in all four languages (verified in generated
  source for Go, TS, Python and Java).
