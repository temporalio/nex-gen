# `deprecated`

Source: JSON Schema 2020-12, Validation, §9.3 "A Vocabulary for Basic
Meta-Data Annotations → deprecated".

Marks the schema location — a type, member, service, or operation — as
**discouraged for continued use**: still valid on the wire, but callers
should migrate away. In the spec it is a **pure annotation** — it never
affects validation. **Supported** — it lowers into each target's
**native deprecation marker** (Go's `// Deprecated:` godoc paragraph, a
TS JSDoc `@deprecated` tag, a Java `@Deprecated` annotation, a Python
PEP 702 `@deprecated(…, category=None)` marker), so editors, compilers,
and type checkers surface the marker to the extent their native mechanism
allows — a genuine limit in TypeScript (JSDoc deprecation reaches language
services, not a `tsc` diagnostic) and in Python (PEP 702 has no field form).
It is **not** a licence for a marker the generator has simply placed wrongly:
Go's `// Deprecated:` must be its own godoc paragraph, and a marker emitted
without the preceding blank line is inert through no fault of the toolchain. It has no validation, serialization, or access-warning effect. It is
a **tag** in the doc-comment assembly
[[description]] owns; because `deprecated` is boolean-only it carries no
message of its own, so the human rationale comes from the sibling
[[description]].

## Spec summary

Verbatim (2020-12 validation, §9.3):

> The value of this keyword MUST be a boolean.

> When multiple occurrences of this keyword are applicable to a single
> sub-instance, applications SHOULD consider the instance location to be
> deprecated if any occurrence specifies a true value.

> If "deprecated" has a value of boolean true, it indicates that
> applications SHOULD refrain from usage of the declared property. It MAY
> mean the property is going to be removed in the future.

> A root schema containing "deprecated" with a value of true indicates
> that the entire resource being described MAY be removed in the future.

Distilled:
- Value **MUST be a boolean**; default `false`.
- Multiple applicable occurrences **OR together**: **any** `true` ⇒ the
  location is deprecated. This is *not* the last-wins merge that
  [[description]] / [[title]] / [[default]] use (see Loader behavior).
- An **annotation**, not an assertion: per spec it **never changes
  whether an instance validates**. A wire value is judged by the schema's
  assertions alone, `deprecated` present or not.
- Applies to **any subschema** — a `$defs` type, a property subschema —
  and, on the Nexus envelope, to **services and operations** (see
  [[services]]). A root-schema `deprecated: true` deprecates the whole
  generated type.

## Support decision

**Support:** yes — as a **native deprecation marker** (plus a doc-comment
tag) on the declaration it decorates. It emits no validator and no
adapter behavior.

The defining choices (citing [[PRINCIPLES.md]]):
- **Idiomatic native markers (P2).** Unlike [[examples]] — the other
  metadata annotation that is merely *ignored* — `deprecated` maps to a
  **first-class deprecation mechanism in every target**: godoc's
  `Deprecated:` convention, JSDoc `@deprecated`, `java.lang.Deprecated`,
  PEP 702 `@deprecated`. Emitting the native marker is what makes the
  generated code read like code a human wrote, and it makes the
  deprecation *actionable* wherever the target tooling can carry it — and
  where the generator controls the placement, it must produce the form the tool
  recognizes rather than one it ignores.
  TypeScript exposes JSDoc deprecation through language services rather than a
  `tsc` diagnostic; Python's PEP 702 signal applies to decorated types and
  callables, while its field/operation `Annotated` form is documentation-only.
  So we emit, not ignore.
- **No modeling problem to block it.** [[readOnly]] / [[writeOnly]]
  reject because their directional intent has no single-type lowering;
  `deprecated` has no such fork — it merely *marks* a location and leaves
  the type, its fields, and the wire shape unchanged. There is exactly
  one declaration to attach the marker to, in every language.
- **Annotation, no runtime effect (P10/P12/P1).** `deprecated`
  contributes **no** constraint predicate to the shared `Validate` and
  **no** parse/encode adapter logic. A deprecated value is accepted and
  round-trips exactly like a non-deprecated one. Its entire effect is at
  generation time, in the emitted declaration — with no runtime validation,
  serialization, or access-warning effect in any target.
  Python is held to the same bar: it uses PEP 702's `category=None` so
  even its `@deprecated` marker raises no access-time `DeprecationWarning`
  (see Type mapping); we deliberately do **not** emit the runtime
  `Field(deprecated=True)` form, keeping the four targets in parity (P1).
- **Never an identifier; no P15 surface (P13/P1/P15).** `deprecated`
  never names a type, field, service, or operation and synthesizes no new
  identifier, so — like [[description]] — it adds **nothing** to the
  per-scope collision pass.

Loader behavior:
- `deprecated` value **not a boolean** → **reject** (P7.1; the spec's own
  MUST). Diagnostic names the offending value: `deprecated must be a
  boolean, got "true"` for `{deprecated: "true"}`, likewise `{deprecated:
  1}`.
- `deprecated: false` → **accepted and ignored** — it is the explicit
  form of the default (not deprecated), so no marker is emitted and no
  diagnostic is raised. This is the [[examples]] ergonomic exception to
  P7.1: `deprecated: false` is pervasive in OpenAPI / imported schemas as
  an explicit "not deprecated" flag, and it is unambiguous and inert, so
  rejecting it would force authors to strip meaningful, harmless metadata
  just to generate. (Contrast [[readOnly]] / [[writeOnly]], whose `false`
  rejects — but those keywords are rejected wholesale regardless.)
- Multiple `deprecated` occurrences applicable to one node after an
  [[allOf]] merge (P6) — an explicit `allOf` branch or a `$ref`
  sibling — → **OR**: if **any** applicable occurrence is `true`, the
  location is deprecated. This differs from [[description]] / [[title]] /
  [[default]], which resolve last-wins; `deprecated` follows the spec's
  own any-true-wins rule (§9.3). A merged `false` occurrence is ignored
  (as in isolation), so it never suppresses a sibling `true` — the
  location is deprecated iff some applicable occurrence is `true`.
- **Orthogonal** to [[required]], [[default]], [[const]], and
  [[nullability]]: a deprecated member may still be required, carry a
  default, and be nullable. `deprecated` changes none of those; it only
  adds the marker.

## Type mapping

**None.** `deprecated` does not change the emitted type and contributes
no identifier. Its sole materialization is the **native deprecation
marker** (and a doc-comment tag) on the nearest generated declaration:

| Placement | What gets marked |
|---|---|
| `$defs` type | the generated **type** (Go `type` / TS `interface` / Python class / Java class) |
| property subschema | the generated **field / getter / member** |
| service (envelope) | the generated **service interface**; see [[services]] |
| operation (envelope) | the generated **operation method**; see [[services]] |
| inline subschema with no declaration | dropped — nowhere to attach a marker |

Per-language marker (the meat — a native construct, not just comment
text):

| Language | Native marker | Effect for consumers |
|---|---|---|
| Go | a `// Deprecated: This <kind> is deprecated.` paragraph in the doc comment (godoc convention; generic reason — see below) | `gopls` / `staticcheck` SA1019 flag every use; `go doc` renders it. A doc-comment tag, not a keyword. |
| TypeScript | a bare JSDoc `@deprecated` tag in the `/** … */` block | Editors and language services strike through or suggest at call sites; `tsc` emits no deprecation diagnostic. |
| Python | PEP 702 `@deprecated("…", category=None)` (`typing_extensions` backport) on a generated type/service; fields and operation attributes use `Annotated[T, deprecated("…", category=None)]` as documentation metadata | Type/service decorators are recognized by PEP 702-aware checkers. Field and operation attribute metadata is not a PEP 702 warning construct. `category=None` suppresses access-time `DeprecationWarning`. |
| Java | the `@Deprecated` annotation on the type / getter / method, paired with a Javadoc `@deprecated` tag | `javac` warns at every use; IDEs strike-through. |

No new identifier is ever synthesized, so `deprecated` has **no P15
collision surface**.

## Doc-comment tag and the message limitation

`deprecated` is the **tag** slot (assembly part 3) of the doc comment
[[description]] owns — summary line ([[title]]) → body ([[description]])
→ **tags**. It renders as a trailer in the doc comment *and*, where the
target has one, the native marker (annotation / decorator / godoc
paragraph). The shared placement, wrapping, and escaping rules are those
[[description]] specifies; this keyword only fills the tag slot.

`deprecated` is **boolean-only** — the spec gives it no message string —
and the sibling [[description]] is **never copied** into the marker's
message slot: the rationale, replacement pointer, or removal timeline is
prose that lives in the doc-comment body ([[description]]) directly above
the marker, and copying it in would repeat the same text twice in the
generated source.

The marker is emitted **bare** wherever the native construct allows it —
a JSDoc `@deprecated` tag with no text (TS), the `@Deprecated` annotation
plus a bare Javadoc `@deprecated` tag (Java). Two targets *require* a
non-empty reason string, and there we emit a **generic** one,
`This <kind> is deprecated.` — where `<kind>` ∈ type / field / service /
operation, already known from the placement:
- **Go** — the godoc convention is a `// Deprecated: <text>` paragraph; a
  bare `Deprecated:` reads incomplete, so the generic reason fills it:
  `// Deprecated: This field is deprecated.`
- **Python** — PEP 702 `deprecated(msg, category=None)` takes a
  **mandatory** message argument, so both forms carry the generic
  string: `@deprecated("This type is deprecated.", category=None)` on a
  declaration, `Annotated[T, deprecated("This field is deprecated.",
  category=None)]` on a field. (`category=None` is what keeps the marker
  static-only — see Type mapping.)

The rationale the author actually wrote still travels with the symbol —
in the doc-comment body immediately above the marker — for all four
targets.

## Validator mapping

`deprecated` emits **no validator** and **no adapter behavior**. It is an
annotation (§9.3): it never appears in the shared `Validate`, never runs
in the parse or encode adapter, and never causes a runtime pass/fail in
either direction (**P12**). A deprecated value deserializes, validates,
and re-serializes identically to a non-deprecated one.

There is no runtime validation, serialization, or access-warning effect in any
target. Python emits `category=None`, which suppresses access-time
`DeprecationWarning`; Python also retains `__deprecated__`, and Java's
`@Deprecated` is runtime-visible metadata. Generated code does not inspect
either artifact.

## Property-testing matrix

### Accepted (positive)

| Shape | Handling |
|---|---|
| Deprecated `$defs` type | `{deprecated:true, type:"object", …}` → marker on the generated type |
| Deprecated property | `properties:{legacyId:{deprecated:true, type:"string"}}` → marker on the field/getter |
| Deprecated + description | `{deprecated:true, description:"Use `uuid` instead."}` → body carries the rationale, tag/marker below it |
| Deprecated + required | `required:["legacyId"]` with `legacyId:{deprecated:true, …}` → still required; marker added (orthogonal) |
| Deprecated + default | `{deprecated:true, type:"string", default:"x"}` → default machinery unchanged; marker added |
| Merged occurrences OR to true | `allOf:[{deprecated:true},{…}]` → deprecated (any true wins; see [[allOf]]) |
| `deprecated: false` (explicit not-deprecated) | `{deprecated:false, type:"string"}` → accepted, no marker, no diagnostic |
| Deprecated service / operation | envelope `services.ChatService.operations.legacyCall.deprecated` → marker on the generated method (see [[services]]) |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Value not a boolean | `{deprecated:"true"}`, `{deprecated:1}` |

### Runtime fixtures

None. `deprecated` has no runtime behavior — it neither validates nor
(de)serializes. Its only observable output is the generated declaration
(marker + doc tag), covered by generation-snapshot tests. The Python
snapshot asserts the `category=None` form. A dedicated access-warning runtime
assertion is still needed; the snapshot alone cannot prove warning behavior.

## Interactions

- **[[description]]**: owns the doc-comment assembly `deprecated` plugs
  into as a **tag**, and supplies the rationale prose the bare marker
  lacks (the boolean carries no message). The tag renders directly below
  the `description` body.
- **[[title]]**: the summary line of the same doc comment; unaffected by
  `deprecated`.
- **[[properties]]**: `deprecated` decorates a **member**, never renames
  it — the field name still comes from the resolved naming policy. The
  marker attaches to the generated field/getter.
- **[[services]]**: the Nexus envelope can mark a service or operation
  `deprecated`; the marker lands on the generated interface / method.
- **[[allOf]]** / **[[ref]]**: merges can bring multiple `deprecated`
  occurrences onto one node; they **OR** (any true ⇒ deprecated), unlike
  the last-wins annotations.
- **[[required]]**, **[[default]]**, **[[const]]**, **[[nullability]]**:
  fully orthogonal — a deprecated member keeps its presence, default,
  fixed value, and nullability; only the marker is added.
- **[[examples]]** / **[[readOnly]]** / **[[writeOnly]]**: the sibling
  metadata annotations. `deprecated` is the **supported** one — it lowers
  cleanly to a native marker, where [[examples]] is ignored (inert) and
  [[readOnly]] / [[writeOnly]] reject (directional).

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native annotation (§9.3, added in 2019-09) → native deprecation marker. |
| OpenAPI 3.1 | Adopts 2020-12 — `deprecated` native on schemas → marker. OpenAPI's own `deprecated` on operations/parameters carries the same intent. |
| OpenAPI 3.0 | Has `deprecated` (boolean) on schemas, operations, and parameters, same intent → marker. |
| draft-07 | **No `deprecated` keyword** (introduced in 2019-09); a draft-07 source simply has nothing to lower. |

`deprecated` is a boolean with a single, consistent intent across every
dialect that has it, so no rewrite is needed — only the per-language
marker choice above.

## See also

- [[description]] — owns the doc-comment assembly `deprecated` renders
  into as a tag, and supplies the rationale prose the bare boolean lacks.
- [[title]] — the summary line of the same doc comment.
- [[services]] — the Nexus envelope marks services/operations
  `deprecated`; the marker lands on the generated interface/method.
- [[examples]] — the sibling metadata annotation that is *ignored*
  (inert), the contrast to this *supported* one.
- [[readOnly]] / [[writeOnly]] — the sibling metadata annotations that
  *reject* (directional, no single-type lowering).
- [[PRINCIPLES.md]] — **P2** (idiomatic, native markers), **P7/P7.1**
  (reject loudly with fix-its), **P10** (annotation, no advisory
  validation), **P12** (no adapter/validator effect), **P13/P15** (no
  synthesized identifier, no collision surface).
