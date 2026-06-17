# Plan — JSON Schema → Cross-Language Code Generator

## Mission

Build a code generator that emits idiomatic, statically-typed model
code **and** runtime validators for Go, TypeScript, Python, and Java
from JSON Schema 2020-12 input. Output feeds the Temporal Nexus API
SDK ecosystem; emitted runtime depends only on each language's
Temporal/Nexus SDK plus minimal contrib libraries (Pydantic for
Python).

The generator implements a **strict subset** of JSON Schema:
ambiguous or non-lowerable features are rejected at generator time
with clear fix-it-style diagnostics rather than producing silently-
incorrect output.

## Foundations

- **`PRINCIPLES.md`** — authoritative source for design decisions.
  Cross-cutting principles are numbered (P1–P16) with sub-principles
  (e.g. P7.1); per-language principles live under language sections.
- **Target spec:** JSON Schema 2020-12 (per P5).
  - <https://json-schema.org/draft/2020-12/json-schema-validation>
  - <https://json-schema.org/draft/2020-12/json-schema-core>
- **Per-feature specs:** `features/<keyword>.md`, following the
  template established by `features/type.md`.

## Spec template (per feature)

Each `features/<keyword>.md` follows this skeleton — worked
example at `features/type.md`:

1. **Spec summary** — what the keyword does (3–5 bullets)
2. **Support decision** — support / partial / reject + rationale
   citing PRINCIPLES.md by P-number
3. **Type mapping** — emitted bare type for Go / TS / Python / Java
4. **Validator mapping** — runtime check per language + strategy,
   **including a `Serialize-side (P12)` subsection**: which checks the
   shared `Validate` re-runs on emit vs. which are parse-adapter-only
   (deserialize) or encode-adapter-only (serialize: omit/emit-`null`,
   default omission, const auto-emit)
5. **Property-testing matrix** — accepted / rejected-at-load / runtime
   fixtures
6. **Interactions** — how this keyword changes meaning of others
7. **Ecosystem variance** — which input dialects accept/reject this
8. **Open questions** — unresolved design points
9. **See also** — wikilinks to related features

## Work completed

### Cross-cutting

- `PRINCIPLES.md`: P1–P16 with sub-principles. P12: serialize-side validation — both directions share one
  `Validate(model)` over the decoded model, flanked by a deserialize-only
  parse adapter and a serialize-only encode adapter; no IR round-trip.
  P15: synthesized-identifier collisions reject at load time (no
  mangling), sharing one per-scope namespace with declared names. (The
  uniform `±(2^53−1)` integer cap now lives in [[type]]; `default`
  off-the-wire / materialized-on-read lives in [[default]].) Java §5: a
  per-POJO class-level collecting `@JsonDeserialize`/`@JsonSerialize`
  (two-stage lenient-tree-then-validate, the Jackson analog of Go's
  shadow-layout `UnmarshalJSON`), throwing one `ValidationException
  extends JsonMappingException` with `List<Violation{path,reason}>`.
  All four language sections complete — Go (6), TypeScript (6),
  Python (6), Java (6).
- `nullability.md`: cross-cutting design note (not a
  keyword). Covers optionality + nullability for all 4 languages with
  per-language enforcement strategies. Carries the per-field serialize
  omit-vs-emit-`null` table (P12). Python optional+nullable uses
  `model_fields_set`/`exclude_unset` — faithful round-trip, same tier as
  TS. Go/Java are conservative-omit. Zero open questions.

### Features

- `features/type.md`: complete. Integer cap: ±(2^53−1). 1 open
  question — cross-language conformance suite.
- `features/properties.md`: complete. Shared 4-stage case-mapping
  algorithm + `x-*-name` escape hatch + P15 per-scope collision pass.
  One implementation dependency: the Python serialize keep-set must map
  field name↔alias when a JSON name is case-mapped (PRINCIPLES Python §6).
- `features/additionalProperties.md`: complete. Open-by-default;
  typed-extras supported in every position. All four languages wrap the
  catch-all in a dedicated named member (even pure maps) for shape
  stability and clean declared/extra key separation. Go exported
  `AdditionalProperties` map field; a declared member named
  `additionalProperties` is rejected at load time. Zero open questions.
- `features/required.md`: complete. Zero open questions.
- `features/maxProperties.md`, `features/minProperties.md`:
  complete (runtime count assertions). Count is over **distinct wire
  member keys**, taken before default population (see [[default]]) — defaults never
  count; counted as one number, never summed across declared/extras
  buckets. Zero open questions.
- `features/dependentRequired.md`: complete (runtime cross-field
  presence; `dependentSchemas` split off as P6-reject). Zero open Qs.
- `features/patternProperties.md`: complete — **temporarily
  unsupported** (rejected at load time in v1, deferred not categorically
  excluded: a single-pattern typed-map form is plausibly lowerable).
  1 open question — single-pattern carve-out.
- `features/propertyNames.md`: complete — **partial** (map-shaped
  objects only; rejected alongside `properties`). 1 open question —
  static enforcement alongside `properties`.
- `features/const.md`: complete — **supported (scalar,
  string/integer/number/boolean)**. Emits a type **closed** to the const
  value for every scalar kind (P13.1): TS the closed literal (`'v'` /
  `1.5` / `true`), Python `Literal['v']` (`float` consts plain `float`),
  Go a defined type + typed value const (`type Priority int64` +
  `Priority3`), Java a value class (known constant obtainable only through
  the class). Any non-matching value is a hard reject — the same closed
  machinery [[enum]] uses with more than one known value; a bump is a
  breaking change surfaced as a compile break. Value constants are named
  from the value (`{Type}{EncodedValue}` Go / class-scoped Java), encoded
  through the [[properties]] Stage 1–4 pipeline (string values ASCII, no
  whitespace; float `.`→`_`; negatives `Neg`; Java `V_` prefix when
  needed; P15 rejects collisions). Float consts use exact `==` (Ryu
  round-trip makes it cross-language/arch identical). `const` is a pure
  assertion validated in both directions; presence owned by `required`;
  the value reaches the wire because it is set in memory. Mutually
  exclusive with `default` and `enum`. `const:null` rejected; composite
  (object/array) const temporarily unsupported. 1 open question —
  composite-const carve-out.
- `features/default.md`: complete — **supported** with off-the-wire /
  materialized-on-read semantics: annotation (no validator, never fails validation);
  off-the-wire; set-ness tracked; omit-unset with no deep-equals;
  materialized on read. Strengthens the spec's "RECOMMENDED valid
  default" to a load-time MUST (P7.1); rejects `default` on a required
  member and `default`+`const`. Read-side surfacing is native in Python
  (Pydantic field default) and Java (getter), advisory in TS
  (`?? DEFAULT_X` constant), and a generated `<Field>OrDefault()`
  accessor in Go (proto3 `GetX()`-style). Scalar-only in v1
  (`string`/`number`/`integer`/`boolean`); object/array defaults and
  `default:null` rejected, expected to relax. 1 open question —
  composite-default materialization.

- `features/maximum.md` (+ `minimum`, `exclusiveMaximum`,
  `exclusiveMinimum`): complete — **supported** (runtime comparison
  assertions). `maximum` is the canonical numeric-bound spec; the other
  three reference its machinery. Comparison is a shared-`Validate`
  predicate (P12), enforced both directions. **Integer-field bounds MUST
  be integer-valued** (`5.0` ok, `5.5` reject) — Pydantic can't build a
  fractional `le`/`ge` on an `int` field, and an integer bound keeps the
  integer-vs-float comparison lossless (the `±(2^53−1)` cap is exactly
  representable as a double, so `(double)cap == cap`, verified). Combined
  bounds reject when the interval is empty (incl. the integer "no integer
  in range" case). A **same-axis bound pair on one node is rejected as
  redundant** (`maximum`+`exclusiveMaximum`, `minimum`+`exclusiveMinimum`
  — one always dominates; P7.1), with an allOf-tightening caveat noted in
  the Applicators section. The **exclusive pair reject the draft-4 /
  OpenAPI-3.0 boolean form** (`exclusiveMaximum: true`) with a rewrite
  fix-it — the largest source-dialect difference in the family. Zero open
  questions.
- `features/multipleOf.md`: complete — **partial**: positive
  **integer** divisor only; **fractional divisor temporarily unsupported**
  (reject at load, deferred not excluded). Empirically justified (P1):
  integer modulo and IEEE `fmod` agree value-for-value across all four
  languages, but Pydantic's native float `multiple_of` is *tolerant* and
  diverges for fractional divisors (accepts `0.3` against `multipleOf:0.1`
  where `fmod` rejects) — irreconcilable without a shared decimal lib (P4).
  Integer field → integer modulo; number field → `fmod` (Python uses an
  explicit `fmod` AfterValidator for numbers, native `multiple_of` for
  ints). Combined with a range: reject when no multiple lies in the
  interval. 1 open question — fractional-divisor decimal-scaling carve-out.
- `features/maxLength.md` (+ `minLength`, `pattern`): complete —
  **string assertions**. `maxLength` is the canonical string-length spec
  (like [[maximum]] for numerics); `minLength` mirrors it. Both are
  **supported**: length is counted in Unicode **code points** (RFC 8259),
  never bytes/UTF-16/graphemes — the P1 crux, since the naive per-language
  primitive counts the wrong unit (Go `len`=bytes, TS/Java `.length`=UTF-16
  units; only Python `len`=code points). Each language uses its
  code-point-counting primitive (`utf8.RuneCountInString` / `[...v].length`
  / `len` / `codePointCount`); no normalization (NFC vs NFD lengths differ
  and all four agree per form). `pattern` is **partial** — portable
  (RE2-safe) subset only, matched **unanchored** with **ASCII** classes and
  a **code-point `.`**: the load-time compile gate is the **pure-Rust
  `regex` crate** (the generator's own engine — **no Go toolchain
  dependency**; same regular/no-backtracking RE2 family, rejects
  lookahead/lookbehind/backreferences the other three accept). Runtime
  matching is pinned per target: anchoring uses search/find (NOT Java
  `matches()` / Python `re.match`); Python compiles with `re.ASCII` via an
  explicit `AfterValidator` (not pydantic-core's Rust engine for *matching*,
  mirroring the [[multipleOf]] number decision — though that same `regex`
  crate *is* trusted for the compile gate), JS emits the `u` flag, Java uses
  default flags. **An 83-pair conformance corpus
  (`research/pattern_conformance/`) proved compile-gate + pinned-flags
  insufficient**, adding rules via a `regex-syntax` AST walk: **reject
  inline flag groups** `(?i)` (JS can't compile); **normalize `\s`/`\S`** →
  explicit ASCII class `[\t\n\x0B\f\r ]` (JS whitespace is Unicode & not
  flag-controllable; only `\S` in a *multi-member* class is rejected as an
  open complement); and **normalize the `$` anchor** (`\Z` Python / `\z`
  Java, keep `$` Go/JS) for the trailing-`\n` divergence. `\d`/`\w` kept
  (identical ASCII). **Prospective targets .NET + Ruby verified conformant**
  with per-target emitter transforms only (no new gate rules): .NET needs
  `RegexOptions.ECMAScript` + `$`→`\z` + an astral-`.`→surrogate rewrite (no
  `u`-flag equivalent); Ruby needs `^`→`\A`/`$`→`\z` (line anchors) + a
  leading `(?a)` (its `\b` is Unicode). Regex emitted as a compile-once
  constant. All three specs close the string half of the
  deferred literal-vs-constraint obligation ([[const]] / [[default]] /
  [[enum]]). Verified across all four current targets +
  prospective .NET/Ruby in `json-schema/research/string_probe/` +
  `pattern_conformance/` (incl. `runner.rb`, `dotnet_runner/`) +
  `rust_regex_gate/` + `ws_normalize/`, incl. Pydantic 2.13.4 confirmed to
  count **code points** for `min/max_length` (`pydantic_length_probe.py`)
  and native `pattern=` rejected as Unicode-class-divergent
  (`pydantic_pattern_probe.py`). All three string specs have zero open
  questions except pattern's one deferred widen-the-subset item.

### Cross-cutting (continued)

- `services.md`: complete — **supported**. Nexus extension
  (not a JSON Schema keyword, like [[nullability]]): a top-level
  `services` map → per-language service bindings (Go `struct` of
  `OperationReference`s, TS `nexus.service`, Python `@nexusrpc.service`
  class, Java `@Service` interface). Two-name model (identifier key +
  optional `fqn` wire name); operation default wire name = PascalCase
  canonical; the generator always emits the resolved wire name explicitly
  (the four SDKs default differently) for P1. `input`/`output` is
  **object-only** — a `$ref` to an object `$defs` or an inline object
  promoted to a synthesized `<Op>Input`/`Output` type (P15 namespace);
  non-object I/O rejects; omitted → void (`nexus.NoValue`/`void`/`None`/
  `void`). **Document gating now lives in [[input-files]]:** `services` is
  recognized only in a Nexus document (root `nexusrpc: "1.0.0"`); the
  envelope, dialect, and stray-`services` rules moved there. TS service
  const is `camelCase` (the WIT generator emits
  PascalCase and must be fixed). Services emit into the declaring module;
  Java is the per-file exception. Shape-compatible with the existing WIT
  emitters for reuse by the future separate crate. 2 open questions.
- `input-files.md`: cross-cutting design note (not a keyword) — the
  document-level concerns of an input file. Two file modes selected by the
  root `nexusrpc` property: **Nexus document** (root is an envelope, not a
  type; enables [[services]]; a schema-shaped root keyword rejects) vs
  **pure JSON Schema** (root is a type). Root rules: `nexusrpc` v1 =
  exactly `"1.0.0"` (else reject, P13 forward-compat); `$schema` optional,
  2020-12 only / assumed-when-absent / reject-other (P5); `$id` rejected
  (owned by [[ref]]); stray-`services` guard (P7.1). References [[ref]] /
  [[generated-file-layout]] for the input-set/closure and module
  computation rather than duplicating them. Houses the dialect/`$id`
  decisions the standalone `$schema`/`$id` keyword specs should defer to.
  2 open questions (keyword-spec deferral; widening the `nexusrpc` range).

### Key decisions taken

- **Serialize-side validation is first-class (P12).** Validation runs in
  both directions over one shared `Validate(model)` (constraint
  predicates over the decoded model), with mirror-image adapters: a
  deserialize-only parse adapter (spec-number parse, explicit-`null`
  reject, wire-absence→required, type-token classification) and a
  serialize-only encode adapter (omit-vs-emit-`null`, default omission,
  const auto-emit). No IR round-trip — sharing is at the predicate layer.
  Serialize fails before emitting a byte; Python re-validates to catch
  `model_construct`/mutation bypasses. The Python encode adapter is not a
  call site we own — the default Temporal `pydantic_data_converter`
  serializes via plain `pydantic_core.to_json` — so the omit/const/guard
  logic is baked into a generated `@model_serializer(mode='wrap')`, which
  `to_json` honors.
- **`default` materialized on read, not stored (see [[default]]).** Track set-ness;
  serialize omits unset fields with no deep-equals; surface the default
  on read. Preserving absent-vs-set (P9) protects forward-compat (P13),
  live default evolution, and proxy/intermediary fidelity — exactly
  proto3's omit-on-wire model. Serialize/omit mechanisms: Go
  `,omitempty`+pointer, Pydantic a generated `@model_serializer` over
  `model_fields_set`, TS `undefined`, Java `@JsonInclude(NON_NULL)`.
  Read-side materialize mechanisms: Java getter / Pydantic field default
  (native), Go `<Field>OrDefault()` accessor (proto3 `GetX()`-style),
  TS `?? DEFAULT_X` + emitted constant.
- **`const` is a pure assertion — no serialize-side special-casing.**
  Validate `== value` in both directions; the generator does not
  force-write. Presence is owned by `required`; the fixed value reaches
  the wire because it is set in memory. Emits a type **closed** to the
  value for every scalar kind (P13.1): TS the closed literal, Python
  `Literal` (`float` plain `float`), Go a defined type + typed value
  const, Java a value class. `const` is a *closed contract* — the same
  machinery [[enum]] uses, with one known value — so a bump breaks loudly.
  Go sets it via the value const (zero value fails `Validate` loudly);
  Java initializes a `final` field to the known constant with a getter,
  and the collecting deserializer's non-throwing membership lookup records
  a Violation for a bad wire value (the value class's `fromString` throws
  only on the standalone interop path); Python injects it in a
  `model_validator(mode='before')` (which marks it set → `model_fields_set`
  → emitted by the generic `@model_serializer`). Mutually exclusive with
  `default`/`enum`; `const:null` and composite consts rejected/deferred.
- **Synthesized-identifier collisions reject at load time, never mangle
  (P15).** Synthesized names — [[const]] value constants,
  future [[enum]] value class/members, the Go `<Field>OrDefault()`
  accessor and TS `DEFAULT_<FIELD>` ([[default]]) — share one per-scope
  namespace with declared types/members and with each other
  (package/module scope; the Go accessor sits in the struct method-set,
  where a field/method clash is a hard compile error). A single collision
  pass (after case-mapping) rejects loudly on any coincidence.
  Auto-mangling is rejected as unstable under schema evolution (P13). The
  escape hatch is the [[properties]] `x-*-name` override on the
  declaring member.
- **Optional+nullable round-trip is capability-tiered:** faithful in
  TS and Python (`undefined` / `model_fields_set` via the generated
  `@model_serializer`), conservative-omit in Go/Java (`*T` nil / `null`
  collapse; faithful would need a presence wrapper — rejected for v1 as
  P2 overhead). Per-field omit-vs-emit-`null` is a static decision from
  the optional/nullable/required declaration; the full table lives in
  [[nullability]].
- **`type` is single-string only** — array form rejected; missing
  `type` rejected; `type: "null"` standalone rejected (only allowed
  inside the nullability pattern).
- **`type: "object"` requires explicit shape** — bare `{type:"object"}`
  rejected; must add `properties`, `additionalProperties: true`, or
  `additionalProperties: false`.
- **Typed structs are open by default** (per spec + P13) — extras
  preserved into a catch-all; closed behavior requires explicit
  `additionalProperties: false`. Typed `additionalProperties` is
  supported in every position — including alongside `properties` — via a
  named catch-all field (`AdditionalProperties` in Go,
  `additionalProperties` in Java), which sidesteps the TS index-signature
  conformance problem. All four languages wrap the catch-all in a
  dedicated named member, even for pure maps: Go `AdditionalProperties`,
  Java `additionalProperties`, TS `additionalProperties: Record<string,T>`,
  Python `BaseModel` + `model_extra`. A declared member named
  `additionalProperties` collides → reject (Go/Java/TS; Python exempt via
  `model_extra`).
- **Integer cap = ±(2^53−1)** (`Number.MAX_SAFE_INTEGER`), uniform
  across all four languages (see [[type]]). TS `Number.isSafeInteger` enforces it
  soundly with no third-party parser.
- **Nullability via `oneOf: [{T}, {null}]`** — the degenerate two-branch
  case of the general `oneOf` union rule (see [[nullability]], [[oneOf]]).
- **Required + nullable supported** (P8) — presence and
  null-acceptance are independent axes; all four states are legal,
  including required+nullable ("must be present, may be `null`").
  Required+nullable is decidable, enforceable (presence-check on,
  null-rejection off), and round-trips losslessly in all four languages.
  The only residual absent-vs-`null` collapse is optional+nullable in
  Java/Go/Python (conservative-omit); TS round-trips all states
  faithfully.
- **Optional-non-nullable strictly rejects explicit `null`** —
  per-language enforcement strategies in `nullability.md`.
- **Integer parsing honors spec** — accept `1.0`/`1e2` as integers;
  reject `1.5`. Per-language runtime helpers (`parseSpecInteger`,
  `_parse_spec_integer`, Java node helper `SpecNumbers.specLong`).
- **Java reference types carry JSpecify nullness annotations**
  (PRINCIPLES Java §3). Emitted packages are `@NullMarked`; optional
  reference fields are `@Nullable`, required ones non-null by default.
  CLASS retention → no runtime dependency (P4 intact). JSpecify chosen
  over JSR-305 (abandoned + JPMS split-package).
- **Java baseline = Java 8; POJOs, not records** (PRINCIPLES Java §1).
  Records require Java 16+ and would impose a stricter floor than the
  Temporal Java SDK (Java 8+) — emitted code must never be more
  restrictive than the SDK it plugs into (P3/P4).
- **Go-specific:** `int64`/`float64` numeric primitives; optional via
  `*T`; `new(expr)` for pointer-from-literal (Go 1.26+ preferred, not
  required); custom `UnmarshalJSON` on every struct with `*json.RawMessage`
  shadow, collecting `Violation`s into a single aggregating
  `ValidationError` (a struct over `[]Violation` implementing `error`, not
  `errors.Join`).
- **Java error aggregation is a per-POJO collecting (de)serializer
  (Java §4–§6).** Each emitted POJO carries class-level
  `@JsonDeserialize(using=<Pojo>.Deserializer.class)` +
  `@JsonSerialize(using=<Pojo>.Serializer.class)`. The (de)serializers
  are emitted as `public static final` nested classes on the model
  (`User.Deserializer` / `User.Serializer`) — each model owns its pair,
  names never collide across models (same nesting idiom as P15's
  [[enum]] value classes). The deserializer does a two-stage
  lenient-tree-then-validate bind (`readValueAsTree()` defeats Jackson's
  fail-fast `MismatchedInputException`, then every field runs through
  shared spec-strict + constraint helpers, collecting
  `Violation{path,reason}`) and throws one `ValidationException extends
  JsonMappingException`. The spec-strict integer parse is a node helper
  (`SpecNumbers.specLong(JsonNode,…)`) called by the collecting
  deserializer; the explicit-`null` decision is a per-field branch over
  `node.isNull()`. This works through the default Temporal data converter
  (which owns a stock `new ObjectMapper()` we can't configure): the hook
  is baked into the POJO via annotations, and the aggregated
  `ValidationException` surfaces as the cause of `DataConverterException`
  (handler walks the chain → `getViolations()` → one BAD_REQUEST).
  Serialize side (§6) rides the same primitive. Closed-struct extra-key
  aggregation falls out of the tree stage, closing the additionalProperties
  Java question.
- **NOTE — aggregated error does not yet surface to the caller.** The
  current Temporal SDKs offer no hook to aggregate per-field validation
  errors and map them onto the `BAD_REQUEST` `HandlerError` returned to
  the Nexus caller (the path P11 assumes — handler walks the cause chain,
  pulls `getViolations()`, emits one `HandlerError`). All the generated
  (de)serializers still *collect* every violation into the per-language
  aggregated primitive (the Go/TS `ValidationError` over `[]Violation` /
  `pydantic.ValidationError` / `ValidationException`), but for now that
  aggregate stays internal: it
  fails the (de)serialize boundary without being projected onto the
  wire-level error the caller receives. Surfacing it requires an SDK
  change; tracked in `TODO.md`.

### Methodology established

- **Empirical verification.** Pydantic v2, Jackson 2.18, and
  `JSON.parse` + `Number.isInteger` all have non-trivial behaviors
  that diverge from what their docs imply. Verify with throwaway
  probes before committing the spec text.
- **Probes at `json-schema/research/`** — re-runnable at any time, candidate
  to promote into a conformance suite (see `features/type.md` OQ2).
- **Decisions cite principles by P-number.** Every Support decision
  in a feature spec must reference the P-number(s) it's grounded in.
- **Key empirical findings for future work:**
  - Pydantic's `_FIELD: ClassVar[T]` is required — bare `_FIELD`
    becomes a private model attr.
  - Pydantic's `model_validator(mode='before')` that raises
    short-circuits Pydantic's own field validation; use `mode='wrap'`
    for P11 aggregation across both error sources.
  - A key injected into the input dict by a `model_validator(mode='before')`
    lands in `model_fields_set` — Pydantic treats it as provided. We
    deliberately do **not** use this to auto-fill `const`: a required+const
    is a genuinely required field, so an absent value is a required
    violation (not healed), and the consumer-set value emits through the
    generic omit-unset serializer with no special keep-set.
  - Jackson's default `Long` deserializer silently truncates `1.5` to `1`.
    Custom deserializer is mandatory.
  - Jackson is fail-fast: the first field's `MismatchedInputException`
    aborts the whole bind, so per-field `@JsonDeserialize` cannot aggregate
    (P11). The class-level collecting deserializer (tree-first) is the only
    approach that works through the default converter (Java §5). A
    mapper-level `DeserializationProblemHandler` is out: we can't reach
    the default converter's mapper, and it misses 4 of 6 P11 cases anyway.
    Jackson 3.1's built-in `CollectingProblemHandler` is also out: it
    floors at Jackson 3.1 (SDK default is 2.x; P3/P4), is
    reader-configured + per-call so it never fires under the default
    converter, and collects only structural problems — not P10 constraints.
  - A `JsonMappingException` subclass thrown from a custom
    `JsonDeserializer` propagates verbatim through the Temporal
    `DefaultDataConverter` as the cause of `DataConverterException`,
    carrying its `List<Violation>` intact via `getCause()`.
  - The default Temporal Java converter owns a stock `new ObjectMapper()`
    we cannot configure — so all (de)serialize behavior must be baked into
    the POJO via class-level `@JsonDeserialize`/`@JsonSerialize`, exactly
    as Python's `to_json` case requires `@model_serializer(mode='wrap')`.
  - Pydantic's `model_fields_set` includes extras and excludes
    default-filled fields — the exact wire-key count for min/maxProperties.
    Summing `model_fields_set` + `__pydantic_extra__` double-counts extras.
  - `JSON.stringify` silently coerces `NaN`/`±Infinity` to `null` — the
    TS serializer must reject non-finite numbers before stringifying.

## Remaining work

### High priority

All former high-priority blockers are resolved. Completed:
- ±(2^53−1) integer cap (see [[type]]); all four PRINCIPLES.md language sections;
  open-by-default typed structs; Java error-aggregation primitive; `$ref`
  spec (`features/ref.md` + `generated-file-layout.md`).

### Feature specs to write (≈50)

Roughly in priority order — start with keywords that gate other
decisions:

**Object structure:**
- ✅ `properties`, ✅ `additionalProperties` (open/closed landed)
- ✅ `required`, ✅ `minProperties`, ✅ `maxProperties`,
  ✅ `dependentRequired`, ✅ `patternProperties` (temporarily unsupported),
  ✅ `propertyNames` (partial)
- Remaining: `unevaluatedProperties` (expect P6-reject),
  `dependentSchemas` (expect P6-reject)

**Any-type assertions:**
- `enum` — remaining. Representation pre-decided, shared with `const`
  (enum = the multi-value case): a **closed** value set that rejects any
  unrecognized value — TS a union of literals `'a' | 'b'`, Python
  `Literal['a', 'b']`, Go a defined type + one typed value const per
  member, Java a generated value class (static known constants + private
  ctor + `@JsonValue`; a `@JsonCreator fromString` that throws only on the
  standalone/interop path, while the collecting deserializer uses a
  non-throwing membership lookup so misses aggregate). Member constants are
  named from each value via the [[properties]] Stage 1–4 encoding (float
  `.`→`_`, negatives `Neg`, Java `V_` prefix when needed). Collision
  handling pre-settled (P15): two enum values whose encodings collide →
  load reject; the value-class/type name is package-scoped; the class-body
  collision pass is the [[properties]] policy applied per value class.
- ✅ `const` — landed (scalar, all primitive kinds). Pure assertion,
  validated both directions; presence owned by `required`. Closed-value
  emit per target (P13.1) — TS/Python literals, Go defined type + typed
  const, Java value class. Mutually exclusive with `default`/`enum`;
  composite consts deferred.

**Numeric assertions** (gated by integer-cap decision):
- ✅ `maximum`, ✅ `minimum`, ✅ `exclusiveMaximum`,
  ✅ `exclusiveMinimum`, ✅ `multipleOf` — landed. See "Features" below.

**Array structure:**
- ✅ `items` (homogeneous list), ✅ `minItems`, ✅ `maxItems`,
  ✅ `uniqueItems` (scalar elements), ✅ `prefixItems` (P6-reject landed),
  ✅ `contains` (scalar matcher, ≥ 1 existential),
  ✅ `minContains`, ✅ `maxContains` (count-of-matches bounds)
- Remaining: `unevaluatedItems` (expect P6-reject)

**String assertions:**
- ✅ `maxLength`, ✅ `minLength`, ✅ `pattern` — landed. See "Features" below.

**Applicators:**
- ✅ `anyOf` — spec'd (`features/anyOf.md`). P6-reject: inclusive-or
  with overlapping branches and no decidable selector; fix-it points at a
  discriminated `oneOf` (exclusive) or an `allOf` (combine).
- ✅ `if-then-else` — spec'd (`features/if-then-else.md`). P6-reject:
  runtime conditional shape (the `dependentSchemas` generalization); fix-it
  points at `oneOf` / `dependentRequired` / unconditional
  `properties`+`required`. A stray `then`/`else` without `if` rejects as a
  no-op.
- ✅ `not` — spec'd (`features/not.md`). P6-reject: negation names an
  open complement (no positive type/member/shape); fix-it points at a
  positive `type`/constraint, an `enum`/`const` closed value set, or the
  complementary `exclusiveMinimum`/`exclusiveMaximum` bound. `not:{}`/`true`
  rejects as unsatisfiable; `not:false` as a no-op.
- ✅ `allOf` — spec'd (`features/allOf.md`). Admitted as a **load-time
  merge/flatten**, not a retained combinator: branches fold into a single
  materialized schema that the ordinary keyword loaders then lower (no
  `allOf` residue, no new emitted type). Same-axis numeric bounds from
  different branches **tighten** (`allOf:[{maximum:10},{exclusiveMaximum:8}]`
  → `exclusiveMaximum:8`; two `maximum`s → the smaller), `multipleOf` →
  LCM, value sets intersect, object/array subschemas merge recursively;
  satisfiability/shape/collision checks are delegated to the owning specs
  on the merged result. Unmergeable branches reject loudly (P7.1): disjoint
  `type`, disagreeing `const`, empty `enum` intersection, distinct
  `pattern`/`format`/`contains`, a `false`/combinator branch. `$ref`
  branches fold in (flatten, not subtype — the base-extension idiom);
  `$ref`-with-siblings is the implicit-`allOf` sugar, now **merged**
  (supersedes the old [[ref]] sibling-reject). Closed-object merge fixes
  the raw-allOf `additionalProperties:false` footgun by closing against the
  union of declared properties.
- ✅ `oneOf` — spec'd (`features/oneOf.md`). Selector-separable unions
  supported, emitted as a closed sum type (Go sealed interface, TS/Python
  native union, Java 8 by-convention interface): (a) disjoint JSON kinds
  separate by the wire token (mixed kinds + nullable unions included), and
  (b) two+ object branches separate by a shared required `const`-tag
  (discriminated/tagged union — TS discriminant literal, Pydantic
  `Field(discriminator=)`, Go/Java discriminator peek). The nullability
  `[{T},{null}]` pattern is the degenerate case, still owned by
  [[nullability]]. Deferred: only the OpenAPI `discriminator` object
  (optional sugar over the const tag). Rejected outright: `integer`+
  `number` overlap (unsatisfiable).

**Core / structural:**
- ✅ `$ref`, ✅ `$defs` — landed (`features/ref.md` +
  `generated-file-layout.md`). Named-targets-only, local-file-only, no
  siblings, no `$id`; nested package tree per language (Go flattens); cyclic types hoist
  (not merge; P14); unsatisfiable-cycle reject.
- ✅ `$comment` — landed (`features/comment.md`). Known core
  keyword whose spec-mandated behavior is "ignore": accepted and silently
  dropped (non-string → reject), never surfaced as a doc comment (the line
  that separates it from [[description]]).
- Folded into their owning docs (no standalone spec, no restatement):
  `$schema` (dialect, [[input-files]]), `$id` (reject, [[ref]] + restated
  in [[input-files]]), `$anchor` / `$dynamicRef` / `$dynamicAnchor`
  (reference-mechanism rejects, promoted to a named subsection in [[ref]]),
  `$vocabulary` (meta-schema-only reject, [[input-files]]).

**Metadata / annotations:**
- `format` — high priority, codegen-relevant (e.g. `date-time` →
  `time.Time` in Go).
- ✅ `default` — landed. Off-the-wire semantics (annotation, set-ness tracking,
  omit-unset, materialize-on-read). Native in Python/Java, advisory in
  TS, `<Field>OrDefault()` accessor in Go.
- `title`, `description`, `examples`, `deprecated`,
  `readOnly`, `writeOnly`, `contentEncoding`, `contentMediaType`,
  `contentSchema` — lower priority; mostly pure metadata.

## Open question inventory

### `features/type.md`
1. **Cross-language conformance suite** for integer runtime helpers.

### `features/properties.md`
1. **Python serialize keep-set name↔alias mapping** — the
   `@model_serializer` keep-set (PRINCIPLES Python §6) filters Python
   field names against serialized keys; an `x-py-name`/case-mapped JSON
   alias means the keep-set must map name↔alias.

### `features/patternProperties.md`
1. Possible future single-pattern typed-map carve-out (deferred).

### `features/propertyNames.md`
1. Static enforcement of `propertyNames` alongside `properties`
   (currently rejected; deferred).

### `features/const.md`
1. Composite (object/array) const — temporarily unsupported; would need
   a deep structural-equality check. Deferred.
2. Validating the const value against constraint keywords (`pattern`,
   `minLength`, `minimum`, `multipleOf`, …) at load time — deferred to
   land with those constraint features.

### `input-files.md`
1. **Widening the `nexusrpc` version range** — v1 pins `"1.0.0"`; the
   reject path is forward-compatible but the acceptance policy for
   `>1.0.0` is deferred (P13.2).

### `features/multipleOf.md`
1. **Fractional-divisor carve-out** — `multipleOf: 0.1`/`2.5` is rejected
   (deferred). A future decimal-scaling lowering could support
   fixed-precision fractional divisors if all four targets agree; revisit
   on demand.

### `features/maximum.md` (+ minimum / exclusive pair)
- Zero open questions. (Flooring a fractional bound on an integer field —
  rather than rejecting — was considered and rejected: Pydantic can't
  represent it and silent flooring violates "reject ambiguity loudly".)

### `features/maxLength.md` (+ minLength)
- Zero open questions. (The Pydantic length-unit question is **resolved**:
  verified to count code points — pydantic 2.13.4,
  `research/string_probe/pydantic_length_probe.py`.)

### `features/pattern.md`
1. **Widen the accepted subset** — v1 still rejects backtracking constructs,
   inline flag groups, and `\S` in a multi-member class; each could later be
   admitted via a semantics-preserving rewrite (`(?i)`→case-fold, etc.),
   gated on the conformance corpus agreeing.

Resolved (were OQ2/OQ3, plus the follow-on .NET/Ruby + `\s` normalization):
- **Conformance corpus built** (`research/pattern_conformance/`, 83 pairs
  through the Rust gate + all runtimes). It proved compile-gate + pinned
  flags **insufficient** and drove: **reject** inline flags (JS can't
  compile) + the narrow `\S`-in-multi-member-class case; **normalize**
  `\s`/`\S`→explicit ASCII class `[\t\n\x0B\f\r ]` (`research/ws_normalize/`:
  13 divergences → 0, all placements incl. `[^\S]`) and `$`→`\Z`(Py)/`\z`
  (Java, keep `$` Go/JS). All via a `regex-syntax` AST walk. Corpus stays as
  the regression guard (feeds the [[type]] conformance suite).
- **Pydantic native `pattern` rejected** — `research/pydantic_pattern_probe.py`
  (pydantic 2.13.4): native `pattern=` uses pydantic-core's Rust engine
  whose `\d\w\s` are Unicode (4/32 corpus disagreements vs our ASCII);
  anchoring/dot do match. Keep the explicit `re`+`re.ASCII`+`search`
  `AfterValidator`.
- **.NET + Ruby verified future-conformant** (`pattern_conformance/dotnet_runner/`,
  `runner.rb`), per-target emitter transforms only, no new gate rules: .NET
  = `RegexOptions.ECMAScript` + `$`→`\z` + astral-`.`→surrogate rewrite (no
  `u`-flag equivalent, the sole divergence); Ruby = `^`→`\A`/`$`→`\z` (line
  anchors) + inject `(?a)` (Unicode `\b`). Captured in the spec's
  "Prospective targets" note.

### `services.md`
1. **Explicit-vs-default wire name (Python/Java)** — generator always
   emits `name=` for P1 clarity; could omit when `fqn` equals the SDK
   default. Deferred.
2. **Async/handler-shape metadata** — generated binding is a typed
   contract only; sync/async, headers, links are implementation-time and
   not surfaced. Flagged for future I/O cardinality/streaming concepts.

### `features/default.md`
1. Composite (object/array) defaults — deferred, expected to relax. v1
   is scalar-only; lifting needs a spec for materializing a literal
   object/array default into a constructed language value on read and
   folding it into the omit-unset machinery. Tracks with [[const]]'s
   composite-const carve-out (same materialization problem).
2. Validating the default value against constraint keywords at load time
   — deferred to land with those constraint features.

### `features/ref.md`
1. **Pointer into a non-`$defs` subschema** — currently rejected (must
   extract to `$defs`); could relax via anonymous-name-synthesis.
   Deferred pending demand.

### Cross-cutting
1. **Literal-value-against-constraint validation at load time** (now
   closed for scalar constraints). A `const`, `default`, or `enum` value
   must satisfy every sibling assertion on the same node.
   `type`-compatibility, the **numeric constraints**
   (`minimum`/`maximum`/`exclusive*`/`multipleOf`), and now the **string
   assertions** (`minLength`/`maxLength`/`pattern`) are all enforced at
   load — each constraint spec requires the supplied literal to satisfy it.
   Nothing scalar remains deferred here; array/object literal checks will
   land with the array/object constraint specs (and with composite
   const/default, currently deferred).

## How to pick up the work in a new session

1. Read `PRINCIPLES.md` and this `PLAN.md`.
2. Read `features/type.md` as the worked-example template
   and `nullability.md` for cross-cutting conventions.
3. Pick a feature from the priority list above.
4. Use `WebFetch` to grab the JSON Schema 2020-12 spec text for that
   keyword (links at top). **Don't trust the doc-fetcher's summary**
   — it has truncated/misreported tables on several keywords. Quote
   verbatim from the spec proper.
5. Draft the spec.md against the template.
6. For any non-trivial language behavior (Pydantic, Jackson, JS
   parsing), write a quick probe in `/tmp/` and verify empirically
   before committing prose.
7. Cite PRINCIPLES.md P-numbers in every Support decision.
8. If a decision needs human input, surface it explicitly — don't
   guess and don't quietly defer.
9. Update this `PLAN.md` open-question inventory and "work completed"
   sections after the spec lands.

## Files of record

- `json-schema/PRINCIPLES.md` — decisions
- `json-schema/PLAN.md` — this file (state + next steps)
- `json-schema/features/<keyword>.md` — per-feature design (one
  directory per JSON Schema keyword only)
- `json-schema/*.md` cross-cutting design notes (not keywords):
  `input-files.md`, `generated-file-layout.md`, `nullability.md`,
  `services.md`, `pipeline.md`
