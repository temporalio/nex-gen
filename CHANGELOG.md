# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- WIT signal-with-start request models now carry Temporal headers while keeping
  them out of generated convenience operation APIs.
- .NET proto-backed models now use Temporal SDK transfer-type converters. The
  generated output requires `Temporalio` 1.18.0 or newer; generic proto-backed
  .NET models report an explicit unsupported-conversion error until the SDK
  supports generic converter registration.
- Python JSON Schema packages now export `ValidationError` and `Violation`
  from their root `__init__.py`; callers no longer need to import the private
  `_definitions` module.
- Added grouped protobuf `oneof` authoring and bidirectional Python conversion,
  including required and optional oneofs, scaffolding through `add-rpc` and
  `add-message`, and explicit diagnostics for unsupported target backends.
- Python proto-backed generic records may use Temporal `Payload` and `Payloads`
  fields (including oneof members) as runtime value carriers. Decoding preserves
  concrete runtime type arguments through nested models and `Payload` values.
- JSON Schema: An object `oneOf` branch may now be written **inline**, whatever
  its shape. A structured branch (declared `properties`, or a typed
  `additionalProperties`) is named — `<Union>Object` for a lone branch, or the
  branch's own `x-<lang>-name` — and emitted as the same named model an authored
  `$defs` definition of that shape produces, so it validates, exports, and
  round-trips identically in all four languages. Two or more inline object
  branches must each carry the target's `x-<lang>-name`: every branch would
  otherwise derive the same name, and a discriminator `const` is a wire value,
  not an identifier.
- JSON Schema: A `oneOf` sum type may now be written in an **element
  position** — an array's `items` (at any depth) or an object's typed
  `additionalProperties`. It is named after its position (`<Enclosing>Item` /
  `<Enclosing>Value`, or its own `x-<lang>-name`), moved into `$defs`, and the
  position rewritten to a `$ref`, so the element type is an ordinary named union
  in all four languages — including any inline object branch it carries, which is
  named in turn. Previously such a union was silently collapsed to one branch
  (Go, Java) or distributed over the array (TypeScript).
- JSON Schema: The free-form object (`type: object` with
  `additionalProperties: true` and no `properties`) is now supported as a `oneOf`
  branch. Its members are carried verbatim in every language: Go and Java
  synthesize the wrapper the union needs (`<Union>Object`), TypeScript and Python
  inline it as `Record<string, unknown>` / `dict[str, Any]`.

### Changed

- JSON Schema conformance is now enforced end to end: the loader rejects unknown
  schema and Nexus-envelope members (including `endpoint` and OpenAPI
  `discriminator`), discovers transitive local `$ref` files and nested RFC 6901
  `$defs` pointers, and normalizes `allOf` annotations and constraints before
  generation. Generated Go, TypeScript, Python, and Java APIs now keep mixed
  declared/typed catch-all objects fully typed, apply complete `contains` and
  `propertyNames` scalar matchers in both directions, enforce wire-string
  constraints around temporal/byte materialization, keep closed/default values
  native, and mark deprecated types, fields, services, and operations
  idiomatically. Generated documentation also preserves paragraphs, wraps at 88
  columns, and escapes target comment syntax. These changes add no CLI flags and
  do not change the JSON wire format.

- JSON Schema numbers round-trip by mathematical JSON value rather than token
  spelling: whitespace, object-member order, and spellings such as `5`, `5.0`,
  and `5e0` are not identity-bearing. Generated Java keeps idiomatic `double`
  serialization rather than applying a Java-only spelling normalization.

- Generated source headers now include the version of the `nexgen` binary that
  produced them.
- Protobuf-backed models now consistently generate conversions in both
  directions whenever they are reachable. Go and TypeScript emit previously
  suppressed complementary helpers, operation-free exported models receive the
  same validation as operation-used models, and Java now reports its existing
  lack of protobuf model support instead of silently dropping protobuf
  operation types.
- JSON Schema: An `x-<lang>-name` alongside a `$ref` is no longer merged as an
  implicit-`allOf` conjunct, which cloned the referenced target into the use site.
  It names the _member_ the reference is bound to and leaves the reference intact
  — the one sibling keyword treated this way, because it asserts nothing about the
  value, and the only way to rename a member whose type is a `$ref` (a member
  named `class` was otherwise unfixable in Python and Java).
- TypeScript: JSON Schema models now export `TransferTypeConverter` instances
  (`fromTransferType`/`toTransferType`) instead of mapper classes, and generated
  operations reference them through `inputType`/`outputType`. Converter names
  follow resolved model names, participate in collision checks, and require the
  nexus-rpc type-info API.
- Generating into an existing `--output` directory no longer deletes it first.
  The directory is written into instead, so pre-existing files and
  subdirectories are preserved; generated files are still overwritten in place.
  A file left over from an earlier run whose definition has since been renamed
  or removed now stays behind until it is deleted. The `build-examples` and
  `build-json-examples` maintenance commands, which own the directories they
  write, delete each example's output directory before regenerating it, so the
  checked-in samples stay free of stale files.
- Java: A `oneOf` union now carries its `JsonNode` dispatcher as a static
  `fromNode` on the union interface in **both** positions — a named `$defs` union
  and a union written inline on a property (whose interface is nested in the
  declaring class). The enclosing deserializer reads the member with one
  delegating call instead of an inlined token chain, and a union member
  serializes by runtime class: object branches through their POJO's serializer,
  scalar/array wrappers through Jackson's `@JsonValue` on `getValue()`.
- JSON Schema: A **materializing** keyword on a non-object branch of a `oneOf`
  sum type — a temporal `format` (`date-time`/`date`/`time`/`duration`) or a
  `contentEncoding` — is now **rejected at load** with a located diagnostic. The
  synthesized `<Union><Kind>` wrapper has no native construct to hold, so the
  branch materialized in Python while Go, TypeScript, and Java carried an
  unvalidated `string`. The remedy is a plain `string` branch (still fully
  validated) or an object branch carrying the value as a property, where
  materialization already works. Asserted string formats (`uuid`, `email`,
  `hostname`, `uri`, `ipv4`, `ipv6`) are unaffected, and the nullability
  `oneOf:[{T},{null}]` is not a sum type, so a materialized nullable field keeps
  working.

### Deprecated

### Breaking Changes

- JSON Schema (Java): a `const`/`enum` value constant is now named after the
  **value** rather than the declaring member — `Circle.Kind.CIRCLE` instead of
  `Circle.Kind.KIND`, `Status.PENDING` instead of `Status.STATUS_PENDING`, and
  `Tier.V_1` instead of `Tier.TIER_1` (numeric tokens take the `V_` guard, since
  a Java constant is class-scoped and has no type prefix to supply a leading
  letter). The value class already carries the member in its own name, and a
  single-valued `const` previously dropped the value from the constant
  altogether. The change is also what makes P15's escape hatches match their
  fix-its: a member-derived constant moved under `x-java-name` as well as
  `x-java-const-name`, so a value-constant collision had two remedies while the
  diagnostic named one — which now points at `x-java-const-name` /
  `x-java-enum-names` for the constant scope. Go is unaffected (it already named
  constants `{Type}{Value}`), as are constants given an explicit
  `x-java-const-name` / `x-java-enum-names` override.
- JSON Schema (Python): a `type: number` value is now narrowed to binary64 in
  both directions instead of being stored exactly as it arrived. Python is the
  one target whose `int` is unbounded, so a `number` past 2^53 kept its exact
  value there while Go, TypeScript and Java rounded it into their
  `float64`/`number`/`double` — the same payload reading back as a *different*
  number. A `number` member now always holds the `float` its annotation
  promises, and the re-emitted lexeme follows (`5` → `5.0`), which is the same
  JSON value: a number's spelling is not part of JSON identity. `type: integer`
  members are unaffected and stay a plain `int`.

- JSON Schema (Go, Java): a numeric value constant's name now derives from the
  value rather than its authored spelling, so `const: 1`, `const: 1.0` and
  `const: 1e0` all name one constant (`Score1` / `V_1`) instead of `Score1_0` /
  `V_1_0` for the fractional spellings. P1 makes those one mathematical number,
  so re-spelling a `const` is a no-op on the wire and must not rename a public
  constant (P13).

- JSON Schema: the `pattern` portability gate now verifies that a pattern is
  portable across all four target regex engines, not merely that Rust's `regex`
  can compile it. Patterns that previously loaded and then failed — or silently
  matched differently — in a target now reject at load with a named fix-it:
  non-portable escapes (`\-`, `\_`, `\a`, `\v`, octal, `\x{…}`, `\p{…}`),
  lone `{`/`}`/`]`, `\A`/`\z`, named capture groups, POSIX classes, nested
  character classes, and the class set operators `&&`/`--`/`~~`. An ordinary
  `^\d{3}\-\d{4}$` previously emitted TypeScript that threw `SyntaxError` at
  module import. A bare `.` is now normalized to `[^\n]` so the four engines
  agree on `\r`, U+0085, U+2028 and U+2029.
- JSON Schema: patterns whose unbounded quantifiers can backtrack exponentially
  (for example `^(a+)+$`) now reject at load. Such a pattern is linear in Go and
  Rust but was measured at 39 s for a 31-character input in Python, so an
  accepted schema was a remote denial of service in three of four targets.
- JSON Schema: `contentEncoding` now accepts only the canonical encoding. A
  value with non-canonical trailing bits (`aGl=`, `AB==`, base64url `aGl`)
  previously decoded and then re-serialized to a *different* wire string,
  contradicting the documented byte-identical round trip.
- JSON Schema: a materializing temporal `format` inside `propertyNames` or a
  `contains` matcher now rejects at load. It previously emitted an unenforced
  key check (and, in Go and Python, output that did not compile or import).
- JSON Schema: these now reject at load rather than producing silently wrong or
  uncompilable output — a `default` on a sum-type `oneOf`; an empty `fqn` on a
  service or operation; a service or operation key that is a reserved word in an
  emitted target; two operation keys that recase to one identifier (previously
  duplicate members *and* a duplicate wire name); an input file whose module path
  segment is a target keyword (`class.json` emitted `from .class import Class`);
  a member colliding with a generated Go method, Java nested class, Java
  generated local, or Java `get<Field>OrDefault`; `enum` members equal by
  mathematical value (`[1, 1.0]`, `[0, -0.0]`); a `oneOf` branch that is a
  shapeless `object`/`array`; a `propertyNames` subschema carrying object or
  array applicators; a `number` field whose `multipleOf` admits no value in its
  range; and a non-string `title`/`description` on a nested schema or an
  envelope service/operation description, which previously coerced silently.

- Python: JSON Schema output now uses slotted, keyword-only dataclasses instead
  of Pydantic and works with the default Temporal converter, removing the
  Pydantic dependency and contrib converter wiring. Generated transfer converters
  preserve wire names, carry unknown fields in `additional_properties` instead of
  `model_extra`, aggregate structured validation errors, collapse absent and
  explicit-null optional-and-nullable values to `None`, and surface schema
  defaults through mutable properties whose deleter restores unset state.
- Java: A map-shaped model (a pure typed map — `additionalProperties` with no
  declared `properties`) now names its catch-all member `additionalProperties`,
  matching the struct-shaped POJOs and the other languages (Go
  `AdditionalProperties`, TypeScript `additionalProperties`) as
  `additionalProperties.md` specifies. The generated accessor is
  `getAdditionalProperties()` (was `getValues()`); the constructor keeps its
  single positional map parameter, so only getter call sites need updating. The
  wire form is unchanged.
- Renamed the project to `nexgen`. The crate is published as `nexgen` (was
  `nex-gen`), generated .NET code uses the `Nexgen.*` namespaces and the
  `NexgenClient`/`RequireNexgenClient` members (were `NexGen.*` and
  `NexGenClient`/`RequireNexGenClient`), the TypeScript definitions namespace is
  `__nexgenDefinitions` (was `__nexGenDefinitions`), and the samples honor
  `NEXGEN_BIN` (was `NEX_GEN_BIN`). Generated-file headers and the
  `[GeneratedCode]` attribute now read `nexgen`.

### Fixed

- JSON Schema: constraints declared on the non-null branch of a nullable
  `oneOf: [T, null]` are enforced again in Go and Java. Both read the wrapper
  rather than the branch, silently dropping `minLength`, `maxLength`, `pattern`,
  `format`, `enum`, `const`, numeric bounds and `contains` — so Java accepted
  payloads TypeScript and Python rejected, and Go emitted code that did not
  compile for any nullable non-string member. Element-level nullability
  (`items: {oneOf: [T, null]}`) is fixed in Java at every array depth.
- JSON Schema: Go no longer accepts a quoted numeric token. `encoding/json`
  decodes `"7"` into `json.Number` silently, so `{"n": "7"}` was accepted for
  `type: integer` and `type: number` where the other three targets rejected it —
  affecting every numeric member.
- JSON Schema: `multipleOf` on a `number` field is IEEE `fmod` in Go, matching
  the other three. Go previously used exact rational arithmetic over the
  shortest decimal spelling, so `1e23 % 5` was Go-accepted and others-rejected,
  and `1e300 % 3` the reverse.
- JSON Schema: discriminated unions dispatch on the JSON *value* in Go and Java.
  Go switched on the raw wire text, so `{"kind": 1.0}` missed a `const: 1` tag it
  matched elsewhere; Java gated dispatch on `isTextual()`, so an integer or
  boolean tag never selected a branch at all.
- JSON Schema: `uniqueItems` compares values, not representations. Java's
  serialize side treated `-0.0` and `0.0` as distinct; TypeScript and Java
  compared materialized elements (`Uint8Array`, `Temporal`, `byte[]`) by
  reference, so byte-equal duplicates serialized cleanly and were then rejected
  on read; Go emitted uncompilable code over any materialized element. All four
  now key on the canonical wire value.
- JSON Schema: the serialize-side ±(2^53−1) integer cap is enforced in all four
  targets. It existed only in Go, so TypeScript, Python and Java emitted an
  over-cap integer that every parser — including their own — then rejected.
- JSON Schema: a fractional second wider than a target's resolution is accepted
  and truncated in every target, rather than rejected by some. Java previously
  rejected 10-or-more digits (raising a bare `DateTimeParseException` for
  `time`), and the TypeScript `temporal` representation raised a bare
  `RangeError`; both now truncate to nanoseconds and aggregate a `Violation`.
- JSON Schema: `contains` matcher semantics agree across targets. Python derived
  the element type guard from the first `const`/`enum` literal, so a mixed-kind
  matcher's verdict depended on member order; Go omitted the integer cap;
  TypeScript emitted no guard for a typeless matcher, throwing a bare `TypeError`
  instead of aggregating; and a fractional bound over integer elements was
  truncated inconsistently, with Java disagreeing with itself across serialize
  and deserialize.
- JSON Schema: `const`/`enum` on a materialized `format` or `contentEncoding`
  member compare the canonical wire string in all four targets. Go compared
  decoded bytes and native temporal values, so it accepted values the others
  rejected, and a model parsed from a non-canonical literal could not be
  serialized at all elsewhere. Temporal literals are canonicalized at load.
- JSON Schema: Go and Java validate temporal values before emitting. Neither had
  a serialize-side predicate for `duration`, `time` or offsets, so a negative
  `Duration` emitted the ill-formed `"PT-1H-30M"`, a sub-second duration
  silently became `"PT0S"`, a sub-minute offset rounded to `"+00:00"`, and a
  year past 9999 emitted a five-digit year.
- JSON Schema: a `.0`-valued count bound (`minItems: 2.0`, `maxLength: 10.0`,
  `minProperties: 1.0`, `minContains: 2.0`) now generates. The loader accepted it
  per spec and every emitter then aborted with an unlocated internal error.
- JSON Schema: generated output compiles and imports in cases that previously
  did not — TypeScript emitted `if () {` for a closed empty object and could not
  assign an optional `const` member; Python emitted an unterminated docstring for
  any documentation ending in `"`, a nested same-quote f-string that is a
  `SyntaxError` before 3.12, and a `NameError` on import for any package with a
  deprecated operation; Go emitted string checks against `[]byte`, empty loop
  bodies for collections of `time`/`duration`, and a pointer-to-interface for a
  cross-module union; Java emitted an unsatisfiable type constraint for a
  typed-map union branch.
- JSON Schema: cross-file `$ref` union branches resolve in Go, TypeScript and
  Python. All three searched only the declaring module, so a named cross-file
  union emitted an empty type and a property-level one collapsed to its first
  branch. Go also no longer drops a service binding when the module declares no
  models of its own.
- JSON Schema: the recursive `allOf` merge no longer discards a child-position
  `$ref` or `oneOf`. Two branches declaring the same property lost the
  referenced type's fields and required set, and a nullable property merged with
  a constrained sibling silently became non-nullable.
- JSON Schema: Java re-paths nested violations on serialize, gives `byte[]`
  value equality in `equals`/`hashCode`/`toString`, places member Javadoc on the
  getter, and round-trips temporal and binary collection elements under a stock
  `ObjectMapper` (they previously required the jsr310 module to be registered
  externally, and Jackson silently wrote a non-canonical base64 variant).
- JSON Schema: `x-<lang>-enum-names` now applies to numeric and boolean members
  in Go and Java; the lookup keyed on string members only, so the sole escape
  hatch for a numeric or boolean value-constant collision did not exist.

- Java now rejects numeric JSON tokens outside the finite binary64 domain (for
  example `1e400`) with an aggregated, fully pathed violation in ordinary
  properties, union branches, nested arrays, and typed-map members.
- JSON Schema `minItems`, `maxItems`, `uniqueItems`, and `contains` now inspect
  the original wire array in every target even when one or more elements fail
  `items`. Failed conversions no longer fabricate count or duplicate results;
  indexed violations precede sibling array-keyword violations at every depth.

- JSON Schema converters now apply scalar, reference, union, nested-array,
  temporal, and content-encoding handling recursively inside array elements and
  typed-map members. Go reports indexed/keyed violations instead of collapsing
  them to the collection, TypeScript no longer passes a `oneOf` array branch
  through verbatim, and required temporal/base64 runtime support is discovered
  at every nesting depth.
- JSON Schema `number` values now reject `NaN` and positive/negative infinity
  with aggregated, fully pathed validation errors before serialization in every
  target. Go also accepts every valid integer-valued JSON number spelling
  (`1`, `1.0`, `1e2`, `1.5e1`) while continuing to reject fractional and
  over-cap values.
- JSON Schema temporal dates and date-times now use `0001` as their shared
  minimum year. Year `0000` is rejected by schema-literal validation and by the
  generated Go, TypeScript, Python, and Java runtime predicates.
- JSON Schema: Cross-input emission and naming now follow each target's actual
  scope and `x-<lang>-name` overrides. Foreign types are imported rather than
  duplicated, empty TypeScript model modules are omitted, member-derived names
  stay aligned, and root/`$defs`/synthesized collisions fail at load time.
- TypeScript: String array elements now enforce their own constraints and report
  type errors at the indexed element path.
- Python: Closed-value checks now use tuple membership, array-element errors name
  the expected type, converter locals cannot be shadowed by properties, and all
  synthesized module names participate in collision checks. `_definitions` is
  reserved for the generated runtime module.
- JSON Schema: A **non-object `oneOf` branch's own constraints** were dropped in
  three of four languages: only Go carried them, in the synthesized
  `<Union><Kind>` variant's `Validate`. TypeScript cast the narrowed value
  through unchecked, Python emitted a bare `str | SpecInt` with no constraint
  metadata, and Java's wrapper classes held the value without validating it. A
  branch is now held to everything it declares — string lengths, `pattern`, an
  asserted `format`, numeric bounds and `multipleOf`, `minItems`/`maxItems`/
  `uniqueItems`/`contains`, a `const`/`enum` value set — in all four languages and
  both directions, under the union's own violation path. Go additionally dropped a
  branch's `pattern`/`format` while emitting an (empty) `Validate` for it.
- TypeScript: A `const`/`enum` on a non-object `oneOf` branch generated code that
  **did not compile** (`tsc` TS2322): the branch narrowed the member type to its
  literal set while the parse path assigned the wider primitive into it. The
  branch's member type and its narrowed assignment now agree.
- TypeScript: A nested violation carrying no path of its own — a union branch's
  own constraint, an element-level check — was reported with a dangling separator
  (`segments[0].`). The prefix is now the whole path, matching Go and Java.
- JSON Schema: `uniqueItems` and `contains` were dropped on an array-typed **typed
  map member** in Python. Both now run in the member's converter through the
  runtime's `_check_unique_items` / `_check_contains`, with the same reasons the
  property position emits (and the same mechanism now serves a `oneOf` branch).
- JSON Schema: A typed map's members were validated against their type *token*
  only, so every constraint the member type declared was silently dropped — a
  string's `minLength`/`maxLength`/`pattern`/`format`, a number's bounds and
  `multipleOf`, an array's `minItems`/`uniqueItems`/`contains`, a `const`/`enum`
  value set. Every member is now held to everything its type declares, in both
  directions, with the member's key as the violation path. Python additionally
  validated only that a member was a _string_, leaving an object, union, or
  numeric member unchecked and unmaterialized; members now validate and
  materialize through the member type's own converter, so
  `additional_properties` holds the declared type (an `Inner`, an `int` parsed
  from `1.0`, a `datetime`, `bytes`) and re-encodes through it on the way out.
  TypeScript checked members on the way in but not on the way out, and dropped
  a nullable value's constraints in both positions (a member's *and* a
  declared field's).
- JSON Schema: A **nullable** typed-map member (`additionalProperties` as the
  nullability `oneOf`) was mishandled: Go typed the member `T` and dropped a
  `null` member from the map entirely, and Java rejected it. A null member is now
  kept as a null member — Go `map[string]*T`, Java `Map<String, @Nullable T>` —
  matching TypeScript's `Record<string, T | null>` and Python's `T | None`.
- Java: A **nested array** (`items` inside `items`) and an **array-valued typed
  map member** both bound to the placeholder violation `"unsupported nested
array"` at runtime, though `items.md` accepts them. Both now decode elementwise,
  one loop per level, with each level's index in the violation path
  (`grid[1][0]`).
- Java: A materialized temporal `format` or `contentEncoding` in an array element
  or a typed map member emitted `var` for the parsed value, which does not compile
  at the Java 8 baseline the generated code targets.
- TypeScript: A nested array's element loop reused the enclosing loop's variable
  names, so it emitted `item!.push(item)` — which does not compile — and reported
  the inner index twice in the violation path. Each level now carries its own
  element, index, and item bindings.
- TypeScript: A `pattern` or `format` on anything but a declared property — a
  typed map's member, an array element, a key-shape subschema — emitted a check
  referencing an undeclared `PATTERN_…` const, throwing `ReferenceError` at
  validation time. Every string position's regex is now declared.
- JSON Schema: An object written **inline** in a value position — a property, an
  array element at any depth, a typed `additionalProperties` member — had its
  declared shape silently discarded. Go, TypeScript, and Python typed the member
  as an opaque map (`map[string]json.RawMessage` / `Record<string, unknown>` /
  `dict[str, Any]`) and Java as `String`, so declared properties and member
  constraints never reached the generated code; Go additionally never decoded the
  member at all, leaving the value neither typed nor preserved in the catch-all.
  Every inline object is now named after its position (`<Model><Property>`,
  `<Enclosing>Item`, `<Enclosing>Value`), moved into `$defs`, and the position
  rewritten to a `$ref`, so it emits as the ordinary named model the
  `$defs` + `$ref` form produces — materialized, validated, exported, and
  round-tripped identically in all four languages. Nullability no longer changes
  the name: the object inside a `oneOf: [{object}, {"type":"null"}]` wrapper takes
  the position's name too. This covers every object shape, including a typed map
  and the free-form object, whose member-count and key-shape constraints were
  dropped along with the rest.
- JSON Schema: A union-typed array element or map member decoded through the
  whole-collection path, which cannot allocate a sealed interface: Go failed at
  runtime on `[]Union` / `map[string]Union` (`json.Unmarshal` into an interface)
  and Java on `List<Union>` / `Map<String, Union>`
  (`readTreeAsValue(node, Union.class)` on an abstract type). Each element/member
  is now routed through the union's own dispatcher, with its index or key in the
  violation path (`shapes[1]`, `choices.primary`), and the serialize side re-runs
  each element's branch constraints (P12).
- JSON Schema: A **nullable** array element (`items: {oneOf: [{T}, {null}]}`) was
  mishandled in every language: Go typed it `[]T` and silently decoded a wire
  `null` to `T`'s zero value, TypeScript emitted `T | null[]` (an unparenthesized
  union under `[]` — "a T or an array of nulls"), Python dropped the _field's_
  own `| None` because the element annotation already contained one, and Java
  rejected a null element outright. All four now follow `items.md`: `[]*T`,
  `(T | null)[]`, `list[T | None]`, `List<@Nullable T>`.
- TypeScript: An array of models or unions serialized its elements verbatim, so
  an element's in-memory `additionalProperties` bag reached the wire as a literal
  member (and an element's temporal/bytes members were never re-encoded). Each
  element now re-serializes through its own converter, as does a typed map's
  member.
- Go: A schema `description` ending a sentence with a package-like word ("one at
  a time.") added that package to the import block, and an unused import is a Go
  compile error. Package use is now read off the emitted code, not the doc
  comments.
- JSON Schema: A `oneOf` with an inline object branch generated uncompilable Go
  (a marker method on an undeclared `<Union>Object` type) and uncompilable
  TypeScript (a converter named after the anonymous `Record<string, unknown>`
  branch type); Java bound the branch to `null` without a violation.
- Java: An object branch of a union written inline on a property was silently
  dropped — the branch's class implemented nothing, and the parse arm for the
  object token was empty. The branch class now implements the nested union
  interface (`implements <DeclaringClass>.<Union>`) and parses through it.
- Java: A named `oneOf` union def with a scalar, array, or free-form-object
  branch generated uncompilable code — its `fromNode` referenced wrapper classes
  (`<Union>String`, `<Union>Array`, `<Union>Object`) that were never declared.
  The wrappers are now declared inside the union interface.
- Java: An array branch of a `oneOf` union parsed to `null` without a violation;
  its items are now parsed and validated elementwise.
- JSON Schema: A free-form object _definition_ generated an empty Go struct that
  rejected every member as an unknown field, and an empty TypeScript interface
  that dropped every member.
- JSON Schema: A typed map whose members are not strings (for example
  `additionalProperties: {type: integer}`) generated uncompilable Go — a
  `map[string]int64` member decoded as `map[string]string`, with the member
  values never parsed.
- JSON Schema: `minProperties`/`maxProperties`/`propertyNames` on a free-form
  object were dropped in Go, TypeScript, and Python; they are now enforced in
  both directions (P12).
- JSON Schema: TypeScript serialized an object member of a property-level union
  by copying the in-memory value, so the model's `additionalProperties` member
  reached the wire as a literal key and its extras were never spread back out.
  The union now serializes through the branch's converter.
- JSON Schema: TypeScript's serializer for a mixed-kind union returned the lone
  object branch unconditionally, making the scalar/array branches unreachable;
  the object branch is now guarded by the object token, matching the parse side.

### Security

## [0.2.1] - 2026-07-31

### Added

- WIT: Added `@nexus.name` directive for customizing generated field names.

## [0.2.0] - 2026-07-28

### Added

- Added the `nexgen` CLI for generating Go, Java, Python, and TypeScript
  bindings from NexusRPC definition files. Types are modeled with JSON Schema
  2020-12: each type becomes a typed model backed by a single shared validator
  that runs on both sides of the wire, so a payload can never be parsed or
  serialized in a shape the contract forbids. Constraint failures aggregate into
  one native error naming every violation, which a Nexus handler maps straight to
  `BAD_REQUEST`. The supported subset is deliberately strict — anything that
  can't be lowered cleanly and identically into all four languages is rejected at
  generation time with a fix-it diagnostic. See the [README](README.md) for the
  supported JSON Schema features, naming overrides, and usage.

### Breaking Changes

- All advanced WIT/proto-oriented functionality now lives behind an `advanced`
  Cargo feature that is off by default, so the published binary offers only the
  documented JSON Schema workflow. This gates the `dotnet` generate target, the
  WIT/proto generate flags (`--support-file`, `--descriptors`, `--format`,
  `--native-api`), and the maintenance subcommands (`build-examples`,
  `build-json-examples`, `add-rpc`, `debug-wit-dir`). Build with
  `cargo build --features advanced` to restore the previous surface.
