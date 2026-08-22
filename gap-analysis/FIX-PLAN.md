# nexgen JSON Schema — consolidated fix plan

Synthesis of the 14 per-spec gap analyses in this directory (54 specs, 47 P0
divergences, ~60 P1s). Findings are cited as `<report>#<n>` — e.g. `13#2` is
divergence 2 in `13-type-nullability.md`.

The ordering principle is **leverage, then dependency**: root causes that close
many findings come first, and the verification mechanism that would have caught
them comes before the fixes it is meant to guard.

---

## The shape of the problem

Nearly every P0 is one of six root causes, not an isolated bug:

| # | Root cause | P0s | Where |
|---|---|---|---|
| A | The nullability `oneOf` wrapper is read verbatim by Go's parse dispatcher and Java's field planner | 9 | `go.rs`, `java.rs` |
| B | Materialized types (temporal `format`, `contentEncoding`) keep string-shaped checks | 11 | all four emitters |
| C | The P15/Stage-3 identifier passes cover `properties` and little else | 6 | `parser/json_schema.rs` |
| D | Four independent reimplementations of "are these two values equal?" | 9 | all four emitters |
| E | `pattern`'s portability gate is one-directional (Rust-compilable ≠ portable) | 4 | `json_schema/pattern.rs` |
| F | Emitter-local `$ref` resolution can't see other modules | 3 | `go.rs`, `typescript.rs`, `python.rs` |

The remaining ~15 P0s are discrete emitter bugs (Wave 5).

**Why they all survived:** the cross-language conformance manifest has 4 cases,
and `tests/json_schema_conformance_manifest.rs` never executes any of them — it
validates the manifest's own JSON shape and greps consumer files for an anchor
substring (`13#gap-9`). Parity is currently asserted by four independently
hand-written suites that use *different* schemas. That is precisely the setup in
which "Go accepts what Java rejects" goes unnoticed.

---

## Wave 0 — Make verification real (prerequisite)

Without this, every fix below lands unverified across languages.

### 0.1 Make the conformance manifest executable — **P0**
`tests/json_schema_conformance_manifest.rs:118-303` checks manifest shape +
anchor substrings only. Build a driver that generates all four backends from a
case's schema, runs each `accepted_wire_values` / `parse_failures` /
`serialize_failures` through the generated code, and asserts identical verdicts.
Make `permitted_presence_nullability_collapse` a **closed** declaration: any
fixture member not listed must round-trip byte-identically in every target
(Python already does this locally at
`samples/python/tests/test_wire_fixtures.py:130-142`).

Closes the mechanism gap behind: `01#gap-7`, `02#gap-2`, `03#gap-3`, `04#gap-8`,
`05#gap-5`, `06#gap-3`, `07#gap-1`, `08#gap-12`, `09#gap-1`, `10#gap-3`,
`11#gap-18`, `13#gap-9`, `14#gap-6`.

### 0.2 Compile/import the generated output in tests — **P0**
- `tests/generate_go.rs` renders to a `String` and greps it; only committed
  samples ever hit `go vet`. Five P0s are Go **build breaks**
  (`05#1`, `05#5`, `07#1`, `09#1`, `13#1`).
- `tests/generate_{typescript,java,python}.rs` are text-assertion only — they
  never run `tsc`, `javac`, or `ast.parse` (`10#gap-11`).
- Generated Python is only syntax-checked *after* `ruff format`, which masks
  `05#7` (nested same-quote f-string, `SyntaxError` below 3.12; the declared
  floor is 3.10).

Add a probe-matrix harness that generates a list of adversarial schemas and
runs the real toolchain per language, unformatted.

### 0.3 Run the corpora against the four runtimes — **P0**
`pattern_conformance` (83 pairs) and the five `format_*` corpora are executed
only by the Rust gate (`json_schema/pattern.rs:319`, `json_schema/format.rs:589`).
The pattern corpus carries no expected match result at all, so even a runner
could only check pairwise agreement — add an `expect_match` field first.
`08#gap-1`, `09#gap-1`, `09#gap-2`.

---

## Wave 1 — Nullability unwrap (root cause A) — 9 P0s, one fix shape

When a property is `oneOf: [T, null]`, Go's parse dispatcher and Java's field
planner read the **wrapper**, which carries no `type` and no keywords.

### 1.1 Go: parse dispatcher falls through to `parseStringField` — **P0**
`go.rs:3301-3311`. Every preceding arm dispatches on `property.ty`, which is
`None` for the wrapper, so a nullable `integer`/`number`/`boolean`/`array`
assigns a `*string` into a `*int64`/`*float64`/`*bool`/`[]T`. **The package does
not compile.** `13#1`, `07#1`, `06#4`.

### 1.2 Go: `render_validate` drops every constraint — **P0**
`go.rs:2631`, `:2720-2810`, `:3131`. Gated on `property.ty` and
`is_closed_value_schema(property)`; never calls `nullable_non_null_schema`.
Loses `minLength`, `maxLength`, `pattern`, `format`, `enum`, `const`,
`minimum`/`maximum`/`multipleOf`, `contains`. Tellingly `go.rs:325-332` *does*
unwrap, so the compiled pattern/format regex vars are emitted and never
referenced. `13#2`, `07#1`, `11#1`.

### 1.3 Java: `resolve_model_kind` reads the wrapper — **P0**
`java.rs:1899-1968` — `const_value`/`enum_values` (`:1899`),
`NumericConstraints::from_schema` (`:1962`), `StringLengthConstraints` (`:1963`),
`ArrayConstraints` (`:1964`), `schema: property.clone()` (`:1965`) all take the
wrapper. Java **silently accepts** payloads TS and Python reject.
`13#2`, `07#1`, `11#1`, `06#4`.

**Fix template:** `java.rs:1885-1887` already unwraps for `additionalProperties`;
`go.rs:5271`/`:5287` already unwrap for `temporal_kind`/`content_encoding_kind`.
TS does it at `typescript.rs:956`/`:1132`/`:3989`. Apply the same unwrap at every
site that reads a property's shape.

### 1.4 Go: required+nullable array emits `*[]T` — **P1**
Spec says `[]T` (nil = null). `13#6`.

### 1.5 Tests
Nullable non-`string` properties are generated by **exactly one test in the
repo** (`tests/generate_python.rs:557`). Add nullable `integer`/`number`/
`boolean`/`array`/`enum`/`const`/constrained-string to the Go, TS and Java wave-3
matrices and to `showcase.nexusrpc.yaml`. `13#gap-1`, `13#gap-2`.

---

## Wave 2 — Materialized-type guards (root cause B) — 11 P0s

Wherever a field stops being a plain `string` — `format: date-time|date|time|
duration`, `contentEncoding: base64|base64url` — the emitters keep generating
string-shaped code.

### 2.1 Compile/parse breaks — **P0**
- Go `go.rs:2810-2812`: the shared-`Validate` string branch guards on
  `temporal_kind` but **not** `content_encoding_kind` → `utf8.RuneCountInString`
  against a `[]byte`. `08#4`, `10#1`.
- Go `go.rs:5696-5703`: closed-value `subject = (*m.Field)` derefs a slice. The
  sibling accessor at `go.rs:2571-2578` already special-cases bytes — that is
  the fix template. `10#1`.
- Go `go.rs:634-641`: `uniqueItems` picks the map key type from the *schema's*
  `type: string` while the slice holds `time.Time`/`[]byte`. (`[]byte` is not
  comparable at all, so it can never be a Go map key.) `05#1`.
- Go `go.rs:106-111` → `:3341-3343`: the loop scaffold is emitted for any
  `format`, but the body at `:3450-3471` only appears when a sibling
  `minLength`/`maxLength`/`pattern` is present → `declared and not used` for
  arrays, typed maps (`:4119`) and `propertyNames` (`:843`, `:920`) of `time`
  or `duration`. `09#1`, `04#1`.
- Python `python.rs:1752-1759` + `:1814`: same guard/body mismatch in
  `propertyNames` → empty `for` body → `IndentationError` **at import**. `04#1`.
- TS/Java silently emit **no key check at all** for the same shape. `04#1`.

**Fix:** make every `render_*` guard agree with its body — gate on
`check_for(format).is_some()`, as Java already does at `java.rs:631-635` — and
independently decide whether a materializing `format` inside `propertyNames`/
`contains` should reject at load (see Decision D6).

### 2.2 `const`/`enum` compared on different sides of the codec — **P0**
Two separate instances of the same design hole:
- **contentEncoding:** Go compares *decoded bytes* (`go.rs:5705-5726`,
  `bytes.Equal`); TS/Python/Java compare the *wire string*. With the spec's own
  `const: "aGk="`, the wire `"aGl="` is accepted by Go and rejected by the other
  three. Mirror case: a non-canonical literal makes the field **unsatisfiable on
  serialize** in TS/Python/Java. `10#2`.
- **temporal:** Go compares native values, TS/Python/Java compare wire strings,
  and the literal is never canonicalized (`parser/json_schema.rs:2150-2169`).
  `const: "PT90M"` → wire `"PT1H30M"` is Go-only-accepted, and a model parsed
  from `"PT90M"` **cannot be serialized at all** in TS/Python/Java. The helper
  that would fix this, `format::canonicalize_duration` (`format.rs:204-208`), is
  **dead code**. `09#2`.

**Fix:** canonicalize the literal at load, and compare on one agreed side in all
four (recommend: the canonical wire string).

### 2.3 No serialize-side predicate for `duration`/`time`/offsets in Go and Java — **P0**
`go.rs:2656-2718` and `java.rs:443-447` emit only a `Year() < 1` check, and only
for `DateTime|Date`. Measured Go output: negative duration → `"PT-1H-30M"` (its
own parser rejects it); 500 ms → `"PT0S"`; a 30-second offset → `"+00:00"`; year
10000 → a 5-digit year. Python raises for all four; TS re-validates the wire.
The spec's justifying parenthetical ("`time.Duration` always represents a
supported time-only duration") is factually false. `09#3`.

### 2.4 Python's temporal regexes skip the `\Z` rewrite — **P0**
`python.rs:965-985` interpolates the pinned pattern verbatim, unlike
`:1643`/`:1817`/`:5028`, which all call `pattern::rewrite_end_anchor`. A trailing
`\n` passes and escapes as a raw `ValueError`/`KeyError` — not an aggregated
`ValidationError` (P11 break). Visible in the checked-in sample:
`samples/python/temporal/_definitions.py:214`. `09#4`.

### 2.5 `contentEncoding` alongside `format` is ungated — **P0**
`validate_content_encoding` (`parser/json_schema.rs:2186-2262`) never checks
`format`. A temporal `format` silently drops the `contentEncoding` in all four
(the field becomes a date, never decoded; Go emits a dead regex var); a
non-temporal `format` yields non-compiling Go and an unsatisfiable field
elsewhere. `10#3`.

### 2.6 TS/Java compare materialized elements by reference — **P0**
`typescript.rs:886-905` (`Map<unknown, number>`) and `java.rs:857-860`
(`HashMap<byte[], Integer>`) fall back to reference identity, so byte-equal
`Uint8Array`/`Temporal`/`byte[]` duplicates serialize fine and are then rejected
on deserialize and by Python. `05#3`.

### 2.7 Smaller, same cluster
- Java materializes `time` as `String`, not `OffsetTime`/`LocalTime`
  (`java.rs:1235-1242`) — and so gets no serialize check. `09#6` (P1).
- Java renders temporal `const`/`default` literals raw:
  `OffsetDateTime.parse("2021-06-15t12:30:45z")` throws at call time; the loader
  accepts lowercase. `java.rs:5140-5154`. `09#7` (P1).
- Java `byte[]` uses `Objects.equals`/`Objects.hash` — two models parsed from the
  same payload are unequal; `toString()` prints `[B@1b6d…`. `10#5` (P1).

---

## Wave 3 — Identifier passes (root cause C) — 6 P0s

The P15 collision pass and Stage-3 validity check cover model `properties`.
Everything else the generator synthesizes is outside them.

| Missing from the pass | Effect | Cite |
|---|---|---|
| Go union interface / variant wrappers / dispatcher (`json_schema.rs:6963`) | Two unions with the same derived name **silently merge** — one binds the other's interface. The exact schema `rejects_colliding_union_functions_python` (`:11561`) rejects for Python is accepted for Go with wrong output. Also `FooBarBaz redeclared`. | `01#2` P0 |
| Operation identifiers (`json_schema.rs:6787-6851`) | `getId` + `getID` → duplicate Go fields / duplicate Python attrs / duplicate TS keys **and the same default wire name**, no diagnostic. | `14#3` P0 |
| Stage-3 validity for service/operation keys (`:4516-4535`, `:7018-7043`) | Operation `import` → uncompilable Java `void import(In)`; Go/TS/Python auto-mangle to `import_`, which P15 forbids outright. | `14#2` P0 |
| Module-path segments (`:447-462`, `:568-577`) | `class.json` → `from .class import Class` (Python `SyntaxError`, verified) and `package outj.class;`. Only *generated-file* names are checked. | `14#4` P0 |
| Java nested classes (`collect_synthesized_top_level` returns early for every language but Go, `:6966-6975`) | A `const`/`enum` member named `deserializer`/`serializer` emits duplicate nested classes; one named `violation` shadows the imported runtime `Violation`. Four probed schemas load clean and emit non-compiling Java — one of them **verbatim the repo's own Go-only test schema** at `:11119`. | `03#4`, `11#5` P1 |
| Go's fixed method set (`validate_member_scope`, `:7018-7104`) | A member named `validate` → `type X has both field and method named Validate`. | `03#5` P1 |
| Java `get<Field>OrDefault` (`java.rs:2914`) | `{a: default}` + `{aOrDefault}` emits two identical methods; Go rejects the same schema. | `11#6` P1 |
| TS `<MODEL>_DECLARED` | Asymmetry with Python; no exploitable input found (**unverified**). | `03#13` P2 |

Also here: **TypeScript rejects keyword-named members** (`member_identifier_defect`,
`:6416-6430`), contradicting `properties.md`'s own positive matrix row — the
spec says Go and TS need no override. `03#6` (P1; spec is internally
inconsistent, pick a side).

---

## Wave 4 — Cross-language value equality (root cause D) — 9 P0s

P1 mandates one accepted/rejected value set. Each backend derives equality
independently.

### 4.1 Numbers
- **Go `multipleOf` on a `number` field is not `fmod`** — `isJSONMultiple`
  (`go.rs:1516-1521`) does exact rational arithmetic over the *shortest decimal
  spelling*. `1e23 % 5`: Go accepts, others reject. `1e300 % 3`: Go rejects,
  others accept. Both measured against real Go/Node/Java/CPython. Commit
  `e2b8de6` deleted the test that documented this semantics rather than the
  semantics. `07#2` **P0**.
- **Python compares `number` bounds as exact ints** (`py_bound_literal`,
  `python.rs:101-106`, plus the parse adapter storing the raw value). Wire
  `9007199254740993` against `maximum: 9007199254740992`: Python rejects,
  Go/TS/Java accept. `07#3` **P0**. Same root cause makes a `type: number` field
  round-trip a different value in Python than everywhere else (`13#5`, P1).
- **Serialize-side ±(2^53−1) cap exists only in Go** (`go.rs:2729-2751`). TS,
  Python and Java emit an over-cap integer that **every** parser — including
  their own — then rejects. `13#3` **P0**.
- **Fractional literals ≥ 2^52**: Go parses the decimal text and rejects
  `4503599627370496.5`; TS (`Number.isSafeInteger`), Python (`float.is_integer`)
  and Java (`node.doubleValue()`) see the already-rounded double and accept.
  Java's `specLong` is also *not* the `BigDecimal` helper the spec prints.
  `13#4` **P0**. (`type.md`'s "`Number.isSafeInteger` is complete and sound"
  claim is itself false.)
- **Loader literal-vs-`multipleOf` uses `(v/m).fract()`** — always `0.0` above
  2^52, so `{multipleOf: 3, const: 1e22}` loads and every runtime then rejects
  the const. A *third* divisibility semantics. `07#5` (P1).
- **`multipleOf` × range emptiness is gated on `is_integer`**
  (`json_schema.rs:1926-1939`) → `{type: number, minimum: 1, maximum: 2,
  multipleOf: 5}` loads. `07#4` (P1).
- **Java integer `const` written `1.0` → `0L`** (`java.rs:5184`,
  `as_i64().unwrap_or_default()`). Java accepts `0` and rejects `1` where the
  other three do the opposite. `11#2` **P0**.
- **`enum` uniqueness is by JSON representation** (`json_schema.rs:3090`):
  `enum: [1, 1.0]` and `[0, -0.0]` load and produce duplicate Go `switch` cases →
  compile error. `11#7` (P1).
- **TS untyped extras corrupt integers > 2^53** — the converter receives an
  already-`JSON.parse`d value, so `9007199254740993` → `…992` where Go
  (`json.RawMessage`), Java (`JsonNode`) and Python (`int`) preserve it.
  `additionalProperties.md:212` claims the opposite. Fixtures deliberately stop
  at 2^53. `03#2` **P0** — and possibly unfixable without owning the parse step
  (see Decision D8).

### 4.2 Discriminators
- **Go switches on the discriminator's raw JSON text** (`go.rs:2256`) —
  `{"kind":1.0}` misses in Go, matches in TS/Python. `01#4` **P0**.
- **Java gates dispatch on `disc.isTextual()`** (`java.rs:2186`) — an
  integer/boolean `const` tag never selects a branch, though the loader admits
  any scalar. `01#3` **P0**.

### 4.3 `uniqueItems`
- **Java serialize says `-0.0 ≠ 0.0`** (`java.rs:857`, `Map<Double,Integer>`),
  contradicting P1 *and* Java's own deserialize path, which normalizes via
  `SpecNumbers.valueKey`. `05#2` **P0**.
- Reference comparison of materialized elements — see 2.6.
- Python compares decoded `datetime`s (instant-based) while Java compares
  `OffsetDateTime` (offset-sensitive). The spec's "deserialize compares the wire,
  serialize compares decoded values" rule **cannot hold** for materialized types.
  Needs a spec decision. `05#3`.

### 4.4 `contains` matcher predicate
Each backend derives the matcher's effective kind from a different source (Go:
no type guard; TS: declared `type` only; Python: `type` → first `const`/`enum`
literal's kind → element type; Java: `type` → element type):
- Python's guard from the first enum member: `contains: {enum: [2, 1.5]}` + `[1.5]`
  is accepted by Go/TS/Java, rejected by Python — and **flips if you reorder the
  enum**. `06#1` **P0**.
- A fractional matcher bound over `integer` elements is truncated by
  Go/TS/Java-typed (`>= 1`) but not Python/Java-raw (`>= 1.5`) — **Java disagrees
  with itself across the P12 boundary**. `06#2` **P0**.
- Go's integer matcher over `number` elements omits the 2^53 cap: `[1e300]`
  accepted by Go, rejected by the other three. `06#3` **P0**.
- TS emits no element type guard for a typeless matcher → bare `TypeError`
  escapes instead of an aggregated `ValidationError`; `"9" >= 5` coercion also
  miscounts. `06#5` **P0**.

**Recommended shape:** extract one shared value-equality / number-classification
contract (mathematical equality, ±0, the cap, integral-spelling) and generate all
four from it, rather than patching nine sites.

---

## Wave 5 — Regex portability (root cause E) — 4 P0s

`gate_and_normalize` (`json_schema/pattern.rs:41-69`) rejects only what Rust's
`regex` **cannot** compile. Rust's accepted language is a superset of
ECMA-262-with-`u`, Python `re`, and `java.util.regex` in several directions.

- `^\d{3}\-\d{4}$` — an ordinary phone pattern — emits TS that throws
  `SyntaxError` **at module import** (the mandatory `u` flag makes `\-` illegal).
  `\p{L}` breaks Python; `a}b`, `a]b`, `\x{1F600}`, `(?P<n>a)` each break at
  least one target; `[[:alpha:]]` and `[a-z&&[^aeiou]]` compile in ≥2 targets and
  **match differently**. `08#1` **P0**.
- **`.` diverges on `\r`, U+0085, U+2028, U+2029** — Go/Python match, JS/Java
  don't. Only the astral axis was pinned; the corpus tests `a.b` vs `a\nb` only.
  `08#2` **P0**.
- **`\s`/`\S` normalization escapes nested classes and `&&`** — `flatten`
  (`pattern.rs:224-240`) recurses only into `Union`, treats `Bracketed` as opaque
  and returns empty for `BinaryOp`, so `[a[\s]]`, `[\w&&\s]`, `[[\S]]` pass
  through raw — reintroducing the JS-Unicode divergence the rewrite exists to
  eliminate and **bypassing the `\S`-in-class reject**. `08#3` **P0**.
- **ReDoS**: `^(a+)+$` is gate-accepted, linear in Go/Rust, **39 s for a 31-char
  input in Python** (measured). A gate-accepted schema is a remote DoS in three
  of four targets. `08#9` (P1, needs a policy decision).
- The loader's literal-vs-`pattern` check runs Rust's **Unicode** `\d`/`\w`, so
  `{pattern: "^\\d+$", default: "٣"}` loads and emits a default all four runtimes
  reject. Fix: `RegexBuilder::unicode(false)`. `08#6` (P1).
- Compile-once violated: Go recompiles per element (`contains`) and per key
  (`propertyNames`); Java recompiles inside nested loops. `08#7`, `09#11` (P1).

---

## Wave 6 — Cross-module resolution (root cause F) — 3 P0s

- **Cross-file `$ref` union branches are silently dropped in Go/TS/Python.**
  `find_ref_model` searches only the current module's model list
  (`go.rs:1790`/`:1815`/`:1914`, `typescript.rs:2184`, `python.rs:2604`). A named
  cross-file union becomes `type Shape struct{}` in Go; a property-level one
  becomes the first branch's concrete type. **Java is correct** — it resolves
  against the whole plan (`java.rs:1409`), which is the fix template. `01#1` **P0**.
- **Go drops the service binding when the module owns no models.** Reachability
  strips the foreign `$ref`'d type (`planning/reachability.rs:80-85`), so
  `GoExternalModels::new` (`go.rs:823`) picks the WIT backend: no `var Svc`, WIT
  operation funcs instead, and a `go.temporal.io/sdk/workflow` dependency in
  definitions-only output. The repo's own fixture (`tests/generate_go.rs:3121`)
  triggers it; the test passes because it only counts `type Page struct {`.
  `14#1` **P0**.
- **Cross-module `$ref` to a named union emits `*Foo`** (pointer to interface) in
  Go — `go build` fails. `01#6` (P1).

---

## Wave 7 — Discrete emitter bugs

Independent of the clusters above; each is self-contained.

**P0**
- TS emits `if () {` for a **closed empty object** — `render_closed_object_unknown_key_check`
  (`typescript.rs:3821`) joins zero field terms. The spec's own positive matrix
  row produces unparseable TS. No schema anywhere in the repo uses the shape.
  `03#1`.
- TS's nested `uniqueItems` loop **shadows the enclosing index**
  (`typescript.rs:890`, hard-coded `(element, index)`), so `[[7,7],[1,2]]` reports
  `matrix[1]` instead of `matrix[0]` — exactly the shadowing `items.md:158-161`
  forbids by name. `05#4`.
- Go emits `{"tags":null}` for a **required non-nullable array whose slice is
  `nil`** (`go.rs:4016`); its own decoder then rejects its own output.
  Inconsistent even within Go — materialized-element arrays correctly emit `[]`.
  `05#5`.
- Python **docstring escaper misses a trailing `"`** (`python.rs:7013-7017`,
  `:6959-6963`) → `"""He said "hi""""` → hard `SyntaxError`. Hits class,
  attribute, service and operation docstrings, and the shared WIT front-end.
  `12#1`.
- A **deprecated operation** makes every Python definitions-only package
  unimportable: `python.rs:5030` writes `typing.Annotated` but `import typing` is
  gated at `:4171`; `nexusrpc`'s `@service` evaluates the annotation → `NameError`.
  The one existing test uses native-api mode, which incidentally adds the import.
  `14#5`.

**P1**
- Generated Python for array count bounds is a **`SyntaxError` below 3.12**
  (nested same-quote f-string, `python.rs:1677` × `:4722`); masked because
  samples are `ruff format`ed. Declared floor is 3.10. `05#7`.
- Go `enum` + `default` **does not compile** — `<Field>OrDefault()` returns the
  primitive while the field is the closed defined type (`go.rs:2537`/`:2591`).
  This pair is an accepted-positive row in `enum.md` and is tested nowhere
  without a `format` that sidesteps the closed type. `11#3`.
- The **serialize-side required-presence check exists in no target**, and the
  four disagree on the wire: Go/Python emit `null`, Java/TS omit the key.
  `required.md:107-115` mandates one aggregated failure. `03#7`.
- **Java does not re-path nested violations on serialize** (`zip` vs
  `address.zip`, `java.rs:3608`); Go/TS/Python do. `03#8`.
- Go emits **no catch-all/declared key-collision check for untyped open objects**
  (`go.rs:2937`, gated on a *typed* catch-all) — silently drops the extra where
  TS/Python/Java all raise. `03#3`, `04#11`.
- The base64 pinned regex **admits non-canonical trailing bits**
  (`content_encoding.rs:54-59`) — `"aGl="`, `"AB=="`, base64url `"aGl"` all match
  and re-serialize to a *different* wire, contradicting
  `contentEncoding.md:93-95`. Verified end-to-end: `{"req":"aGl="}` →
  `{"req":"aGk="}`. `10#4`.
- **Java has no serialize-side `contains` assertion**; Go/TS/Python all do.
  `06#gap-8`.
- Go's `Validate()` on a declared-property model **omits `minProperties`/
  `maxProperties`/`dependentRequired`** (`go.rs:2613`) while the map-shaped
  `Validate()` includes them — and its doc comment claims completeness. `04#4`.
- **Java closed-value constants are named from the member, not the value**
  (`Kind.KIND`, `Tier.TIER_1` vs the spec's `SHOWCASE`, `V_1`); the `V_`
  leading-letter rule is dead code. `11#4` — see Decision D3.
- `x-<lang>-enum-names` **silently ignores numeric and boolean members** (all
  three lookups gate on `Value::String`) — the only escape hatch for those
  collisions does not exist. `11#11`.
- A **single-element `enum` is not normalized to `const`**, so TS emits no
  `<FIELD>_CONST`, Python omits the dataclass default, and both report
  `must be one of` where Go/Java report `must equal`. `11#8`.

---

## Wave 8 — Loader accepts that should reject

Each is a missing P7.1 guard; all are small and independent.

| Gap | Cite |
|---|---|
| `oneOf` branches skip `validate_type_presence` (`json_schema.rs:1408`) → an itemless `{type: array}` branch loads and Java infers `List<String>` vs `any`/`unknown`/`Any` elsewhere | `01#5` **P0** |
| `default` on a sum-type union is neither validated nor lowered (`:4357`) → Go emits `return *m.F` on an interface | `01#7` P1 |
| Unsatisfiable-recursion check treats every `oneOf` edge as terminating (`:3184`) | `01#9` P1 |
| `propertyNames` subschema allowlist walks only `Schema::extra` (`:2869`), so `properties`/`required`/`items`/`oneOf`/`additionalProperties` pass silently | `04#3` P1 |
| Non-string `title`/`description` **silently coerce** everywhere except the flattened root — `$defs.R.title: 42` → `// R 42`. The two existing tests only exercise the root | `12#2` P1 |
| `merge_multiple_of` LCM **panics** on i64 overflow (`:5674`); a release build wraps and emits a negative divisor | `02#5`, `07#6` P1 |
| Empty `fqn` accepted → empty wire name in all four | `14#7` P1 |
| A redundant same-axis bound pair inside an `allOf` branch is silently collapsed, while the same typo on a plain node rejects loudly | `02#11` P2 |
| `properties: true`/`false`/`[]`, `items: true`/`false`/array fall out as raw serde errors with no location or fix-it | `03#12`, `05#10` P2 |

---

## Wave 9 — `allOf` recursive merge (2 P0s)

`merge_two` (`json_schema.rs:5339`) is only correct for the **top-level**
conjunct list; `merge_properties`/`merge_items`/`merge_additional_properties`
call it on raw, un-flattened child schemas.

- **`acc.reference = None` unconditionally** (`:5341`) → when two branches declare
  the same property name, the referenced type's fields and `required` vanish
  entirely. When *both* branches use the same `$ref`, the load fails with a
  nonsense fix-it telling the user to supply the `$ref` they already supplied.
  `02#1` **P0**.
- **`Schema::one_of` is never touched** (`:5339-5376`) and
  `reject_combinator_branch` is only called from `expand_branches` → a nullable
  `oneOf: [T, null]` property merged with a constrained sibling emits a
  non-nullable field and rejects `null`. `02#2` **P0**.
- The `$ref` branch fold copies the target's `x-<lang>-name` (`:5247`, `own_conjunct`
  `:5184`) → **Go rejects with a P15 collision while TS/Python/Java accept the
  identical schema** — a direct P1 load-time disagreement. `02#3` P1.
- The same fold copies the target's nested `$defs`, duplicating types with a
  fix-it the user cannot apply (P15 forbids a lying fix-it). `02#4` P1.

**No test anywhere exercises the recursive merge** — every existing test merges
disjoint property sets. `allOf.md:323` mandates exactly the row that would have
caught both P0s.

---

## Wave 10 — Unimplemented spec surface

Features the specs describe that do not exist. Each needs a build-or-amend call.

- **The temporal `string` opt-out** (authority model A) is entirely unimplemented
  — no keyword, no mode, no derived accessor. **P1's bounded exception (b) is
  conditioned on the loss being recoverable through it**, so Python's sub-µs
  truncation is currently unrecoverable and the exception is not satisfied on its
  own terms. `09#8`.
- **The bare-`$ref`-root alias** — no `type A = Main` anywhere, and referencing
  such a file rejects as unresolvable. `01#8`.
- **A `$defs`-named scalar `const`/`enum`** is rejected outright
  (`json_schema.rs:1374`), making both specs' `$defs`-naming branch — and
  `x-<lang>-const-name` on a def — unreachable. `11#9`.
- **`x-output-module`**, promised as a fix-it at `generated-file-layout.md:206`,
  does not exist. `14#11`.
- **The `$vocabulary: {format-assertion: true}` IDE schema** does not exist.
  `09#12`.
- Go/Java express **no closedness in array-item or typed-map positions**
  (`[]string`, `List<String>`) while TS/Python keep the literal union. `11#10`.

---

## Wave 11 — Spec corrections (no code change)

The specs are wrong or stale here; fixing the code would be the error.

- `additionalProperties.md:212` claims TS is safe for large integers — it is not.
- `type.md:149-156` claims `Number.isSafeInteger` is "complete and sound" — false
  for fractional literals ≥ 2^52.
- `type.md:58-61`,`:266` still list `patternProperties` as an object-shape
  resolution; it is unconditionally rejected.
- `allOf.md:256` says reject a differing `default`; `allOf.md:151` says last-wins.
  Code implements last-wins. `deprecated`'s OR-merge is implemented but absent
  from the merge table.
- `format.rs:11-17` and `json_schema.rs:2100-2104` still say the temporal formats
  are load-rejected; materialization shipped.
- `format.md:456-465` cites a 68-row `duration` corpus that does not exist, and a
  "compare harness" that does not exist.
- `format.md:405-424`'s "`time.Duration` always represents a supported time-only
  duration" is false (signed, nanosecond).
- `nullability.md:356-358` describes Java serialize as `@JsonInclude`; the emitter
  writes fields in code (PRINCIPLES Java §6 already says so).
- Count-family reason strings: the specs mandate `too few items: at least N, got M`
  / `too few properties: …`; all four emitters agree with each other on
  `must have at least N …` and the round-trip suites assert it. `05#9`, `04#7`.
- `oneOf.md`'s own tagged-union examples write `kind: {const: cat}` with no
  `type` — unloadable per `type.md:51`. `01#11`.
- `contentMediaType`'s diagnostic says "the string is carried verbatim" — false,
  the schema is rejected. `10#6`.
- `pattern.md` credits the corpus with pinning `[\s.]`, `[^\s]`, `[\S]`, `[^\S]`,
  `[\S.]` — none are in it; the one inline-flag pair is special-cased **by id** in
  the test rather than flagged in the data. `08#12`.
- `properties.md:140-144` says TS never hits Stage-3 rejection; the loader
  rejects every TS keyword. `03#6`.
- `contains.md`/`uniqueItems.md` specify runtime semantics for a nullable element
  type the loader refuses. `06#7`, `05#8`.

---

## Decisions needed

These are spec-vs-code calls I can't make from the code alone.

| # | Question | Recommendation |
|---|---|---|
| D1 | `contentEncoding` non-canonical trailing bits: tighten the regex to the canonical form, or drop the byte-identity claim and re-canonicalize on serialize? | Tighten the regex — the byte-identity claim is load-bearing for `const`/`pattern` |
| D2 | `contains`/`uniqueItems` over a **nullable element**: loosen the loader, or delete the spec's runtime rules? | Loosen — Go already carries dead unwrap code for it (`go.rs:745`) |
| D3 | Java closed-value constant naming: value-derived (spec: `INACTIVE`, `V_1`) or member-derived (code: `STATUS_INACTIVE`, `TIER_1`)? | Changing it re-opens the `V_` rule **and** widens the Java P15 hole (`"user"`/`"USER"` would both fold to `USER`); sequence after Wave 3 |
| D4 | Count-family reason strings: move the specs or the emitters? | Move the specs — all four agree and the suites assert the current text |
| D5 | `default` on a sum-type union: reject, or define a lowering? | Reject (P7.1) — no spec defines it and Go can't compile it |
| D6 | A materializing `format` inside `propertyNames`/`contains`: load-reject, or implement the predicate? | Reject — a materialized value can't assert a key |
| D7 | ReDoS: reject nested quantifiers at the gate, document the hazard, or per-call timeouts? | Gate-reject; "regular" is a property of the language, not the engine |
| D8 | TS untyped-extras integer corruption is unfixable without owning `JSON.parse` — accept and document, or change the TS converter boundary? | Document; amend `additionalProperties.md:212` |
| D9 | Java `get<Field>OrDefault` is arguably *better* than the spec's design (preserves absent-vs-default) — keep and update spec + P15, or remove? | Keep; register it in `validate_member_scope` |
| D10 | `uniqueItems` over materialized temporals: Python compares instants, Java compares offsets. The spec's "serialize compares decoded values" rule cannot hold. | Compare the canonical wire string in both directions |
| D11 | Empty `fqn` — reject or document as legal? | Reject (P7.1) |

---

## Suggested sequencing

```
Wave 0  (verification)     ─┐
Wave 1  (nullability)       ├─ can run in parallel; Wave 0 gates acceptance of 1-2
Wave 2  (materialized)     ─┘
Wave 3  (identifiers)      ─┐
Wave 9  (allOf merge)       ├─ loader-side, independent of each other
Wave 8  (missing rejects)  ─┘
Wave 4  (value equality)    ── needs Wave 0 to verify; largest design component
Wave 5  (regex gate)        ── independent; needs D7
Wave 6  (cross-module)      ── independent
Wave 7  (discrete bugs)     ── independent, parallelizable per bug
Wave 10 (unimplemented)     ── needs D-series answers
Wave 11 (spec edits)        ── anytime
```

Waves 0–2 alone close roughly 20 of the 47 P0s and every known
generated-code-does-not-compile break. Wave 0 should land first or concurrently;
without it, the rest is unverifiable across languages.
