# `items`

Source: JSON Schema 2020-12, Core (Applicator vocabulary),
§10.3.1.2 "Keywords for Applying Subschemas to Arrays → items".

Applies one subschema to every array element. The structural backbone of
every generated list type — the array analog of [[properties]]: it is
what turns `type:"array"` into a *typed* collection (`[]T` / `T[]` /
`list[T]` / `List<T>`) rather than an opaque one. The element type comes
from recursively mapping the `items` subschema.

## Spec summary

Verbatim (2020-12 core, Applicator):

> The value of "items" MUST be a valid JSON Schema.

> This keyword applies its subschema to all instance elements at indexes
> greater than the length of the "prefixItems" array in the same schema
> object, as reported by the annotation result of that "prefixItems"
> keyword. If no such annotation result exists, "items" applies its
> subschema to all instance array elements.

> The annotation result of this keyword is a boolean true if the
> subschema was applied to any positions of the instance array. This
> annotation affects the behavior of "unevaluatedItems".

> Omitting this keyword has the same assertion behavior as an empty
> schema.

Distilled:
- `items` constrains every element (in our subset there is no
  [[prefixItems]], so "every element" is literal — see Support).
- The value is a full subschema; the element type is that subschema's
  mapping, applied recursively.
- Omitting `items` = empty schema = **no constraint on elements** (spec
  default: an untyped array).
- 2020-12 note: the draft-7 array-of-schemas *tuple* spelling of `items`
  moved to [[prefixItems]]; in 2020-12 `items` is **always a single
  schema**. An array-valued `items` is a stale-draft artifact.

## Support decision

**Support:** yes — the homogeneous list case (`type:"array"` + a single
`items` subschema).

An array with `items` emits a typed collection whose element type is the
recursive mapping of the `items` subschema (itself rejected at load if it
is out of subset, per [[type]] and **P7.1**).

Rationale (citing [[PRINCIPLES.md]]):
- **P2 (idiomatic output)**: a homogeneous element schema lowers cleanly
  to the one collection type every target has (`[]T` / `T[]` /
  `list[T]` / `List<T>`).
- **P7 / P7.1 (strict schema)**: the element schema must carry an
  explicit, supported `type` (or the [[nullability]] `oneOf` pattern); a
  shapeless element (`{}` / `true` / `false`) leaves the element type
  undecidable and is rejected with a located diagnostic.

Loader behavior:
- `type:"array"` **without** `items` → **reject** per **P7.1**. Per spec
  this is a valid "array of anything", but the typed-codegen contract
  requires an explicit element type (the array parallel of `type:object`
  requiring a shape — see [[type]]). Diagnostic names the location and
  asks for `items:{...}`.
- `items` **without** `type:"array"` → reject per [[type]] (missing or
  mismatched `type`); require explicit `type:"array"`.
- `items` value not a valid subschema → reject (recurse).
- `items` that is shapeless — `{}` / `true` / `false` → reject per
  **P7.1** (no element shape). Diagnostic names the array and asks for an
  explicit element `type`.
- `items` value that is an **array** (draft-7 tuple spelling) → reject;
  diagnostic notes 2020-12 moved tuples to [[prefixItems]], which is
  itself rejected per **P6** (below).
- [[prefixItems]] present (tuple form) → reject per **P6**. Heterogeneous
  positional tuples have no single coherent lowering — Go and Java have
  no tuple type, and a mixed `[]any`/`Object[]` would forfeit the typing
  the subset exists to provide. Deferred; diagnostic suggests an object
  with named [[properties]] instead.

## Type mapping

A `{type:"array", items:S}` schema emits the language-native ordered
collection over `T`, where `T` is the recursive mapping of the element
subschema `S`. Optional/nullable wrapping of the **array itself** is
owned by [[required]] + [[nullability]]; this table is the bare
collection type.

| Element `T` from `items` | Go | TypeScript | Python | Java |
|---|---|---|---|---|
| any supported `T` | `[]T` | `T[]` | `list[T]` | `List<T>` |

Notes:
- **Elements recurse.** `T` is produced by the same pipeline as any
  field type: a scalar (`items:{type:string}` → `[]string` / `string[]`
  / `list[str]` / `List<String>`), an object (`items` is a
  `{type:object, properties:{...}}` → a slice/array/list of the emitted
  aggregate), a nested array (`items:{type:array, items:{...}}` →
  `[][]T` / `T[][]` / `list[list[T]]` / `List<List<T>>`), or a `$ref`
  (see [[ref]]).
- **Element nullability is the element's own concern.** An element
  schema that is the recognized [[nullability]] `oneOf` pattern makes the
  *elements* nullable — `[]*T` (Go), `(T | null)[]` (TS),
  `list[Optional[T]]` (Python), `List<@Nullable T>` (Java) — distinct
  from the array field itself being optional/nullable, which wraps the
  whole collection. The two axes compose (an optional array of nullable
  elements is legal), and neither implies the other: an optional array of
  nullable elements is `list[T | None] | None`, not `list[T | None]`.
- **A union element is named.** An element schema that is a `oneOf` *sum
  type* (two or more non-`null` branches) is named after its position at
  load — `<EnclosingType><Property>Item`, `…ItemItem` for a nested array —
  moved into `$defs`, and the element rewritten to a `$ref`, so the element
  type is an ordinary named union in every target (see [[oneOf]] §"Unions in
  element positions"). Go and Java decode such an element through the
  union's dispatcher one value at a time — neither can allocate a sealed
  interface from a whole-collection decode.
- **Java** uses `List<T>` (interface type; the concrete `ArrayList` is an
  implementation detail of the deserializer). `List<T>` is a reference
  type, so it carries a non-null validator rather than boxing (see
  PRINCIPLES Java §3); element boxing follows `T`'s own [[type]] mapping.
- **Empty vs absent.** `[]` (present, empty) is distinct from an absent
  array (owned by [[required]]) and from `null` (owned by
  [[nullability]]); see the Go nil-slice hazard under Serialize-side.

## Validator mapping

Per **P10** the array type and every element are validated at the
(de)serializer boundary; per **P11** element failures aggregate.
`items` contributes the per-element dispatch; the outer array-type check
(`Array.isArray` / typed slice / Pydantic `list` / typed `List` binding)
comes from [[type]]'s `"array"` row.

| Language | Strategy |
|---|---|
| Go | Custom `UnmarshalJSON` decodes the field into a shadow `[]*json.RawMessage`, then dispatches each element through `T`'s runtime helper, collecting `Violation{Path, Reason}` into the one `ValidationError`. `Path` threads the index: `tags[2]`. |
| TypeScript | Hand-emitted `Array.isArray` guard, then a per-element loop running `T`'s checks; push `Violation { path: "tags[2]", reason }` per bad element into the list, throw one `ValidationError`. |
| Python | Pydantic `list[T]` in strict mode; per-element validation is native and aggregates via `pydantic.ValidationError.errors()` (`loc` carries the element index). |
| Java | the per-POJO collecting deserializer (PRINCIPLES Java §5) reads the array node, walks each element through `T`'s spec-strict/constraint helper, and collects `Violation{path:"tags[2]", reason}` into the one `ValidationException`. The Go parallel. |

- **Path convention.** Element failures use bracketed indices appended to
  the field path (`tags[2]`, and for nested arrays `matrix[1][2]`),
  distinct from the dotted member paths [[properties]] uses — so a caller
  can locate the offending element unambiguously (**P11**).
- **Element recursion.** Each element validates recursively — an array of
  objects runs each object's own `Validate`, an array of arrays recurses
  again, an array of `$ref` follows the reference (see [[ref]]).
- **Empty array.** The element loop is vacuous; an empty `[]` passes the
  `items` check (array-length floors, when supported, live in their own
  specs — see Interactions).

### Serialize-side (P12)

`items` is symmetric across directions: serialize recurses the shared
`Validate` into each element (a nested aggregate element runs its own
`MarshalJSON`/`toIntermediate`/`model_dump`; a scalar element re-runs the
same predicate the deserializer used) **before emitting a byte**, failing
with the same aggregated primitive (**P11**), and re-emits elements in
order (arrays are ordered — unlike object members, element order is part
of the value and is preserved).

- **Go nil-slice hazard.** A `nil` `[]T` marshals to JSON `null` under
  `encoding/json`, not `[]`. For a **required, non-nullable** array that
  is the wrong wire form. The generated `MarshalJSON` therefore emits
  `[]` for a required non-nullable array whose in-memory slice is `nil`
  (or the serialize-side `Validate` rejects `nil`, per the [[nullability]]
  omit-vs-emit table) — the same absent-vs-zero distinction **P9** forces
  everywhere. TS/Python/Java don't alias an empty list to `null`, so this
  is a Go-only encoder concern; the decision itself is owned by
  [[required]] + [[nullability]], flagged here because the array type is
  where it bites.

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| List of scalars | `{type:array, items:{type:string}}` |
| List of objects | `{type:array, items:{type:object, properties:{id:{type:integer}}}}` |
| Nested array | `{type:array, items:{type:array, items:{type:integer}}}` |
| Element with assertions | `{type:array, items:{type:string, minLength:1}}` |
| Nullable elements | `{type:array, items:{oneOf:[{type:string},{type:null}]}}` |
| Union elements (named after the position) | `{type:array, items:{oneOf:[{type:string},{type:integer}]}}` |
| Array member of a struct | `{type:object, properties:{tags:{type:array, items:{type:string}}}}` |
| Self-reference via `items`, **required** OK (see [[ref]]) | tree node — the empty array terminates the recursion: `{type:object, properties:{value:{type:string}, children:{type:array, items:{$ref:"#"}}}}` with `children` required |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| `type:array` without `items` (P7.1) | `{type:array}` |
| `items` without `type:array` (per [[type]]) | `{items:{type:string}}` (no `type`) |
| Shapeless element (P7.1) | `{type:array, items:{}}`, `…items:true`, `…items:false` |
| Element not a schema | `{type:array, items:"string"}`, `…items:5` |
| Out-of-subset element (P7.1, recurse) | `{type:array, items:{type:object}}` (object with no shape) |
| Array-valued `items` (draft-7 tuple) | `{type:array, items:[{type:string},{type:integer}]}` |
| Tuple via `prefixItems` (P6) | `{type:array, prefixItems:[{type:string},{type:integer}]}` |
| `items` paired with non-array `type` | `{type:string, items:{type:string}}` |

### Runtime fixtures (validator)

- Element valid / invalid against the element schema; a bad element
  reports at its index path (`tags[2]`).
- Multiple bad elements → all reported in one shot (**P11**).
- Empty array `[]` → accepted (element check vacuous).
- Non-array instance (`{}`, `"x"`, `5`) → rejected by the [[type]]
  `"array"` check, not by `items`.
- Nested array element failure → nested index path (`matrix[1][2]`).
- Array of objects → each element runs its aggregate's own `Validate`;
  failures thread the index into the member path (`items[0].id`).
- Self-reference via `items` (see [[ref]]): a recursive instance of
  bounded depth validates and round-trips; the empty array terminates the
  chain, so a required `items:{$ref:"#"}` field is satisfiable (contrast
  the direct self-reference rule in [[properties]] / [[ref]]).
- Round-trip preserves element **order** and empty-vs-populated shape
  (Go required-array `nil` → `[]`, per Serialize-side).

## Interactions

- **[[type]]**: `items` is only meaningful under `type:"array"`; pairing
  with any other `type` is a generator-time error, and `type:"array"`
  **requires** `items` (no untyped arrays — the array parallel of the
  object-shape requirement in [[type]]).
- **[[prefixItems]]**: the 2020-12 positional-tuple keyword — **rejected**
  per **P6** (no coherent cross-language tuple lowering). With it rejected,
  `items` applies to *all* elements, so the annotation-dependent "elements
  past the prefix" clause never engages in our subset.
- **[[contains]]**: the array **existential** — supported for a scalar
  matcher over a scalar `items` element type (its own spec); layers over the
  same element set `items` types. **[[unevaluatedItems]]**: **rejected** per
  **P6** (annotation-dependent semantics don't lower).
- **[[minItems]] / [[maxItems]]**: array-level count assertions that layer
  over the same element set (see their specs); both count the full array
  including every `items`-validated element, and are orthogonal to the
  element typing `items` supplies. **[[uniqueItems]]**: element-uniqueness
  assertion, covered by its own spec when landed.
- **[[nullability]]**: owns optional/nullable wrapping of the array field;
  element-level nullability is expressed by the element subschema (the
  `oneOf` null pattern) and composes with the field wrapping. The Go
  nil-slice → `null` encoder concern (Serialize-side) is decided here.
- **[[required]]**: orthogonal — decides whether the array member must be
  present; `items` types its elements. A present-but-empty `[]` satisfies
  `required` (presence, not non-emptiness — non-emptiness is
  [[minItems]]).
- **[[ref]]**: `items` may be a `$ref`, including one resolving back to the
  containing type. A self-reference wrapped in an array **terminates** (the
  empty array ends the chain), so — unlike a direct object self-reference —
  it may be **required and non-nullable** (see [[properties]] / [[ref]]).

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native (`items` = single schema over all elements). |
| OpenAPI 3.1 | Aligns with 2020-12. Native. |
| OpenAPI 3.0 | `items` is single-schema-only (no tuple form) — matches our list case. Native; `nullable:true` on the element → reject (rewrite to the nullability `oneOf`). |
| Swagger 2.0 / draft-4 | Single-schema `items` maps natively; the **array** `items` tuple form (and its `additionalItems` tail) → reject, rewrite to a homogeneous list or an object with named [[properties]]. |

## See also

- [[type]] — gates `items` to `type:"array"`; supplies the array-type
  check and the element base type.
- [[properties]] — the object analog; array-of-objects elements are
  emitted aggregates.
- [[prefixItems]], [[unevaluatedItems]] — rejected per **P6**; `items` is
  the supported per-element applicator. [[contains]] — the supported
  scalar existential over the element set.
- [[minItems]], [[maxItems]], [[uniqueItems]] — array-level assertions
  layered over the element set.
- [[required]], [[nullability]] — own the array field's optional/nullable
  wrapping and the empty-vs-absent-vs-`null` distinction.
- [[ref]] — element `$ref`, including array-terminated recursion.
