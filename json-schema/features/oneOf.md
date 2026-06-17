# `oneOf`

Source: JSON Schema 2020-12, Core (Applicator vocabulary), §10.2.1.3
"Keywords for Applying Subschemas With Boolean Logic → oneOf".

The **tagged-union / sum-type** keyword: an instance is valid iff it
validates against **exactly one** branch. Among the boolean-logic
applicators, `oneOf` and [[allOf]] are the two the subset admits — but by
different mechanisms: `allOf` (intersection) collapses to a single
materialized schema at load ([[allOf]]), whereas `oneOf` is *retained* as a
closed sum type. [[anyOf]], [[not]], [[if-then-else]] stay **rejected** per
**P6** (see [[dependentSchemas]] for the same rationale). `oneOf` earns its
place because "exactly one" is a *sum
type*, which lowers coherently to all four targets — **but only when the
branches carry a decidable selector**, so a deserializer can route a wire
value to exactly one branch without trial-validating every subschema. Two
selectors are supported, composed in layers: the **JSON type token**
separates branches of different kinds, and — among two or more **object**
branches — a shared required **`const`-tag** property separates them by
value. Only the OpenAPI `discriminator` *object* is deferred (as optional
sugar over the const tag; see Support decision).

## Spec summary

Verbatim (2020-12 core, Applicator, §10.2.1.3):

> This keyword's value MUST be a non-empty array. Each item of the array
> MUST be a valid JSON Schema.

> An instance validates successfully against this keyword if it validates
> successfully against exactly one schema defined by this keyword's value.

Distilled:
- The value is a non-empty array of subschemas — the **branches**. The
  instance is valid iff **exactly one** branch validates: zero matches
  fails, and — unlike [[anyOf]] — **two or more** matches also fails.
- "Exactly one" is what makes it a sum type rather than an intersection
  ([[allOf]]) or an inclusive-or ([[anyOf]]). It is the only combinator
  whose semantics correspond to a single, closed, discriminated choice —
  the shape every target can model.
- The keyword says nothing about *how* to tell the branches apart;
  validity is defined by brute-force validation against all branches. A
  faithful *typed* lowering needs more than validity — it needs a
  **decidable selector** (below), or the deserializer is reduced to
  guessing (**P7**).

## Support decision

**Support:** yes, for a `oneOf` whose branches are separable by a
**decidable selector**. Concretely, the generator accepts a `oneOf` iff:

- it has **≥ 2 branches** (a single-branch `oneOf` is a pointless wrapper
  — reject per **P7.1**, diagnostic points at using the branch directly);
- **every branch declares a single recognized [[type]]** (directly, or via
  a `$ref` to a named typed definition) — a branch with no `type`, a
  boolean/empty schema (`true`/`false`/`{}`), or one that is itself a
  combinator has no classifiable kind and is rejected;
- **grouped by JSON kind** across {`null`, `boolean`, `string`, `number`,
  `array`, `object`}: every non-`object` kind holds **at most one** branch
  (type-token separable), and the `object` kind, if it holds **two or
  more** branches, is separated by a shared **`const`-tag discriminator**
  (below).

Selection is layered. The outer selector is the wire token (`{`→object,
`[`→array, `"`→string, number→the numeric branch, `true`/`false`→boolean,
`null`→null). When the token is `{` and there are multiple object branches,
the inner selector reads the **discriminator property's value** and maps it
to the branch bearing that `const`. Because the branches are proven
disjoint at load (distinct kinds, and distinct `const` values within the
object kind), **at most one** branch can match, so "exactly one" decomposes
into two decidable checks:

- **≤ 1 (load time):** disjoint kinds + pairwise-distinct discriminator
  `const`s → no instance can satisfy two branches. Proven once, statically.
- **≥ 1 (runtime):** the token (then, for objects, the discriminator value)
  selects a branch and the value must validate against it; a token or
  discriminator value matching no branch is zero matches → a `Violation`.

This is why mixed-kind unions (`object | array`, `object | string`,
`string | integer`) and object tagged unions (`Cat | Dog`) are all
supported. The [[nullability]] pattern `oneOf:[{T},{null}]` is the
**degenerate two-kind instance** of the type-token layer (the `null` token
is the selector); it remains owned by [[nullability]].

### Discriminated object unions — the `const`-tag

Two or more **object** branches are separated by a shared discriminator
property. The generator accepts them iff there is **exactly one** property
name that, in **every** object branch:

- is listed in that branch's [[required]] array (so it is always on the
  wire to select on), and
- has a scalar **`const`** (a single-value [[enum]] is equivalent), with
  the `const` **values pairwise-distinct** across the branches.

That property is the **discriminator**; its `const` value is the inner
selector. This is the pure-JSON-Schema tagged-union idiom — no OpenAPI
`discriminator` object required — and it is **validation-bearing**: the
`const` is already enforced both directions ([[const]], **P12**), and the
discriminator is a **closed value set** (**P13.1**), so an unknown
discriminator value is *rejected*, not preserved. Keying selection on the
`const` value (not the `$ref` name) keeps it stable under type renaming.

- **Zero** shared required-`const` properties across the object branches →
  reject: no discriminator (diagnostic shows the const-tag form).
- **More than one** qualifying property → reject as **ambiguous** (**P7.1**);
  the deferred OpenAPI `discriminator` object is how one would later name
  the intended tag explicitly.
- The discriminator property is otherwise an ordinary member of each branch
  (emitted as the [[const]] literal); the rest of each branch is a normal
  open object (**P13**) — extra keys are still preserved *within* the
  selected branch. The tag closes selection *across* branches; it does not
  close the branches themselves.

Grounding ([[PRINCIPLES.md]]): **P1** — a sum type with a decidable
selector round-trips identically across all four targets; **P6** — the
selector requirement is the strict-subset line that admits `oneOf` as a
retained union while still excluding the non-collapsing applicators
(`anyOf`/`not`/`if`); **P7 / P7.1** — a selector-less
union (overlapping branches) would force trial-validation guessing, which
we reject loudly rather than approximate; **P2** — the type-token rule is
exactly the set of unions TypeScript can narrow with native
`typeof`/`Array.isArray` and Go/Java can decode by a single token peek, so
it reads like hand-written code in every language.

### Why a selector is required, not optional

The subset's objects are **open by default** ([[additionalProperties]],
**P13**): unknown keys are preserved, not rejected. Structural
discrimination ("does this payload match branch A's shape?") is therefore
unsound for object branches — an extra key never disqualifies a match, so
two object branches can both plausibly accept the same payload. This is
exactly why an explicit **`const`-tag** is required to separate object
branches: it is a closed value the wire must carry, immune to extra keys.
For *disjoint-kind* branches the JSON token itself is the tag —
self-describing, forward-compatible, and free.

### Null branches — nullable unions

`null` is one of the disjoint kinds, so a `null` branch is admitted like
any other. It doesn't add a sum-type *member*; it marks the whole field
**nullable**, and the remaining non-null branches form the value type:

- **exactly one non-null branch** (`[{T},{null}]`) → a plain nullable
  field, **owned by [[nullability]]** (its canonical two-branch pattern);
- **two or more non-null branches** (`[{object},{array},{null}]`) → a
  **nullable union**: the non-null branches form the sum type (and must
  themselves be pairwise-disjoint), and the field is nullable.

The nullable union needs no new machinery — every target already has a
nullable channel for the union type: a Go interface is nilable (`nil` =
`null`), Python wraps in `Optional[...]`, TS adds `| null`, and a Java
reference is `@Nullable`. The `null` token selects "no value" exactly as
the value tokens select their branches; decode/encode of the null state
follow the [[nullability]] tables over the union type (including the
optional-vs-null collapse in Go/Java and the faithful round-trip in
TS/Python). Required-vs-optional and the nullable state remain orthogonal
(**P8**), so all four presence/null combinations apply to a union just as
to a scalar.

### Deferred (reject with a "not yet supported" diagnostic)

- **The OpenAPI `discriminator: {propertyName, mapping}` object** — the
  explicit form that names the tag property and maps values to schemas. It
  is deferred as *optional sugar* over the const-tag form: when accepted, it
  must be **consistent with** the branch `const`s (each mapped value's
  branch must already carry that `const`), so the `const` stays the single
  validation-bearing source of truth and we don't inherit the
  discriminator object's `$ref`-name brittleness. Until then it is rejected
  with a diagnostic pointing at the const-tag form, which needs no OpenAPI
  extension.

### Rejected outright (incoherent, not merely unsupported)

- **`integer | number`** — both are the JSON number token *and*
  `integer ⊂ number`, so any integer satisfies both branches: `oneOf`'s
  exactly-one is **unsatisfiable**. Reject per **P7.1** (not deferred — no
  discriminator can fix an overlap). Distinct numeric-vs-non-numeric pairs
  (`integer | string`, `number | boolean`) are fine.
- **Empty `oneOf: []`** — the value MUST be non-empty; reject as an
  invalid schema.

Loader behavior:
- `oneOf` value not a non-empty array of valid subschemas → reject
  (recurse into each branch).
- Single-branch `oneOf` → reject (**P7.1**, pointless wrapper).
- Any branch without a single recognized `type` (missing `type`,
  `true`/`false`/`{}`, a nested combinator) → reject: no classifiable kind.
- Two or more **object** branches → require the `const`-tag discriminator
  (exactly one shared required-`const` property, pairwise-distinct values);
  zero such properties or more than one qualifying → reject (no
  discriminator / ambiguous).
- Two or more branches of the same **non-object** kind (two strings, two
  integers) → reject: a scalar same-kind choice is an [[enum]] (or `const`
  union), not a `oneOf` — diagnostic points at [[enum]].
- `integer` and `number` branches together → reject (unsatisfiable
  overlap).
- A `null` branch with exactly one other branch → hand off to
  [[nullability]] (its canonical two-branch pattern); with two or more
  others → accept as a **nullable union** (null marks the field nullable;
  the non-null branches form the sum type).
- Synthesized union type name collides after case-mapping → reject per
  **P15** (`x-<lang>-name` on the declaring member/definition is the
  escape hatch), exactly as [[const]]/[[enum]] synthesized types.

## Type mapping

A supported `oneOf` emits a **closed sum type** — a value that holds
exactly one of the branch types, structurally (the in-memory
representation cannot hold two, so the "exactly one" invariant is carried
by the type itself; the runtime check is only *which* branch on decode).

The union type is **named** by the same rule [[const]]/[[enum]] synthesized
types use ([[properties]] §"Synthesized type names"): a named `$defs`
union reuses the def name; an **anonymous** inline union on a property is
named `<EnclosingType><Property>` (Go flat / Java nested), while TS and
Python inline the union with no synthesized name.

Running example (named union in `$defs`):

```yaml
$defs:
  Widget: { type: object, properties: { size: { type: integer } } }
  Foo:
    oneOf:
      - { $ref: '#/$defs/Widget' }        # object branch
      - { type: string }                  # string branch
      - { type: array, items: { type: number } }   # array branch
```

### TypeScript

A bare union — inline on the field, or a `type` alias for a `$def`. No
synthesized name; narrowing is native (**PRINCIPLES TS §2** — structural
types, no runtime footprint):

```ts
export type Foo = Widget | string | number[];

function handle(f: Foo) {
  if (typeof f === "string") {        // f: string
  } else if (Array.isArray(f)) {      // f: number[]
  } else {                            // f: Widget
  }
}
```

The type-token selector maps 1:1 onto TS's built-in narrowing primitives
(`typeof`, `Array.isArray`, and — for object-vs-object — a discriminant
literal property). This is the best-fit target: the acceptance rule is
*exactly* what TS narrows without hand-written type guards (**P2**).

### Python

A `Union` (PEP 604 `X | Y` on 3.10+, `Union[...]` alias otherwise); a
named `$def` becomes a `TypeAlias`. Pydantic v2 strict mode discriminates
disjoint kinds natively:

```python
Foo = Union[Widget, str, list[float]]     # TypeAlias for the $def
```

For an object tagged union, the const tag becomes a `Literal` field
([[const]]) and the union carries `Field(discriminator=...)` — Pydantic's
native discriminated-union feature, which gives O(1) selection and precise
errors (see "Discriminated object unions" below).

### Go

A **sealed interface** with an unexported marker method (**Option B**);
the field is the bare interface, type-switched directly (no wrapper, no
accessor):

```go
type Foo interface{ isFoo() }

func (Widget) isFoo() {}          // a $ref/named branch implements the marker directly
type FooString string             // a non-nameable branch gets a synthesized variant type…
func (FooString) isFoo() {}
type FooArray []float64           // …named <Union><Kind>
func (FooArray) isFoo() {}

type Container struct {
    Value Foo `json:"value"`
}

switch v := c.Value.(type) {      // direct type switch on the field
case Widget:     use(v)
case FooString:  use(string(v))
case FooArray:   use([]float64(v))
}
```

The marker method is unexported, so **only generator-emitted types
implement the interface** — the set is closed by construction. Any branch
that is (or `$ref`s) a named Go type implements the marker directly; an
inline scalar/array/object branch synthesizes a variant type named
`<Union><Kind>` (Go has no nested types → flat, P15-backstopped). Because
at most one branch occupies each kind, `<Union><Kind>` is unambiguous.

### Java

Java 8 baseline (**PRINCIPLES Java §1**) has no sealed interfaces, so the
union is a **plain interface, sealed by convention** — the collecting
deserializer (§5) only ever constructs the known variants:

```java
public interface Foo {}                       // sealed by convention (no Java-8 `sealed`)
// Widget implements Foo (object branch — the POJO gains `implements Foo`)
public final class FooString implements Foo { private final String value; /* ctor, getter */ }
public final class FooArray  implements Foo { private final List<Double> value; /* … */ }
```

Java has no union type: an object `$ref` branch just gains `implements
Foo` on its existing POJO, while a **scalar/array branch must be wrapped**
in a variant class (`String` can't implement an interface). This wrapping
is the cost only of the *tagless token-based* form (mixed kinds); a pure
object tagged union has no wrappers — every branch is already a POJO. The
verbosity is hidden behind the POJO style (**P2**).

### Nullable unions

A `null` branch adds no member type; it makes the union field nullable
using each target's existing nullable channel ([[nullability]]), applied
to the union type rather than a scalar:

| | value type | nullable form |
|---|---|---|
| Go | `Foo` (interface) | already nilable — `nil` = `null`; no `*Foo` wrapper |
| TypeScript | `Foo` | `Foo \| null` (optional adds `?`) |
| Python | `Union[…]` | `Optional[Union[…]]` |
| Java | `Foo` | `@Nullable Foo` |

The presence/null state machine (required+nullable emits `null`, optional
collapses in Go/Java, faithful in TS/Python) is exactly the
[[nullability]] serialize/round-trip tables, unchanged — the union type
simply takes the place of the scalar.

### Discriminated object unions

Two or more object branches separated by a `const`-tag emit the same
closed sum type; the tag is an ordinary member of each branch, emitted as
the [[const]] literal, and drives selection.

```yaml
$defs:
  Cat: { type: object, required: [kind], properties: { kind: {const: cat}, meow: {type: string} } }
  Dog: { type: object, required: [kind], properties: { kind: {const: dog}, bark: {type: string} } }
  Animal: { oneOf: [ {$ref: '#/$defs/Cat'}, {$ref: '#/$defs/Dog'} ] }
```

- **TypeScript** — a discriminated union; the tag is a literal, narrowed by
  `switch`:
  ```ts
  interface Cat { kind: "cat"; meow: string; }
  interface Dog { kind: "dog"; bark: string; }
  export type Animal = Cat | Dog;
  switch (a.kind) { case "cat": /* Cat */ break; case "dog": /* Dog */ break; }
  ```
- **Python** — Pydantic native discriminated union:
  ```python
  class Cat(BaseModel): kind: Literal["cat"]; meow: str
  class Dog(BaseModel): kind: Literal["dog"]; bark: str
  Animal = Annotated[Union[Cat, Dog], Field(discriminator="kind")]
  ```
- **Go** — the sealed interface; the container's `UnmarshalJSON` peeks the
  discriminator on an object token, then unmarshals into the concrete
  struct:
  ```go
  type Animal interface{ isAnimal() }
  func (Cat) isAnimal() {}
  func (Dog) isAnimal() {}
  // decode: raw["kind"] == "cat" → *Cat ; "dog" → *Dog ; else Violation
  ```
- **Java** — every branch is already a POJO implementing the interface;
  the collecting deserializer (**PRINCIPLES Java §5**) peeks the
  discriminator `JsonNode` and dispatches to the matching POJO's collecting
  deserializer (keeping P11 aggregation — the reason we peek rather than
  lean on Jackson's fail-fast `@JsonTypeInfo`, though the const tag is
  exactly what an `@JsonTypeInfo(use=NAME, include=EXISTING_PROPERTY)` form
  would key on).

## Validator mapping

Per **P10**/**P11**/**P12**. "Exactly one" is decomposed as above: the
disjointness invariant (distinct kinds + distinct discriminator `const`s)
is proven at **load** (so ≤ 1 is structural and needs no runtime work), and
the boundary enforces **≥ 1** by routing the wire token — then, for an
object token with a multi-branch object kind, the **discriminator value** —
to its branch and validating the value against that branch's own shared
predicates. A token *or* discriminator value matching no branch → one
`Violation`. The discriminator is a **closed value set** (**P13.1**): an
unknown discriminator value is rejected, not preserved (unlike a branch's
own unknown *members*, which stay open per **P13**). This is **selection
then delegate**, never a trial-all-branches loop.

| Language | Strategy |
|---|---|
| Go | The container's collecting `UnmarshalJSON` (shadow `*json.RawMessage` layout, **PRINCIPLES Go** / [[nullability]]) peeks the field's first non-space token, routes to the branch of that kind (`{`→object; `[`→array: `FooArray`; `"`→string: `FooString`; number→the numeric branch via `parseSpecInteger`/spec-number so `1.5` still yields a `Violation`). For an object token with 2+ object branches it further reads the discriminator property and selects the branch with that `const`. It then runs that branch's shared `Validate` and assigns the concrete type to the interface field. No matching kind / unknown discriminator value → `Violation` collected into the single `ValidationError`. |
| TypeScript | `fromIntermediate` is the `typeof`/`Array.isArray` chain shown above; for an object it switches on the discriminant literal (`raw.kind`) and delegates to that branch's converter (e.g. `CatTypeHint.fromIntermediate`); the fall-through pushes one `Violation`. Plain checks only (**PRINCIPLES TS §1** — no runtime schema lib). |
| Python | Pydantic v2 strict `Union` selects by kind; an object tagged union uses `Field(discriminator=...)` for O(1) selection. Zero matches / unknown discriminator raise, aggregated into `pydantic.ValidationError`. |
| Java | The per-POJO collecting deserializer (**PRINCIPLES Java §5**) switches on the `JsonNode` kind (`isObject`/`isArray`/`isTextual`/`isNumber`/`isBoolean`); for an object with 2+ object branches it peeks the discriminator node and dispatches to the matching POJO's collecting deserializer. On no match / unknown discriminator it pushes a `Violation` into the single `ValidationException`. |

Reason strings name **what was expected** — the set of admissible kinds/
branch types (`expected Widget, string, or number[]`), never a bare
`oneOf` — per the informative-reason convention the constraint families
use.

### Serialize-side (P12)

In the statically typed targets (Go/TS/Java) the in-memory value **is** a
single branch member, so "exactly one" is structurally guaranteed and the
encode adapter simply emits the held variant: Go `json.Marshal` on the
interface marshals its dynamic type (a `FooString`/`FooArray` named type
serializes as its underlying JSON kind; an object branch emits its
fields); TS `toIntermediate` branches on `typeof`/`Array.isArray` and
delegates to the member's converter; Java's `Serializer` writes by runtime
class. The shared `Validate` still **re-runs the selected branch's
constraints before emit**, so an in-memory member violating its own
branch's rules fails serialize with the same aggregated primitive rather
than being written (real teeth where construction is unchecked).

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Scalar ∪ scalar (disjoint kinds) | `{oneOf:[{type:string},{type:integer}]}` |
| Object ∪ scalar | `{oneOf:[{$ref:"#/$defs/Widget"},{type:string}]}` |
| Object ∪ array | `{oneOf:[{$ref:"#/$defs/Widget"},{type:array,items:{type:number}}]}` |
| Three disjoint kinds | `{oneOf:[{$ref:"#/$defs/Widget"},{type:string},{type:array,items:{type:number}}]}` |
| Branch constraints carried through | `{oneOf:[{type:string,minLength:3},{type:integer,minimum:0}]}` |
| Named `$defs` union reused by `$ref` | `{$defs:{Foo:{oneOf:[…]}}, properties:{f:{$ref:"#/$defs/Foo"}}}` |
| Object tagged union (`const`-tag) | `{oneOf:[{$ref:"#/$defs/Cat"},{$ref:"#/$defs/Dog"}]}` with `kind:{const:…}` required in each |
| Tagged union mixed with a scalar kind | `{oneOf:[{$ref:"#/$defs/Cat"},{$ref:"#/$defs/Dog"},{type:string}]}` (token picks object-vs-string; `const` picks Cat-vs-Dog) |
| Two-branch `[T,null]` | owned by [[nullability]] — the degenerate type-token case |
| Nullable union — `null` among 3+ disjoint kinds | `{oneOf:[{$ref:"#/$defs/Widget"},{type:array,items:{type:number}},{type:"null"}]}` |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Single-branch wrapper (P7.1) | `{oneOf:[{type:string}]}` |
| Empty array (invalid schema) | `{oneOf:[]}` |
| Branch with no classifiable kind | `{oneOf:[{type:string},{minLength:3}]}`, `…{oneOf:[{type:string},true]}`, `…[{type:string},{}]` |
| Object branches with no shared required-`const` (no discriminator) | `{oneOf:[{type:object,properties:{a:{…}}},{type:object,properties:{b:{…}}}]}` |
| Discriminator `const` not `required` in a branch | `{oneOf:[{Cat with kind required},{Dog with kind optional}]}` |
| Non-distinct discriminator values | `{oneOf:[{kind:{const:"x"}…},{kind:{const:"x"}…}]}` |
| Ambiguous discriminator (2+ qualifying properties) | two object branches sharing both `kind` and `variant` as required-`const` |
| Two same-**scalar**-kind branches (use `enum`) | `{oneOf:[{type:string,const:"a"},{type:string,const:"b"}]}` → [[enum]] |
| `integer`+`number` overlap (unsatisfiable, P7.1) | `{oneOf:[{type:integer},{type:number}]}` |
| Duplicate `null` branches | `{oneOf:[{type:string},{type:"null"},{type:"null"}]}` (same-kind, and a tautology) |
| Branch is a nested combinator | `{oneOf:[{type:string},{anyOf:[…]}]}` |
| Synthesized union name collision (P15) | two anonymous unions recasing to the same Go type name (fix: `x-go-name`) |

### Runtime fixtures (validator)

- Wire value whose token selects a branch and validates → OK, bound to
  that variant (both directions).
- Token selects a branch but the value fails that branch's constraints
  (`""` for `{type:string,minLength:3}`) → one `Violation` from the branch
  predicate.
- Token matches **no** branch (`true` against `Widget | string |
  number[]`) → one `Violation` naming the admissible kinds.
- `1.5` against a `string | integer` union → routed to the integer branch,
  rejected by the spec-number rule (**P12** parse adapter), not truncated.
- Object with `kind:"cat"` against `Cat | Dog` → bound to `Cat`; extra
  unknown keys on the object are **preserved** (branch stays open, **P13**).
- Object with `kind:"fish"` (unknown discriminator) → one `Violation`
  naming the admissible values (`cat`, `dog`) — closed value set (**P13.1**),
  not preserved.
- Object with the discriminator **absent** → one `Violation` (it is
  `required`); the deserializer never falls back to trial-matching branches.
- `null` against a nullable union (`Widget | number[] | null`) → accepted
  as the null state (required+nullable) / omitted-or-null per the
  [[nullability]] serialize table (optional); `null` against a
  non-nullable union → one `Violation`.
- Serialize of an in-memory member violating its branch's own constraints
  → rejected before emit (**P12**).
- A failing union combined with a failing sibling field → **both**
  reported in one shot (**P11**).

## Interactions

- **[[nullability]]**: owns the two-branch `oneOf:[{T},{null}]` pattern —
  the degenerate type-token case (the `null` token is the selector) — and
  the per-language nullable encoding/serialize tables. A `null` branch
  among 3+ kinds is a **nullable union**: supported here, reusing those
  same tables over the union type (the `null` branch marks the field
  nullable, the non-null branches form the sum type).
- **[[const]] / [[enum]]**: the `const`-tag discriminator's basis — the
  validation-bearing selector for object branches, reusing [[const]]'s
  exact value-equality (already enforced both directions, **P12**) and its
  closed-value-set rejection of unknowns (**P13.1**); a single-value
  [[enum]] is an equivalent tag. Same-kind *scalar* branches are an [[enum]]
  rather than a `oneOf`. Also: [[properties]]'s synthesized-type naming rule
  — reused verbatim here for the union type name.
- **[[type]]**: every branch must declare one recognized `type` (or `$ref`
  to a typed def); it supplies the JSON kind that is the selector. A
  branch's own [[type]] siblings (formats, numeric/string constraints)
  validate normally once the branch is selected.
- **[[ref]]**: a branch may be a `$ref` to a named typed definition; the
  resolved type supplies the kind, and (in Go/Java) that named type
  implements the union marker / interface directly rather than being
  wrapped.
- **[[additionalProperties]]**: object openness (**P13**) is *why*
  same-kind object branches need an explicit `const` tag — extra keys never
  disqualify a structural match. The tag closes selection *across* branches
  while each branch stays open *within* (unknown members preserved), so the
  two concerns coexist: closed discriminator, open payload.
- **[[required]]**: the discriminator property **must** be `required` in
  every object branch — otherwise a payload could omit it and be
  unselectable. Separately, whether the whole union-typed member is present
  is its own `required` question; an absent optional union raises no
  `oneOf` violation (**P8**).
- **[[allOf]]**: the *other* admitted boolean-logic applicator, but by a
  different mechanism — intersection **collapses to one type at load** (a
  merge/flatten), so it disappears downstream, whereas `oneOf` is retained
  and emits a union. An `allOf` branch that is itself a `oneOf` is rejected
  ([[allOf]]): an intersection with a union does not collapse.
- **[[anyOf]] / [[dependentSchemas]] / [[not]] / [[if-then-else]]**: the
  boolean-logic / conditional applicators that stay **rejected** per
  **P6**. `oneOf` is admitted because "exactly one" is a *closed sum type*
  with a decidable selector, which inclusive-or ([[anyOf]]), negation
  ([[not]]), and runtime shape-forking ([[if-then-else]] / [[dependentSchemas]])
  are not.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native. Type-token-separable unions (incl. nullable unions), plus object tagged unions via a shared required-`const` discriminator. `integer`+`number` overlap rejected. |
| OpenAPI 3.1 | Adopts 2020-12 — `oneOf` identical; the const-tag form works unchanged. The 3.1 `discriminator: {propertyName, mapping}` object is **deferred** (will be accepted as optional sugar over the `const`-tag form, consistency-checked against the branch `const`s). |
| OpenAPI 3.0 / Swagger 2.0 | `oneOf` exists (3.0) with the same exactly-one semantics; the standalone `discriminator` is the same deferred sugar. Swagger 2.0 has no `oneOf`. |
| draft-4..7 | `oneOf` present since draft-4 with identical semantics — no rewrite; the subset rules (selector required, overlaps rejected) apply unchanged. |

## See also

- [[nullability]] — owns the two-branch `oneOf:[{T},{null}]` pattern (the
  degenerate type-token case) and the per-language nullable encoding a
  nullable union reuses over the union type.
- [[const]] / [[enum]] — the `const`-tag discriminator's validation-bearing
  selector (and closed-value-set rejection), and the source of the
  synthesized-type naming rule reused for the union type.
- [[type]] — supplies each branch's JSON kind, the outer selector.
- [[ref]] — `$ref` branches; the resolved type implements the Go/Java
  union marker directly.
- [[additionalProperties]] — object openness (**P13**) is why object
  branches need an explicit `const` tag; branches stay open within.
- [[required]] — the discriminator must be required in each branch; also
  the union member's own presence, distinct from branch selection.
- [[allOf]] — the other admitted applicator; intersection collapses to
  one type at load (merge), while `oneOf` is retained as a union.
- [[dependentSchemas]] — a rejected applicator; shares the P6 rationale
  that `oneOf` (as a closed sum type) is the exception to.
- [[PRINCIPLES.md]] — **P1** (polyglot sum type), **P6** (strict subset),
  **P7/P7.1** (reject ambiguity), **P2** (idiomatic per-language output),
  **P13** (open objects).
