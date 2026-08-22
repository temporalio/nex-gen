# Arrays (items / prefixItems / min-maxItems / uniqueItems / unevaluatedItems) — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/items.md` — the homogeneous-list applicator; supplies `[]T` / `T[]` / `list[T]` / `List<T>`, per-element recursion, and the indexed violation path.
- `specs/json-schema/features/prefixItems.md` — positional tuples; categorically rejected at load (P6), in every position.
- `specs/json-schema/features/minItems.md` — inclusive element-count floor; runtime-only, `0` is a no-op, `minItems > maxItems` rejects.
- `specs/json-schema/features/maxItems.md` — inclusive element-count ceiling; owns the shared count machinery and combined-size satisfiability.
- `specs/json-schema/features/uniqueItems.md` — all-distinct assertion over **scalar** elements (composite deferred); JSON value equality, exact `==` for numbers.
- `specs/json-schema/features/unevaluatedItems.md` — annotation-dependent tail; categorically rejected at load (P6).

## Summary

- The **loader** is in good shape: `prefixItems` and `unevaluatedItems` reject in every reachable position (property, `items`, `additionalProperties`, `oneOf` branch, `allOf` branch, `contains`, `propertyNames`, live and dead `$defs`, service operation I/O); `type:array` without `items`, shapeless `items`, `items` on a non-array type, negative/fractional/non-numeric count bounds, non-boolean `uniqueItems`, `minItems > maxItems` (including after an `allOf` merge), and composite-element `uniqueItems` all reject with located, fix-it diagnostics.
- The **emitters** are where the damage is. `uniqueItems` is broken in three of four languages the moment the element type materializes or the number is a signed zero, and it is broken in TypeScript at any nesting depth ≥ 1.
- **P0: Go emits non-compiling code** for `uniqueItems: true` over any materialized element (`format: date-time|date|time|duration`, `contentEncoding: base64|base64url`). Confirmed with `go vet`.
- **P0: Java's serialize-side `uniqueItems` treats `-0.0` and `0.0` as distinct** (`Double.equals` uses `doubleToLongBits`), contradicting P1's explicit "positive/negative zero compare equal" and Java's *own* deserialize path, which normalizes zero via `SpecNumbers.valueKey`.
- **P0: TypeScript and Java compare materialized elements by reference** on the serialize side (`Map<unknown,number>` over `Uint8Array`/`Temporal.ZonedDateTime`/`Date`; `HashMap<byte[],Integer>`), so byte-equal duplicates serialize fine but are rejected on deserialize and by Python.
- **P0: TypeScript's nested `uniqueItems` loop shadows the enclosing `index`**, so the duplicate violation's `path` names the *inner* index instead of the outer one — exactly the shadowing `items.md` forbids by name.
- **P0: Go emits `"tags": null` for a required non-nullable array whose in-memory slice is `nil`** — the hazard `items.md` §Serialize-side names and requires be fixed. Go's own decoder then rejects its own output. Inconsistent even within Go: a required array of *materialized* elements correctly emits `[]`.
- **P1: `minItems: 2.0` / `maxItems: 3.0`** — an explicitly spec-accepted shape — loads fine and then **crashes code generation in all four languages** (`invalid type: floating point 2.0, expected u64`).
- **P1: raw generated Python for array count bounds is a SyntaxError on Python < 3.12** (nested same-quote f-string). Masked in-repo only because samples are `ruff format`ed after generation; the declared floor is 3.10.
- **Combination coverage is thin where it matters**: no schema anywhere combines `uniqueItems` with a materialized element, a nested array, a nullable element, an `enum` element, or a `-0.0`/`[1, 1.0]` wire value; the conformance manifest has **zero** array cases.

## Implementation divergences

### 1. Go generates non-compiling code for `uniqueItems` over a materialized element type
- **Severity** P0
- **Spec cite** `uniqueItems.md:51-60` (scalar element types are supported), `uniqueItems.md:99` (Go row: "normalizes each original `json.RawMessage` to a JSON value key … serialize performs the equivalent typed-slice walk"); `items.md:169-173` (materializing elements use their ordinary adapters at every depth).
- **Code cite** `src/generator/json_schema/go.rs:634-641` — the map key type is chosen from the *schema's* element `type` string:
  ```rust
  let key_ty = match element_ty {
      Some("integer") => "int64",
      Some("number") => "float64",
      Some("boolean") => "bool",
      _ => "string",
  };
  ```
  while the emitted slice element is whatever `format`/`contentEncoding` materialized it into.
- **What the spec requires** `uniqueItems: true` is supported for a scalar element `type`; a `format: date-time` element is `type: string`, so the loader admits it and the generator must emit a working all-distinct walk.
- **What the code does** Emits `seen := make(map[string]int, …)` and `seen[e]` where `e` is `time.Time`, `time.Duration`, or `[]byte`.
- **Concrete failing input**
  ```yaml
  type: object
  properties:
    stamps: { type: array, items: { type: string, format: date-time }, uniqueItems: true }
  ```
  → `go vet`: `cannot use e (variable of struct type time.Time) as string value in map index`. Reproduced for `date-time`, `date`, `time`, `duration`, `contentEncoding: base64`, and `base64url`. (`[]byte` is additionally not comparable at all, so it can never be a Go map key.)
- **Confidence** High — reproduced end-to-end with `go vet`.

### 2. Java serialize-side `uniqueItems` distinguishes `-0.0` from `0.0`
- **Severity** P0
- **Spec cite** PRINCIPLES P1 ("positive/negative zero compare equal"); `uniqueItems.md:116` ("`-0.0` equals `0.0`"); `uniqueItems.md:102` (Java row: serialize "walks the typed `List<T>`", same predicate as deserialize).
- **Code cite** `src/generator/json_schema/java.rs:857-860` — `let boxed = element_ty.boxed_name(); … java.util.Map<{boxed}, Integer> seen = new java.util.HashMap<>();`. For a `number` element that is `Map<Double, Integer>`. The deserialize path (`src/generator/json_schema/java.rs:938`, `SpecNumbers.valueKey`) *does* normalize zero.
- **What the spec requires** The same all-distinct verdict in both directions and in all four languages.
- **What the code does** `Double.equals`/`Double.hashCode` are bit-pattern based; verified: `new HashMap<Double,Integer>().put(-0.0,0); get(0.0) → null`, `Double.valueOf(-0.0).equals(0.0) → false`.
- **Concrete failing input** `{type:array, items:{type:number}, uniqueItems:true}` with the in-memory list `[-0.0, 0.0]`. Java serialize **accepts**; Go (`map[float64]` hashes ±0 identically), TypeScript (`Map` uses SameValueZero), Python (`-0.0 == 0.0`) and Java's own deserialize all **reject**. A Java producer can therefore emit a payload every consumer, including Java, refuses.
- **Confidence** High — ran the Java snippet.

### 3. TypeScript and Java compare materialized elements by reference on serialize
- **Severity** P0
- **Spec cite** `uniqueItems.md:87-102` (serialize compares decoded elements with the same equality); PRINCIPLES P1 (identical accepted/rejected value set in both directions).
- **Code cite** `src/generator/json_schema/typescript.rs:886-905` (`const seen = new Map<unknown, number>();`, used for both `fromTransferType` and `toTransferType`); `src/generator/json_schema/java.rs:857-860` (`Map<byte[], Integer>`).
- **What the spec requires** Element-vs-element JSON value equality, identical in both directions.
- **What the code does** JS `Map` and Java `HashMap` fall back to reference identity for object keys. Two distinct `Uint8Array`s / `Temporal.ZonedDateTime`s / `Date`s / `byte[]`s holding the same value are **not** duplicates on serialize, while the deserialize path (raw base64/RFC-3339 strings) and Python's `_json_values_equal` (`bytes.__eq__`) do report them.
- **Concrete failing input** `{type:array, items:{type:string, contentEncoding:base64}, uniqueItems:true}`, in-memory `[new Uint8Array([1,2]), new Uint8Array([1,2])]` → TS `toTransferType` emits `["AQI=","AQI="]`; feeding that back to `fromTransferType` throws `duplicate items: element at index 1 equals index 0`. Verified: Java `HashMap<byte[],Integer>.get(new byte[]{1,2})` after `put(new byte[]{1,2},0)` → `null`. With `--date-time-types temporal|date` the same applies to `Temporal.ZonedDateTime[]` / `Date[]`.
- **Additional wrinkle (same family, lower confidence on which side is "right")** Python's serialize side compares *decoded* `datetime`s, whose `__eq__` is instant-based, while Java's compares `OffsetDateTime`, whose `equals` requires the same offset. `["2020-01-01T00:00:00Z", "2020-01-01T01:00:00+01:00"]` is therefore a duplicate for Python-on-serialize and distinct for Java-on-serialize and for every language on deserialize. The spec's "deserialize compares the wire, serialize compares decoded values" rule cannot hold for materialized types; this needs a spec decision, not just a code fix.
- **Confidence** High for TS/Java identity; high for the Python/Java datetime divergence (language semantics), medium on the intended remedy.

### 4. TypeScript's nested `uniqueItems` loop shadows the enclosing element index
- **Severity** P0
- **Spec cite** `items.md:158-161` — "each level's loop variables carrying their depth **so an inner element never shadows the level above it**, and each level appending its own index to the path (`matrix[1][2]`)"; P11 (structured `{path, reason}`).
- **Code cite** `src/generator/json_schema/typescript.rs:890-896` — the loop parameters are the hard-coded `(element, index)` while `path_expr` is a template literal that interpolates the *enclosing* loop's `index`:
  ```rust
  output.push_str(&format!("  {array_expr}.forEach((element, index) => {{\n"));
  …
  "      violations.push({{ path: {path_expr}, reason: `duplicate items: element at index ${{index}} …` }});\n"
  ```
- **What the spec requires** The duplicate violation for an inner array at outer index `i` must be pathed `matrix[i]`.
- **What the code does** The inner `index` shadows the outer one, so the path carries the inner (duplicate) index.
- **Concrete failing input** `{type:array, items:{type:array, items:{type:integer}, uniqueItems:true}}` with `matrix: [[7,7],[1,2]]`. Executed the generated snippet under node: emitted `{"path":"matrix[1]", …}`; correct is `matrix[0]`. Go (`p0`), Python (`matrix_value_item_path`) and Java (`"matrix" + "[" + validationIndex0 + "]"`) all get it right, so this is a cross-language `path` disagreement.
- **Confidence** High — reproduced with node.
- **Note** The same code shadows in `toTransferType` too. The surrounding `minItems`/`maxItems` blocks at the same depth are correct — only the `uniqueItems` block reuses the bare names.

### 5. Go emits `null` for a required non-nullable array whose slice is `nil`
- **Severity** P0
- **Spec cite** `items.md:193-202` — "The generated `MarshalJSON` therefore emits `[]` for a required non-nullable array whose in-memory slice is `nil` (or the serialize-side `Validate` rejects `nil` …)"; P9 (absent ≠ zero value); P1 (a value one language emits must round-trip through any other).
- **Code cite** `src/generator/json_schema/go.rs:4016-4020` — the required, non-nullable, non-wire-converting branch emits a bare `marshalField(out, "<name>", m.<Field>, &errs)` with no `nil` handling; `marshalField` (`go.rs:1560`, runtime at `definitions.go:278`) just calls `json.Marshal`, which renders a `nil` slice as `null`. Neither `Validate` nor `MarshalJSON` rejects it.
- **What the spec requires** `[]` on the wire, or a serialize-side rejection.
- **What the code does** Emits `null`.
- **Concrete failing input** `{type:object, required:[tags], properties:{tags:{type:array, items:{type:string}}}}`; `json.Marshal(R2{})` → `{"tags":null}` (executed). Round-tripping that payload back through the generated `UnmarshalJSON` yields `tags: explicit null not allowed`.
- **Intra-Go inconsistency** A required array whose elements *are* materialized takes the `render_go_array_wire_value` path (`go.rs:3983-3988`), which builds `make([]any, 0, len(...))` — non-nil — and therefore correctly emits `[]`. So `{"plain":null,"stamped":[]}` from one model. Verified on a two-field probe.
- **Confidence** High — executed.
- **Adjacent (owned by `required`/`nullability`, flagged here)** Java's serializer gates every field on `if (value.<f> != null)` (`java.rs`, emitted at `R2.Serializer`), so a required `List<T>` that is `null` is **silently omitted** with no violation — a third distinct behavior for the same in-memory state (Go `null`, Python `null`, Java absent, TS `undefined` → omitted).

### 6. A `.0`-valued `minItems`/`maxItems` bound crashes code generation in all four languages
- **Severity** P1
- **Spec cite** `minItems.md:41-42, 96` ("`minItems:2.0` accepted (≡ `2`)", listed in the **Accepted** matrix); `maxItems.md:47-48, 108` (same for `maxItems:3.0`).
- **Code cite** Loader accepts: `src/parser/json_schema.rs:2447-2473` (`value.fract() == 0.0` → `Ok(Some(value as u64))`) but never normalizes the value back into `schema.extra`. Emitters then re-deserialize the planned schema into `min_items: Option<u64>` / `max_items: Option<u64>`: `go.rs:66-69`, `java.rs:67-69`, `python.rs:73-75`, `typescript.rs:227-229`; error surfaced at `go.rs:5007` / `java.rs:1766` / `python.rs:5153` / `typescript.rs:4011`.
- **What the spec requires** `{type:array, items:{type:string}, minItems:2.0}` generates identically to `minItems:2`.
- **What the code does** `invalid JSON schema in <…>: failed to read planned JSON schema `T1`: invalid type: floating point `2.0`, expected u64`.
- **Concrete failing input** the schema above; reproduced for Go, Python, TypeScript (and Java by the same code path).
- **Confidence** High — reproduced via the CLI for three of four targets; the fourth shares the identical struct field type.

### 7. Raw generated Python for array count bounds is a SyntaxError below Python 3.12
- **Severity** P1
- **Spec cite** `minItems.md:73` / `maxItems.md:79` (Python row: the transfer converter checks `len(raw)` after parsing the elements); `items.md:164-168` (array-level keywords must see the original wire array — which is what forces the `typing.cast` wrapper).
- **Code cite** `src/generator/json_schema/python.rs:1677` and `:1686` interpolate `array_expr` into an f-string; on the deserialize path `array_expr` is built at `python.rs:4722` as `typing.cast("list[typing.Any]", {raw_expr})` — containing `"` inside an `f"…"`.
- **What the spec requires** (implicitly) importable output on the supported Python floor; `samples/python/pyproject.toml:4` declares `requires-python = ">=3.10"`.
- **What the code does** Emits `f"must have at least 2 items, got {len(typing.cast("list[typing.Any]", matrix_value_element))}"`. Nested same-quote f-strings are PEP 701, Python 3.12+.
- **Concrete failing input** any `{type:array, items:{…}, minItems:N}`; under Python 3.11: `SyntaxError: f-string: unmatched '('` (executed via `uv run --python 3.11`).
- **Why it is invisible in-repo** `xtask/src/build_examples.rs:444` runs `ruff` on the committed samples, which rewrites the inner quotes to `'`. A user invoking `nexgen python` directly gets the unformatted output.
- **Confidence** High — reproduced on a real 3.11 interpreter.

### 8. `uniqueItems: true` (and `contains`) over a *nullable scalar* element rejects as "composite"
- **Severity** P1
- **Spec cite** `uniqueItems.md:188-190` — "**[[nullability]]**: if the element schema is the nullable [[nullability]] pattern, a `null` element is one value for uniqueness purposes — two `null` elements are a duplicate"; `uniqueItems.md:51-52` defers only **object/array** element types.
- **Code cite** `src/parser/json_schema.rs:2487-2493` — `items_is_scalar` is `scalar_type(items.ty)`, and a nullability `oneOf` element has no `ty`, so it falls into the composite arm at `:2500-2504` (and `:2595-2599` for `contains`).
- **What the spec requires** Either support the combination with the stated `null`-is-a-value semantics, or say in the spec that it is deferred. The spec currently specifies *runtime behavior* for a combination the loader refuses.
- **What the code does** `root schema.properties.a: `uniqueItems: true` over a composite element type is not yet supported; deep structural equality is deferred (scalar `items` only)`.
- **Concrete failing input** `{type:array, items:{oneOf:[{type:string},{type:"null"}]}, uniqueItems:true}`.
- **Corroborating evidence that support was intended** `src/generator/json_schema/go.rs:749-753` already unwraps the nullable pattern (`nullable_non_null_schema(items)`) to derive the element type for the `contains` raw check — dead code today, since the loader rejects before it runs.
- **Note** `minItems`/`maxItems` over nullable elements *are* accepted and emit correctly (verified), which is right — they are element-type-agnostic.
- **Confidence** High on the behavior; the divergence is genuine but may be resolved on the spec side.

### 9. `minItems`/`maxItems` reason strings do not match the spec's text
- **Severity** P2
- **Spec cite** `minItems.md:72, 76-78, 114` (`too few items: at least 2, got 1`); `maxItems.md:78, 82-87, 126` (`too many items: at most 3, got 4`).
- **Code cite** `go.rs:619-623, 627-631`; `typescript.rs:867-880`; `python.rs:1677, 1686`; `java.rs` (`must have at least … items, got …`).
- **What the spec requires** the quoted strings, "per the [[maxProperties]] count-family convention".
- **What the code does** `must have at least 2 items, got 1` / `must have at most 3 items, got 4` — uniformly, in all four targets, and matching what `maxProperties` actually emits. The sibling `contains` reasons *do* use the spec's `too few matching items: at least N, got M` form, so the codebase is internally split.
- **Why P2 only** P11 explicitly does not hold reason text byte-identical across targets, all four agree with each other, and both forms name the concrete bound and offending count. This is spec drift, not a wire risk. Either the three count-family specs or the emitters should move; note that `specs/json-schema/features/maxProperties.md:60-62` carries the same stale text.
- **Confidence** High.

### 10. Array-valued / boolean / non-schema `items` reject with a generic serde message, not the mandated fix-it
- **Severity** P2
- **Spec cite** `items.md:72-74` — "`items` value that is an **array** (draft-7 tuple spelling) → reject; **diagnostic notes 2020-12 moved tuples to [[prefixItems]]**"; `items.md:69-71` — shapeless `items: true`/`false` "→ reject per P7.1 … Diagnostic **names the array and asks for an explicit element `type`**"; `prefixItems.md:76`.
- **Code cite** `src/parser/json_schema.rs:69` (`items: Option<Box<Schema>>`) — a non-object `items` fails serde before any subset validation runs; the existing test at `src/parser/json_schema.rs:11882-11889` asserts only `failed to parse JSON schema` and its comment concedes the point.
- **What the code does**
  - `items: [{type:string},{type:integer}]` → `failed to parse JSON schema from …: invalid type: sequence, expected struct Schema`
  - `items: true` / `items: false` → `invalid type: boolean `true`, expected struct Schema`
  - `items: 5` → `invalid type: integer …`
  No file position within the document, no mention of `prefixItems`, no fix-it. (`items: {}` *does* get the correct located diagnostic, via `validate_type_presence`.)
- **Confidence** High — reproduced.
- **Related** `additionalItems: {…}` (the draft-4/6/7 tuple tail, `items.md:289`, `prefixItems.md:99`) rejects only as `unknown schema keyword `additionalItems``, with no rewrite guidance.

### 11. `minItems: 0` emits a dead comparison instead of being treated as omitted
- **Severity** P2
- **Spec cite** `minItems.md:47-51` — "**`minItems:0`** → accepted, **treated as omitted** … it constrains nothing".
- **Code cite** `src/generator/json_schema/go.rs:614-624` (and the TS/Python/Java equivalents) gate on `if let Some(min) = schema.min_items`, not on `min > 0`.
- **What the code does** Emits `if n := len(v); n < 0 { … "must have at least 0 items, got %d" … }` in both directions — unreachable, but noise in output that P2 says should read hand-written. Same for the (legitimate) `maxItems: 0`, which must stay.
- **Confidence** High — reproduced.

### 12. `NaN` duplicate detection differs across targets (verdict unaffected)
- **Severity** P2
- **Spec cite** `uniqueItems.md:116` — "`NaN`/`±Infinity` cannot appear" (on the wire).
- **What differs** On the **serialize** side an in-memory `NaN` *can* appear. TS `Map` (SameValueZero) and Java `Double.equals` both treat `NaN` as equal to `NaN` → duplicate reported; Go's `map[float64]` and Python's `==` do not. The payload is rejected by all four either way (the finite-number check fires), so the accepted/rejected value set is unchanged and P1 holds; only the aggregated violation *list* differs.
- **Confidence** High on the semantics; deliberately ranked P2 because the verdict agrees.

## Testing gaps

### 1. No test loads *and generates* a `.0`-valued count bound
- **Severity** P0 (this is what let divergence #6 ship)
- **Untested** `minItems: 2.0` / `maxItems: 3.0` reaching an emitter. The loader-only inline tests never call a backend, and no generator test uses a `.0` bound.
- **Spec line** `minItems.md:96` and `maxItems.md:108` — both list the `.0`-valued bound as an **Accepted** shape.
- **Where** `tests/generate_{go,typescript,python,java}.rs` (add the bound to the shared fixture schema), plus a loader positive in `src/parser/json_schema.rs`.
- **Suggested case** `{type:array, items:{type:string}, minItems:2.0, maxItems:5.0}` — assert it generates and that the emitted bounds read `2` / `5`, not `2.0`.

### 2. No schema anywhere pairs `uniqueItems` with a materialized element type
- **Severity** P0 (divergences #1 and #3)
- **Untested** `uniqueItems: true` over `format: date-time|date|time|duration` or `contentEncoding: base64|base64url`. `rg 'uniqueItems' samples/schemas advanced tests` shows every occurrence is over a plain `string`/`number`.
- **Spec line** `uniqueItems.md:51-52` (scalar element types are supported) crossed with `items.md:169-173` (materializing elements use their ordinary adapters at every depth).
- **Where** `samples/schemas/showcase.nexusrpc.yaml` (a `distinctStamps` / `distinctBlobs` member), so the four round-trip suites and the `go vet` / `tsc` / `ruff` / `javac` gates in `scripts/validate.sh` all see it.
- **Suggested case** `{type:array, items:{type:string, contentEncoding:base64}, uniqueItems:true}`; assert `["AQI=","AQI="]` is rejected on **both** deserialize and serialize in all four.

### 3. Generated Go is only compiled for the committed samples
- **Severity** P0 (nothing else would have caught #1)
- **Untested** Any schema shape outside `samples/` and `advanced/` — `tests/generate_go.rs` renders to a `String` and greps it; it never runs `go build`/`go vet`.
- **Spec line** P1/P2 (the output is source people compile).
- **Where** a new `tests/` harness, or extend `scripts/validate.sh` to `go vet` a small matrix of generated probe schemas.
- **Suggested case** at minimum the `uniqueItems` × element-type matrix (`string`/`integer`/`number`/`boolean`/`date-time`/`base64`/`enum`).

### 4. No nested array carries an array-level keyword
- **Severity** P0 (divergence #4)
- **Untested** `minItems`/`maxItems`/`uniqueItems` at depth ≥ 1. `samples/schemas/showcase.nexusrpc.yaml:383-399` (`grid`, `numberGrid`) are nested arrays with **no** constraints on either level.
- **Spec line** `items.md:158-161` — inner loop variables must not shadow the level above; `matrix[1][2]` path convention.
- **Where** add inner constraints to `grid` in the showcase; assert the path in all four sample suites.
- **Suggested case** `{type:array, items:{type:array, items:{type:integer}, minItems:2, uniqueItems:true}}` with `[[7,7],[1,2]]` → the sole violation must be pathed `grid[0]`, not `grid[1]`.

### 5. No cross-language conformance case for `uniqueItems` equality
- **Severity** P0 (divergence #2)
- **Untested** `[-0.0, 0.0]`, `[1, 1.0]`, `[5, 5e0]`, `[true, 1]`, `[null, null]` against a `uniqueItems` array — in either direction. `samples/conformance/json-schema.json` has four cases and **none** touch an array keyword; its `mathematical-number-equality` case is about round-trip *spelling*, not element uniqueness.
- **Spec line** PRINCIPLES P1 ("`5`, `5.0`, and `5e0` are the same mathematical number, and positive/negative zero compare equal"), restated at `uniqueItems.md:109-116`.
- **Where** a new case in `samples/conformance/json-schema.json` with `parse_failures` **and** `serialize_failures` entries, wired to all four consumer suites.
- **Suggested case** id `unique-items-number-equality`: wire `{"measurements":[1,1.0]}` → reject; in-memory `[-0.0, 0.0]` → reject on serialize in all four.

### 6. No test pins the Go required-array `nil` wire form
- **Severity** P0 (divergence #5)
- **Untested** `json.Marshal` of a model with a required non-nullable array left at its zero value.
- **Spec line** `items.md:193-202`.
- **Where** `samples/go/tests/json_schema_showcase_test.go` (the showcase has required arrays), plus a conformance `serialize_failures` / canonical-form entry so the other three are pinned to the same answer.
- **Suggested case** construct the model with the array field omitted; assert the emitted JSON contains `"<field>":[]` (or that serialize fails), and that the result round-trips.

### 7. Python < 3.12 is never exercised, and the *unformatted* output is never syntax-checked
- **Severity** P1 (divergence #7)
- **Untested** Importability of raw generator output on the declared 3.10 floor. `scripts/validate.sh:50-51` runs `ruff check`/`ruff format --check` on the already-formatted samples.
- **Spec line** PRINCIPLES Python §1 / P4 (the output is ordinary Python a user drops into a repo); `samples/python/pyproject.toml:4`.
- **Where** a Rust integration test that generates to a temp dir and runs `python3.10 -m py_compile`, or a CI matrix entry that skips the format step.
- **Suggested case** any `{type:array, items:{type:string}, minItems:1}` schema.

### 8. `prefixItems` and `unevaluatedItems` are each tested in exactly one spelling
- **Severity** P2
- **Untested** `prefixItems.md:75` (`prefixItems` + an `items` tail), `unevaluatedItems.md:94` (`prefixItems` + `unevaluatedItems: {schema}`), `unevaluatedItems.md:95` (bare `{type:array, unevaluatedItems:true}`). `src/parser/json_schema.rs:9089-9093` covers only `prefixItems: [{type:string}]` and `unevaluatedItems: false`.
- **Spec line** the "Rejected at load time — the whole surface" tables in both specs.
- **Where** extend the `rejects_structural_keywords_with_fixits` table at `src/parser/json_schema.rs:9083`.
- **Note** I verified all three reject correctly today; the gap is that nothing guards it. Reject-in-every-position (property, `items`, `additionalProperties`, `oneOf`/`allOf` branch, `contains`, `propertyNames`, live and dead `$defs`, operation I/O) is also verified-but-untested.

### 9. `items: true` / `items: false` have no test at all
- **Severity** P2 (divergence #10)
- **Untested** the boolean-schema spelling of `items`.
- **Spec line** `items.md:225` — "Shapeless element (P7.1) | `{type:array, items:{}}`, `…items:true`, `…items:false`". Only the `{}` form is tested (`src/parser/json_schema.rs:11866-11873`).
- **Where** `src/parser/json_schema.rs` next to `rejects_shapeless_array_element`.
- **Suggested case** assert both reject and — once #10 is fixed — that the message names the array and asks for an element `type`.

### 10. Spec-accepted array shapes with no positive test
- **Severity** P2
- **Untested** `maxItems: 0` (`maxItems.md:109`), `minItems: 0` (`minItems.md:97`), `minItems == maxItems` exact pin (`minItems.md:98`, `maxItems.md:110`), `uniqueItems: false` (`uniqueItems.md:138`), `uniqueItems: false` over a **composite** `items` (`uniqueItems.md:139` — the one place a composite `items` is legal alongside the keyword), boolean-element `uniqueItems` (`uniqueItems.md:137`), and `minItems:3` + `uniqueItems:true` over `boolean` staying **accepted** at load (`uniqueItems.md:171-173`).
- **Where** `src/parser/json_schema.rs` alongside `accepts_valid_array_constraints` (`:8653`).
- **Note** I verified all seven behave correctly today.

### 11. `uniqueItems` over nullable / union / `enum` elements
- **Severity** P2
- **Untested** No test asserts either outcome for `items: {oneOf:[{type:string},{type:"null"}]}` + `uniqueItems` (divergence #8), for a union element, or for an `enum`-constrained element.
- **Spec line** `uniqueItems.md:188-190` (nullable), `:180-184` (`enum` composes).
- **Where** `src/parser/json_schema.rs` (loader verdict) + the showcase (runtime, if the verdict becomes "accept").

### 12. Serialize-side count bounds are under-tested outside the showcase's one field
- **Severity** P2
- **Untested** An in-memory over-long / under-filled list failing serialize is asserted only for `uniqueItems` (`samples/typescript/tests/json-schema-showcase.test.ts:1010-1016`, `samples/go/tests/json_schema_showcase_test.go:399`, `samples/python/tests/test_showcase.py:1077`). `minItems`/`maxItems` serialize-side rejection has no assertion in any suite.
- **Spec line** `maxItems.md:131-132`, `minItems.md:118-119` — "Serialize of an in-memory over-long slice/list → rejected before emit (**P12**)".
- **Where** the four sample round-trip suites, on `tags` (`minItems:1`/`maxItems:5`).

### 13. Empty-vs-absent-vs-`null` round trip for arrays
- **Severity** P2
- **Untested** No suite asserts that a present `[]` survives a round trip as `[]` (not omitted, not `null`) for an **optional** array, in any language.
- **Spec line** `items.md:122-124` ("**Empty vs absent.** `[]` (present, empty) is distinct from an absent array … and from `null`"), `items.md:238` ("Empty array `[]` → accepted").
- **Where** `samples/wire/json_schema/showcase/` fixtures + all four suites.
- **Note** I read the generated code for all four and it looks correct (Go `make([]T,0,n)` is non-nil), but nothing pins it.

### 14. `maxItems` on a `oneOf` array branch
- **Severity** P2
- **Untested** The showcase's `measurements` branch carries `minItems` + `uniqueItems` only.
- **Spec line** `items.md:187-191` (a `oneOf` array branch runs the ordinary recursive array parser, constraints and all).
- **Where** `samples/schemas/showcase.nexusrpc.yaml:348`.

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| `uniqueItems` × `format` (date-time/date/time/duration) | uniqueItems.md:51 supports scalar `type:string`; items.md:169 materializes | **no** | **P0** — Go does not compile (#1); Python/Java disagree on offset-equal instants (#3) |
| `uniqueItems` × `contentEncoding` (base64/base64url) | same | **no** | **P0** — Go does not compile; TS/Java serialize by reference (#1, #3) |
| `uniqueItems` × `number` `-0.0`/`+0.0` | P1 + uniqueItems.md:116 — equal | **no** | **P0** — Java serialize says distinct (#2) |
| `uniqueItems` × `number` `[1, 1.0]` / `5e0` | uniqueItems.md:110-115 — duplicate | partly (TS uses `[1.5,1.5]`; showcase `measurements`) | P1 — the *mathematical* spelling case is untested in every language |
| `uniqueItems` × nested array (depth ≥ 1) | items.md:158-161 — no index shadowing | **no** | **P0** — TS emits the wrong `path` (#4) |
| `uniqueItems` × nullable element | uniqueItems.md:188-190 — two `null`s are a duplicate | **no** | P1 — loader rejects the combination outright (#8) |
| `uniqueItems` × composite element, `true` | uniqueItems.md:74-78 — reject | yes (`rejects_unique_items_on_object_element_array`, `src/parser/json_schema.rs:8502`) | low |
| `uniqueItems` × composite element, `false` | uniqueItems.md:77-78 — accept (no-op) | **no** | P2 — verified correct, unguarded |
| `uniqueItems` × `enum` element | uniqueItems.md:180-184 — compose | **no** | P2 — verified Go stays `[]string`, so low |
| `uniqueItems` × `minItems`/`maxItems` | uniqueItems.md:166-173 — independent, both aggregate; **no** load cross-check | partly (`checkedArray` in `tests/generate_go.rs:178`) | low |
| `minItems:3` + `uniqueItems` over `boolean` | uniqueItems.md:171-173 — unsatisfiable but **not** caught at load | **no** | P2 — verified accepted, unguarded |
| `minItems` > `maxItems` | minItems.md:53, maxItems.md:54 — reject | yes (`rejects_empty_items_interval`, `:8495`) | low |
| `minItems` == `maxItems` (exact pin) | maxItems.md:110 — accept | **no** | P2 — verified accepted |
| `minItems`/`maxItems` × `allOf` merge producing an empty range | maxItems.md:54 + allOf merge | **no** dedicated test (`:9722` covers the grammar error only) | P2 — verified correct |
| `minItems`/`maxItems` × `.0` bound | minItems.md:96, maxItems.md:108 — accept | **no** | **P1** — generation crashes (#6) |
| `minItems` × `required` (optional array, non-empty when present) | minItems.md:128-133 — orthogonal | **no** | P2 — Go's `Validate` guards the count on `m.X != nil`, which is right for optional; unpinned |
| `minItems` × Go `nil` required slice | minItems.md:83-87 — must fail serialize | **no** | P1 — verified correct today, unguarded |
| required array × Go `nil` slice, **no** `minItems` | items.md:193-202 — emit `[]` or reject | **no** | **P0** — emits `null` (#5) |
| `items` × element failure vs sibling count keywords | items.md:162-168 — siblings see the whole wire array | yes (Go `:371-377`, Java, Python, TS) | low — verified all four use the raw length |
| `items` × violation ordering (elements first, then array-level) | items.md:167 | implicitly | low — verified consistent in all four on deserialize; on **serialize** all four put array-level first, which the spec does not address |
| nested `items` × path `matrix[1][2]` | items.md:141-144 | yes (showcase `grid`) | low |
| `items` of objects × member path `rows[0].id` | items.md:243-244 | yes (showcase `rows`) | low |
| `items` × `$ref` self-recursion, **required** | items.md:217, 245-246 | yes (parser `:12235`, conformance `recursive-collections`) | low |
| `items` × `oneOf` union element | items.md:109-117 | yes (showcase `shapes`, `segments`) | low |
| `items` × typed-map member array | items.md:168 | yes (`tests/generate_go.rs:198`) | low |
| `items` × array branch of a `oneOf` | items.md:187-191 | yes (showcase `measurements`, `addressListOrLabel`) | low, but `maxItems` in a branch untested |
| `prefixItems` × `items` tail | prefixItems.md:75 — reject | **no** | P2 — verified rejects |
| `prefixItems` / `unevaluatedItems` × every schema position | P6, reject everywhere | **no** (one position each) | P2 — verified rejects in 8 positions incl. dead `$defs` |
| `unevaluatedItems: true` bare / with a schema value | unevaluatedItems.md:94-95 — reject | **no** (only `false`) | P2 — verified rejects |
| array `items` (draft-7 tuple) → `prefixItems` fix-it | items.md:72-74 | test exists but asserts the *generic* message (`:11882`) | P2 — diagnostic quality (#10) |
| `maxItems` × array-valued `const`/`default` | maxItems.md:155-160 — deferred to that feature | n/a | none (composite literals not in subset) |

## Verified-good

- **Loader reject surface for `prefixItems` / `unevaluatedItems`** fires in every position I could reach: property, `items` subschema, `additionalProperties` value schema, `oneOf` branch, `allOf` branch, `contains` matcher, `propertyNames`, a live `$defs`, an **unreferenced** `$defs`, and a service operation's `$ref`ed input — always with a located, fix-it diagnostic (`src/parser/json_schema.rs:1583-1605`, `:1505-1513`).
- **`type:array` without `items`**, **`items` without `type:array`**, **`items: {}`**, and **out-of-subset elements** all reject with the spec's diagnostics (`src/parser/json_schema.rs:1675-1690`).
- **Count-bound grammar**: non-number, negative, fractional, and above-2^53−1 all reject; `minItems > maxItems` rejects, including **after an `allOf` merge** (`merge_extra_value` at `src/parser/json_schema.rs:5543-5546`, re-validated).
- **`uniqueItems` grammar**: non-boolean rejects in both the raw-grammar pass (`:1168-1175`) and the array pass (`:2506-2508`); `false` is a true no-op (no predicate emitted anywhere).
- **Type mapping** matches `items.md:89-124` exactly in all four: `[]T` / `T[]` / `list[T]` / `List<T>`; nullable elements → `[]*string` / `(string | null)[]` / `list[str | None]` / `List<@Nullable String>`; nested → `[][]T`; inline object elements named `<Model><Prop>Item` and `…ItemItem` at depth two.
- **Sibling array keywords inspect the original wire instance** on deserialize in all four (`len(elems0)` / `raw.x.length` / `len(typing.cast(...))` / `field.size()`), and duplicate detection walks the raw elements — matching `items.md:162-168`. Asserted by the showcase suites for the element-fails-but-count-passes case.
- **Indexed paths** `tags[2]`, `grid[1][0]`, `rows[0].id`, and the typed-map form `maps.<key>` are correct in Go, Python and Java at every depth (and in TS everywhere except the `uniqueItems` block).
- **Violation ordering** — indexed element violations before array-level ones — holds on deserialize in all four.
- **Duplicate-reporting semantics** agree across the four: the *first* occurrence is kept as the anchor, so `[a,a,a]` reports `1 equals 0` and `2 equals 0`.
- **Recursive runtime discovery**: an element-only `format`/`contentEncoding` correctly pulls in `TemporalSupport`/`Base64Support` (`items.md:169-173`) — verified by generating a schema whose only temporal/base64 use is inside `items`.
- **Empty array is vacuously valid**, `minItems: 0`/`maxItems: 0`/`minItems == maxItems`/`uniqueItems: false`-over-composite all load, and `minItems:3 + uniqueItems` over `boolean` is *not* rejected at load — all matching the specs.
- **Scalar `uniqueItems` equality on the mainstream path** is correct and agrees across all four for `[1, 1.0]`, `[5, 5e0]`, `true`-vs-`1`, and (except Java serialize) `±0`.
- **`minItems`/`maxItems` over nullable elements** load and emit correctly — right, since the count keywords are element-type-agnostic.
