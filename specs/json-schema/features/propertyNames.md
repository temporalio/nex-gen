# `propertyNames`

Source: JSON Schema 2020-12, Core (Applicator vocabulary), §10.3.2.4
"Keywords for Applying Subschemas to Objects → propertyNames".

Constrains every **member name** of an object against a subschema (the
name, always a string, is the instance under test). Partially supported:
map-shaped objects only.

## Spec summary

Verbatim (2020-12 core, Applicator):

> The value of "propertyNames" MUST be a valid JSON Schema.

> If the instance is an object, this keyword validates if every property
> name in the instance validates against the provided schema. Note the
> property name that the schema is testing will always be a string.

> Omitting this keyword has the same behavior as an empty schema.

Distilled:
- The subschema validates **keys**, not values; the instance it sees is
  always a string.
- In practice the subschema is a string schema using [[pattern]],
  [[minLength]], [[maxLength]], [[enum]], or [[format]].

## Support decision

**Support:** partial — accepted **only** on a map-shaped object (an
object with [[additionalProperties]] and **no** [[properties]]);
**rejected** when [[properties]] is present.

Rationale (citing [[PRINCIPLES.md]]):
- **P10 (enforced)**: on a map, key constraints lower to a clean runtime
  loop over the keys — checked at the boundary, aggregated per **P11**.
- **P7 / P7.1 (reject ambiguity)**: alongside [[properties]] the
  declared member names are static and known at generation time;
  layering a name constraint over them is ambiguous (does it gate the
  declared names, the extras, or both?) and adds little. Reject and ask
  the author to encode key shape on the map form instead.
- The propertyNames subschema must itself be a **supported string
  subschema** — implicitly/explicitly `type:"string"` with only
  string-applicable assertions. Anything else (e.g. `type:"integer"`,
  which can never match a string key) → reject per **P7.1**.

Loader behavior:
- `propertyNames` value not a valid schema → reject.
- Subschema not a string schema (or carrying non-string assertions) →
  reject; diagnostic explains keys are always strings.
- `propertyNames` present **with** [[properties]] → reject; diagnostic
  points at the map form. A future relaxation could validate declared names
  at generation time and apply the runtime check only to extras — deferred
  pending demand.
- `propertyNames` with no [[additionalProperties]] (so no map, no
  properties) → already rejected by [[type]] (`type:object` needs a
  shape); `propertyNames` alone is not a shape.
- Empty / `true` subschema → reject per **P7.1** (no constraint; just
  drop the keyword).

## Type mapping

None of its own. The host object's type comes from
[[additionalProperties]] — all four languages wrap the map in a named
catch-all member (`AdditionalProperties map[string]T` /
`Map<String,T> additionalProperties` / `additionalProperties:
Record<string,T>` / `additional_properties: dict[str, V]`).
`propertyNames` only adds a key validator over those keys.

## Validator mapping

Per **P10**/**P11**. Loop over the parsed object's keys; validate each key
string against the (string) constraint.

| Language | Strategy |
|---|---|
| Go | The key-constraint check is a predicate in the shared `Validate`, which `UnmarshalJSON` calls after decoding: iterate the decoded keys and run the check (compiled `regexp` for [[pattern]], length checks); a failure → `Violation{Path:key, Reason: fmt.Sprintf("invalid property name %q: %s", key, why)}` (`why` is the underlying assertion's reason, e.g. `must match ^[a-z]+$`), collected into one `ValidationError`. |
| TypeScript | the shared `Validate` predicate over `Object.keys(parsed)` applies the check; a failure → push ``Violation{path:k, reason: `invalid property name "${k}": ${why}`}``, throw one `ValidationError`. |
| Python | both directions of the `_<Model>TransferTypeConverter` (**PRINCIPLES Python §3**) loop the map's keys and apply the shared key check; a failure appends ``Violation(path=key, reason=f'invalid property name "{key}": {why}')`` per bad key into the single `ValidationError`. |
| Java | in the per-POJO collecting deserializer (PRINCIPLES Java §5), iterate the parsed tree's keys, apply the shared key check, and push a `Violation{path:key, "invalid property name \"" + key + "\": " + why}` per bad key into the single `ValidationException`. |

Reuses whatever the string-assertion specs ([[pattern]], [[minLength]],
[[maxLength]], [[enum]], [[format]]) emit — `propertyNames` is just those
checks applied to keys instead of values, so it inherits their
dialect/strategy decisions (notably [[pattern]]'s regex-dialect caveat).

### Serialize-side (P12)

The key check is part of the shared `Validate`, so it runs again before
emit: every catch-all key about to be written is re-validated against the
constraint, and a key inserted in memory that violates it (e.g. a map key
not matching the `pattern`) fails serialization rather than emitting an
out-of-contract object. Extra keys serialize **verbatim** (no
case-mapping — see [[additionalProperties]]), so the in-memory key is
exactly the wire key the check applies to, in both directions.

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Pattern keys on a map | `{type:object, additionalProperties:{type:integer}, propertyNames:{type:string, pattern:"^[a-z]+$"}}` |
| Length-bounded keys | `{type:object, additionalProperties:true, propertyNames:{type:string, maxLength:64}}` |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| With `properties` (P7) | `{type:object, properties:{id:{type:integer}}, propertyNames:{type:string, pattern:"…"}}` |
| Non-string subschema | `propertyNames:{type:integer}` |
| Shapeless subschema | `propertyNames:{}`, `propertyNames:true` |
| No host map | `propertyNames` with neither `properties` nor `additionalProperties` (caught by [[type]]) |

### Runtime fixtures (validator)

- All keys satisfy the constraint → OK.
- One key violates (bad pattern / too long) → one
  `ValidationError{path:key, reason}`.
- Multiple bad keys → all reported in one shot (P11).
- Empty object → vacuously OK.

## Interactions

- **[[additionalProperties]]**: the host. `propertyNames` constrains the
  map's keys; `additionalProperties` constrains its values.
- **[[properties]]**: mutually exclusive with `propertyNames` in our
  subset (reject if both present).
- **[[patternProperties]]**: temporarily unsupported (rejected at load
  time in v1); `propertyNames` is the supported way to constrain key
  shape without per-pattern value schemas.
- **[[pattern]] / [[minLength]] / [[maxLength]] / [[enum]] / [[format]]**:
  the string assertions reused against keys; inherit their decisions.
- **[[minProperties]] / [[maxProperties]]**: count constraints compose
  with key-shape constraints on the same map.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Partial (map-only) as above. |
| OpenAPI 3.1 | Aligns with 2020-12. Partial. |
| OpenAPI 3.0 | No `propertyNames` keyword — nothing to map. |
| Swagger 2.0 / draft-4 | `propertyNames` (draft-6+) → same partial handling. |

## See also

- [[additionalProperties]] — the map host; constrains values.
- [[patternProperties]] — temporarily unsupported; key-constraint alternative.
- [[pattern]], [[minLength]], [[maxLength]], [[enum]], [[format]] —
  string assertions reused on keys.
- [[type]] — requires the object to have a shape.
