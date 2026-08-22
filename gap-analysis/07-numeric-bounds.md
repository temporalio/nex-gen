# Numeric bounds (minimum / maximum / exclusive* / multipleOf) — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/minimum.md` — inclusive lower bound; mirror of `maximum`, same-axis reject against `exclusiveMinimum`.
- `specs/json-schema/features/maximum.md` — inclusive upper bound; **owns** the shared numeric-bound machinery (integer-valued bounds on `integer` fields, satisfiability, reason-string convention, serialize re-check).
- `specs/json-schema/features/exclusiveMinimum.md` — strict `>` lower bound; draft-4 boolean-form reject.
- `specs/json-schema/features/exclusiveMaximum.md` — strict `<` upper bound; draft-4 boolean-form reject.
- `specs/json-schema/features/multipleOf.md` — divisibility; **positive integer divisors only** (fractional deferred), `fmod` on `number` fields, range-emptiness reject.
- Read for context: `PRINCIPLES.md` (P1, P7.1, P10–P12), `features/type.md` (number/integer model, ±(2^53−1) cap), `nullability.md` (the non-null `oneOf` branch "may carry any sibling keyword recognized for that `type`").

## Summary

- **Three confirmed P0 cross-language wire disagreements**, all inside this keyword family, none of which any test covers.
- **Bounds are silently dropped on a nullable field** (`oneOf: [{type:T, minimum:…}, {type:"null"}]`) by **Java**, and by **Go** — which additionally emits Go that does not compile. TypeScript and Python enforce them correctly. Verified by generating and compiling.
- **Go's `multipleOf` on a `number` field is not `fmod`.** It is exact rational arithmetic over the value's shortest decimal spelling (`big.Rat` over `strconv.FormatFloat`), so it disagrees with TS/Python/Java **in both directions** (`1e23 % 5`: Go accepts, others reject; `1e300 % 3`: Go rejects, others accept). Verified by actually running Go, Node, Java and CPython.
- **Python compares a `number` field's bound in exact integer arithmetic** when the wire token is integral, while Go/TS/Java compare rounded binary64 — so a value above 2^53 can be rejected by Python and accepted everywhere else.
- The loader's `multipleOf` × range emptiness check is gated on `is_integer`, so **`{type:"number", minimum:1, maximum:2, multipleOf:5}` loads** even though nothing can satisfy it.
- The loader's literal-vs-`multipleOf` check uses `(value / divisor).fract()`, which is **always `0.0` for quotients ≥ 2^52** — `{type:"number", multipleOf:3, const:1e22}` loads, and every target's runtime then rejects the const.
- The system now carries **three different divisibility semantics** for one keyword: loader quotient-fract, Go exact-rational, TS/Python/Java IEEE `fmod`.
- `merge_multiple_of`'s LCM **panics on i64 overflow** (`attempt to multiply with overflow`) instead of producing a diagnostic.
- The loader side is otherwise in good shape: type gate, boolean draft-4 form, integer-valued bounds, same-axis pairs, interval emptiness (integer and number), and `const`/`default`/`enum`-vs-bound are all implemented and mostly tested.
- **Combination coverage is the weak spot**: the cross-language conformance manifest (`samples/conformance/json-schema.json`) has **zero** numeric-bound cases, no test anywhere asserts an *inclusive* boundary value is **accepted**, and Java has no runtime exclusive-boundary test at all.

## Implementation divergences

### 1. Bounds are dropped on a nullable field (Java silently, Go non-compiling)

- **Severity:** P0
- **Spec cite:** `nullability.md:178-186` — "The non-null branch is a full subschema and **may carry any sibling keyword recognized for that `type`**", with `minLength: 5` as the worked example; `maximum.md:77-86` (Validator mapping applies to the field); **P1**.
- **Code cite:** `src/generator/json_schema/go.rs:3301-3311` (the `allows_null(property)` arm hard-codes `parseStringField` and emits no constraint checks at all); `src/generator/json_schema/java.rs:1962` (`numeric: NumericConstraints::from_schema(property)` reads the `oneOf` **wrapper** node, which carries no bounds, so `render_java_numeric_checks` at `java.rs:222` is never reached with a non-empty set).
- **What the spec requires:** the bound on the non-null branch is enforced in all four targets, both directions.
- **What the code does:**
  - TypeScript, Python: correct (`must be >= 5` emitted on both parse and serialize).
  - Java: **no `must be` string is emitted at all** in the generated POJO — the value is bound with `SpecNumbers.specLong` and accepted unconditionally.
  - Go: emits `parseStringField(get("v"), "v", false, true, &errs)` assigning a `string` to a `*int64` field — the package does not compile, and no bound check is present either.
- **Concrete failing input:**
  ```yaml
  properties:
    v:
      oneOf:
        - { type: integer, minimum: 5, maximum: 9 }
        - { type: "null" }
  ```
  Wire `{"v":1}` → TypeScript rejects, Python rejects, **Java accepts**, Go fails to build (`cannot use &v (value of type *string) as *int64 value in assignment`).
- **Confidence:** high — generated all four backends and ran `go build` on the Go output. Note the Go arm drops *all* constraint families (a nullable `{type:string, minLength:3}` loses `minLength` too), so the root cause is shared with the nullability/oneOf work; the numeric consequence is what is reported here.

### 2. Go's `number`-field `multipleOf` is exact-rational, not IEEE `fmod`

- **Severity:** P0
- **Spec cite:** `multipleOf.md:97` ("`number` field: `fmod(v, m) == 0` (IEEE remainder — exact for the stored double)"), `multipleOf.md:101` (Go: "same message when `math.Mod(v, m) != 0`"), `multipleOf.md:112` ("For `number` fields IEEE `fmod` is exact and portable across all four (verified, including large integer-valued doubles like `1e300`)"), **P1**.
- **Code cite:** `src/generator/json_schema/go.rs:204-215` and `go.rs:563` emit `isJSONMultiple(float64(v), "<divisor>")`; the runtime helper is emitted at `go.rs:1516-1521`:
  ```go
  v, ok := new(big.Rat).SetString(strconv.FormatFloat(value, 'g', -1, 64))
  d, ok := new(big.Rat).SetString(divisor)
  return new(big.Rat).Quo(v, d).IsInt()
  ```
  Compare `src/generator/json_schema/typescript.rs:337-343` (`v % m !== 0`), `src/generator/json_schema/python.rs:1541` (`math.fmod(v, m) != 0`), `src/generator/json_schema/java.rs:266` (`v % m != 0`).
- **What the spec requires:** all four run IEEE `fmod` over the stored binary64 value.
- **What the code does:** Go reconstructs the *decimal* the double round-trips to and does exact rational division. That is a different predicate whenever the double is not exactly the decimal it prints as.
- **Concrete failing inputs** (schema `{type:"number", multipleOf:5}` / `{type:"number", multipleOf:3}`):

  | wire value | divisor | Go | TypeScript | Java | Python |
  |---|---|---|---|---|---|
  | `1e23` | 5 | **accept** | reject (`% == 2`) | reject (`2.0`) | reject (`fmod == 2.0`) |
  | `1e300` | 3 | **reject** | accept (`% == 0`) | accept (`0.0`) | accept (`fmod == 0.0`) |
  | `1e22` | 3 | reject | reject | reject | reject |

- **Confidence:** high — generated the Go package and ran it (`{"v":1e23}` → no error; `{"w":1e300}` → `must be a multiple of 3, got 1e+300`); ran `node -e "1e23 % 5, 1e300 % 3"` → `2 0`; ran `java J.java` → `2.0 / 0.0`; `python3 math.fmod` → `2.0 / 0.0`.
- **Extra evidence this is unintentional leftover:** commit `e2b8de6` ("Align Go matcher test with loader policy") deleted the old assertion *"mathematical decimal multiple rejected"* — the test that existed specifically to lock in `isJSONMultiple`'s decimal semantics for `multipleOf: 0.1`. The divisor was changed to `2` so the loader would accept it, but the helper's semantics were left in place.

### 3. Python compares a `number`-field bound in exact integer arithmetic

- **Severity:** P0
- **Spec cite:** **P1** ("a value one language rejects … must be rejected by all"); `maximum.md:99-111` (Cross-language exactness — argued only for `integer` fields); `type.md:164-166` (Python "A classified `number` is stored **exactly as it arrived**, never coerced").
- **Code cite:** `src/generator/json_schema/python.rs:101-106` (`py_bound_literal` emits the authored number verbatim, so a `number`-field bound `9007199254740992` becomes a Python **`int`** literal) combined with the parse adapter storing the raw value (`samples/python/showcase/models.py:1766` `ratio_value = ratio_value_raw`, no `float()` coercion). Contrast `go.rs:124-131`, `typescript.rs:277-282`, `java.rs:204-217` (Java appends `.0`, forcing a `double` comparison).
- **What the spec requires:** identical accept/reject sets.
- **What the code does:** for `{type:"number", maximum: 9007199254740992}` all four emit the literal `9007199254740992`, but Python evaluates `int > int` exactly while Go/TS/Java evaluate `double > double` after the wire token has already been rounded.
- **Concrete failing input:** wire `{"n": 9007199254740993}` → Python's `9007199254740993 > 9007199254740992` is `True` → `must be <= 9007199254740992, got 9007199254740993`; Go/TS/Java parse to `9007199254740992.0` and accept. Verified by running the generated Go (`json.Unmarshal` returns `nil`).
- **Confidence:** high for the generated source in all four (inspected) and for Go's runtime (executed); the Python runtime step is verified by reading the emitted comparison rather than executing it (`temporalio` is not installed in this checkout) — **unverified at runtime**, but the semantics of `int`/`float` comparison in CPython are exact by definition.
- **Note:** the root cause is `type.md`'s deliberate "store exactly as it arrived" Python rule; the *bound comparison* is the place where it becomes an accept/reject divergence rather than a re-spelling difference, which is why it belongs to this family.

### 4. `multipleOf` × range emptiness is not checked on `number` fields

- **Severity:** P1
- **Spec cite:** `maximum.md:184-185` ("with a range present, if no multiple of the divisor lies in the accepted interval the schema is unsatisfiable → reject"); `multipleOf.md:72-77` and `multipleOf.md:161-164` (Interactions: `minimum`/`maximum`/`exclusive*` "combine for satisfiability … → load reject"). The rule is stated for the keyword generally, not for `integer` alone.
- **Code cite:** `src/parser/json_schema.rs:1926-1939` — the check is guarded by `if is_integer && let Some(divisor) = multiple_of && …`.
- **What the spec requires:** reject when `floor(hi/m)*m < lo`.
- **What the code does:** for `type: number` the check is skipped entirely.
- **Concrete failing input:** `{type:"number", minimum:1, maximum:2, multipleOf:5}` → **ACCEPT** (verified via the CLI); also `{type:"number", exclusiveMinimum:0, maximum:1, multipleOf:5}` → ACCEPT. Both emit a field that no value can ever satisfy in any of the four targets.
- **Confidence:** high (executed).

### 5. Loader's literal-vs-`multipleOf` check is a floating quotient `.fract()`

- **Severity:** P1
- **Spec cite:** `multipleOf.md:166-169` ("a supplied numeric literal MUST be a multiple of `m` at load"); `maximum.md:190-196`.
- **Code cite:** `src/parser/json_schema.rs:1961-1962`:
  ```rust
  } else if let Some(divisor) = multiple_of
      && (value / divisor).fract() != 0.0
  ```
- **What the spec requires:** reject a `const`/`default`/`enum` literal that is not a multiple of the divisor.
- **What the code does:** `f64::fract()` returns `0.0` for every magnitude ≥ 2^52, so any literal with a quotient at or above 2^52 is unconditionally treated as divisible. It is also a third divisibility semantics, agreeing with neither Go's rational check nor the `fmod` the other three run.
- **Concrete failing input:** `{type:"number", multipleOf:3, const:1e22}` → **ACCEPT** at load (verified), but the generated validators reject `1e22` in all four (`fmod(1e22,3) == 1.0`; Go's rational check also rejects) — the pinned constant can never round-trip. Symmetrically `{type:"number", multipleOf:3, const:1e300}` → ACCEPT at load, TS/Python/Java accept the value at runtime but **Go rejects it** (finding 2).
- **Confidence:** high (executed the loader; computed `fmod` in Python/Node/Java; ran the Go validator on `1e300`).

### 6. `merge_multiple_of` panics on LCM overflow instead of diagnosing

- **Severity:** P1
- **Spec cite:** **P7.1** ("Reject ambiguity loudly at generator time … an explicit error"); `allOf.md` intersection rule.
- **Code cite:** `src/parser/json_schema.rs:5674` — `let lcm = a / gcd * b;` with no checked multiply.
- **What the spec requires:** a diagnostic, or a correct merged divisor.
- **What the code does:** debug builds abort with `thread 'main' panicked at src/parser/json_schema.rs:5674:15: attempt to multiply with overflow`; a release build (overflow checks off) wraps and emits a **negative** divisor into all four generated validators.
- **Concrete failing input:**
  ```yaml
  allOf:
    - { type: integer, multipleOf: 4611686018427387904 }
    - { type: integer, multipleOf: 3 }
  ```
  → panic (verified).
- **Confidence:** high for the panic (executed). The release-mode wrap-around behavior is **unverified** (not built in release); the divisibility *set* would be unchanged by a sign flip, so the practical impact there is a nonsense `reason` string.

### 7. Java's `reason` trims the bound's suffix; the others interpolate it verbatim

- **Severity:** P2
- **Spec cite:** `maximum.md:88-97` (reason names the concrete bound and value — satisfied); **P11** explicitly does *not* require byte-identical text.
- **Code cite:** `src/generator/json_schema/java.rs:996-1006` (`trim_java_bound` strips a trailing `L` or `.0`) vs `go.rs:124-131` / `typescript.rs:277-282` / `python.rs:101-106` (raw literal).
- **What the code does:** for `{type:"number", multipleOf: 2e1}` Java reports `must be a multiple of 20` while Go/TS/Python report `must be a multiple of 20.0`. All four name the bound and the value, so the mandate is met; this is cosmetic drift only.
- **Confidence:** high (`tests/generate_java.rs:666` asserts `value.edgeNumber % 20.0 != 0` and the Python conformance test at `tests/generate_python.rs:266` asserts the reason text `must be a multiple of 20.0`).

### 8. The "must be a finite number" bound diagnostic looks unreachable

- **Severity:** P2
- **Code cite:** `src/parser/json_schema.rs:1825-1831`.
- **What the code does:** the arm fires only when `schema.extra[key]` is a `serde_json::Number` whose `as_f64()` is non-finite — but `serde_json::Number` cannot hold a non-finite value (no `arbitrary_precision` feature; `Cargo.toml:45`), so a YAML `.inf` or `1e400` arrives as a non-number and takes the generic `` `minimum` must be a number `` path instead. Verified: both `minimum: .inf` and `minimum: 1e400` produce `` `minimum` must be a number ``.
- **Confidence:** high for the observed diagnostic; the claim that the arm is *strictly* dead is **unverified** (a JSON front-end path could in principle differ).

## Testing gaps

### 1. No numeric-bound case in the cross-language conformance manifest

- **Severity:** P0
- **Untested:** that the four targets agree, value-for-value, on any bound. `samples/conformance/json-schema.json` has 4 cases (`recursive-collections`, `mathematical-number-equality`, `year-zero-rejection`, `optional-null-presence-collapse`) — none exercises `minimum`/`maximum`/`exclusive*`/`multipleOf`. This is precisely the mechanism that would have caught divergences 1–3.
- **Spec line mandating it:** `maximum.md:99-111` (Cross-language exactness), `multipleOf.md:108-112`, **P1**.
- **Where:** `samples/conformance/json-schema.json`, checked by `tests/json_schema_conformance_manifest.rs`, with per-language anchors in `samples/{go,python,typescript,java}` suites.
- **Suggested case:** id `numeric-bound-boundaries` over `samples/schemas/showcase.nexusrpc.yaml` — `accepted_wire_values` pinning `priority:1`, `priority:10`, `ratio:5`, `step:0`; `parse_failures` for `priority:0`, `priority:11`, `level:0`, `ratio:4.999999999999999`, `step:1`; plus a `multipleOf`-on-`number` case with `1e23`/`1e300` once finding 2 is settled.

### 2. No test asserts an *inclusive* boundary value is accepted

- **Severity:** P1
- **Untested:** `v == minimum` and `v == maximum` round-trip. Every existing runtime fixture uses interior values (`showcase-metrics.json` has `priority: 5` against `[1,10]`) or out-of-range values (`99`, `42`, `-1`).
- **Spec line:** `maximum.md:150` ("`v == max` → OK (`≤` is inclusive)"), `minimum.md:108` ("`v == min` → OK").
- **Where:** all four sample suites (`samples/go/tests/json_schema_showcase_test.go`, `samples/typescript/tests/json-schema-showcase.test.ts`, `samples/python/tests/test_showcase.py`, `samples/java/src/test/java/jsonschema/JsonSchemaShowcaseRoundTripTest.java`) plus the manifest case above.
- **Suggested case:** parse `{"priority":1}` and `{"priority":10}` and assert no violation; parse `{"ratio":5}` (exactly `minimum` **and** exactly on the `multipleOf` grid) and assert no violation.

### 3. Java has no runtime exclusive-boundary test

- **Severity:** P1
- **Untested:** that Java rejects `v == exclusiveMinimum` / `v == exclusiveMaximum` at runtime. `tests/generate_java.rs:665-666` only asserts the *source string* `value.edgeNumber >= -1.0`; the sample suite never exercises `level` (`exclusiveMinimum: 0`) — `samples/java/.../JsonSchemaShowcaseRoundTripTest.java:158` only reads `getLevel()` on a valid payload. TypeScript (`json-schema-showcase.test.ts:260`) and Python (`test_showcase.py:310`) both assert `level: 0` → `must be > 0, got 0`; Go has an equivalent only in `tests/generate_go.rs:2711`.
- **Spec line:** `exclusiveMinimum.md:100` ("`v == exclMin` → **reject** (strict; boundary excluded) — the key difference from `minimum`").
- **Where:** `samples/java/src/test/java/jsonschema/JsonSchemaShowcaseRoundTripTest.java`.
- **Suggested case:** deserialize `{…,"level":0}` and assert the message chain contains `must be > 0, got 0`; and Go: add the same `level: 0` assertion to `samples/go/tests/json_schema_showcase_test.go` so all four sample suites match.

### 4. `multipleOf` on a `number` field is untested against large integral doubles

- **Severity:** P0 (this is what hides finding 2)
- **Untested:** `1e23`, `1e300`, and any value where the shortest decimal and the stored binary64 differ. `multipleOf.md:112` claims `1e300` was "verified" across all four; nothing in the repo verifies it.
- **Spec line:** `multipleOf.md:42` ("`1e300` accepted for divisor `2`"), `multipleOf.md:147` ("`1e300`-if-divisible"), `multipleOf.md:112`.
- **Where:** the conformance manifest, plus `tests/generate_go.rs` / `tests/generate_typescript.rs` runtime blocks.
- **Suggested case:** `{type:"number", multipleOf:3}` with wire `1e300` (must have the same verdict in all four) and `{type:"number", multipleOf:5}` with wire `1e23`.

### 5. `number`-field bound vs. an integral wire token above 2^53

- **Severity:** P0 (hides finding 3)
- **Untested:** any `number` field whose bound and wire value straddle the 2^53 boundary.
- **Spec line:** **P1**; `maximum.md:99-111` argues exactness only for `integer` fields, leaving `number` unaddressed.
- **Where:** conformance manifest + all four sample suites.
- **Suggested case:** `{type:"number", maximum: 9007199254740992}` with wire `9007199254740993`.

### 6. `multipleOf` + range emptiness on a `number` field (loader)

- **Severity:** P1
- **Untested:** no test covers finding 4. `src/parser/json_schema.rs:8117` (`rejects_unsatisfiable_integer_range_with_multiple_of`) covers only the `integer` path.
- **Spec line:** `multipleOf.md:141` (Rejected-at-load matrix), `maximum.md:184-185`.
- **Where:** `src/parser/json_schema.rs` inline tests, next to `rejects_unsatisfiable_integer_range_with_multiple_of`.
- **Suggested case:** `numeric_reject("type: number\nminimum: 1\nmaximum: 2\nmultipleOf: 5")` expecting `no multiple of`.

### 7. Integer interval containing no integer

- **Severity:** P1
- **Untested:** `{type:"integer", exclusiveMinimum:1, exclusiveMaximum:2}` — the flagship example in `maximum.md:179-181`, `exclusiveMinimum.md:96`, `exclusiveMaximum.md:104`. The loader does reject it (verified by probe), but no test pins it, and it exercises a different arm of `json_schema.rs:1912-1918` than the tested `minimum:10, maximum:2`.
- **Where:** `src/parser/json_schema.rs` inline tests.
- **Suggested cases:** the pair above, plus the `number` variants `{type:"number", minimum:5, exclusiveMaximum:5}`, `{type:"number", exclusiveMinimum:5, maximum:5}`, `{type:"number", exclusiveMinimum:5, exclusiveMaximum:5}` (all currently rejected, none tested).

### 8. `enum` member violating a bound

- **Severity:** P1
- **Untested:** `src/parser/json_schema.rs:1982-1993` (the `enum` arm of the literal check) has no test. `rejects_const_violating_bound:8082` and `rejects_default_violating_bound:8112` cover `const`/`default` only.
- **Spec line:** `maximum.md:190-196`, `minimum.md:127-130`, `multipleOf.md:166-169` ("`const` / `default` / `enum`").
- **Where:** `src/parser/json_schema.rs` inline tests.
- **Suggested case:** `numeric_reject("type: integer\nminimum: 5\nenum: [2, 7]")` expecting `` `enum` value 2 violates ``.

### 9. Positive-acceptance matrix for the loader is almost entirely untested

- **Severity:** P2
- **Untested:** every row of the four "Accepted (positive)" tables. Specifically: `.0`-valued bounds on an `integer` field (`minimum: 0.0`, `maximum: 10.0` — `maximum.md:133`, `minimum.md:92`), `multipleOf: 2.0` (`multipleOf.md:130`), `minimum == maximum` single-value range (`maximum.md:135`), a bound beyond the ±(2^53−1) cap being *allowed* (`maximum.md:51-53`), and a fractional bound on a `number` field (`minimum:-1.5`, `maximum.md:134`).
- **Where:** `src/parser/json_schema.rs` inline tests (a `numeric_accept` helper mirroring `numeric_reject`) plus one emission assertion per language that `minimum: 0.0` renders as the integer literal `0` (it does — verified across all four).
- **Suggested case:** accept `{type:"integer", minimum:0.0, maximum:10.0, multipleOf:2.0}` and assert the emitted Go/TS/Python literals are `0`/`10`/`2` and Java's are `0L`/`10L`/`2L`.

### 10. Mirror-keyword rejects are only tested on one side

- **Severity:** P2
- **Untested:** `rejects_fractional_bound_on_integer_field:8046` covers `maximum: 5.5` only; `minimum: 0.5` and `exclusiveMaximum: 5.5` have no test (`exclusiveMinimum: 0.5` is rejected — verified by probe — but untested). `rejects_zero_multiple_of:8058` covers `0` but not `-2`. `rejects_numeric_bound_on_string_field:8076` covers `type: string` but not `type: boolean`.
- **Spec line:** the "Rejected at load time" table of each of the five specs.
- **Where:** `src/parser/json_schema.rs` inline tests.

### 11. Signed-zero and non-finite values at a bound

- **Severity:** P2
- **Untested:** `-0.0` against `{type:"number", minimum: 0}` (must be accepted — `+0 == -0` per **P1**) and against `{type:"number", exclusiveMinimum: 0}` (must be rejected). Also `NaN`/`Infinity` interaction with a bound (all four reject via the finiteness check first, but the ordering differs: TS/Python nest the bounds in an `else`, Go/Java run them unconditionally after — same verdict, different violation counts).
- **Spec line:** **P1** ("positive/negative zero compare equal").
- **Where:** the four sample suites; `tests/generate_*.rs` runtime blocks.

### 12. `multipleOf: 0.1` fractional-divisor reject is tested only once

- **Severity:** P2
- **Untested:** `rejects_fractional_multiple_of:8064` covers `{type:"number", multipleOf:0.1}`. The spec's other listed forms (`multipleOf: 2.5`, `{type:"integer", multipleOf: 0.5}`) are untested, and there is no test that the diagnostic is *distinct* from the `≤ 0` one (`multipleOf.md:66-69` requires exactly that).

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| bound × **nullable** (`oneOf` + `type:"null"`) | branch keywords enforced (`nullability.md:178`) | **no** | **P0** — Java drops the bound, Go emits non-compiling code (finding 1) |
| `multipleOf` × **`number` field, large doubles** | `fmod`, identical in all four (`multipleOf.md:97,112`) | **no** | **P0** — Go uses exact-rational; two-way divergence (finding 2) |
| bound × **`number` field, integral token > 2^53** | identical accept/reject (**P1**) | **no** | **P0** — Python compares exactly, others round (finding 3) |
| `multipleOf` × range, **`number` field** | reject if no multiple in interval (`maximum.md:184`) | **no** | **P1** — not implemented (finding 4) |
| `multipleOf` × `const`/`default`/`enum`, **large literals** | literal must be a multiple at load (`multipleOf.md:166`) | **no** | **P1** — quotient-`fract()` false-negative (finding 5) |
| `multipleOf` × `allOf` (LCM) | intersect/tighten (`allOf.md`) | LCM merge tested (`json_schema.rs:9539`) | **P1** — overflow panics (finding 6) |
| `exclusiveMinimum` × `exclusiveMaximum`, integer, no integer between | load reject (`maximum.md:179`) | **no** (implemented) | P1 — untested arm of the emptiness check |
| `minimum` × `maximum` (`min > max`) | load reject | yes (`json_schema.rs:8040`) | low |
| `minimum` × `exclusiveMinimum` same node | load reject | yes (`:8100`) | low |
| `maximum` × `exclusiveMaximum` same node | load reject | yes (`:8070`) | low |
| bound × `allOf` tightening | intersect, collapse same-axis pair | yes (`:9509`, `:9527`, showcase `size` → `[10,20]`) | low |
| bound × `const` / `default` | literal must satisfy | yes for `const`/`default` (`:8082`, `:8112`) | low |
| bound × `enum` member | member must satisfy | **no** (implemented) | P1 — untested arm |
| bound × `type: integer` + 2^53 cap | over-cap bound allowed, redundant | **no** | P2 — all four render the same f64-rounded literal; dead range, verified harmless |
| bound × `contains` matcher | matcher carries the bound | yes — Python/Java cover both inclusive **and** exclusive matchers (`generate_python.rs:105-120`, `generate_java.rs:140-156`); Go/TS cover `minimum`+`exclusiveMaximum` only (`generate_go.rs:155-163`) | P2 — Go/TS lack an inclusive-`maximum` / exclusive-`minimum` matcher case |
| bound × `contains` matcher boundary value | matcher accepts/rejects at the bound | Python only (`inclusiveNumbers: [2, 8]`) | P2 |
| bound × `propertyNames` | N/A (string matcher; numeric bound is a load reject) | reject tested via `type: string` gate | low |
| bound × array `items` | bound applies per element | yes (showcase `quotas`, `matchedNumbers`) | low |
| bound × typed `additionalProperties` | bound applies per map value | yes (showcase, `models.py:1087-1119`) | low |
| bound × non-null `oneOf` branch | branch constraints enforced per selected branch | yes (showcase `idOrName`, `mode`) | low |
| bound × serialize direction (P12) | re-runs before emit | yes, all four | low |
| bound × aggregation (P11) | all violations in one shot | yes (`generate_python.rs:263-270` asserts a 3-violation list) | low |
| `multipleOf: 0.1` × `number` | load reject, deferred | yes (`:8064`) | low |
| draft-4 boolean `exclusive*` | load reject with rewrite fix-it | yes (`:8052`, `:8106`) | low |

## Verified-good

- **Loader type gate** (`json_schema.rs:1809-1818`): `{type:"string", maximum:5}`, `{type:"boolean", minimum:1}`, `{type:"string", multipleOf:2}` and a bound on a bare/typeless node all reject with the P7.1 fix-it. A bound inside a `contains` matcher over a string element also rejects (`json_schema.rs:2632`).
- **Draft-4 boolean form** (`:1832-1844`): both `exclusiveMaximum: true` and `exclusiveMaximum: false` produce the rewrite diagnostic; `minimum: "0"`, `maximum: null`, `multipleOf: true` produce "must be a number".
- **Integer-valued bounds on `integer` fields** (`:1861-1876`): `maximum: 5.5` and `exclusiveMinimum: 0.5` reject; `minimum: 0.0` / `maximum: 10.0` / `multipleOf: 2.0` accept and render as the integer literals `0`/`10`/`2` in Go, TypeScript and Python, and `0L`/`10L`/`2L` in Java — checked by generating all four.
- **Same-axis redundancy** (`:1893-1902`): both `minimum: 0, exclusiveMinimum: 0` and `minimum: 0, exclusiveMinimum: 2` reject with "specify exactly one".
- **Interval emptiness** (`:1904-1924`): correct for `integer` (`exclusiveMinimum:1, exclusiveMaximum:2` → reject) and for `number` (`minimum:5, exclusiveMaximum:5`, `exclusiveMinimum:5, maximum:5`, `exclusiveMinimum:5, exclusiveMaximum:5` all reject); `minimum == maximum` on an integer accepts.
- **`multipleOf` positivity and fractional deferral** (`:1879-1890`): `0` and `-2` reject with the `> 0` message; `0.1` rejects with the distinct "not yet supported" message.
- **`const`/`default` vs. bounds** (`:1969-1981`), including through a nullable wrapper: `{oneOf:[{type:integer,minimum:5},{type:"null"}], default: 0}` **is** rejected (`validate_default` reaches the branch) — better than the letter of the spec requires.
- **allOf bound merging**: `numeric_extreme` (`:5624-5645`) keeps the tighter bound; `collapse_numeric_pair` (`:5756-5767`) correctly prefers the exclusive bound on a tie in both directions; `merge_multiple_of` computes the LCM (correct apart from the overflow in finding 6). Covered by `json_schema.rs:9509`, `:9527`, `:9539`, `:9657` and the showcase `size` field.
- **Reason-text quality**: all four targets name the concrete bound *and* the offending value in every arm — `go.rs:176-215`, `typescript.rs:309-343`, `python.rs:1533-1547`, `java.rs:235-269`. No bare-keyword reason exists anywhere in this family.
- **Serialize-side re-check (P12)**: exercised for bounds and `multipleOf` in Go (`tests/generate_go.rs:2724`), TypeScript (`tests/generate_typescript.rs:2181`), Python (`tests/generate_python.rs:269`) and Java (`samples/java/.../JsonSchemaShowcaseRoundTripTest.java:1341`).
- **Aggregation (P11)**: `tests/generate_python.rs:263-270` asserts a bound violation, a `multipleOf` violation and a sibling-field violation are reported together in one `ValidationError`, in both directions.
- **Integer-field divisibility exactness**: Go `int64 %`, Java `long %`, Python `int %` and TypeScript safe-integer `%` all agree within the ±(2^53−1) cap; the negative-dividend sign difference between Python's `%` and C `fmod` does not affect the `== 0` test.
