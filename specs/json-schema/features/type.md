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

We accept the single-string form for all seven primitive type names, with
`"null"` admitted only as a [[oneOf]] branch. We
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
- `type: "object"` with no `properties` and no `additionalProperties` →
  reject (P7.1). Per spec this is "any object",
  but the typed-codegen contract requires explicit intent. Diagnostic
  names the three resolutions: add `properties: {...}` (typed struct),
  add `additionalProperties: true` (open opaque map), or add
  `additionalProperties: false` (closed empty object).
- `type: "null"` standalone (outside any [[oneOf]]) →
  reject. A field that is *always* `null` carries no information and
  is almost always a schema bug. A legitimate `{"type":"null"}` appears as
  a branch of a [[oneOf]]: in the two-branch form it is [[nullability]], and
  among 3+ kinds it is a nullable sum type.

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
| `"object"`  | struct from [[properties]] | interface from [[properties]] (**not classes**) | `@dataclasses.dataclass` from [[properties]]; inline object shapes are hoisted and named, while only a free-form `oneOf` object branch stays `dict[str, typing.Any]` | POJO class (Java 8; **not records** — see PRINCIPLES Java §1) |
| `"array"`   | `[]T` (T from [[items]])   | `T[]`                | `list[T]`         | `List<T>` |
| `"null"`    | only as a [[oneOf]] branch † | only as a [[oneOf]] branch † | only as a [[oneOf]] branch † | only as a [[oneOf]] branch † |

† Two branches is the [[nullability]] pattern; among three or more kinds it
is a nullable sum type. Standalone `type: "null"` — outside a `oneOf` — is a
reject.


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
- **Number range and identity**: `number` is the finite IEEE-754 binary64
  domain shared by all four targets. A numeric token whose correctly rounded
  value overflows to infinity (for example `1e400`) is rejected. Accepted
  numbers round-trip by mathematical value, not source spelling: `5`, `5.0`,
  and `5e0` are equivalent, as are positive and negative zero. Each target may
  use its idiomatic JSON serializer; Java therefore continues to emit an
  integral `double` with its normal `.0` spelling.

## Validator mapping

Per **P10** validation is enforced at the (de)serializer boundary. Per **P11**
errors aggregate into the language-native primitive.

| `type` token | Go | TypeScript | Python | Java |
|---|---|---|---|---|
| `"string"`  | typed `Unmarshal` into `string` | `typeof v === 'string'` | `isinstance(v, str)` | Jackson typed binding |
| `"integer"` | raw-map `*json.RawMessage` → runtime `parseSpecInteger` → `int64` (accepts `1.0`, rejects `1.5`, caps ±(2^53−1)) | `typeof v === 'number' && Number.isSafeInteger(v)` (accepts `1.0` natively; caps ±(2^53−1)) | runtime `_parse_spec_integer(v, path, violations)` → `int` (accepts `1.0`, rejects `1.5` and `bool`, caps ±(2^53−1)) | node helper `SpecNumbers.specLong(node, path, errs)` called by the collecting deserializer (accepts `1.0`, rejects `1.5`, caps ±(2^53−1)) |
| `"number"`  | finite `float64` parse | `typeof v === 'number' && Number.isFinite(v)` | numeric, non-`bool`, and finite-binary64 check, narrowed through `_binary64` | node helper `SpecNumbers.specDouble(node, path, errs)` (numeric and finite) |
| `"boolean"` | `bool` unmarshal | `typeof v === 'boolean'` | `isinstance(v, bool)` (rejects `1`/`0`) | `Boolean` binding |
| `"object"`  | typed struct unmarshal | `typeof v === 'object' && v !== null && !Array.isArray(v)` | `isinstance(v, dict)`, then the branch/member converter builds the dataclass | typed class binding |
| `"array"`   | typed slice unmarshal | `Array.isArray(v)` | `isinstance(v, list)` | typed `List` binding |
| `"null"`    | `raw == nil` / `isNull(*raw)` | `v === null` | `v is None` | `v == null` |

Strategy per language:
- **Go**: Every generated struct gets a custom `UnmarshalJSON`. It decodes
  the object into `map[string]json.RawMessage` and uses per-key raw-message
  pointers to preserve absence (P9), dispatches per field, builds
  `Violation{Path, Reason}` and collects them into a single non-retryable
  Temporal `ApplicationError` with type `PayloadValidationError` and
  `[]Violation` as its first detail.
  Integer fields go through a runtime helper that also enforces the
  cross-language integer cap (`±(2^53−1)`):
  ```go
  // integerCap = 1<<53 - 1 = 9007199254740991 (== JS Number.MAX_SAFE_INTEGER)
  func parseSpecInteger(n json.Number) (int64, error) {
      // Pseudocode: the emitted helper classifies n.String() directly,
      // normalizes sign/exponent, rejects non-zero fractional digits, compares
      // digit strings against integerCap, then converts the proven-safe token.
      // It never rounds through Float64 before testing integer-ness.
      // ...exact token algorithm...
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
  into a list, then throw one non-retryable Temporal `ApplicationFailure` with
  type `PayloadValidationError` and the `Violation[]` as its first detail.
  Integer fields emit `typeof v === 'number' && Number.isSafeInteger(v)`.
  `JSON.parse` silently rounds integers past 2^53 to the nearest double,
  and with the cap at `Number.MAX_SAFE_INTEGER` (`2^53−1`) that post-parse
  check is sound **for magnitude**: every integer literal past the cap
  rounds to a double that fails `Number.isSafeInteger` (e.g.
  `9007199254740993` → `9007199254740992`, which is `> MAX_SAFE_INTEGER`
  → rejected), so no text pre-scan and no `lossless-json` (P4) is needed
  to enforce the cap.
  It is **not** a complete fractional-part check. A post-parse predicate
  cannot see a fractional digit that the parse already dropped. The effect is
  systematic for literals in `[2^52, 2^53)`, where the double spacing is `1`, so
  `JSON.parse("9007199254740991.1") === 9007199254740991` and
  `Number.isSafeInteger` reports `true`. It can also occur at smaller
  magnitudes when a sufficiently fine fractional part rounds away (for example,
  `JSON.parse("1.00000000000000001") === 1`). See the parse-boundary note
  under the large-integer fixtures below for which targets share this
  limitation.
- **Python**: models are inert dataclasses (**PRINCIPLES Python §1**), so
  every type-classification check is a hand-emitted `isinstance` call in the
  model's `_<Model>TransferTypeConverter` (**PRINCIPLES Python §3**), each
  mismatch appending a `Violation { path, reason }` to the list the converter
  raises as one non-retryable Temporal `ApplicationError` with type
  `PayloadValidationError` (**PRINCIPLES Python §2**). Because `bool`
  is a subclass of `int`, an integer or number check **must exclude `bool`
  explicitly** — otherwise `True` classifies as `1`. A classified `number` is
  **narrowed to binary64** in both directions, through the generated runtime's
  `_binary64(value)`: an integral `5` is stored as `5.0`, the `float` its member
  is annotated. Python is the one target whose `int` is unbounded, so keeping the
  wire `int` let a `number` past 2^53 hold its exact value here while Go,
  TypeScript and Java rounded it into their `float64`/`number`/`double` — the
  same payload reading back as a *different* number, which P1 forbids (the
  binary64 domain is shared by all four targets, not a Python-side choice). The
  re-emitted lexeme changes with it (`5` → `5.0`), which P1 does not constrain —
  a number's spelling is not part of JSON identity. `_binary64` returns its
  argument unchanged on `OverflowError` rather than raising: a magnitude past the
  binary64 range has nothing to narrow to, and the finiteness check has already
  recorded that violation and will raise it with the others (P11). Integer fields
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
  per-field `@JsonDeserialize`, no `Long`/`Double` binding. It calls node-based
  runtime helpers per numeric field; `SpecNumbers.specDouble` rejects a
  non-number or a token such as `1e400` whose `doubleValue()` is not finite,
  while `specLong` additionally enforces integer semantics. Each helper takes the field's
  `JsonNode`, and on a bad value **pushes a `Violation` and returns
  `null`** (it never throws, so aggregation stays a clean list-append):
  ```java
  // SpecNumbers.specLong — CAP = 9007199254740991L (2^53-1).
  static Long specLong(JsonNode n, String path, List<Violation> errs) {
      if (!n.isNumber()) {                        // rejects "1", true, etc.
          errs.add(new Violation(path, "expected integer"));  return null;
      }
      if (!isFiniteNode(n)) {
          errs.add(new Violation(path, "expected finite integer")); return null;
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
  The custom helper is the only path that matches the spec. The body
  printed above is **normative**, including its finiteness guard and use of
  `n.decimalValue()`: a finite `BigDecimal` built from the token is exact, so
  `4503599627370496.5` rejects, whereas the `n.doubleValue()` shortcut
  would round it to an integer first and accept it.
  The `±(2^53−1)` cap is enforced explicitly above, before Java's own
  `longValueExact()` conversion. **Reading from a `JsonNode`
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
  `PayloadValidationError` application failure, not silently emitted. Go `int64` / Java `long` hold
  magnitudes the cap forbids, and Python ints are unbounded, so this
  check has real teeth on the out-path.
- **`number` finiteness.** Every target rejects `NaN` and `±Infinity`
  before emit with `must be a finite number`; the check applies recursively
  to properties, union branches, array elements (at every depth), and typed-map
  members. The runtimes otherwise disagree (`JSON.stringify` produces `null`,
  Go's encoder returns an unstructured error, and Python/Jackson may emit a
  non-JSON extension), so delegating to the encoder would violate P1/P11.
  TypeScript uses `Number.isFinite`; Go uses `math.IsNaN`/`math.IsInf`; Python
  uses an exact comparison against the binary64 finite range (which also safely
  handles unbounded integers); Java uses `Double.isFinite`. Integer validation
  keeps its existing cap/integrality checks and is necessarily finite already.

`object`/`array`/`string`/`boolean` carry no extra serialize check
beyond structural recursion into nested `Validate` and the
omit/emit-`null` rules owned by [[nullability]].

## Property-testing matrix

### Accepted values (positive tests)

| Shape | Values |
|---|---|
| Single primitive | `"boolean"`, `"object"`, `"array"`, `"number"`, `"string"`, `"integer"` |
| Typeless via combinator/reference | `{"oneOf":[{"type":"string"},{"type":"null"}]}`, `{"allOf":[…]}`, `{"$ref":"#/$defs/X"}` — shape from the branches / merge / target (see [[oneOf]] / [[allOf]] / [[ref]]) |

### Rejected at load time (negative tests)

Loader must produce a clear, located diagnostic for each.

| Reason | Values |
|---|---|
| Array form (P6/P7) | `["string","null"]`, `["integer","number"]`, full 7-element union, `[]`, `["string"]` |
| Absent `type` on a **leaf** schema (P7) | `{}`, `{"description":"…"}` (no `oneOf`/`allOf`/`$ref` to supply the shape) |
| Object without shape (P7.1) | `{"type":"object"}` with no `properties` and no `additionalProperties` (spec says "any object"; we require explicit intent). [[patternProperties]] does not supply a shape — it is rejected unconditionally. |
| `"null"` standalone | `{"type":"null"}` anywhere except as a branch of a [[oneOf]] |
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
- **Fractional literals rounded to integral binary64 values.** The normative rule
  is the one stated at the top of this spec: `integer` matches a JSON
  number whose **written** fractional part is zero, so
  `1.00000000000000001`, `4503599627370496.5`, and
  `9007199254740991.1` reject. Enforcing it requires reading the literal's
  decimal text, which only two targets
  can: Go's helper takes a `json.Number` (the verbatim token) and Java's
  takes a `JsonNode` it converts with `decimalValue()`. TypeScript and
  Python are handed a value the platform parser already produced — the
  double nearest the literal — and when that double is an integer within
  the safe-integer cap, `Number.isSafeInteger` / `float.is_integer()` report
  a whole number and the field is accepted. The divergence therefore applies
  to **any non-integral decimal token that rounds to an integral binary64 value
  within the cap**. The `[2^52, 2^53)` band makes the effect systematic because
  every binary64 value there is integral, but sufficiently fine fractional
  parts can round away at smaller magnitudes too. This is a **documented
  limitation of the two parse-boundary targets**, not a licence to loosen Go
  and Java. This limitation has the same root cause as the untyped-extras
  precision note in
  [[additionalProperties]] — the byte boundary sits outside the
  converter (PRINCIPLES TypeScript §4, Python §3).

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
| OpenAPI 3.0         | Human porting guidance: rewrite `nullable: true` as canonical `oneOf:[{T},{null}]` ([[nullability]]). OpenAPI 3.0 documents are not a separate accepted input dialect. |
| Swagger 2.0 / draft-4 | Human porting guidance: rewrite nullable forms during conversion to 2020-12; a declared older `$schema` rejects at the document dialect gate. |

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
