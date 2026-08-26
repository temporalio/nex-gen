# `minProperties`

Source: JSON Schema 2020-12, Validation vocabulary, §6.5.2
"Validation Keywords for Objects → minProperties".

Sets a floor on the number of members an object instance must have. A
pure runtime count assertion — no type impact. Mirror of
[[maxProperties]].

## Spec summary

Verbatim (2020-12 validation, §6.5.2):

> The value of this keyword MUST be a non-negative integer.

> An object instance is valid against "minProperties" if its number of
> properties is greater than, or equal to, the value of this keyword.

> Omitting this keyword has the same behavior as a value of 0.

Distilled:
- Counts **all** members including preserved extras.
- Omission ≡ `0` (no floor).

## Support decision

**Support:** yes — runtime assertion.

Lowers to a boundary count check; no effect on emitted types. Citing
[[PRINCIPLES.md]]: **P10**, **P11**.

Loader behavior:
- Value not a non-negative integer (honors `1.0`-as-integer + the
  integer cap, see [[type]]) → reject.
- The portable count ceiling from [[maxItems]] applies.
- The keyword requires `type: "object"`; a missing or different type rejects
  at load time under **P7.1**.
- `minProperties: 0` → accepted (no-op; equals omission).
- `minProperties > maxProperties` (both present) → reject
  (unsatisfiable).
- `minProperties` greater than the number of members the schema can
  ever have — i.e. a closed object ([[additionalProperties]] `false`)
  with fewer declared [[properties]] than `minProperties` → reject
  (unsatisfiable). Diagnostic names the gap.
- **The count keywords own reconciliation against every keyword that bounds
  the key space**, not only the closed-object case above. A `minProperties`
  above the largest inhabitable key count is a load reject whatever imposes the
  ceiling — including a [[propertyNames]] whose key language is finite (an
  `enum`, or a `maxLength: 0`), which closes the key space to a countable set
  exactly as a declared member list does. The mirror duty for the upper bound,
  including [[dependentRequired]]'s forced closure, is in [[maxProperties]].
  This duty sits with the count keywords because the count is what makes the
  combination decidable; the bounding keyword's own spec need not restate it.

**Which object is counted, in both directions.** The count is always over the
**wire object at the boundary being validated** — on parse the raw decoded
object, on serialize the object the encoder is about to emit. It is never the
count of non-`null` in-memory fields. This keeps the keyword a wire contract
under **P1** and satisfies **P12**'s "identical in both directions": the same
predicate over the same *kind* of operand at each boundary.

One consequence must be stated rather than discovered. Under P1's
optional+nullable collapse an explicit `null` for an optional nullable member
reads back as absent, so `{"a": null}` is a **one-key** object inbound and a
**zero-key** object outbound. A schema with a count floor therefore accepts that
payload on parse and rejects the model it just produced on re-serialize. That is
a real consequence of the collapse, not an implementation defect, and it is the
reason a count bound and an optional nullable member are a poor combination:
authors who need the floor should make the member required, or non-nullable, or
drop the bound. An implementation must not "fix" the asymmetry by counting
in-memory fields on one side — that would make the keyword mean two different
things and break the wire contract.

## Type mapping

None. Constraint lives only in the validator.

## Validator mapping

Per **P10**/**P11**. The "number of properties" is the count of **distinct
member keys present on the wire**, taken at the deserialize boundary
**before** default population (see [[default]]) — a default-filled key is never on
the wire and does not count (see Interactions). Count the wire object as a
single number; do **not** sum a declared-fields bucket and an extras
bucket separately (case-mapping can route a key to either, and the
declared-vs-extras split is a language-side artifact, not a wire fact). Same
per-language strategy as [[maxProperties]] with `< min` as the failing
comparison:

| Language | Strategy |
|---|---|
| Go | `UnmarshalJSON` applies the predicate to the raw wire-key count; `Validate` applies the same comparison and reason to the model-derived set of keys that serialize will emit. |
| TypeScript | `fromTransferType` applies the predicate to `Object.keys(raw).length`; `toTransferType` applies it to the keys the outbound conversion will emit. The comparison is inlined in each converter. |
| Python | `from_transfer_type` counts `len(raw)` on the raw wire dict — one number over the wire object, taken before any default is materialized — and appends `Violation(path="", reason=f"must have at least {min} properties, got {n}")` when `n < min`, into the single generated `PayloadValidationError` application failure. |
| Java | the per-POJO collecting deserializer (PRINCIPLES Java §5) counts distinct keys in the parsed tree (`< min`) — one number over the wire object, **not** POJO fields + catch-all map summed post-bind; a violation joins the single `PayloadValidationError` application failure. |

### Serialize-side (P12)

The count runs again before emit, over the keys that **will actually be
written** — *after* default omission and the omit-vs-`null` decision (the
serialize mirror of "before default population"). A field whose default
is unset is omitted and does **not** count toward the floor, exactly as
it didn't on the way in — so a model that reads as populated in memory
(defaults visible) can legitimately fall **under** `minProperties` on the
wire, and serialize fails
(`MarshalJSON`/`toTransferType`/`to_transfer_type`) rather than emitting
an under-floor object; in Python the count is `len(out)` on the dict
`to_transfer_type` has built. See [[maxProperties]] serialize note
(symmetric).

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Floor satisfiable | `{type:object, additionalProperties:true, minProperties:1}` |
| Floor = 0 | `{type:object, properties:{a:{type:string}}, minProperties:0}` |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Not non-negative integer | `minProperties:-1`, `minProperties:1.5`, `minProperties:"1"` |
| Missing/mismatched object type | `{minProperties:1}`, `{type:"array", minProperties:1}` |
| `> maxProperties` | `minProperties:5, maxProperties:2` |
| Unsatisfiable on closed object | `properties:{a:{…}}, additionalProperties:false, minProperties:2` |

### Runtime fixtures (validator)

- Member count `== min` → OK (≥ inclusive).
- Member count `min-1` → one `PayloadValidationError` application failure whose reason names the
  floor and count (`must have at least 2 properties, got 1`).
- Open struct reaching the floor via extras → OK (extras count).

## Interactions

- **[[maxProperties]]**: paired bound over the same member set;
  `min > max` is a load error.
- **[[required]]**: required members count toward the floor but
  `minProperties` may demand *more* than the required set names —
  satisfiable only if extras are permitted ([[additionalProperties]]
  not `false`, or enough optional [[properties]]).
- **[[additionalProperties]] `false`**: caps how many members can exist;
  a `minProperties` above the declared count is then unsatisfiable
  (load error).
- **`default`**: `default` is an annotation, not an assertion — a
  default-filled key is never on the wire, so it does **not** count
  toward the floor. The count is taken before default population
  (see [[default]]); a client sending fewer than `minProperties` keys is invalid
  regardless of server-side defaults.
- **`const`** (future feature): an object-level `const` pins the exact
  member set, making `minProperties` statically decidable — a const
  object with fewer members than `minProperties` is unsatisfiable (load
  reject), mirroring the [[additionalProperties]] `false` case.
  Property-level `const` only constrains a value *if present*, so it has
  no count impact unless paired with [[required]]. Enforcement deferred
  to the `const` spec.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native. |
| OpenAPI 3.1 / 3.0 | `minProperties` identical. Native. |
| Swagger 2.0 / draft-4 | `minProperties` identical. Native. |

## See also

- [[maxProperties]] — upper bound on member count.
- [[required]], [[additionalProperties]] — interact with satisfiability.
