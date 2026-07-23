# `maxProperties`

Source: JSON Schema 2020-12, Validation vocabulary, §6.5.1
"Validation Keywords for Objects → maxProperties".

Caps the number of members an object instance may have. A pure runtime
count assertion — no type impact.

## Spec summary

Verbatim (2020-12 validation, §6.5.1):

> The value of this keyword MUST be a non-negative integer.

> An object instance is valid against "maxProperties" if its number of
> properties is less than, or equal to, the value of this keyword.

Distilled:
- Counts **all** members of the instance object, including ones not
  named in [[properties]] (i.e. preserved extras count too).
- No annotation behavior; pure assertion.

## Support decision

**Support:** yes — runtime assertion.

Lowers to a boundary count check in every language; no effect on emitted
types. Citing [[PRINCIPLES.md]]: **P10** (enforced at the boundary),
**P11** (aggregated).

Loader behavior:
- Value not a non-negative integer (per spec; honors the `1.0`-as-integer
  rule and the integer cap, see [[type]]) → reject.
- `maxProperties: 0` → accepted (object must be empty). Note this is a
  near-equivalent of a closed empty object; prefer
  `additionalProperties:false` with no [[properties]] when *emptiness*
  is the intent, and `maxProperties` when a numeric ceiling is.
- `maxProperties < minProperties` (both present) → reject
  (unsatisfiable). Diagnostic names both.
- `maxProperties` less than the count of [[required]] members → reject
  (unsatisfiable: required forces more members than the cap allows).

## Type mapping

None. The emitted aggregate is unchanged; the constraint lives only in
the validator.

## Validator mapping

Per **P10**/**P11**. The "number of properties" is the count of **distinct
member keys present on the wire**, taken at the deserialize boundary
**before** default population (see [[default]]) — a default-filled key is never on
the wire and does not count (see Interactions). Count the wire object as a
single number; do **not** sum a declared-fields bucket and an extras
bucket separately (case-mapping can route a key to either, and in Pydantic
the two sets overlap — verified `json-schema/research/pyd_minprops_probe.py`).

| Language | Strategy |
|---|---|
| Go | `UnmarshalJSON` counts decoded members (wire keys, pre-population) and hands the count to the shared `Validate`, whose `> max` predicate raises `Violation{Path:"", Reason: fmt.Sprintf("too many properties: at most %d, got %d", max, n)}`; collected into one `ValidationError`. |
| TypeScript | count `Object.keys(parsed).length` on the raw parsed wire object (before defaults applied); the shared `Validate`'s `> max` check pushes ``Violation{path, reason: `too many properties: at most ${max}, got ${n}`}``, throw one `ValidationError`. |
| Python | `model_validator`; `len(model_fields_set) > max` — `model_fields_set` already includes extras and excludes default-filled fields, so it is the exact wire-key count; raise into the aggregated `ValidationError`. |
| Java | the per-POJO collecting deserializer (PRINCIPLES Java §5) counts distinct keys in the parsed tree (`> max`) — one number over the wire object, **not** populated POJO fields + catch-all map summed post-bind; a violation joins the single `ValidationException`. |

### Serialize-side (P12)

The count runs again before emit, over the keys that **will actually be
written** — i.e. *after* default omission and the omit-vs-`null` decision
(the serialize mirror of "before default population"). A field whose
default is unset is omitted and does **not** count, exactly as it didn't
on the way in. `model_fields_set` (Python) is again the exact emitted-key
count under `exclude_unset`; Go/TS count the members the encoder will
emit; an over-cap model fails `MarshalJSON`/`toIntermediate`/`model_dump`
rather than emitting an out-of-bounds object. Because the in-memory model
can *read* a default as present that *serializes* as absent, a model can
legitimately fail `maxProperties`/`minProperties` on serialize that
"looked" satisfiable in memory — correct, since the assertion is about
the wire object.

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Cap with room | `{type:object, additionalProperties:true, maxProperties:3}` |
| Cap = 0 | `{type:object, additionalProperties:false, maxProperties:0}` |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Not non-negative integer | `maxProperties:-1`, `maxProperties:1.5`, `maxProperties:"3"` |
| `< minProperties` | `minProperties:5, maxProperties:2` |
| `<` required count | `required:["a","b","c"], maxProperties:2` |

### Runtime fixtures (validator)

- Member count `== max` → OK (≤ is inclusive).
- Member count `max+1` (including via extras) → one `ValidationError`
  whose reason names the cap and count (`too many properties: at most 3,
  got 4`).
- Combined with other failing assertions → all reported in one shot (P11).

## Interactions

- **[[minProperties]]**: paired bound; both count the same member set.
  `min > max` is a load error.
- **[[required]] / [[properties]]**: extras count toward the total, so
  an open struct ([[additionalProperties]] `true`) can exceed a cap the
  declared members alone wouldn't.
- **[[additionalProperties]] `false`**: bounds the max member count to
  the declared set; a `maxProperties` larger than that count is
  redundant (allowed, not an error).
- **`default`**: `default` is an annotation, not an assertion, and
  defaults are dropped on serialize (see [[default]]) — a default-filled key is
  never on the wire, so it does **not** count toward the cap. The count
  is taken before default population on deserialize.
- **`const`** (future feature): an object-level `const` pins the exact
  member set, making `maxProperties` statically decidable — a const
  object with more members than `maxProperties` is unsatisfiable (load
  reject), mirroring the [[required]] / [[additionalProperties]] `false`
  satisfiability checks. Property-level `const` only constrains a value
  *if present*, so it has no count impact unless paired with
  [[required]]. Enforcement deferred to the `const` spec.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native. |
| OpenAPI 3.1 / 3.0 | `maxProperties` identical. Native. |
| Swagger 2.0 / draft-4 | `maxProperties` identical. Native. |

## See also

- [[minProperties]] — lower bound on member count.
- [[required]] — pins which members; interacts with the cap.
- [[additionalProperties]] — whether extras (which also count) are
  allowed.
