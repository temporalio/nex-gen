# `nullability` (cross-cutting design note)

Not a JSON Schema keyword. Captures the generator's per-language convention
for how **optionality** ("key may be absent") and **nullability** ("value
may be `null`") are encoded in emitted code. Per **P8** these are two
different concerns and we keep them distinct.

## Concept matrix

| Concern | JSON Schema source | Wire shape | Type-level question |
|---|---|---|---|
| **Optional** | property name *not* in `required` array of the enclosing object | key may be absent | "How do we represent absent?" |
| **Nullable** | the `oneOf:[{type:"T"},{type:"null"}]` pattern (see "Nullability convention" below); the array form `["T","null"]` is rejected per [[type]] support decision | value present, equal to JSON `null` | "How do we represent JSON null?" |

This doc owns the optionality conventions per language and the
nullability convention (the recognized `oneOf:[{type:T},{type:null}]`
pattern, defined under "Nullability convention" below).

## Optionality conventions per language

### Java

Primitive type for required fields; boxed type for optional. Emitted as
a POJO (Java 8 floor — **not** a record; see PRINCIPLES Java §1), so the
fields below are private with generated getters/constructor. The
package is `@NullMarked` (JSpecify; non-null by default), so optional
reference fields carry `@Nullable` and required ones need no annotation
(see PRINCIPLES Java §3):

```java
@NullMarked
public final class User {
    private final long      id;        // required: integer
    private final @Nullable Long   nickname;  // optional: integer — null if absent
    private final String    name;      // required: string (non-null by default; validator enforces)
    private final @Nullable String email;     // optional: string
    // generated constructor, getters, equals/hashCode/toString
}
```

| `type` token | required | optional |
|---|---|---|
| `"integer"` | `long`    | `@Nullable Long`    |
| `"number"`  | `double`  | `@Nullable Double`  |
| `"boolean"` | `boolean` | `@Nullable Boolean` |
| `"string"`  | `String` *(non-null validator)* | `@Nullable String` |
| `"object"`  | `T` *(non-null validator)* | `@Nullable T` |
| `"array"`   | `List<T>` *(non-null validator)* | `@Nullable List<T>` |

Required-field validators must still check absence and `null` explicitly
for reference types (`String`, `List<T>`, object types) — the type
system can't carry that constraint by itself. The `@Nullable`/
`@NullMarked` annotations are **complementary**: they track in-memory
post-construction nullness (required → non-null; optional → may be
null when the key is absent) and propagate it into the consumer's
static null-analysis. They are compile-time only (CLASS retention →
no runtime dependency, **P4**) and do **not** encode the wire-level
nullable distinction — every optional reference field is `@Nullable`
regardless of whether it is optional-non-nullable or optional+nullable.

### Go

Pointer to primitive for optional fields; bare type for required.

```go
type User struct {
    ID       int64   `json:"id"`              // required
    Nickname *int64  `json:"nickname,..."`    // optional — nil if absent
    Name     string  `json:"name"`            // required
    Email    *string `json:"email,..."`       // optional
}
```

| `type` token | required | optional |
|---|---|---|
| `"integer"` | `int64`   | `*int64`   |
| `"number"`  | `float64` | `*float64` |
| `"boolean"` | `bool`    | `*bool`    |
| `"string"`  | `string`  | `*string`  |
| `"object"`  | `T`       | `*T`       |
| `"array"`   | `[]T`     | `[]T` *(nil-slice = absent)* |

Pointer-from-literal ergonomics: Go 1.26 extended the `new` builtin
to take an expression — `new(expr)` allocates a variable of the
inferred type, initializes it to `expr`'s value, and returns `*T`.
The release notes explicitly call out optional-pointer JSON fields
as the motivating use case.

```go
// Pre-1.26 (verbose):
tmp := int64(42)
u := User{Nickname: &tmp}

// 1.26+ (the convention we emit at call sites):
u := User{Nickname: new(int64(42))}
u := User{Nickname: new(req.Count)}        // arbitrary expressions work
u := User{Nickname: new(yearsSince(t))}    // including function calls
```

Generator constructors and builders prefer `new(expr)` for
ergonomics. Go 1.26+ is **not** a hard requirement — on older
toolchains the equivalent verbose form (`tmp := 42; u.Nickname = &tmp`)
compiles correctly. The generated *user-facing* API surface is
identical either way; only the call-site idiom shown in examples and
emitted constructors differs.

`[]T` for optional arrays: a `nil` slice and an empty slice are
distinguishable in Go (`s == nil` vs `len(s) == 0`), so a pointer
wrapper is unnecessary; the runtime check is `if user.Tags != nil`.

### TypeScript

Optional fields use the `?` modifier; the field type stays the bare
type. Absence is `undefined`, not `null`.

```ts
interface User {
  id: number;          // required
  nickname?: number;   // optional — undefined if absent
  name: string;        // required
  email?: string;      // optional
}
```

| `type` token | required | optional |
|---|---|---|
| any                 | `T`       | `T` with `?` on the field |

Validator emits `if (parsed.x === undefined) ...` for required-presence
checks. We never use `T | null` for the optional-only case — `?`/
`undefined` is the absence channel; `null` is a value reserved for the
nullability convention (`x?: T | null` is the optional+nullable form).

### Python

Use `Optional[T]` (alias for `Union[T, None]`) on the field type.
Default value is `None` for absence.

```python
from typing import Optional
from pydantic import BaseModel

class User(BaseModel):
    id: int                           # required
    nickname: Optional[int] = None    # optional — None if absent
    name: str                         # required
    email: Optional[str] = None       # optional
```

| `type` token | required | optional |
|---|---|---|
| any | `T` | `Optional[T]` (with `= None` default) |

Pydantic strict mode + `Optional[T]` accepts `None` for the optional
case. At the *value* level absence and explicit `null` both read as
`None`, but Pydantic's `model_fields_set` records **which keys the wire
actually carried**, so the distinction is *not* lost at the model level
— it is recoverable for serialization. The generated
`@model_serializer` (keyed on `model_fields_set`) therefore round-trips
optional+nullable **faithfully** (wire `null` → set → re-emitted as
`null`; wire-absent → unset → omitted), putting Python in the same
faithful tier as TypeScript (see "Round-trip behavior" and
"Serialize-side behavior" below, and PRINCIPLES Python §3).

## Nullability convention

The only accepted source-level expression for "this field's value may
be `null`" is the JSON Schema 2020-12 canonical idiom:

```json
{ "oneOf": [{"type": "<T>"}, {"type": "null"}] }
```

Order is insensitive — `[{"type":"null"}, {"type":"<T>"}]` is
equivalent. The non-null branch is a full subschema and may carry any
sibling keyword recognized for that `type`:

```json
{ "oneOf": [
    {"type": "string", "format": "email", "minLength": 5},
    {"type": "null"}
]}
```

This two-branch form is the **degenerate type-token case** of the general
`oneOf` rule (the `null` token is the selector); [[oneOf]] owns the
general treatment, and this doc owns this specific shape.

### Pattern acceptance rules

This doc recognizes a `oneOf` as the nullability pattern iff:
- exactly 2 branches;
- exactly one branch is the literal `{"type": "null"}` with no sibling
  keywords on the null branch;
- the other branch declares a recognized [[type]] (must not itself be
  `"null"` — `{type:"null"}` paired with `{type:"null"}` is a
  tautology and rejected).

Any other `oneOf` shape is [[oneOf]]'s domain — supported there when its
branches are separable by a decidable selector (pairwise-disjoint JSON
type kinds), otherwise rejected or deferred. A `null` branch among 3+
kinds is a **nullable union** — supported by [[oneOf]], reusing this doc's
per-language nullable encoding over the union type (the `null` branch
marks the field nullable; the non-null branches form the sum type).

### Required + nullable is supported

A field listed in the enclosing object's `required` array whose schema
matches the nullable `oneOf` pattern is **accepted**. It encodes "must
be present, value may be `null`" — `{}` is rejected (absent), `{"x":
null}` and `{"x": T}` are both accepted. The construct is fully
decidable: required+nullable accepts `{"x":null}` and `{"x":T}`;
optional+non-nullable accepts `{}` — disjoint edge cases, with none of
the other three states expressing the `{null, T}` space. Per **P8**
optional and nullable are orthogonal, so all four combinations are
legal.

### Per-language emitted type (nullable states)

Two nullable states exist: **optional+nullable** (absent / `null` / T)
and **required+nullable** (`null` / T; absent rejected). They share the
same emitted *type* in every language; only the presence check differs,
and TypeScript/Python also differ at the field-modifier level.

**Optional + nullable** (absent OK, `null` OK, T OK):

| `type` token | Java | Go | TypeScript | Python |
|---|---|---|---|---|
| `"integer"` | `@Nullable Long`    | `*int64`   | `x?: number \| null`  | `Optional[int] = None` |
| `"number"`  | `@Nullable Double`  | `*float64` | `x?: number \| null`  | `Optional[float] = None` |
| `"boolean"` | `@Nullable Boolean` | `*bool`    | `x?: boolean \| null` | `Optional[bool] = None` |
| `"string"`  | `@Nullable String`  | `*string`  | `x?: string \| null`  | `Optional[str] = None` |
| `"object"`  | `@Nullable T`       | `*T`       | `x?: T \| null`       | `Optional[T] = None` |
| `"array"`   | `@Nullable List<T>` | `[]T` (nil = absent or null) | `x?: T[] \| null` | `Optional[list[T]] = None` |

**Required + nullable** (`null` OK, T OK, absent rejected) — same type,
presence enforced by the validator; TS drops the `?`, Python drops the
`= None` default (Pydantic v2: `Optional[T]` with no default is
required-and-nullable):

| `type` token | Java | Go | TypeScript | Python |
|---|---|---|---|---|
| `"integer"` | `@Nullable Long`    | `*int64`   | `x: number \| null`  | `Optional[int]` |
| `"number"`  | `@Nullable Double`  | `*float64` | `x: number \| null`  | `Optional[float]` |
| `"boolean"` | `@Nullable Boolean` | `*bool`    | `x: boolean \| null` | `Optional[bool]` |
| `"string"`  | `@Nullable String`  | `*string`  | `x: string \| null`  | `Optional[str]` |
| `"object"`  | `@Nullable T`       | `*T`       | `x: T \| null`       | `Optional[T]` |
| `"array"`   | `@Nullable List<T>` | `[]T` (nil = null) | `x: T[] \| null` | `Optional[list[T]]` |

(Java is `@Nullable` across every nullable column — the annotation
tracks in-memory nullness, not the wire distinction; see the optionality
section above and PRINCIPLES Java §3. In Java/Go, required+nullable and
optional+nullable share both type *and* annotation; the presence check
is the only difference, exactly as required-non-nullable reference types
already rely on a validator the type can't express.)

### Round-trip behavior

- **Required + nullable round-trips losslessly in all four languages.**
  Presence is guaranteed, so an in-memory `null`/`nil`/`None`
  unambiguously means "the wire sent `null`"; the serializer always
  emits the key (never omits it), and `null` ⟷ `null`. There is no
  absent state to confuse it with.
- **Optional + nullable round-trips faithfully in TypeScript and
  Python; collapses in Go and Java.** TS keeps `undefined` (absent) vs
  `null` distinct in memory. Python reads both as `None` at the value
  level but tracks `model_fields_set`, so the generated
  `@model_serializer` re-emits a wire `null` as `null` and omits a
  wire-absent key. Go (`*T` `nil`) and Java (`null`) genuinely cannot
  distinguish the two in memory, so they emit a single canonical form
  — the key is **omitted** (the conservative choice; emitting `null`
  would fabricate a value the client may never have sent). A client
  that sent explicit `null` on an optional+nullable field reads it
  back as absent **in Go/Java only**.

**Collapse note (Go / Java only):** the in-memory representations of
"absent" and "JSON null" are the same (`nil`, `null`), and — unlike
Python's `model_fields_set` and TS's `undefined` — there is no side
channel recording which the wire carried, so post-validation user code
can't recover it. This matches **P8**'s framing — optional and
nullable are distinct *schema* concerns; runtime collapse is acceptable
when the language can't represent the difference. Making Go/Java
faithful would require a presence-tracking wrapper (`Null[T]` / shadow
bit); this is rejected as ergonomic overhead (P2) that the conservative
omit avoids.

TypeScript and Python enforce *and* preserve the distinction; Go and
Java enforce it at the boundary but collapse it in memory.

### Diagnostics

Wire form → required generator output:

| Source form | Action |
|---|---|
| `"type": ["T", "null"]` (array form) | Reject. Diagnostic suggests `oneOf: [{type:"T"}, {type:"null"}]`. |
| `{type:"T", "nullable": true}` (OAS 3.0) | Reject. Diagnostic suggests `oneOf: [{type:"T"}, {type:"null"}]`. |
| `oneOf` with `{type:"null"}` branch where field is in `required` | **Accept** — required+nullable (must be present, may be `null`). |
| `oneOf` of 3+ branches with `{type:"null"}` among them | **Accept** — a nullable union ([[oneOf]]): the `null` branch marks the field nullable, the non-null branches (which must be pairwise-disjoint) form the sum type. |

## Validator implications

Four schema states × four languages → twelve cells. The two axes are
orthogonal: **presence** (required = reject absent; optional = accept
absent) and **null acceptance** (non-nullable = reject `null`; nullable
= accept `null`).

| State | Java | Go | TS | Python |
|---|---|---|---|---|
| **Required, non-nullable** — must be present, must be T | type is `long`/`String`/etc.; emit `field == null` reject + type binding | type is `int64`/`string`/etc.; shadow `*T` field, reject on `nil` | type is `x: T`; emit `parsed.x === undefined \|\| parsed.x === null` reject | Pydantic field with no default → strict mode raises automatically |
| **Optional, non-nullable** — absent OK, T OK, explicit `null` rejected | strict-variant custom deserializer (see strategy below) | shadow `*json.RawMessage` with explicit `bytes.Equal(*raw, []byte("null"))` reject | `parsed.x === null` rejected; `=== undefined` OK | `model_validator(mode='wrap')` rejects keys present with `None` |
| **Optional + nullable** — absent OK, `null` OK, T OK | type is `@Nullable Long`/`String`/etc.; no extra check beyond type binding | type is `*int64`/`*string`/etc.; no extra check beyond type binding | type is `x?: T \| null`; both `undefined` and `null` accepted | `Optional[T] = None`; both forms accepted |
| **Required + nullable** — must be present, `null` OK, T OK, absent rejected | base (non-strict) deserializer accepts `null`; presence enforced (`field`-present check / required-field machinery) | shadow `*json.RawMessage`; reject on absent (`nil` shadow), accept `null` token | type is `x: T \| null`; emit `parsed.x === undefined` reject; `null` accepted | `Optional[T]` with **no** default → required, accepts `None` |

## Serialize-side behavior

Per **P12** the encode adapter chooses, *per field from the
optional/nullable/required declaration*, whether an empty in-memory
value (`undefined`/`nil`/`None`/`null`) is omitted or emitted as
`null`. The decision is **static** (baked into the generated
serializer), never a function of the value alone: a blanket "omit all
nulls" would drop a required+nullable `null` (violating `required`),
and a blanket "emit all nulls" would fabricate an invalid `null` for
optional-non-nullable.

| required | nullable | empty-value serialize action |
|---|---|---|
| optional | non-nullable | **omit** the key (emitting `null` is invalid; `Validate` also rejects an explicit in-memory `null` where the language can hold one) |
| optional | nullable | **omit** (conservative) in Go/Java; **faithful** in TS/Python — omit if unset, emit `null` if explicitly null |
| required | non-nullable | cannot be empty — `Validate` rejects; always emit the value |
| required | nullable | **emit `key: null`** — omitting violates `required` |

Per-language mechanism (all are *encode-adapter* concerns; the shared
`Validate` runs first and is identical to the deserialize side):

- **Go** — struct tags. Optional → `*T` with `,omitempty` (nil
  omitted); required+nullable → `*T` **without** `omitempty` (nil →
  `null`); required-non-nullable → bare value type. The type-alias
  `MarshalJSON` lets the tags do the work.
- **TypeScript** — `toTransferType` omits `undefined`, emits `null`; the
  three-state gives faithful optional+nullable for free.
- **Python** — a generated `@model_serializer(mode='wrap')` emits only
  `model_fields_set` keys (plus const fields), implementing the whole
  table; `model_fields_set` drives the faithful optional+nullable
  behavior. Baked into the model because the default Temporal
  `pydantic_data_converter` owns the `to_json` call — we don't pass
  `exclude_unset` ourselves (PRINCIPLES Python §3).
- **Java** — `@JsonInclude(NON_NULL)` on optional fields;
  `@JsonInclude(ALWAYS)` forces the required+nullable `null`;
  optional+nullable collapses to the conservative omit (PRINCIPLES
  Java §6).

## Strict enforcement of optional-non-nullable

**Decision:** explicit `"key": null` is rejected when the schema is
optional-non-nullable (the JSON-Schema-default case — a `{type: "T"}`
field not listed in `required`, where T is not the nullability
pattern). This honors the spec: `null` is not a valid value of any
non-`null` type, so a bare `{type: "string"}` doesn't admit `null`.

### Java

The per-POJO collecting deserializer (PRINCIPLES Java §5) decides the
three-way per field over the parsed tree node, mirroring Go:

1. node absent (`root.get(name) == null`) → key absent
   (required → push `Violation`; optional → leave field null/unset)
2. node `isNull()`                         → explicit `null`
   (optional-non-nullable → push `Violation`; nullable → accept as `null`)
3. otherwise                               → call the type-specific
   strict-parse helper (e.g. `SpecNumbers.specLong(node, path, errs)`),
   which pushes its own `Violation` on a spec violation

There is **no** `…StrictDeserializer` / `getNullValue` override and no
per-field `@JsonDeserialize`: the null/absence decision lives in the
collecting deserializer's branch, so the old two-subclass split
collapses into the same three-way Go uses in `UnmarshalJSON`.

### Go

The shadow struct uses `*json.RawMessage` for every field (not just
numeric). Per field, the generated `UnmarshalJSON`:

1. `shadow.Foo == nil`              → key absent
   (required → emit error; optional → leave field zero)
2. `bytes.Equal(*shadow.Foo, []byte("null"))` → explicit `null`
   (optional-non-nullable → emit error; nullable → accept)
3. otherwise                        → delegate to the type-specific
   runtime helper (e.g., `parseSpecInteger`)

### TypeScript

Natural three-way via `=== undefined` vs `=== null`:

```typescript
if (parsed.x === null) {
    violations.push({ path: "x", reason: "explicit null not allowed" });
} else if (parsed.x !== undefined) {
    // validate value
}
```

No runtime helper needed.

### Python

A model-level `model_validator(mode='wrap')` wraps Pydantic's
field-validation pass. It pre-scans the raw input dict for
optional-non-nullable keys present with `None`, runs the inner
handler, and combines any pre-errors with field-validation errors
into a single `ValidationError` — preserving P11 aggregation across
both sources.

Each generated model carries a `ClassVar[frozenset]` listing the
affected field names. The `ClassVar` annotation is required —
without it Pydantic treats `_NAME` as a private model attribute and
the validator can't iterate it.

```python
from typing import ClassVar, Optional
from pydantic import BaseModel, ValidationError, model_validator
from pydantic_core import InitErrorDetails, PydanticCustomError

class User(BaseModel):
    id: SpecInt
    nickname: Optional[SpecInt] = None        # optional, non-nullable
    bio: Optional[str] = None                 # optional + nullable

    _OPTIONAL_NON_NULLABLE: ClassVar[frozenset] = frozenset({"nickname"})

    @model_validator(mode="wrap")
    @classmethod
    def _reject_explicit_null(cls, data, handler):
        pre_errs = []
        if isinstance(data, dict):
            pre_errs = [
                InitErrorDetails(
                    type=PydanticCustomError(
                        "null_for_nonnullable", "explicit null not allowed"
                    ),
                    loc=(f,),
                    input=None,
                )
                for f in cls._OPTIONAL_NON_NULLABLE
                if f in data and data[f] is None
            ]
        try:
            instance = handler(data)
        except ValidationError as e:
            field_errs = [
                InitErrorDetails(
                    type=PydanticCustomError(err["type"], err["msg"]),
                    loc=err["loc"],
                    input=err.get("input"),
                )
                for err in e.errors()
            ]
            raise ValidationError.from_exception_data(
                title=cls.__name__, line_errors=pre_errs + field_errs
            ) from None
        if pre_errs:
            raise ValidationError.from_exception_data(
                title=cls.__name__, line_errors=pre_errs
            )
        return instance
```

Why `mode='wrap'` rather than `mode='before'`: a `mode='before'`
validator that raises short-circuits Pydantic's own field validation,
breaking P11 aggregation across error sources. `mode='wrap'` lets us
run the inner handler, catch its errors, and combine.

Why not a `BeforeValidator` per field: `BeforeValidator` receives
only the value, not "was the key present" — the absent-vs-explicit-
`None` distinction is only recoverable at the dict level.

## See also

- [[type]] — emitted bare type per `type` token; this doc wraps that.
- [[required]] — owns *which* fields are optional (the JSON Schema
  side of the decision).
- [[oneOf]] — owns the general `oneOf` treatment (type-token-separable
  unions); this doc owns the degenerate two-branch `oneOf:[{T},{null}]`
  nullability shape (defined under "Nullability convention" above).
- [[PRINCIPLES.md]] — **P8** (optional ≠ nullable), **P9**
  (distinguish absent from zero value), **P2** (ergonomics).
