# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Deprecated

### Breaking Changes

- Renamed the project to `nexgen`. The crate is published as `nexgen` (was
  `nex-gen`), generated .NET code uses the `Nexgen.*` namespaces and the
  `NexgenClient`/`RequireNexgenClient` members (were `NexGen.*` and
  `NexGenClient`/`RequireNexGenClient`), the TypeScript definitions namespace is
  `__nexgenDefinitions` (was `__nexGenDefinitions`), and the samples honor
  `NEXGEN_BIN` (was `NEX_GEN_BIN`). Generated-file headers and the
  `[GeneratedCode]` attribute now read `nexgen`.

### Fixed

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
  union under `[]` — "a T or an array of nulls"), Python dropped the *field's*
  own `| None` because the element annotation already contained one, and Java
  rejected a null element outright. All four now follow `items.md`: `[]*T`,
  `(T | null)[]`, `list[T | None]`, `List<@Nullable T>`.
- TypeScript: An array of models or unions serialized its elements verbatim, so
  an element's in-memory `additionalProperties` bag reached the wire as a literal
  member (and an element's temporal/bytes members were never re-encoded). Each
  element now re-serializes through its own mapper, as does a typed map's member.
- Go: A schema `description` ending a sentence with a package-like word ("one at
  a time.") added that package to the import block, and an unused import is a Go
  compile error. Package use is now read off the emitted code, not the doc
  comments.
- JSON Schema: A `oneOf` with an inline object branch generated uncompilable Go
  (a marker method on an undeclared `<Union>Object` type) and uncompilable
  TypeScript (`new Record<string, unknown>Mapper()`); Java bound the branch to
  `null` without a violation.
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
- JSON Schema: A free-form object *definition* generated an empty Go struct that
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
  The union now serializes through the branch's mapper.
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
