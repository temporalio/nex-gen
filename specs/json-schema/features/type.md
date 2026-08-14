# `type`

Source: JSON Schema 2020-12, Validation vocabulary, §6.1.1.

Constrains an instance to one of seven named JSON-Schema type families. The
single most fundamental validation keyword — every other validation keyword
is defined relative to an instance type, so `type` gates whether the rest of
a schema's assertions are meaningful for a given instance.

## Spec summary

- Value MUST be either a string or an array of unique strings.
- Each string MUST be one of: `"null"`, `"boolean"`, `"object"`, `"array"`,
  `"number"`, `"string"`, `"integer"`.
- `"integer"` is **not** a JSON primitive type — it matches any JSON number
  whose fractional part is zero (so `1`, `1.0`, and `1e2` all satisfy
  `type: integer`).
- Array form validates if the instance matches **any** listed type (OR).
- Absence of `type` means "any type" — equivalent to listing all seven.

## Support decision

**Support:** partial — single-string form only.

We accept `type: "<primitive>"` for all seven primitive type names. We
**reject** schemas where `type` is an array. A **leaf** schema — one that
must describe its own shape — with no `type` keyword is also **rejected**;
a typeless schema is legal only when its shape comes from a supported
combinator or reference: a [[oneOf]] (including the nullability
`oneOf:[{type:T},{type:null}]`), an [[allOf]] (merged/flattened at load),
or a [[ref]], where the type is supplied by the branches, the merged
result, or the referenced target respectively.

Rationale (citing [[PRINCIPLES.md]]):
- **P6 (strict subset)**: Multi-type unions don't lower coherently across
  Go/Java; we keep the language ceiling at OpenAPI 3.0's level.
- **P7 / P7.1 (strict schema, reject loudly)**: Array `type` is
  structurally ambiguous (is `["T","null"]` an optional T, a nullable T, or
  a sum type?). Reject at load time with a fix-it message.
- **P8 (optional ≠ nullable)**: The `["T","null"]` idiom collapses two
  different concerns; model nullability through the dedicated
  `oneOf:[{type:T},{type:null}]` convention instead (see [[nullability]]).
- Absent `type` on a **leaf** schema makes its shape undecidable,
  violating **P7** — a [[oneOf]] / [[allOf]] / [[ref]] schema is exempt,
  since its shape is fixed by the branches, the merge, or the reference
  rather than a top-level `type`.

Loader behavior:
- Array `type` → reject with diagnostic naming the schema location and
  pointing at the nullability convention.
- Missing `type` on a **leaf** schema → reject with a diagnostic requiring
  an explicit type. A schema whose shape is supplied by [[oneOf]] /
  [[allOf]] / [[ref]] carries no top-level `type` and is **accepted** — the
  type comes from the branches / merged result / referenced target.
- Unknown type name (`"int"`, `"date"`, etc.) → reject.
- `type: "object"` with no `properties`, `patternProperties`, or
  `additionalProperties` → reject (P7.1). Per spec this is "any object",
  but the typed-codegen contract requires explicit intent. Diagnostic
  names the three resolutions: add `properties: {...}` (typed struct),
  add `additionalProperties: true` (open opaque map), or add
  `additionalProperties: false` (closed empty object).
- `type: "null"` standalone (not inside the [[nullability]] pattern) →
  reject. A field that is *always* `null` carries no information and
  is almost always a schema bug. The only legitimate appearance of
  `{"type":"null"}` is as one branch of the recognized nullability
  `oneOf` (see [[nullability]]).

## Type mapping

Emitted field type when `type` appears in a field-producing position.
Optional/nullable wrapping is owned by [[required]] and [[nullability]] —
this table is the bare type only.

Required form below. Optional fields wrap per [[nullability]] (Java
boxes to `Long`/`Double`/`Boolean`; Go uses `*T`; TS uses `?` on the
field; Python uses `T | None`).

| `type` token | Go | TypeScript | Python | Java |
|---|---|---|---|---|
| `"string"`  | `string`            | `string`             | `str`             | `String` |
| `"integer"` | `int64`             | `number`             | `int`             | `long` |
| `"number"`  | `float64`           | `number`             | `float`           | `double` |
| `"boolean"` | `bool`              | `boolean`            | `bool`            | `boolean` |
| `"object"`  | struct from [[properties]] | interface from [[properties]] (**not classes**) | `@dataclasses.dataclass` from [[properties]] (an inline anonymous object schema stays a `dict[str, V]`) | POJO class (Java 8; **not records** — see PRINCIPLES Java §1) |
| `"array"`   | `[]T` (T from [[items]])   | `T[]`                | `list[T]`         | `List<T>` |
| `"null"`    | only inside [[nullability]] pattern | only inside [[nullability]] pattern | only inside [[nullability]] pattern | only inside [[nullability]] pattern |

Notes:
- **TS**: `integer` and `number` collapse to `number`; integer-ness moves
  to the validator.
- **Java**: `long`/`double`/`boolean` for required fields; `Long`/
  `Double`/`Boolean` for optional fields (see [[nullability]]). The
  primitive-vs-boxed split is what the JVM gives us for free; reference
  types like `String`/`List<T>` use a non-null validator instead.
- **Python**: `bool <: int`, so every generated integer/number check
  excludes `bool` explicitly — `True` is not `1` on the wire (see the
  validator mapping).

## Validator mapping

Per **P10** validation is enforced at the (de)serializer boundary. Per **P11**
errors aggregate into the language-native primitive.

| `type` token | Go | TypeScript | Python | Java |
|---|---|---|---|---|
| `"string"`  | typed `Unmarshal` into `string` | `typeof v === 'string'` | `isinstance(v, str)` | Jackson typed binding |
| `"integer"` | shadow `*json.Number` → runtime `parseSpecInteger` → `int64` (accepts `1.0`, rejects `1.5`, caps ±(2^53−1)) | `typeof v === 'number' && Number.isSafeInteger(v)` (accepts `1.0` natively; caps ±(2^53−1)) | runtime `_parse_spec_integer(v, path, violations)` → `int` (accepts `1.0`, rejects `1.5` and `bool`, caps ±(2^53−1)) | node helper `SpecNumbers.specLong(node, path, errs)` called by the collecting deserializer (accepts `1.0`, rejects `1.5`, caps ±(2^53−1)) |
| `"number"`  | `float64` unmarshal | `typeof v === 'number'` | `not isinstance(v, bool) and isinstance(v, (int, float))`, stored as-is | `Double` binding |
| `"boolean"` | `bool` unmarshal | `typeof v === 'boolean'` | `isinstance(v, bool)` (rejects `1`/`0`) | `Boolean` binding |
| `"object"`  | typed struct unmarshal | `typeof v === 'object' && v !== null && !Array.isArray(v)` | `isinstance(v, dict)`, then the branch/member converter builds the dataclass | typed class binding |
| `"array"`   | typed slice unmarshal | `Array.isArray(v)` | `isinstance(v, list)` | typed `List` binding |
| `"null"`    | `raw == nil` / `bytes.Equal(raw, []byte("null"))` | `v === null` | `v is None` | `v == null` |

Strategy per language:
- **Go**: Every generated struct gets a custom `UnmarshalJSON`. It decodes
  into a shadow struct of `*json.Number` / `*T` pointers (absence
  observable per P9), dispatches per field, builds
  `Violation{Path, Reason}` and collects them into a single
  `ValidationError` (a struct over `[]Violation` implementing `error`).
  Integer fields go through a runtime helper that also enforces the
  cross-language integer cap (`±(2^53−1)`):
  ```go
  // integerCap = 1<<53 - 1 = 9007199254740991 (== JS Number.MAX_SAFE_INTEGER)
  func parseSpecInteger(n json.Number) (int64, error) {
      f, err := n.Float64()
      if err != nil { return 0, err }
      if f != math.Trunc(f) { return 0, errFractional }         // "1.5" → reject
      if f < -integerCap || f > integerCap { return 0, errRange } // > ±(2^53-1) → reject
      i, err := n.Int64()
      if err != nil { return 0, err }                            // belt-and-suspenders
      return i, nil                                              // "1", "1.0", "1e2"
  }
  ```
  User-facing field stays plain `int64`. The Go primitive holds ±2^63,
  but the validator rejects anything past the ±(2^53−1) cap so all four
  languages agree on the accepted range.
- **TypeScript**: Hand-emit `typeof`/`Array.isArray` checks per field; no
  runtime schema library (P4). `Number.isInteger(v)` is spec-compliant
  for type-classification (`1.0 === 1` in JS, so
  `Number.isInteger(1.0) === true`) — verified empirically across all
  10 type-classification fixtures. Push `Violation { path, reason }`
  into a list, throw one `ValidationError` (a class extending `Error`,
  holding the `Violation[]`) at the end.
  `JSON.parse` silently rounds integers past 2^53 to the nearest
  double, but with the cap at `Number.MAX_SAFE_INTEGER` (`2^53−1`),
  a plain post-parse `Number.isSafeInteger(v)` is a complete and sound
  check — no text pre-scan, no `lossless-json`, no P4 tension. Every
  integer literal past the cap rounds to a double that fails
  `Number.isSafeInteger` (e.g. `9007199254740993` → `9007199254740992`,
  which is `> MAX_SAFE_INTEGER` → rejected). Integer fields therefore
  emit `typeof v === 'number' && Number.isSafeInteger(v)`.
- **Python**: models are inert dataclasses (**PRINCIPLES Python §1**), so
  every type-classification check is a hand-emitted `isinstance` call in the
  model's `_<Model>TransferTypeConverter` (**PRINCIPLES Python §3**), each
  mismatch appending a `Violation { path, reason }` to the list the converter
  raises as one `ValidationError` (**PRINCIPLES Python §2**). Because `bool`
  is a subclass of `int`, an integer or number check **must exclude `bool`
  explicitly** — otherwise `True` classifies as `1`. A classified `number` is
  stored **exactly as it arrived**, never coerced: an integral `5` stays an
  `int` in a `float`-annotated member, because `float(5)` would re-serialize
  as `5.0` where Go and TypeScript emit `5` — a per-language nicety paid for
  in round-trip byte-identity, which **P1** does not permit. Integer fields
  stay a plain `int` and run through the generated runtime's
  `_parse_spec_integer(value, path, violations)`: it rejects `bool`, accepts
  an `int`, accepts a `float` with zero fractional part (`1.0`, `1e2`), and
  rejects a fractional one (`1.5`) — the same accept/reject set as Go's
  `parseSpecInteger` and Java's `SpecNumbers.specLong`, reached by the same
  mechanism: like the Java helper it **pushes a `Violation` and returns
  `None`** rather than raising, so one bad integer never aborts the rest of
  the object's checks (**P11**). Python ints are unbounded, so the helper
  also enforces the cross-language cap `±(2^53−1)`:
  `abs(v) > 9007199254740991` → reject. The accepted *values* are identical
  in all four targets; the `reason` **text** for a rejected number is not —
  Python follows TypeScript, collapsing both the fractional and the
  over-cap failure into `expected integer`, where Go names them separately
  (`not an integer` / the cap message).
- **Java**: POJOs (Java 8 floor; not records, see PRINCIPLES Java §1)
  bound by the per-POJO collecting deserializer (Java §5) — **no**
  per-field `@JsonDeserialize`, no `Long` binding. It calls a node-based
  runtime helper per integer field; the helper takes the field's
  `JsonNode`, and on a bad value **pushes a `Violation` and returns
  `null`** (it never throws, so aggregation stays a clean list-append):
  ```java
  // SpecNumbers.specLong — CAP = 9007199254740991L (2^53-1).
  static Long specLong(JsonNode n, String path, List<Violation> errs) {
      if (!n.isNumber()) {                        // rejects "1", true, etc.
          errs.add(new Violation(path, "expected integer"));  return null;
      }
      BigDecimal d = n.decimalValue();            // exact; no double rounding
      if (d.stripTrailingZeros().scale() > 0) {   // "1.0"/"1e2" ok, "1.5" rejected
          errs.add(new Violation(path, "not an integer"));    return null;
      }
      if (d.abs().compareTo(BigDecimal.valueOf(CAP)) > 0) {   // ±(2^53-1) cap
          errs.add(new Violation(path, "exceeds cap"));       return null;
      }
      return d.longValueExact();
  }
  ```
  Rationale (empirically verified, Jackson 2.18): Jackson's defaults
  *silently truncate* `1.5`→`1` for `Long` fields — a P7 violation
  blocking shipping with defaults. `ACCEPT_FLOAT_AS_INT=false` fixes
  truncation but rejects spec-valid `1.0`/`1e2` and still coerces `"1"`.
  The custom helper is the only path that matches the spec.
  The `±(2^53−1)` cap is enforced
  explicitly above; `>2^63` would also trip Jackson's own range check,
  but our cap is tighter so ours fires first. **Reading from a `JsonNode`
  (not a live parser) is what lets a spec-strict failure become a
  `Violation{path,reason}` instead of a bind-aborting
  `MismatchedInputException`** — the helper is called from the two-stage
  collecting deserializer (PRINCIPLES Java §4/§5), the exact parallel of
  Go's `parseSpecInteger` and Python's `_parse_spec_integer`. The
  alternative — retaining a `JsonDeserializer<Long>` and driving it over
  a sub-parser — makes identical decisions but re-introduces a per-field
  throw/catch and a sub-parser allocation, so it was rejected.

### Serialize-side (P12)

On the way out the value is already decoded, so the wire-classification
work (the `1.0`-vs-`1.5` parse, token typing) does **not** re-run — it
lives only in the parse adapter. What the shared `Validate` re-checks
before emit:

- **`integer` cap.** The in-memory `int64`/`long`/`int`/`number` is
  re-checked against `±(2^53−1)`; a value constructed past the cap is a
  `ValidationError`, not silently emitted. Go `int64` / Java `long` hold
  magnitudes the cap forbids, and Python ints are unbounded, so this
  check has real teeth on the out-path.
- **TS `number` non-finiteness.** `JSON.stringify` silently turns `NaN`
  and `±Infinity` into `null` (empirically verified — a P7 violation on
  the *out* path). The serializer rejects non-finite numbers for
  `integer`/`number` fields (`Number.isFinite`, plus
  `Number.isSafeInteger` for `integer`) before stringifying. This is the
  only language where the encoder must add a numeric check the type
  system doesn't already give.

`object`/`array`/`string`/`boolean` carry no extra serialize check
beyond structural recursion into nested `Validate` and the
omit/emit-`null` rules owned by [[nullability]].

## Property-testing matrix

### Accepted values (positive tests)

| Shape | Values |
|---|---|
| Single primitive | `"null"`, `"boolean"`, `"object"`, `"array"`, `"number"`, `"string"`, `"integer"` |
| Typeless via combinator/reference | `{"oneOf":[{"type":"string"},{"type":"null"}]}`, `{"allOf":[…]}`, `{"$ref":"#/$defs/X"}` — shape from the branches / merge / target (see [[oneOf]] / [[allOf]] / [[ref]]) |

### Rejected at load time (negative tests)

Loader must produce a clear, located diagnostic for each.

| Reason | Values |
|---|---|
| Array form (P6/P7) | `["string","null"]`, `["integer","number"]`, full 7-element union, `[]`, `["string"]` |
| Absent `type` on a **leaf** schema (P7) | `{}`, `{"description":"…"}` (no `oneOf`/`allOf`/`$ref` to supply the shape) |
| Object without shape (P7.1) | `{"type":"object"}` with no `properties`, `patternProperties`, or `additionalProperties` (spec says "any object"; we require explicit intent) |
| `"null"` standalone | `{"type":"null"}` anywhere except as a branch of the [[nullability]] `oneOf` pattern |
| Unknown type name | `"int"`, `"float"`, `"date"`, `"any"`, `"bigint"`, `"String"`, `"INTEGER"` |
| Wrong outer type | `5`, `null`, `true`, `{"type":"string"}` |
| Nested / malformed | `[["string"]]` |

### Runtime fixtures per accepted type (validator tests)

For each accepted `type`, fuzz over:
- **Canonical accept**: `"x"`, `1`, `1.5`, `true`/`false`, `{}`, `[]`, `null`.
- **Boundary accept**: `""`, `0`, `-0`, `1.0` (must satisfy `integer`), `1e2`.
- **Wrong-type reject**: every other primitive against this type — 7×6=42
  cross-reject cases.
- **`bool`-is-not-`integer` trap**: `true` against `"integer"` must reject
  in all four languages. Go/TS/Java reject naturally (`true` is not a
  number token); Python relies on the explicit `isinstance(v, bool)`
  reject inside `_parse_spec_integer`.
- **Large integers (cap = ±(2^53−1))**: each language's helper
  (`parseSpecInteger`, `_parse_spec_integer`, `SpecNumbers.specLong`,
  and TS's `Number.isSafeInteger` use) must pass an identical fixture
  set: accept `1`, `1.0`, `1e2`, `-0`, and the cap boundary
  `±(2^53−1)`; reject `1.5`, `true`/`false`, `"1"`, non-numeric
  strings, NaN, ±Infinity, and any magnitude past `±(2^53−1)`.
  Specific boundary values: accept `9007199254740991` (`2^53−1`) and
  `-9007199254740991`; reject `9007199254740992` (`2^53`),
  `9007199254740993` (`2^53+1`, which TS silently rounds to `2^53` —
  must still reject), and `18014398509481985` (`2^54+1`). Same
  accept/reject set in all four languages.

## Interactions

- **Gates which assertions apply.** Spec §3.4 silently ignores
  type-mismatched keywords; per **P7.1** we instead **reject** mismatched
  combinations at generator time (e.g. `{type:"string", minimum: 5}`
  errors).
- **[[const]]**: per **P13.1**, the emitted field type is **closed** to
  the const value — a literal / defined type / value class over `type`'s
  primitive mapping. A bump is a deliberate breaking change to the value
  contract, surfaced loudly.
- **[[enum]]**: the value set derives from `type`; per **P13.1** the
  emitted type is **closed** to the known values and an unrecognized value
  is rejected on deserialize.
- **[[oneOf]]**: a supported union is one whose branches occupy
  pairwise-disjoint JSON type kinds — each branch's `type` supplies the
  kind that is the wire selector. `type:"null"` may appear as a branch:
  as one of exactly two (the [[nullability]] pattern) it makes the field
  nullable; a `null` branch among 3+ kinds is a nullable union ([[oneOf]]).
  `integer`+`number` branches together are rejected (unsatisfiable
  overlap).
- **[[properties]] / [[items]]**: only meaningful when `type` is `"object"`
  / `"array"`. Cross-product mismatches are generator-time errors.
  Object-shape decisions live in [[properties]] / [[additionalProperties]];
  in summary, **typed structs are open by default** (per JSON Schema
  spec and **P13** — accept and preserve extras into a catch-all),
  closed behavior requires explicit `additionalProperties: false`.
- **[[format]]**: format hints layer onto `type:"string"` (mostly); a
  format may pick a more specific emitted type (`time.Time` in Go for
  `format:"date-time"`) while staying gated by the underlying string type.
- **[[required]]** + [[nullability]] own optional/nullable wrapping;
  `type` only contributes the inner type.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native. Reject only documented out-of-subset cases. |
| OpenAPI 3.1         | Aligns with 2020-12. Native. |
| OpenAPI 3.0         | `nullable: true` → reject; only the canonical `oneOf:[{T},{null}]` form is accepted ([[nullability]]). User must rewrite. |
| Swagger 2.0 / draft-4 | Same as OAS 3.0; no type arrays; nullable rewrite required. |

Pre-draft-4 union-of-schemas form (`type: [{...},{...}]`) is irrelevant —
no current toolchain emits it.

## See also

- [[enum]], [[const]] — other any-instance-type assertions.
- [[multipleOf]], [[minimum]], [[maximum]], [[exclusiveMinimum]],
  [[exclusiveMaximum]] — numeric assertions gated by `type`.
- [[format]] — string refinements layered on `type:"string"`.
- [[oneOf]] — unions of branches with pairwise-disjoint JSON kinds
  (each branch's `type` is the selector); the nullability
  `oneOf:[{T},{null}]` pattern is the degenerate two-branch case (see
  [[nullability]]).
- [[required]], [[nullability]] — own optional/nullable wrapping.
