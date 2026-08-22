# Object constraints (patternProperties / propertyNames / min-maxProperties / dependent* / unevaluatedProperties) — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/patternProperties.md` — temporarily unsupported; any occurrence must load-reject with a "not yet supported" fix-it.
- `specs/json-schema/features/propertyNames.md` — partial support: map-shaped hosts only, string subschema with only `minLength`/`maxLength`/`pattern`/`enum`/asserted `format`, checked over keys in both directions.
- `specs/json-schema/features/minProperties.md` — runtime floor over the distinct wire member count, taken as one number before default population; unsatisfiability rejects at load.
- `specs/json-schema/features/maxProperties.md` — mirror ceiling, same counting rule; `< minProperties` and `<` required count reject at load.
- `specs/json-schema/features/dependentRequired.md` — runtime cross-field presence assertion; trigger/dependents must be declared, neither may be in `required`.
- `specs/json-schema/features/dependentSchemas.md` — hard P6 reject at load with a fix-it pointing at `dependentRequired` / unconditional `properties`+`required`.
- `specs/json-schema/features/unevaluatedProperties.md` — hard P6 reject at load with a fix-it pointing at `additionalProperties`.

## Summary

- The three reject-only keywords (`patternProperties`, `dependentSchemas`, `unevaluatedProperties`) are genuinely rejected in **every** position I could construct — top-level `$defs`, nested `$defs`, dead `$defs`, array `items`, `contains`, `allOf` branches, `oneOf` branches, `additionalProperties` value schemas, and service operation input/output. Verified empirically against the built CLI. No implementation gap; only test-position coverage is thin (one position tested).
- **P0:** `propertyNames: {type: string, format: <date-time|date|time|duration>}` loads clean and then breaks emission four different ways: Go emits `for k := range … { }` which **does not compile** (`declared and not used: k`), Python emits `for key in raw:` with an empty body which is an **`IndentationError` at import**, and TypeScript + Java **silently drop the key constraint** entirely. That is both a broken-build bug and a cross-language accepted-value-set disagreement (P1/P10).
- **P1:** `minProperties: 1.0` / `maxProperties: 3.0` — the integral spellings `minProperties.md`/`maxProperties.md` explicitly say must be honored — pass the loader and then fail in every backend with an internal, unlocated diagnostic (`<json-generator>: … invalid type: floating point 1.0, expected usize`).
- **P1:** the `propertyNames` subschema keyword allowlist only walks `Schema::extra`, so the *typed* struct fields (`properties`, `required`, `items`, `oneOf`, `additionalProperties`) sail through and are silently ignored — e.g. `propertyNames: {type: string, maxLength: 8, additionalProperties: false}` is accepted, contradicting the spec's "carrying non-string assertions → reject".
- **P2:** Go's `Validate()` on a *declared-property* model omits `minProperties`/`maxProperties`/`dependentRequired` entirely (they live only in `UnmarshalJSON`/`MarshalJSON`), while Go's *map-shaped* `Validate()` includes them. The emitted doc comment says "checks m against every constraint". The spec's Go strategy explicitly routes the count through the shared `Validate`.
- **P2:** Java's `propertyNames` violations for `pattern`/`enum`/`format` lack the spec-mandated `invalid property name "<key>": ` prefix (only the length checks have it); Go and Python's `propertyNames` `enum` reason is the uninformative `must equal an allowed value` while TS/Java name the admissible set and the offending key.
- **P2:** all four backends emit `must have at least N properties` / `must have at most N properties`; the specs' Validator-mapping tables mandate `too few properties: at least N, got M` / `too many properties: at most N, got M`. Uniform, so no wire risk, but the specs and the code disagree.
- **P2:** Go compiles `propertyNames` regexes inline inside the per-key loop (`regexp.MustCompile(...)` per key) instead of hoisting to a package-level `var` the way every value-level `pattern`/`format` does.
- Counting is **correct and consistent** in all four languages on both sides: parse counts the raw wire object as one number (`len(all)` / `Object.keys(raw).length` / `len(raw)` / `node.size()`), serialize counts the to-be-emitted key set, and a field whose `default` is unset is omitted from both. Verified by generating and reading output for a `default` + `minProperties`/`maxProperties` model.
- **Testing:** no conformance-manifest case covers any of these keywords (the manifest has 4 cases, none object-constraint), and the explicit spec Interactions bullets for `default` × count, `required` × `minProperties`, and `nullability` × `dependentRequired` are untested in every language.

## Implementation divergences

### 1. `propertyNames` with a temporal `format` produces non-compiling Go, non-parsing Python, and a silently-dropped constraint in TS/Java

- **Severity:** P0
- **Spec cite:** `propertyNames.md:44-45` ("Anything else … → reject per **P7.1**"), `propertyNames.md:82-85` (reuses `format`'s decisions), `PRINCIPLES.md` P1/P10.
- **Code cite:**
  - Loader accepts: `src/parser/json_schema.rs:2869-2884` — `ASSERTIONS` contains the bare string `"format"`, and `validate_format` (`src/parser/json_schema.rs:2128-2146`) classifies `date-time`/`date`/`time`/`duration` as `FormatClass::Temporal`, i.e. valid.
  - Go: `src/generator/json_schema/go.rs:849-856` guards on the *raw* `subschema.format.is_none()` (so it opens the loop), then `go.rs:920-921` only emits a body when `crate::json_schema::format::check_for(format)` is `Some` — which it is not for temporal formats.
  - Python: identical shape at `src/generator/json_schema/python.rs:1752-1759` (guard) and `python.rs:1814-1815` (body gated on `check_for`).
  - TypeScript: `src/generator/json_schema/typescript.rs:614-621` / `typescript.rs:679-681`.
  - Java: `src/generator/json_schema/java.rs:631-635` early-returns because `StringLengthConstraints::from_schema` (`java.rs:149-157`) maps `format` through `check_for` and gets `None` — so Java emits nothing at all.
- **What the spec requires:** either a working key check, or a load reject naming the unsupported assertion (P7.1). Never a dropped constraint (P10) and never divergent behavior across targets (P1).
- **What the code does:** accepts the schema; Go/Python emit dead loop headers with no body; TS emits an empty `for (const key of keys) { }`; Java emits nothing.
- **Concrete failing input:**
  ```yaml
  $defs:
    M:
      type: object
      additionalProperties: true
      propertyNames: { type: string, format: date-time }
  ```
  Go output → `./pndtgo.go:18:6: declared and not used: k` (`go build` fails, twice).
  Python output → `IndentationError: expected an indented block after 'for' statement` on `python3 -c "ast.parse(...)"`.
  TS/Java output → compiles; key `"nope"` is accepted where the schema says it must be an RFC 3339 date-time.
  Reproduces identically for `format: date`, `format: time`, `format: duration`.
- **Confidence:** high (built the CLI, generated all four, compiled Go, parsed Python).
- **Suggested fix:** reject a temporal `format` inside `propertyNames` at `json_schema.rs:2869` (a materializing format cannot assert a key), and independently make the four `render_*_property_name_checks` guards agree with their bodies (guard on `check_for(format).is_some()`, as Java already does) so an empty loop can never be emitted.

### 2. `minProperties`/`maxProperties` written as `1.0` load-validate, then crash every backend

- **Severity:** P1
- **Spec cite:** `minProperties.md:37` and `maxProperties.md:33-34` — "Value not a non-negative integer (**honors `1.0`-as-integer** + the integer cap, see [[type]]) → reject", i.e. `1.0` is *not* a reject; it is a valid integral spelling.
- **Code cite:** loader accepts at `src/parser/json_schema.rs:1144-1146` (`value.as_f64() … value.fract() == 0.0`) and `json_schema.rs:2771-2797` (same rule, then `value as u64`). Backends then deserialize the raw JSON into `Option<usize>`: `src/generator/json_schema/go.rs:42-45`, `typescript.rs:202-206`, `python.rs:48-52`, `java.rs:42-46`. `serde_json` refuses `1.0 → usize`.
- **What the spec requires:** `minProperties: 1.0` is accepted and behaves as `1`.
- **What the code does:** every backend aborts with an internal error carrying no source location:
  `invalid JSON schema in \`<json-generator>\`: failed to read planned JSON schema \`M\`: invalid type: floating point \`1.0\`, expected usize`
- **Concrete failing input:**
  ```yaml
  $defs:
    M: { type: object, additionalProperties: true, minProperties: 1.0, maxProperties: 3.0 }
  ```
  Fails identically for `go`, `typescript`, `python`, `java`.
- **Confidence:** high (reproduced on all four CLI targets).
- **Note:** the same `Option<u64>`/`Option<usize>` pattern is used for `minLength`/`maxLength`/`minItems`/`maxItems`/`minContains`/`maxContains`; the class of bug is likely wider than this group. Fix belongs in the loader's normalize pass (canonicalize an integral float bound to an integer `Value`) so every keyword is covered at once.

### 3. `propertyNames` subschema allowlist only inspects `extra`, so typed keywords pass silently

- **Severity:** P1
- **Spec cite:** `propertyNames.md:43-45` and `propertyNames.md:50-51` — "Subschema not a string schema (or carrying non-string assertions) → reject; diagnostic explains keys are always strings."
- **Code cite:** `src/parser/json_schema.rs:2869-2876` iterates `subschema.extra.keys()` only. `properties`, `required`, `items`, `oneOf`, `additionalProperties`, `$ref`, `title`, `description` are **typed fields** on `Schema` (`src/parser/json_schema.rs` `struct Schema`), not `extra` entries, so they are never seen.
- **What the spec requires:** reject.
- **What the code does:** accepts and silently drops. Verified accepted:
  - `propertyNames: {type: string, maxLength: 8, properties: {a: {type: string}}}`
  - `propertyNames: {type: string, maxLength: 8, required: [a]}`
  - `propertyNames: {type: string, maxLength: 8, items: {type: string}}`
  - `propertyNames: {type: string, maxLength: 8, additionalProperties: false}`
  - `propertyNames: {type: string, maxLength: 8, oneOf: [{type: string}, {type: "null"}]}` ← reads as "a nullable key", which is meaningless
- **Impact:** none of these can change the accepted key set (object/array applicators are vacuous on a string instance), so this is not silently-wrong *output* — it is a missing P7.1 loud reject on an authoring mistake, and the `oneOf` case in particular lets a nonsensical schema through.
- **Confidence:** high (each case run through the CLI, all `ACCEPTED`).
- **Aside:** `propertyNames: {type: string, maxLength: 8, $ref: "#/$defs/M"}` *does* reject, but with a misleading diagnostic — `` `allOf` branches declare disjoint types `object` and `string` `` — instead of the "`$ref` must not carry sibling keywords" message the same construct gets elsewhere.

### 4. Go's `Validate()` on a declared-property model omits every object-level constraint

- **Severity:** P2
- **Spec cite:** `minProperties.md:61` ("`UnmarshalJSON` counts decoded members … and hands the count to **the shared `Validate`**"), `dependentRequired.md:78` ("The cross-field check is a predicate in the shared `Validate`, which `UnmarshalJSON` calls"), `PRINCIPLES.md` P12.2.
- **Code cite:** `src/generator/json_schema/go.rs:2613-2626` (`render_validate`) never calls `render_go_property_count_checks` or `render_go_dependent_required`. The calls live only in `go.rs:3042-3043` (`UnmarshalJSON`) and `go.rs:4037-4038` (`MarshalJSON`). By contrast the map-shaped `Validate` at `go.rs:4112-4115` *does* include them.
- **What the code does:** for `samples/schemas/showcase.nexusrpc.yaml` `Contact` (`minProperties: 1`, `maxProperties: 3`, `dependentRequired`), the generated `func (m ContactGo) Validate() error` body is literally `var errs []Violation; if len(errs) > 0 {…}; return nil` — `samples/go/showcase/showcase.go:1780-1786`.
- **Concrete failing input:** `ContactGo{}.Validate()` returns `nil` even though the object has 0 members and `minProperties: 1`. Marshalling it does fail, so the wire is safe — but `Validate` is an exported, documented API ("Validate checks m against every constraint") that lies, and a parent's `Validate()` walking children via `mergeNested` inherits the hole.
- **Confidence:** high (read the checked-in generated sample).

### 5. Java `propertyNames` `pattern`/`enum`/`format` violations lose the spec-mandated `invalid property name "…"` prefix

- **Severity:** P2
- **Spec cite:** `propertyNames.md:80` — Java must push `Violation{path:key, "invalid property name \"" + key + "\": " + why}`.
- **Code cite:** `src/generator/json_schema/java.rs:642-655` hand-writes the prefix for the two length checks, but `java.rs:657-674` then delegates `pattern`/`format` to `render_java_string_checks` and `enum` to `render_java_closed_string_checks`, which emit the plain value-level reason.
- **What the code does** (generated from `propertyNames: {type: string, pattern: "^[a-z]+$"}`):
  `violations.add(new Violation(pnKey, "must match pattern " + "^[a-z]+\\z" + ", got " + pnKey));`
  vs Go `invalid property name %q: must match pattern ^[a-z]+$`, TS `` invalid property name "${key}": must match pattern … ``, Python `invalid property name "…": must match pattern …`. Java is the sole outlier for pattern/enum/format.
- **Confidence:** high (generated and read `ByPattern.java` / `ByEnum.java` / `ByFormat.java`).

### 6. Go and Python emit an uninformative reason for a `propertyNames` `enum` failure

- **Severity:** P2
- **Spec cite:** `propertyNames.md:77-79` (the `why` is "the underlying assertion's reason"); reinforced by the repo's informative-reason rule (name the concrete bound + offending value).
- **Code cite:** `src/generator/json_schema/go.rs:912-915` and `src/generator/json_schema/python.rs:1805-1812` both emit `must equal an allowed value`; TypeScript (`typescript.rs:648-665`) and Java (`render_java_closed_string_checks`) emit `must be one of ["alpha", "beta"], got "gamma"`.
- **What the code does:** a Go/Python caller learns neither which values are allowed nor which key failed the membership test (the key is in `path`, but not in the reason as it is for every other check).
- **Confidence:** high (generated Go/Python/TS/Java from the same `propertyNames: {type: string, enum: [alpha, beta]}`).

### 7. All four backends use member-count reason text the specs do not specify

- **Severity:** P2
- **Spec cite:** `minProperties.md:61-64` and `maxProperties.md:60-63` name `too few properties: at least %d, got %d` / `too many properties: at most %d, got %d` for every language.
- **Code cite:** `go.rs:820-822` / `go.rs:829-832`, `typescript.rs:586-588` / `typescript.rs:596-598`, `python.rs:1728-1730` / `python.rs:1737-1739`, `java.rs:610-612` / `java.rs:615-617` all emit `must have at least N properties, got M` / `must have at most N properties, got M`.
- **Impact:** none on the wire (P11 explicitly frees reason text), but the emitted strings are asserted verbatim by the round-trip suites (`samples/python/tests/test_showcase.py:579`, `samples/typescript/tests/json-schema-showcase.test.ts:531`, `samples/java/.../JsonSchemaShowcaseRoundTripTest.java:511`), so the specs and the locked-in behavior disagree. Pick one and change the other.
- **Confidence:** high.

### 8. Go compiles `propertyNames` regexes inside the per-key loop instead of hoisting them

- **Severity:** P2
- **Spec cite:** `PRINCIPLES.md` P2 (hand-written-feeling output); the value-level `pattern`/`format` path already hoists.
- **Code cite:** `src/generator/json_schema/go.rs:885-900` (pattern) and `go.rs:920-943` (format) inline `regexp.MustCompile(...)` inside the `for k := range … {` body opened at `go.rs:857-858`. Compare the hoisted value-level vars in the checked-in sample: `samples/go/showcase/showcase.go:544`, `:2399-2409`, `:4891`.
- **What the code does:** for `propertyNames: {type: string, pattern: "^[a-z]+$"}`, Go emits `if !regexp.MustCompile("^[a-z]+$").MatchString(k) {` *per key, per call, in both directions* — a recompile on every map member, and a `MustCompile` panic site inside a hot loop rather than at package init.
- **Confidence:** high (generated `pn-go/pngo.go:82`, `:206`, `:231`).

### 9. `patternProperties` diagnostic reads as a permanent exclusion, not a deferral

- **Severity:** P2
- **Spec cite:** `patternProperties.md:69-70` — "The diagnostic must read as 'not yet supported,' not 'forbidden'".
- **Code cite:** `src/parser/json_schema.rs:1517-1519` — `` "`patternProperties` is not supported; use a typed map …" ``. Identical phrasing to `dependentSchemas` (`:1514-1516`) and `unevaluatedProperties` (`:1508-1510`), which *are* categorical P6 exclusions, so the deliberate distinction the specs draw is invisible to the user. The two coherent alternatives the spec asks for are both present.
- **Confidence:** high.

### 10. `dependencies` (draft-4..7) is rejected as an unknown keyword rather than handled per the ecosystem table

- **Severity:** P2
- **Spec cite:** `dependentRequired.md:138-139` — "A `dependencies` array form → accept as `dependentRequired`; the schema form → reject"; `dependentSchemas.md:114` mirrors it.
- **Code cite:** `dependencies` is absent from `schema_extra_keyword_is_known` (`src/parser/json_schema.rs:115-170`), so both forms hit the generic `unknown schema keyword \`dependencies\`` at `json_schema.rs:1060-1065`.
- **What the code does:** `dependencies: {a: [b]}` → `$defs.M: unknown schema keyword \`dependencies\``; `dependencies: {a: {required: [b]}}` → the same. No migration hint either way.
- **Judgement:** arguably fine given `$schema` is pinned to 2020-12, but the two specs state an action the loader does not take, and at minimum the array form deserves a "rename to `dependentRequired`" fix-it rather than "unknown keyword".
- **Confidence:** high.

### 11. Go emits no catch-all/declared key-collision check for an *untyped* open object (affects the serialize member count)

- **Severity:** P1 — root cause belongs to `additionalProperties`, listed here because it is the one place the four languages' serialize-side member sets can differ.
- **Spec cite:** `maxProperties.md:65-79` (serialize counts the keys that will actually be written); cross-language agreement is P1.
- **Code cite:** `src/generator/json_schema/go.rs:2937-2945` emits the `catch-all key collides with declared property` violation **only when `additional_shape.is_some()`** (a typed catch-all). For an untyped catch-all (`map[string]json.RawMessage`) the marshal path at `go.rs:3980-4033` simply copies extras into `out` and lets declared fields overwrite them — see `samples/go/showcase/showcase.go:1841-1848` (`ContactGo.MarshalJSON`), which has no collision check. TypeScript (`typescript.rs:3053-3058`), Python (`python.rs:~3745`), and Java (`java.rs:3472-3478`) all raise a violation regardless of typing.
- **Concrete failing input:** an in-memory `ContactGo{Email: ptr("a@b.c"), AdditionalProperties: {"email": …}}` serializes to a 1-member object with no violation in Go, while the analogous TS/Python/Java models throw.
- **Confidence:** high (read the checked-in generated Go sample).

## Testing gaps

### 1. No test builds/parses the generated output for a `propertyNames` with a temporal `format`

- **Severity:** P0
- **Untested:** that a loader-accepted `propertyNames` subschema always produces compilable, non-empty key checks. Divergence #1 slipped through precisely because the conformance-matrix schemas (`tests/generate_go.rs:187-196`, `tests/generate_typescript.rs:216-226`, `tests/generate_python.rs:183-191`, `tests/generate_java.rs:209-217`) only exercise `minLength`/`maxLength`/`pattern`/`enum`/`format: email|hostname`.
- **Spec line:** `propertyNames.md:44-45`, `propertyNames.md:112` (Shapeless subschema row).
- **Where:** a loader reject test in `src/parser/json_schema.rs` (alongside `rejects_shapeless_property_names`), plus a row in each of the four `tests/generate_*.rs` conformance-matrix schemas once the reject lands.
- **Suggested case:** `numeric_reject("type: object\nadditionalProperties: true\npropertyNames: { type: string, format: date-time }")` asserting the diagnostic names the format; repeat for `date`, `time`, `duration`.

### 2. No test for a `1.0`-spelled `minProperties`/`maxProperties`

- **Severity:** P1
- **Untested:** the spec's explicit "honors `1.0`-as-integer" rule. Only `-1` (`rejects_non_integer_min_properties`, `src/parser/json_schema.rs:8774-8778`) and the `2^53` cap (`:8780-8790`) are tested; `1.5` and `"1"` are in the matrix and untested too (they happen to work).
- **Spec line:** `minProperties.md:37`, `maxProperties.md:33`, matrix row `minProperties:-1, minProperties:1.5, minProperties:"1"`.
- **Where:** `src/parser/json_schema.rs` inline tests + one `tests/generate_go.rs`-style end-to-end assert that `1.0` actually generates.
- **Suggested case:** load `type: object\nadditionalProperties: true\nminProperties: 1.0\nmaxProperties: 3.0` and assert it *generates* (currently it fails in the backend, not the loader, so a loader-only test would pass while the CLI is broken).

### 3. No test for `propertyNames` subschemas carrying typed keywords

- **Severity:** P1
- **Untested:** `properties`, `required`, `items`, `oneOf`, `additionalProperties`, `$ref` inside a `propertyNames` subschema (divergence #3).
- **Spec line:** `propertyNames.md:50-51`, matrix row "Non-string subschema".
- **Where:** `src/parser/json_schema.rs` beside `rejects_non_string_property_names`.
- **Suggested case:** a table-driven reject test over `{type: string, maxLength: 8, <keyword>: …}` for each of the six, asserting the diagnostic names the offending keyword.

### 4. `patternProperties` / `dependentSchemas` / `unevaluatedProperties` rejects are tested in exactly one position

- **Severity:** P2
- **Untested:** every position other than "a nested property schema". `rejects_structural_keywords_with_fixits` (`src/parser/json_schema.rs:9081-9127`) routes every case through `numeric_reject`, which wraps the fragment under `properties.value` (`json_schema.rs:8015-8036`). I verified by hand that top-level `$defs`, nested `$defs`, dead `$defs`, `items`, `contains`, `allOf` branches, `oneOf` branches, `additionalProperties`, and operation `input`/`output` all reject — but nothing locks that in, and the `allOf` path in particular relies on the merge preserving the keyword (`json_schema.rs:5581-5611`) rather than on a dedicated reject.
- **Spec line:** `patternProperties.md:68`, `dependentSchemas.md:67`, `unevaluatedProperties.md:83` ("**Any** … present → reject").
- **Where:** `src/parser/json_schema.rs`, a new position-matrix test.
- **Suggested case:** for each of the three keywords × each of {root `$defs`, nested `$defs`, unreferenced `$defs`, `items`, `contains`, `allOf` branch, `oneOf` branch, `additionalProperties`, operation input}, assert the load error names the keyword. The `allOf`-branch row is the highest-value one: it is the only position where a rewrite pass could swallow the keyword.

### 5. The remaining `patternProperties` / `unevaluatedProperties` matrix rows are untested

- **Severity:** P2
- **Untested:** `patternProperties` with `properties`; overlapping patterns; an RE2-incompatible pattern key (`"(?=x)"`); `unevaluatedProperties: {type: string}`; `unevaluatedProperties: true`; `dependentSchemas: {a: {required: ["b"]}}` (the row whose diagnostic is supposed to point at `dependentRequired`).
- **Spec line:** `patternProperties.md:95-98`, `unevaluatedProperties.md:103-105`, `dependentSchemas.md:89-90`.
- **Where:** extend `rejects_structural_keywords_with_fixits`.
- **Suggested case:** all six as reject rows; additionally assert the `dependentSchemas: {a: {required:[b]}}` diagnostic mentions `dependentRequired` (today it does not — see divergence #9's sibling: `json_schema.rs:1514-1516` offers "split the variants into explicit types" but never names `dependentRequired`, which `dependentSchemas.md:69-71` requires).

### 6. Accepted-but-degenerate forms are untested

- **Severity:** P2
- **Untested:** `minProperties: 0` (spec: accepted no-op), `maxProperties: 0` (spec: accepted, object must be empty), `dependentRequired: {}` (vacuous), `dependentRequired: {a: []}` (vacuous), `propertyNames: {}` (rejected — only `propertyNames: true` is tested at `json_schema.rs:8853-8857`).
- **Spec line:** `minProperties.md:36`, `maxProperties.md:35-37`, `dependentRequired.md:64`, `propertyNames.md:59`.
- **Where:** `src/parser/json_schema.rs` inline tests.
- **Suggested case:** four accept assertions and one reject assertion; also worth asserting `maxProperties: 0` generates a check that rejects `{"a":1}` in at least one language.

### 7. Go's incomplete `Validate()` is unasserted

- **Severity:** P2
- **Untested:** that Go's exported `Validate()` reports the object-level constraints. `samples/go/tests/json_schema_showcase_test.go:409+` exercises the constraints only through `json.Unmarshal`/`json.Marshal`.
- **Spec line:** `minProperties.md:61`, `dependentRequired.md:78`.
- **Where:** `samples/go/tests/json_schema_showcase_test.go`.
- **Suggested case:** `if err := (ContactGo{}).Validate(); err == nil { t.Fatal("Validate must report minProperties") }`.

### 8. No conformance-manifest case for any object constraint

- **Severity:** P1 (test-only, but it is the only cross-language parity gate)
- **Untested:** that the four languages agree on the accepted/rejected wire value set for member counts, key shape, and cross-field dependencies. `samples/conformance/json-schema.json` has 4 cases (`recursive-collections`, `mathematical-number-equality`, `year-zero-rejection`, + 1); none touches this group. The four per-language suites assert the same things independently and by hand, which is exactly the setup that lets a per-language drift (divergences #4, #5, #6, #11) survive.
- **Spec line:** `PRINCIPLES.md` P1.
- **Where:** `samples/conformance/json-schema.json`, checked by `tests/json_schema_conformance_manifest.rs`.
- **Suggested case:** one case over `showcase.nexusrpc.yaml` `Attributes` + `Contact`: `parse_failures` for `{}` (minProperties), a 4-key object (maxProperties), a 9-code-point key (propertyNames), and `{"shippingStreet":"x"}` (dependentRequired), with `expected_paths` `["", "", "<key>", "shippingZip"]`, plus a `serialize_failures` mirror.

### 9. `default` × member count is untested in every language

- **Severity:** P1
- **Untested:** the explicit Interactions bullet — "a default-filled key is never on the wire, so it does not count", and its serialize mirror "a model that reads as populated in memory can legitimately fall under `minProperties` on the wire". No sample model combines a `default`-bearing property with `minProperties`/`maxProperties` (`samples/schemas/showcase.nexusrpc.yaml` `Contact` and `Attributes` have no defaults).
- **Spec line:** `minProperties.md:116-119`, `maxProperties.md:116-119`, `minProperties.md:66-78`.
- **Where:** `samples/schemas/showcase.nexusrpc.yaml` (add a `defaulted` member to `Contact`, or a small new `$def`), then all four round-trip suites.
- **Suggested case:** `{type: object, additionalProperties: false, properties: {a: {type: string, default: "x"}, b: {type: string}}, minProperties: 1}` — deserialize `{}` must fail with `at least 1`, and a model with only the unset default must fail to *serialize* with the same violation. (I verified by generating that all four backends already behave correctly here; this only needs locking in.)

### 10. `required` × `minProperties` and `nullability` × `dependentRequired` are untested

- **Severity:** P2
- **Untested:** (a) `minProperties` demanding more members than `required` names, satisfied via optional properties or extras — spec calls this out explicitly; (b) a `dependentRequired` dependent present on the wire as an explicit `null` counting as *present*, and its optional+nullable serialize counterpart (where Go/Java/Python collapse `null` to absent and TypeScript does not).
- **Spec line:** `minProperties.md:108-111`, `dependentRequired.md:129-130`, `PRINCIPLES.md` P1 exception (a).
- **Where:** all four round-trip suites (and ideally a conformance case, since (b) is exactly where the documented nullability collapse can move a member count).
- **Suggested case:** (a) `{properties:{a,b,c}, required:[a], minProperties: 2}` — `{"a":"x"}` fails, `{"a":"x","b":"y"}` passes, `{"a":"x","z":1}` passes via extras. (b) a nullable `shippingZip`: wire `{"shippingStreet":"x","shippingZip":null}` must pass in all four.

### 11. `propertyNames` on an untyped map (`additionalProperties: true`) is only covered in Java/Go matrices

- **Severity:** P2
- **Untested:** the spec's own second accepted row, `{type:object, additionalProperties:true, propertyNames:{type:string, maxLength:64}}`. The TS matrix (`tests/generate_typescript.rs:216`) uses `additionalProperties: {type: integer}`; Python (`tests/generate_python.rs:183`) uses typed maps; `samples/schemas/showcase.nexusrpc.yaml` `Attributes` is typed. Only the Java matrix reaches an untyped-adjacent path.
- **Spec line:** `propertyNames.md:104`.
- **Where:** the four `tests/generate_*.rs` matrix schemas.
- **Suggested case:** add an `additionalProperties: true` + `propertyNames` model to each matrix and assert a bad key is rejected on both parse and serialize.

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| `minProperties` x `maxProperties` | `min > max` is a load reject; both count the same set | yes — `rejects_empty_property_interval` (`json_schema.rs:8701-8707`); also verified post-`allOf`-merge | low |
| `minProperties` x `additionalProperties: false` | `min` above the declared count → load reject | yes — `rejects_min_properties_above_closed_object_capacity` (`:8709-8715`) | low |
| `maxProperties` x `required` | `max <` required count → load reject | yes — `rejects_max_properties_below_required_count` (`:8838-8843`) | low |
| `minProperties` x `required` | required counts toward the floor; `min` may exceed the required set (satisfiable via optionals/extras) | **no** | medium — no test proves an open object reaches the floor via extras alone |
| `minProperties`/`maxProperties` x `default` | default-filled key never on the wire; count taken before population, and after omission on serialize | **no** (behavior verified correct by hand in all four) | medium — the one asymmetry a refactor would silently break |
| `minProperties`/`maxProperties` x `additionalProperties` extras | extras count toward the total; count as one number, never declared-bucket + extras-bucket | yes — Go `mixed` (`tests/generate_go.rs:2777-2792`), TS `audit` (`tests/generate_typescript.rs:2219-2237`), Java `NativeExtras`, Python `test_showcase.py:759-765` | low |
| `minProperties`/`maxProperties` x case-mapped / `x-<lang>-name` members | count is a wire fact, unaffected by member renaming | partly — `Contact` carries `x-<lang>-name` on the *type*, not on a counted member | low |
| `maxProperties` x catch-all/declared key collision | collision must be a violation; the emitted key set is what counts | Go **untested and wrong** for untyped catch-alls (divergence #11); tested for Go typed (`tests/generate_go.rs:2795-2798`), TS, Java | **high** — Go silently merges, other three throw |
| `minProperties`/`maxProperties` x optional+nullable member | Go/Java/Python collapse `null`→absent; TS keeps `null` → the *in-memory* models differ by one member | **no** | medium — wire-in behavior is identical (verified); only the serialize-out count of an "equivalent" model can differ, and this follows from the documented P1 exception (a). Worth an explicit note in the specs. |
| `propertyNames` x `minProperties`/`maxProperties` | count and key-shape compose on the same map | yes — showcase `Attributes` (min 1 / max 3 / maxLength 8) in all four suites | low |
| `propertyNames` x `properties` | mutually exclusive → load reject | yes — `rejects_property_names_alongside_properties` (`:8717-8723`) | low |
| `propertyNames` x `additionalProperties` absent | no map host → reject (or caught by `type`) | yes — `rejects_property_names_without_map_host` (`:8845-8850`) | low |
| `propertyNames` x `pattern`/`minLength`/`maxLength`/`enum`/`format` | inherits each string assertion's decisions, applied to keys | yes for asserted formats — all four matrices; **no** for the four temporal formats (divergence #1) | **high** |
| `propertyNames` x preserved unknown keys | the check applies to the map's keys verbatim, no case-mapping, in both directions | yes — Go/TS/Python/Java round-trip suites check parse *and* serialize | low |
| `propertyNames` x case-mapped declared keys | cannot arise (`propertyNames`+`properties` rejects) | n/a | none |
| `propertyNames` x nested-map-as-`additionalProperties` | the host's key check runs when the parent parses | **no** direct test (verified it generates) | low |
| `dependentRequired` x `required` (trigger) | trigger in `required` → reject | yes — `:8750-8756`; also verified through an `allOf` merge | low |
| `dependentRequired` x `required` (dependent) | dependent in `required` → reject as vacuous | yes — `:8758-8764` | low |
| `dependentRequired` x `properties` | trigger and every dependent must be declared | yes — `:8742-8748`, `:8914-8921` | low |
| `dependentRequired` x self-reference `{a:[a]}` | not addressed by the spec | **no** — currently **accepted**, emitting dead `if a present { if a absent {…} }` in all four | low (cosmetic; a P7.1 reject would be more consistent) |
| `dependentRequired` x `nullability` | independent — presence, not null-ness | **no** (implementations agree: `all[k]` / `k in raw` / `raw[k] !== undefined` / `node.has(k)` all treat wire `null` as present) | medium |
| `dependentRequired` x `default` | a dependent satisfied only by an omitted default does **not** count as present | **no** | medium — all four count `out`/present-expr post-omission, so behavior is right but unlocked |
| `dependentRequired` x `allOf` merge | per-trigger union of dependent lists | partly — grammar reject tested (`json_schema.rs:9673-9677`); the *union* semantics are untested | medium |
| `patternProperties` x `additionalProperties` / `unevaluatedProperties` / `propertyNames` | never co-occurs (patternProperties always rejects first) | reject tested in one position only | low |
| `unevaluatedProperties` x `allOf` flattening | flattening leaves no residual applicator, so the keyword collapses to `additionalProperties` → reject | reject verified by hand in an `allOf` branch; **untested** | medium — this is the position where a merge rewrite could plausibly drop the keyword |
| `dependentSchemas` x `dependentRequired` | a `{required:[…]}`-only `dependentSchemas` should be diagnosed toward `dependentRequired` | **no**; the diagnostic never names `dependentRequired` | low |

## Verified-good

- `patternProperties`, `dependentSchemas`, `unevaluatedProperties` reject at load in all nine positions I could construct (root `$defs`, nested `$defs`, unreferenced `$defs`, array `items`, `contains`, `allOf` branch, `oneOf` branch, `additionalProperties` value schema, service operation `input`) — `src/parser/json_schema.rs:1583-1605` reached via `validate_schema_common`, with `validate_raw_schema_grammar` (`:1159-1311`) covering the raw-grammar walk.
- All three rejects fire for every value shape: `unevaluatedProperties: false`, `: true`, and `: {type: string}`; `dependentSchemas` with a subschema and with `{required:[…]}`; `patternProperties` with one pattern, with `properties`, and with overlapping patterns.
- The member count is the **distinct wire key count as one number** in all four parse adapters — Go `len(all)` (`go.rs:3042`), TS `Object.keys(raw).length` (`typescript.rs:2915`), Python `len(raw)` (`python.rs:3529`), Java `node.size()` (`java.rs:3719`) — never a declared-bucket + extras-bucket sum, and never after default population.
- The serialize-side count is the to-be-emitted key set in all four — Go `len(out)`, TS `Object.keys(out).length`, Python `len(out)`, Java a `wireKeyCount`/`wireKeys` reconstruction (`java.rs:3264-3294`, `:3457-3479`) whose per-field emit condition I checked line-by-line against `render_field_serialize` (`java.rs:3559-3587`); they match, including the de-duping `LinkedHashSet` for open objects.
- An unset `default` is omitted on serialize and does not count toward `minProperties`/`maxProperties` in all four — generated and read the output for `{a: {type: string, default: "x"}, b, minProperties: 1, maxProperties: 2}`.
- Key length is measured in **code points** in all four: Go `utf8.RuneCountInString` (`go.rs:863`), TS `[...key].length` (`typescript.rs:625`), Python `len(key)`, Java `codePointCount(0, length())` (`java.rs:644`).
- `propertyNames` runs in **both** directions in all four (parse over the wire keys, serialize over the emitted/in-memory keys): `go.rs:4113-4115` + `:4448-4450`, `typescript.rs:2991-2993` + `:3089-3091`, `python.rs:3568-3571` + `:3753-3756`, `java.rs:4931-4934` + `:3480-3487` / `:4843-4855`.
- The `propertyNames` string-subschema validation reuses the full string pipeline and rejects properly: non-string `type`, `type: [string, "null"]`, bare `true`, `{}`, a non-assertion keyword (`const`, `default`, `contentEncoding`, `multipleOf`), an unknown `format`, `minLength > maxLength`, an `enum` member violating the sibling `pattern`/`maxLength`, an invalid regex, and a non-RE2-portable regex (`(?=x)y`) — `src/parser/json_schema.rs:2846-2911`.
- Unsatisfiability rejects survive `allOf` flattening: `min > max`, `min` above a closed object's declared count, and `max <` the required count all still reject when the two halves arrive from different branches (`json_schema.rs:5543-5546` keeps the tighter bound; `validate_object_constraints` then runs on the merged schema).
- `dependentRequired` load rules are complete against the spec's reject matrix: non-object value, non-array value, non-string element, duplicate element, undeclared trigger, undeclared dependent, trigger in `required`, dependent in `required` — and the trigger-in-`required` reject also fires after an `allOf` merge.
- `dependentRequired` treats a wire `null` as **present** consistently in all four (`all[k]` / `raw[k] !== undefined` / `k in raw` / `node.has(k)`), and the violation path is the dependent's JSON name with an identical reason string in all four.
- Object-constraint keywords on a non-`object` `type` reject with a single shared diagnostic (`json_schema.rs:2741-2745`), tested by `rejects_object_keyword_on_non_object_field`.
- The `2^53−1` cap on `minProperties`/`maxProperties` is enforced early, before `allOf` merging or `$ref`-sibling lowering can hide it (`json_schema.rs:1138-1157`, tested at `:8792-8836`).
