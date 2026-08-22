# contains / minContains / maxContains — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/contains.md` — the array existential; supported for a **scalar matcher over a scalar `items` element type**, composite matcher/element deferred at load; pure runtime predicate in the shared `Validate` (P10/P11/P12), no type impact.
- `specs/json-schema/features/minContains.md` — inclusive lower bound on the *match count*; omitting ≡ `1`; `minContains: 0` relaxes the existential and is a load reject without a sibling `maxContains`; requires a sibling `contains`.
- `specs/json-schema/features/maxContains.md` — inclusive upper bound on the *match count*; owns the shared tally machinery and the pair-satisfiability rules (`min > max` reject, `min == max` exact pin, `maxContains: 0` needs `minContains: 0`).

All findings below were reproduced by generating code with `target/debug/nexgen` from probe schemas; every code claim cites a line I read.

## Summary

- `contains` is **supported**, not load-rejected, in all four backends, in every position I could reach (declared property, typed-map value, `oneOf` array branch, `allOf`-merged node) and in **both** directions (raw wire scan on deserialize, typed scan on serialize).
- The loader's *shape* rules are in very good order: matcher shape, composite matcher/element deferral, kind compatibility, the sibling-`contains` requirement, pair satisfiability, `minContains: 0` vacuity, and the `allOf` merge rules all behave as specified.
- The **"does this element match?" predicate is NOT implemented identically across the four targets** — this is the headline. Each backend derives the matcher's effective *kind* differently (Go: no type guard at all; TS: matcher's declared `type` only; Python: matcher `type` → first `const`/`enum` literal's kind → element type; Java: matcher `type` → element type), and each derives the `is_integer` flag for bound literals from a different source. I found **four independently reproducible P0 divergences** from this.
- Worst case: `{items:{type:number}, contains:{enum:[2, 1.5]}}` + wire `[1.5]` — accepted by Go/TS/Java, **rejected by Python**, and the outcome flips if you reorder the enum members.
- A fractional matcher bound over an `integer` element type is **silently truncated** by Go, TS and Java's *typed* path (`minimum: 1.5` → `>= 1`) while Python and Java's *raw* path compare against `1.5` — so Java disagrees with **itself** across the P12 boundary.
- Go's `integer` matcher over `number` elements omits the ±(2^53−1) cap the other three enforce.
- `contains`/`minContains`/`maxContains` on a **nullable array member** (`oneOf: [{type: array, …, contains: …}, {type: "null"}]`) load fine but are **silently dropped by Java** (and Go, whose output for that shape does not even compile) while TS/Python enforce them.
- TypeScript's raw-side scan emits **no element type guard** for a matcher that omits `type`, so `contains: {minLength: 2}` over a `string` array throws a raw `TypeError` (not a `ValidationError`) on a wire array holding a non-string — a P11 break.
- `minContains: 2.0` / `maxContains: 2.0` are **spec-mandated accepts** (both matrices list them) but are rejected with an internal-looking diagnostic.
- Loader *positive* cases are badly under-tested: no test anywhere asserts `minContains: 0`+`maxContains: N`, `maxContains: 0` (must-not-contain), `min == max` exact pin, or the `.0`-valued bounds — at load **or** at runtime, in any language.
- `samples/conformance/json-schema.json` has **no** `contains` case, so the four round-trip suites are only cross-checked on `contains` by convention (the showcase `roles` field), not by the manifest gate.

---

## Implementation divergences

### 1. Python derives the matcher type guard from the first `const`/`enum` literal's kind — a mixed-kind enum matcher diverges from all three other targets
- **Severity:** P0
- **Spec cite:** `contains.md` §Support decision / P1 ("the matcher runs over values whose kind every target agrees on value-for-value"); `contains.md` §Interactions → `const`/`enum` ("They share the **scalar value-equality** definition used here").
- **Code cite:** `src/generator/json_schema/python.rs:4946-4958` (`kind` = matcher `type` → `const_value` kind → `enum_values.first()` kind → element type) and `python.rs:4960-4971` (that kind emits the `Integer` guard `abs(e) <= 9007199254740991 and float(e).is_integer()`). Compare `src/generator/json_schema/typescript.rs:776-789` (guard from `matcher.kind` only, i.e. the declared `type`), `src/generator/json_schema/java.rs:811-835` (`matcher_kind` = declared `type`, falling back to the **element** type), `src/generator/json_schema/go.rs:520-527` (no type guard emitted at all).
- **What the spec requires:** one shared matcher predicate, identical value-for-value in every target.
- **What the code does:** for a typeless matcher, Python picks the guard kind from the *first* enum member's JSON kind. A mixed-kind enum whose first member is integer-valued therefore gets an integer-only guard, excluding the non-integral members.
- **Concrete failing input:**
  ```yaml
  type: object
  required: [e]
  properties:
    e: { type: array, items: { type: number }, contains: { enum: [2, 1.5] } }
  ```
  wire `{"e":[1.5]}`. Generated: Go `if (e == 2 || e == 1.5)` → match; TS `(element === 2 || element === 1.5)` → match; Java `Double.isFinite(element) && ((element == 2.0 || element == 1.5))` → match; **Python** `… and abs(element) <= 9007199254740991 and float(element).is_integer() and (…)` → `float(1.5).is_integer()` is `False` → no match → `ValidationError("e", "no element matches the required schema")`. Swapping the enum to `[1.5, 2]` makes Python agree again.
- **Confidence:** high (generated and read the emitted predicate in all four).

### 2. A fractional matcher bound is truncated when the *element* type is `integer` — Go/TS/Java-typed vs Python/Java-raw disagree, and Java disagrees with itself across P12
- **Severity:** P0
- **Spec cite:** `contains.md` §Validator mapping ("Each element is tested against the scalar matcher predicate — the same shared predicate the matcher's own keywords define ([[minimum]]/[[maximum]] range …)"); `contains.md` §Serialize-side (P12) ("the identical predicate in both directions").
- **Code cite:** `src/generator/json_schema/go.rs:522-523` and `go.rs:545-556` with `go_bound_literal` at `go.rs:124-131` (`is_integer` ⇒ `(value.trunc() as i64)`); `src/generator/json_schema/typescript.rs:775-776` + `typescript.rs:277-282`; `src/generator/json_schema/java.rs:738` + `java.rs:204-217` (typed path) versus `java.rs:941-967` (raw path picks `evaluation_ty` from the **matcher** kind, so `is_integer` is `false` there); `src/generator/json_schema/python.rs:4993` (`is_integer = kind == Some(ScalarKind::Integer)`, i.e. the matcher kind).
- **What the spec requires:** the matcher's own `minimum` defines "match": an element matches iff `element >= 1.5`.
- **What the code does:** Go, TS and Java's typed `Validate` compute `is_integer` from the *element* type and truncate the bound literal to `1`; Python and Java's raw deserializer compare against `1.5`. The loader only rejects fractional bounds when the *node's own* `type` is `integer` (`src/parser/json_schema.rs:1861-1876`), and the matcher declares `type: number`, so the schema loads.
- **Concrete failing input:**
  ```yaml
  type: object
  required: [b]
  properties:
    b: { type: array, items: { type: integer }, contains: { type: number, minimum: 1.5 } }
  ```
  wire `{"b":[1]}`. Emitted: Go `if e >= 1`, TS `element >= 1`, Java `Serializer`: `element >= 1L`, Java `Deserializer`: `rawElement.doubleValue() >= 1.5`, Python `element >= 1.5`. So Go and TS **accept**; Python **rejects**; Java **rejects on deserialize and accepts on serialize** (a straight P12 violation inside one target).
- **Confidence:** high (all four emissions read from generated output).

### 3. Go's `integer` matcher over `number` elements omits the ±(2^53−1) integer cap
- **Severity:** P0
- **Spec cite:** `contains.md` §Support decision + P1 (a scalar matcher must be the same comparison in every target); `type.md`'s integer cap is what the other three apply here.
- **Code cite:** `src/generator/json_schema/go.rs:525-527` — the only extra term for an `integer` matcher over a `number` element is `math.Trunc({elem}) == {elem}`; the raw path is the same predicate at `go.rs:764-768`. Compare `typescript.rs:786-788` (`Number.isSafeInteger`), `python.rs:4967-4969` (`abs(e) <= 9007199254740991 and float(e).is_integer()`), `java.rs:830-832` (`Double.isFinite && == Math.rint && >= -INTEGER_CAP && <= INTEGER_CAP`) and `java.rs:955-959` (raw: `SpecNumbers.isSpecLong`).
- **What the spec requires:** identical accept/reject sets across targets.
- **What the code does:** Go treats any integral `float64` as an integer match, however large.
- **Concrete failing input:**
  ```yaml
  type: object
  required: [a]
  properties:
    a: { type: array, items: { type: number }, contains: { type: integer } }
  ```
  wire `{"a":[1e300]}` — `1e300` passes `items` (finite number) in all four. Go: `math.Trunc(1e300) == 1e300` → match → **accepted**. TS/Python/Java: no match → `no element matches the required schema` → **rejected**.
- **Confidence:** high (generated Go: `if math.Trunc(e) == e {`; generated TS: `Number.isSafeInteger(element)`).

### 4. `contains`/`minContains`/`maxContains` on a nullable array member are silently dropped by Java (and Go, which emits non-compiling code)
- **Severity:** P0
- **Spec cite:** `contains.md` §Interactions → `nullability` ("orthogonal — `required` decides whether the array member is present; `contains` shapes its value"); `minContains.md` §Interactions → `uniqueItems`/`required`/`nullability` ("orthogonal").
- **Code cite:** `src/generator/json_schema/java.rs:96-106` (`ArrayConstraints::from_schema` reads `schema.contains` off the node verbatim) called at `java.rs:1964` with the **`oneOf` wrapper** as `property`, so all array constraints come back empty and the guards at `java.rs:3134` / `java.rs:4019` (`if !field.array.is_empty()` / `if !array.is_empty()`) skip emission entirely. TypeScript unwraps the nullable node before dispatching its field checks (`typescript.rs:956`, `typescript.rs:1132`, `typescript.rs:3989` — all `if let Some(non_null) = nullable_non_null_schema(schema)`) and Go's raw array emitter has an unwrap too (`go.rs:745-748`), but Go never reaches it for this shape.
- **What the spec requires:** a present, non-null array must still satisfy `contains`.
- **What the code does:** Java emits no `contains` scan in either direction; Go emits `N []string` plus `parseStringField(...); m.N = &v` (a `*string` assigned to a `[]string`) — the package does not compile, so `contains` is unreachable there too. TS and Python emit and enforce the check normally.
- **Concrete failing input:**
  ```yaml
  type: object
  properties:
    n:
      oneOf:
        - { type: array, items: { type: string }, contains: { const: x }, maxContains: 2 }
        - { type: "null" }
  ```
  wire `{"n":["y"]}` → TS/Python raise `ValidationError` (`no element matches the required schema`); Java accepts silently; Go does not build.
- **Note:** the Java drop is *general* to array constraints under nullability — I confirmed `minItems: 1` on the same shape is dropped too — so it is shared with the `minItems`/`maxItems`/`uniqueItems` group, but it directly nullifies my keywords.
- **Confidence:** high (generated all four; read `P5.java`'s `Serializer`/`Deserializer` in full — no `matchCount`).

### 5. TypeScript's raw-side matcher has no element type guard when the matcher omits `type` — throws a bare `TypeError` instead of aggregating
- **Severity:** P0
- **Spec cite:** P11 ("Aggregate validation errors … one aggregating error type per language holding a list of `Violation { path, reason }`"); PRINCIPLES TypeScript §3 (throw **one** `ValidationError`); `contains.md` §Validator mapping ("deserialize scans the raw array … with the same scalar matcher predicates").
- **Code cite:** `src/generator/json_schema/typescript.rs:776-789` — the guard is emitted only for `matcher.kind`, which is the matcher's *declared* `type`; there is no fallback to the element type (contrast `python.rs:4952-4957` and `java.rs:811-823`, both of which fall back to the element type, and `go.rs:758-779`, which pre-parses the raw element into the element's static type before evaluating). The unguarded string predicates are emitted at `typescript.rs:832-838` (`[...{elem}].length …`).
- **What the spec requires:** every rejection surfaces as one aggregated `ValidationError`.
- **What the code does:** emits `raw.names.filter((element) => [...element].length >= 2)` over the *raw* array. Spreading a number/boolean/null throws.
- **Concrete failing input:**
  ```yaml
  type: object
  required: [names]
  properties:
    names: { type: array, items: { type: string }, contains: { minLength: 2 } }
  ```
  wire `{"names":[5]}` → generated `models.ts:43`: `const matchCount = raw.names.filter((element) => [...element].length >= 2).length;` → `TypeError: e is not iterable` escapes `fromTransferType` (verified in node: `[5].filter(e=>[...e].length>=2)` throws `TypeError`). Go/Python/Java return an aggregated `ValidationError` with `names[0]: expected string`.
  A related but milder case from the same root: `contains: {minimum: 5}` over `items:{type:integer}` emits `raw.nums.filter((element) => element >= 5)`, and JS coerces — `"9" >= 5` is `true` — so TS's match **count** differs from the other three on any wire array holding non-integers (visible in the aggregated violation set, and would change accept/reject if a `maxContains` were present).
- **Confidence:** high (generated `/tmp/cp2/out-ts/models.ts:43,71`; confirmed the throw in node).

### 6. `minContains: 2.0` / `maxContains: 2.0` are spec-mandated accepts but are rejected with an internal-flavored diagnostic
- **Severity:** P1
- **Spec cite:** `minContains.md:62` ("`minContains: 2.0` accepted (≡ `2`)") and its matrix row "`.0`-valued bound"; `maxContains.md:61` ("`maxContains: 2.0` is accepted (≡ `2`, honoring the `1.0`-as-integer rule from [[type]])") and its matrix row.
- **Code cite:** the loader accepts it — `src/parser/json_schema.rs:2447-2473`, whose `bound()` admits any `value.fract() == 0.0`. The generator-side model then refuses it: `src/generator/json_schema/go.rs:73-76`, `typescript.rs:233-236`, `python.rs:79-82`, `java.rs:73-76` all declare `min_contains/max_contains: Option<u64>`, and serde rejects the float.
- **What the spec requires:** accept, normalized to the integer.
- **What the code does:** `nexgen go --output … in` → ``invalid JSON schema in `<go-json-generator>`: failed to read planned JSON schema `S`: invalid type: floating point `2.0`, expected u64``.
- **Concrete failing input:** `{type: array, items: {type: string}, contains: {const: x}, maxContains: 2.0}`.
- **Note:** the same failure occurs for `minItems: 2.0`, `maxItems: 2.0` and `minLength: 2.0`, so the fix belongs in the planned-schema deserializer (accept `.0` floats for every count keyword), not in my keywords alone.
- **Confidence:** high (reproduced for all four keywords).

### 7. A nullable *element* type plus `contains` is load-rejected, though `contains.md` specifies its semantics
- **Severity:** P1
- **Spec cite:** `contains.md` §Interactions → `nullability`: "if the element schema is the nullable [[nullability]] pattern, a `null` element matches `contains` only if the matcher itself is the null pattern … Otherwise a `null` element simply never matches a scalar matcher — orthogonal."
- **Code cite:** `src/parser/json_schema.rs:2487-2493` computes `items_kind` from `items.ty.as_str()`, which is `None` for the `oneOf`-nullability wrapper, so `items_is_scalar` is false and `src/parser/json_schema.rs:2595-2599` rejects with "``contains`` over a composite element type is not yet supported".
- **What the spec requires:** the combination is described as working, with a defined null-element rule.
- **What the code does:** rejects at load. Corroborating evidence that this was meant to work: `src/generator/json_schema/go.rs:745-748` deliberately unwraps `nullable_non_null_schema(items)` to pick the element type for the raw contains scan — that branch is unreachable today.
- **Concrete failing input:** `{type: array, items: {oneOf: [{type: string}, {type: "null"}]}, contains: {const: x}}` → rejected. (The same array **without** `contains` loads fine, and `uniqueItems: true` rejects it the same way — so the reject is at least internally consistent with the `uniqueItems` policy; it is `contains.md`'s Interactions text that is out of step.)
- **Confidence:** high; whether the intent is "loosen the loader" or "fix the spec text" is a judgement call I cannot make from the code.

### 8. Violation reasons never use the matcher-specific text the spec mandates
- **Severity:** P2
- **Spec cite:** `contains.md:135-139` — "Reason strings name **what was required**, not a bare keyword — the matcher is described by its own constraint (`no element equals \"admin\"` for a `const` matcher, `no element matches minimum 5` for a range matcher, `no element matches the required schema` as the general fallback)".
- **Code cite:** `go.rs:690-693`, `go.rs:789-792`, `typescript.rs:929-932`, `java.rs:897-900` and `java.rs:975-979`, `python.rs:1394-1397` (the `bounded_min == False` arm of `_check_contains`) — every one emits only the string `"no element matches the required schema"`.
- **What the code does:** always the general fallback; the `const`- and range-specific wordings are never produced.
- **Note:** P11 explicitly does not hold reason *text* identical across targets, so this is polish rather than a wire break — but it contradicts the repo's own informative-reason convention (which the `minContains`/`maxContains` count messages do follow correctly).
- **Confidence:** high.

### 9. A typeless `pattern` / `format` matcher is rejected while typeless `minimum` / `minLength` / `const` / `enum` matchers are accepted; the loader's own inference branch for `pattern`/`format` is dead code
- **Severity:** P2
- **Spec cite:** `contains.md` §Loader behavior lists only `{}` / `true` / `false` as the shapeless forms; the accepted matrix's range examples (`contains: {minimum: 5}` in `minContains.md:131` and `maxContains.md:126`) are typeless, establishing that a typeless-but-constrained matcher is in-envelope.
- **Code cite:** `src/parser/json_schema.rs:2619-2624` infers `type: "string"` for a matcher carrying `minLength`/`maxLength`/`pattern`/`format` — but (a) `pattern` is rejected earlier, during the normalize pass, at `src/parser/json_schema.rs:5131-5136` (`normalize_pattern` requires `type: "string"` on the node itself), and (b) the `matcher_has_assertion` allowlist at `src/parser/json_schema.rs:2641-2650` omits both `pattern` and `format`, so a typeless `format` matcher falls through to the shapeless diagnostic.
- **What the code does:**
  - `contains: {pattern: "^x"}` over `items: {type: string}` → ``…contains: `pattern` requires `type: string` ``.
  - `contains: {format: email}` over `items: {type: string}` → ``…: `contains` must be a schema object with a scalar matcher (a bare `{}`/`true`/`false` is not a matcher — use `minItems`)`` — a diagnostic that describes a different problem.
  - `contains: {minLength: 2}` / `{minimum: 5}` / `{const: x}` / `{enum: [a,b]}` → accepted.
  The `pattern`/`format` arms of the inference at 2621-2622 can never fire.
- **Confidence:** high (all four probed).

### 10. A type-only matcher equal to the element type is accepted and lowers to a literal `true`
- **Severity:** P2
- **Spec cite:** `contains.md` §Loader behavior, Shapeless matcher: "`{}` / `true` match every element (so `contains` degenerates to 'non-empty' — the diagnostic points at `minItems: 1`)".
- **Code cite:** `src/parser/json_schema.rs:2641` (`scalar_type(matcher_ty).is_some()` alone satisfies `matcher_has_assertion`); `src/generator/json_schema/go.rs:592-596` (empty condition set renders as `"true"`).
- **What the code does:** `{type: array, items: {type: string}, contains: {type: string}}` loads and Go emits `for _, e := range m.S { if true { matchCount++ } }` — the exact "non-empty" degeneration the spec rejects for `contains: {}`, spelled differently. Not a cross-language divergence (TS/Python/Java each emit their element type guard, which is also always true for the typed slice), so it is only a missed reject.
- **Confidence:** high.

### 11. `contains` on a `type: array` with no `items` reports the wrong reason
- **Severity:** P2
- **Code cite:** `src/parser/json_schema.rs:2487-2493` — `items_kind` is `None` when `items` is absent, so `src/parser/json_schema.rs:2595-2599` fires.
- **What the code does:** `{type: array, contains: {const: x}}` → "``contains`` over a composite element type is not yet supported; deep matching is deferred (scalar `items` only)". The actual problem is the missing `items` (`contains.md:79-80`: "a `type: \"array\"` still requires [[items]]"), and the loader has a precise diagnostic for that case ("needs an explicit element type", `src/parser/json_schema.rs:11790`) that this reject pre-empts.
- **Confidence:** high.

### 12. The short-circuit the spec mandates for a plain `contains` is not implemented
- **Severity:** P2
- **Spec cite:** `contains.md:120-125` — "the scan **short-circuits on the first match** — *unless a [[maxContains]] or a [[minContains]] ≥ 2 is present*".
- **Code cite:** all four always tally: `go.rs:669-681`, `typescript.rs:914-917`, `python.rs:1386` (`_check_contains` uses `sum(1 for …)`), `java.rs:885-889`.
- **What the code does:** counts every element even for a bare `contains`. No observable behavior difference (no annotation consumer exists), so this is a spec-vs-implementation documentation mismatch only.
- **Confidence:** high.

---

## Testing gaps

### 1. No test exercises `minContains: 0` / `maxContains: 0` at runtime in any language
- **Severity:** P1
- **Untested:** the entire `0`-bound family — `minContains: 0, maxContains: N` (zero/one/N matches OK, N+1 fails) and `minContains: 0, maxContains: 0` (must-not-contain: zero matches OK, one match fails with `too many matching items: at most 0, got 1`).
- **Spec line mandating it:** `minContains.md:135` and `:157` (matrix + runtime fixtures); `maxContains.md:129-130`, `:149-152`.
- **Where:** the four wave-3 matrix schemas (`tests/generate_go.rs` `GO_WAVE3_MATRIX_SCHEMA`, `tests/generate_typescript.rs:~281`, `tests/generate_python.rs:~95`, `tests/generate_java.rs:~135`) plus one manifest case in `samples/conformance/json-schema.json`.
- **Suggested case:** add `mustNotContain: {type: array, items: {type: string}, contains: {const: x}, minContains: 0, maxContains: 0}` and assert `["a","b"]` passes while `["a","x"]` yields `too many matching items: at most 0, got 1` in both directions.

### 2. No test exercises the exact-count pin (`minContains == maxContains`)
- **Severity:** P1
- **Untested:** load acceptance and runtime behavior of `minContains: 2, maxContains: 2` (1 match → too few, 2 → OK, 3 → too many).
- **Spec line:** `minContains.md:134`, `maxContains.md:128`, `maxContains.md:68-72`.
- **Where:** `src/parser/json_schema.rs` inline tests (an `accepts_*` counterpart to `rejects_min_contains_above_max_contains`) + one wave-3 matrix field.
- **Suggested case:** `{contains: {const: y}, minContains: 2, maxContains: 2}` with `["y"]`, `["y","y"]`, `["y","y","y"]`.

### 3. No test asserts the matcher predicate agrees across languages for the same schema
- **Severity:** P0 (this is the gap that let divergences #1–#3 through)
- **Untested:** the four generator suites each use a *different* matcher schema (`tests/generate_go.rs:158-186`, `tests/generate_typescript.rs:308-339`, `tests/generate_python.rs:97-129`, `tests/generate_java.rs:139-186`), so no single wire value is ever run through all four matcher implementations. `samples/conformance/json-schema.json` has **zero** `contains` cases (only `recursive-collections`, `mathematical-number-equality`, `year-zero-rejection`, `optional-null-presence-collapse`), so the manifest gate (`tests/json_schema_conformance_manifest.rs`) does not cover `contains` at all. The only cross-language coverage is the showcase `roles` field (a plain `const` string matcher), asserted in all four round-trip suites.
- **Spec line:** P1 in `PRINCIPLES.md:14`; `contains.md` §Support decision ("the matcher runs over values whose kind every target agrees on value-for-value (**P1**)").
- **Where:** `samples/conformance/json-schema.json` + `samples/schemas/showcase.nexusrpc.yaml`.
- **Suggested case:** add a `matchers` object to the showcase with `{items:{type:number}, contains:{type:integer}}`, `{items:{type:integer}, contains:{type:number, minimum:1.5}}`, `{items:{type:number}, contains:{enum:[2,1.5]}}` and `{items:{type:string}, contains:{minLength:2}}`, plus manifest `parse_failures` for `[1e300]`, `[1]`, `[1.5]` and `[5]` respectively. Each of these fails today in at least one language.

### 4. No test covers `contains` on a nullable array member
- **Severity:** P0
- **Untested:** load + emission + runtime for `oneOf: [{type: array, …, contains: …}, {type: "null"}]`.
- **Spec line:** `contains.md` §Interactions → `nullability`; `minContains.md:184-186`.
- **Where:** each of the four `tests/generate_*.rs` conformance schemas, and a conformance manifest case.
- **Suggested case:** the schema in divergence #4 with wire `{"n":["y"]}` (must reject in all four) and `{"n":null}` (must accept in all four).

### 5. No test covers a matcher that omits `type`
- **Severity:** P0
- **Untested:** `contains: {minLength: 2}` / `{maxLength: N}` / `{minimum: N}` / `{const: v}` / `{enum: […]}` with no `type` — all load-accepted (I verified) but never generated or executed. This is what hides divergence #5.
- **Spec line:** the accepted matrices themselves use typeless matchers — `contains.md:170`, `minContains.md:131`, `maxContains.md:126` all write `contains: {minimum: 5}`.
- **Where:** the four wave-3 matrix schemas + a conformance case.
- **Suggested case:** `{items:{type:string}, contains:{minLength: 2}}` with wire `["a", 5]` — every target must return an aggregated `ValidationError` containing `names[0]: expected string`, and none may throw a native error.

### 6. Loader negative matrix rows with no test
- **Severity:** P1
- **Untested rows:** `contains: true`, `contains: false`, `contains: [{type: string}]` (only `contains: 5` and `contains: {}` are tested — `src/parser/json_schema.rs:8524`, `:8646`); `{type: "string", contains: {const: "a"}}` (the non-array reject is only tested via `minItems`, `:8489`); `{items:{type:string}, contains:{const: 5}}` and `…contains:{enum:[1,2]}}` (the incompatible-kind reject is only tested via `type: integer`, `:8529`); `minContains: "1"` / `true` / `-1` / `1.5`; `maxContains: "3"` / `true` / `null` / `2.5` (only `maxContains: -1` is tested, `:8620`).
- **Spec line:** `contains.md:176-183`, `minContains.md:140-146`, `maxContains.md:135-141`.
- **Where:** the `#[cfg(test)]` block in `src/parser/json_schema.rs` next to `rejects_non_schema_contains_value`.

### 7. Loader positive matrix rows with no test
- **Severity:** P1
- **Untested rows:** `.0`-valued bounds (`minContains: 2.0`, `maxContains: 2.0` — **currently broken**, see divergence #6); `minContains: 0, maxContains: 2`; `minContains: 0, maxContains: 0`; `minContains: 2, maxContains: 2`; `contains: {const: 1.0}` over `items: {type: integer}` (the integer-valued-number normalization row).
- **Spec line:** `contains.md:165-172`, `minContains.md:129-135`, `maxContains.md:124-130`.
- **Where:** alongside `accepts_valid_array_constraints` (`src/parser/json_schema.rs:8653`).

### 8. Java has no serialize-side (P12) `contains` assertion
- **Severity:** P1
- **Untested:** the `Serializer`'s `contains` re-check. Go (`tests/generate_go.rs` wave-3 matrix, `invalid.MatchedNumbers = []float64{1}`), TypeScript (`tests/generate_typescript.rs:2195-2209`) and Python (`tests/generate_python.rs:302-309`) all assert it; `tests/generate_java.rs` has no `matching items` / `no element matches` assertion at all, and `samples/java/.../JsonSchemaShowcaseRoundTripTest.java:314-335` only exercises `fromPayload`.
- **Spec line:** `contains.md:150-157`, `minContains.md:117-123`, `maxContains.md:111-118`.
- **Where:** `tests/generate_java.rs` (round-trip a valid value, mutate the list, expect `ValidationException`).

### 9. No runtime coverage of `contains` in a typed-map value or a `oneOf` array branch
- **Severity:** P2
- **Untested:** the emission is correct in all four (I generated and read it — `additionalProperties: {type: array, …, contains}` and `oneOf: [{type: array, …, contains}, {type: integer}]`), but no test executes it, and no test pins the violation `path` for the map case (all four use the bare map key).
- **Spec line:** `contains.md:132` (Python row explicitly names "typed-map members and [[oneOf]] branches").
- **Where:** the four wave-3 matrix schemas.

### 10. `contains` reason-string variants are unasserted
- **Severity:** P2
- **Untested:** nothing asserts the matcher-specific reasons the spec mandates (divergence #8) — every existing assertion matches the generic fallback.
- **Spec line:** `contains.md:135-139`.

---

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| `contains` x `items` (scalar element) | supported; `items` types every element, `contains` asserts one (`contains.md:205-209`) | **yes** — all four generator suites + showcase `roles` | low |
| `contains` x `items` (composite element) | load reject, deferred (`contains.md:89-91`) | **yes** — `src/parser/json_schema.rs:11797` | low |
| `contains` x `items` (nullable element `oneOf:[T,null]`) | null never matches a scalar matcher; orthogonal (`contains.md:242-244`) | **no** | **high** — loader rejects the shape outright (divergence #7); Go's unwrap at `go.rs:745` is dead code |
| `contains` x `minContains` | omitting ≡ 1; `≥ 2` cancels the short-circuit (`minContains.md:165-169`) | partial — `minContains: 1` only (showcase `roles`); `minContains: 2` load-tested but never run | medium |
| `contains` x `maxContains` | tally, not short-circuit; `≤` inclusive (`maxContains.md:87-94`) | partial — `maxContains: 2` runtime in all four (showcase) | low |
| `minContains` x `maxContains` (min > max) | load reject, unsatisfiable (`maxContains.md:68`) | **yes** — `rejects_min_contains_above_max_contains` | low |
| `minContains` x `maxContains` (min == max, exact pin) | accepted (`maxContains.md:70-72`) | **no** — neither load nor runtime | medium |
| `minContains: 0` x `maxContains` absent | load reject, vacuous (`minContains.md:76-81`) | **yes** — `rejects_vacuous_min_contains_zero` | low |
| `minContains: 0` x `maxContains: N` | accepted, existential relaxed (`minContains.md:72-75`) | **no** | **high** — the whole relaxed-existential branch (`effective_min == 0`, no min check emitted) is generated but never executed |
| `maxContains: 0` x default `minContains: 1` | load reject, unsatisfiable (`maxContains.md:74-78`) | **yes** — `rejects_max_contains_zero_at_default_min` | low |
| `maxContains: 0` x `minContains: 0` (must-not-contain) | accepted (`maxContains.md:130`) | **no** — load or runtime | **high** |
| `minContains`/`maxContains` without `contains` | load reject (`minContains.md:65`, `maxContains.md:61`) | **yes** — both directions | low |
| `contains` x `minItems`/`maxItems` | independent, all aggregate (`contains.md:216-222`) | **yes** — `checkedArray` in the Go and TS wave-3 matrices | low |
| `contains` x `uniqueItems` | independent, both aggregate (`contains.md:223-225`) | **yes** — `checkedArray` (Go/TS) | low |
| `contains` x a bad element per `items` (raw wire scan) | the scan sees the original wire elements (`contains.md:114-116`) | **yes** — `roles:[1,"admin"]` in all four round-trip suites | low |
| `contains` x `const`/`enum` matcher | natural matcher; shared scalar value-equality (`contains.md:226-230`) | partial — only single-kind enums; **mixed-kind enum untested** | **high** — divergence #1 |
| `contains` x `minimum`/`maximum`/`exclusive*`/`multipleOf` matcher | matcher's own predicate defines "match" (`contains.md:231-234`) | partial — same-kind matcher/element only; **cross-kind (number matcher over integer elements, integer matcher over number elements) untested for bound truncation and the integer cap** | **high** — divergences #2, #3 |
| `contains` x `pattern`/`minLength`/`maxLength` matcher | matcher's own predicate (`contains.md:231-234`) | partial — typed matchers only; **typeless untested** | **high** — divergence #5 |
| `contains` x `format` matcher | matcher's own predicate | typed only; typeless is rejected with the wrong reason (divergence #9) | medium |
| `contains` x `type` (non-array node) | load reject P7.1 (`contains.md:48-49`) | indirect only — tested via `minItems` | low |
| `contains` x `required` (optional absent array) | no violation; nothing to scan (`contains.md:238-240`) | **yes** implicitly (all four guard on presence: Java `if (value.n != null)`, Go/TS/Python equivalents) but not asserted | low |
| `contains` x `nullability` (nullable array **member**) | orthogonal (`contains.md:238-244`) | **no** | **critical** — divergence #4: silently dropped in Java, non-compiling in Go |
| `contains` x `allOf` (conflicting matchers) | reject (merge cannot combine two existentials) | **yes** — `rejects_all_of_differing_contains` | low |
| `contains` x `allOf` (bounds merged across branches) | `minContains` takes the max, `maxContains` the min; post-merge satisfiability re-checked | **no test**, but I verified all three behaviors work (`src/parser/json_schema.rs:5544-5546`) | low |
| `contains` x `additionalProperties` (typed map of arrays) | Python row names typed-map members explicitly (`contains.md:132`) | emission verified by me, **no test executes it** | medium |
| `contains` x `oneOf` (array branch of a union) | Python row names `oneOf` branches (`contains.md:132`) | emission verified by me, **no test executes it** | medium |
| `contains` x `prefixItems`/`unevaluatedItems` | both rejected per P6 | **yes** — covered by the general P6 rejects | low |
| `contains` x `contentEncoding` on the matcher | rejected in a scalar matcher | **yes** — `recursively_validates_contains_scalar_matcher_constraints` | low |

---

## Verified-good

- **Loader shape rules.** `contains: 5` / `"x"` / `[…]` / `{}` / `true` / `false` all reject; composite matcher (`type: object`/`array`, `$ref`, `properties`, `oneOf`, composite `const`/`enum`) rejects; composite `items` element rejects; matcher/element kind incompatibility rejects; `minContains`/`maxContains` without `contains` rejects; `min > max` rejects; `maxContains: 0` at the default `minContains: 1` rejects; `minContains: 0` alone rejects. All probed against the binary (`src/parser/json_schema.rs:2415-2712`).
- **Recursive matcher validation.** The matcher is fully re-validated as a scalar schema — fractional `multipleOf`, negative `minLength`, unknown `format`, a `const` violating a sibling matcher bound, `not`, `contentEncoding`, `oneOf` hidden beside a scalar assertion, and array assertions on a scalar matcher all reject with a `.contains`-qualified context (`src/parser/json_schema.rs:2632-2637`, tested at `:8517`).
- **`allOf` merge.** `minContains` merges to the max, `maxContains` to the min, post-merge `min > max` is caught, and two different `contains` matchers reject (`src/parser/json_schema.rs:5544-5575`) — all four verified against the binary.
- **Positional reach.** `contains` is validated and emitted at a declared property, inside `additionalProperties` (typed map), inside a `oneOf` array branch, and through an `allOf` merge, in all four backends and in both directions. Java's `oneOf` array branch even emits both a raw scan in `fromNode` and a typed scan in `BranchArray.validate`.
- **Raw-vs-typed split (P12).** The deserialize scan reads the original wire elements (Go `json.RawMessage`, TS `raw.x`, Python `*_raw`, Java `JsonNode`) so elements that fail `items` conversion still count — asserted in all four round-trip suites via `roles: [1, "admin"]`.
- **Violation shape (P11).** All four push exactly one `{path, reason}` per bound, with the array member's path (or the map key for a typed-map value), never an element index — verified in generated output for all four.
- **Count reason strings.** `too few matching items: at least N, got M` and `too many matching items: at most N, got M` are byte-consistent across Go/TS/Python/Java and asserted in all four round-trip suites (`samples/go/tests/json_schema_showcase_test.go:386`, `samples/typescript/tests/json-schema-showcase.test.ts:303`, `samples/python/tests/test_showcase.py:518`, `samples/java/.../JsonSchemaShowcaseRoundTripTest.java:327`).
- **Bare-`contains` vs explicit `minContains`.** All four correctly switch between the existential reason and the count reason on `min_contains.is_some()` (`go.rs:686`, `typescript.rs:923`, `java.rs:893`, `python.rs:1702` → the `bounded_min` flag).
- **`effective_min == 0` skips the floor check.** All four omit the `minContains` comparison entirely when the effective minimum is `0` (`go.rs:682`, `typescript.rs:920`, `java.rs:890`, `python.rs:1386` via `match_count < 0` being unreachable) — correct, just untested.
- **Optional absent array raises no `contains` violation** (P8) — all four guard the scan on member presence.
- **Go's integer-element raw scan** uses `parseSpecInteger`, so `1.5` correctly fails to match an integer matcher; this is asserted by `go_json_scalar_matchers_have_runtime_type_and_decimal_semantics` in `tests/generate_go.rs:2298`.
