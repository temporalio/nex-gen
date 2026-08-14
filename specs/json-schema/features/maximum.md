# `maximum`

Source: JSON Schema 2020-12, Validation vocabulary, §6.2.2
"Validation Keywords for Numeric Instances → maximum".

Sets an **inclusive** upper bound on a numeric instance. A pure runtime
comparison assertion — no type impact. The canonical spec for the numeric
bound family; [[minimum]], [[exclusiveMaximum]], and [[exclusiveMinimum]]
share the machinery documented here and differ only in the comparison
operator (and, for the exclusive pair, one ecosystem-variance wrinkle).

## Spec summary

Verbatim (2020-12 validation, §6.2.2):

> The value of "maximum" MUST be a number, representing an inclusive upper
> limit for a numeric instance.

> If the instance is a number, then this keyword validates only if the
> instance is less than or exactly equal to "maximum".

Distilled:
- Value MUST be a JSON number (integer- or fraction-valued).
- Instance valid iff `instance ≤ maximum`.
- Applies **only** to numeric instances; the spec silently ignores it for
  non-numbers. Per **P7.1** we instead reject a `maximum` on a
  non-numeric [[type]] at load time.
- Pure assertion; no annotation behavior.

## Support decision

**Support:** yes — runtime comparison assertion.

Lowers to a single boundary comparison in every language; no effect on
emitted types. Citing [[PRINCIPLES.md]]: **P10** (enforced at the
boundary), **P11** (aggregated), **P12** (a pure predicate over the
decoded value in the **shared `Validate`** layer — identical in both
directions, no parse/encode adapter logic of its own).

Loader behavior:
- Value not a number → reject (`maximum:"5"`, `maximum:true`,
  `maximum:null`).
- `maximum` on a non-numeric [[type]] (`{type:"string", maximum:5}`) →
  reject per **P7.1** (statically meaningless).
- **On an `integer` field the bound MUST be integer-valued.** `maximum:5.0`
  is accepted (≡ `5`, honoring the `1.0`-as-integer rule from [[type]]);
  `maximum:5.5` is **rejected** with a fix-it ("use an integer bound, or
  make the field `number`"). An integer bound lets all four languages
  compare against one integer value with no float/round ambiguity.
- On a `number` field any finite numeric bound is accepted.
- A `maximum` larger than the [[type]] integer cap `+(2^53−1)` (or
  `minimum` below `−(2^53−1)`) on an `integer` field is **redundant** (the
  cap already rejects) but **allowed** — not an error, just dead range.
- **`maximum` and `exclusiveMaximum` both present on the same node →
  reject (redundant).** Both are upper bounds, so one always dominates
  (`v ≤ M ∧ v < X` reduces to whichever is tighter) — keeping both is
  ambiguous redundancy, not a tighter constraint. Fix-it: specify exactly
  one — `exclusiveMaximum:N` for a strict bound, `maximum:N` for an
  inclusive one (**P7.1**). (This is a *same-axis* rule; the lower+upper
  mix `minimum` + `exclusiveMaximum` is **not** redundant and stays a
  satisfiability check below.) When [[allOf]] support is considered, two
  bounds arriving from *different* subschemas must be **intersected
  (tightened)**, not rejected — see [[allOf]].
- Combined-bound satisfiability (a lower with an upper — [[minimum]] /
  [[exclusiveMinimum]] against [[maximum]] / [[exclusiveMaximum]]): if the
  accepted set is empty, reject. See **Interactions → satisfiability**.
- Combined with [[multipleOf]]: if no multiple of the divisor lies in the
  accepted range, reject (deferred detail lives in [[multipleOf]]).

## Type mapping

None. The emitted field type is [[type]]'s primitive (`int64`/`long`/
`int`/`number` etc.); the bound lives only in the validator.

## Validator mapping

Per **P10**/**P11**. A single `≤` comparison against the fixed bound,
identical in both directions (a pure predicate over the decoded value —
the **shared `Validate`** layer of **P12**).

| Language | Strategy |
|---|---|
| Go | The comparison is a predicate in the shared `Validate(model)` (`if v > max { push(Violation{Path, Reason: fmt.Sprintf("must be <= %v, got %v", max, v)}) }`), which the generated `UnmarshalJSON` calls after decoding; violations collect into one `ValidationError`. Integer field: compare the decoded `int64` to the integer bound directly (exact). Number field: compare `float64` to the `float64` bound. |
| TypeScript | ``if (v > max) push(Violation{path, reason: `must be <= ${max}, got ${v}`})``, throw one `ValidationError`. `number` covers both `integer` and `number` fields; `max` is an emitted numeric constant. |
| Python | An explicit comparison in the transfer type converter (PRINCIPLES Python §3): `if v > max: violations.append(Violation(path=…, reason=f"must be <= {max}, got {v}"))`, aggregated into the single generated `ValidationError`. On an `integer` field it runs **after** `_parse_spec_integer` has normalized the wire value (`5.0`→`5`, see [[type]]), so the comparison is against a Python `int`. `max` is an inlined numeric literal. |
| Java | The per-POJO collecting deserializer (PRINCIPLES Java §5) reads the field node via the [[type]] `SpecNumbers` helper, then checks `v > max` (integer field: `long` vs `long`; number field: `double` vs `double`), pushing a `Violation{path, "must be <= " + max + ", got " + v}` into the single `ValidationException`. **Not** bean-validation `@Max` — the check is hand-written in the collecting deserializer like every other constraint. |

**Informative `reason` strings.** The `Violation` `reason` is *not* a bare
keyword tag (`"maximum"`); it states the **concrete bound and the offending
value** — `must be <= 10, got 15` — so the aggregated error tells the caller
exactly which limit was crossed and by what. The bound is an emitted
compile-time constant; the actual value is interpolated at runtime. This
matches [[type]]'s descriptive style (`expected integer`, `exceeds cap`).
All four targets hand-build the string from the emitted bound and the
runtime value. The rest of the numeric family
([[minimum]], [[exclusiveMaximum]], [[exclusiveMinimum]], [[multipleOf]])
follows the same convention with its own operator/word.

**Cross-language exactness (integer field).** An `integer` field's bound is
itself integer-valued (loader rule above), so Go and Java compare the
decoded `int64`/`long` against the integer bound directly — exact, no
promotion. TypeScript has only IEEE doubles, so it necessarily performs the
comparison in `double`; this still agrees value-for-value because both the
capped value and the bound lie within `±(2^53−1)`, which is exactly
representable as a double — the probe confirms `(double)cap == cap` (and
e.g. `(double)cap <= 5.5 == false`). Python normalizes the wire value to
`int` via `_parse_spec_integer` before comparing, so it too compares
exactly. This is the same
cap guarantee the integer runtime helpers lean on in [[type]].
(This is *why* an integer-field bound is required to be integer-valued: it
keeps even the mixed integer/float comparison exact and unambiguous.)

### Serialize-side (P12)

The bound is a shared-`Validate` predicate, so it **re-runs before emit**
over the decoded value — a model constructed out of range (a Go `int64` /
Java `long` / unbounded Python `int` set past `maximum` in memory) fails
serialize with the same aggregated primitive rather than emitting an
out-of-range number. This has real teeth in the statically-typed targets,
where in-memory construction is unchecked (identical rationale to the
[[type]] integer-cap re-check). No parse-adapter-only or
encode-adapter-only logic: the comparison is pure and direction-agnostic.
TS non-finite (`NaN`/`±Infinity`) rejection on emit is owned by [[type]],
not here.

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Integer inclusive max | `{type:"integer", maximum:10}` |
| Integer max, `.0`-valued | `{type:"integer", maximum:10.0}` |
| Number fractional max | `{type:"number", maximum:1.5}` |
| Single-value range (min==max) | `{type:"integer", minimum:5, maximum:5}` |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Value not a number | `maximum:"5"`, `maximum:true`, `maximum:null` |
| Type mismatch (P7.1) | `{type:"string", maximum:5}`, `{type:"boolean", maximum:1}` |
| Fractional bound on integer field | `{type:"integer", maximum:5.5}` |
| Unsatisfiable range | `{type:"integer", minimum:10, maximum:2}` |
| Empty range vs exclusive | `{type:"integer", minimum:5, exclusiveMaximum:5}` (see [[exclusiveMaximum]]) |
| Redundant same-axis pair | `{type:"integer", maximum:10, exclusiveMaximum:12}`, `{type:"integer", maximum:10, exclusiveMaximum:10}` |

### Runtime fixtures (validator)

- `v == max` → OK (`≤` is inclusive).
- `v == max+1` (integer) / `v` just above `max` (number) → one
  `ValidationError` whose reason names the bound and value
  (`must be <= 10, got 11`).
- `v < max` → OK.
- Combined with other failing assertions (`minimum`, `multipleOf`, a
  failing sibling field) → **all** reported in one shot (**P11**).
- Serialize of an in-memory value past `max` → rejected before emit
  (**P12**), not silently written.

## Interactions

- **[[minimum]]**: the paired lower bound over the same value. `minimum >
  maximum` is a load error; `minimum == maximum` pins a single value
  (accepted — a numeric near-`const`).
- **[[exclusiveMaximum]]**: same-axis (both upper) — `maximum` and
  `exclusiveMaximum` on one node is a **load reject** (one always
  dominates; keep exactly one). See [[exclusiveMaximum]] for the strict
  operator and the draft-4 boolean-form rejection.
- **[[exclusiveMinimum]]**: different axis (a lower bound) — combines with
  `maximum` to form an interval, so co-existing is fine; only the
  emptiness case (empty interval) rejects.
- **Satisfiability (combined bounds).** The accepted set is the
  intersection of the active half-lines: `minimum` → `[m,∞)`,
  `exclusiveMinimum` → `(e,∞)`, `maximum` → `(−∞,M]`, `exclusiveMaximum` →
  `(−∞,x)`. Reject at load if the intersection is **empty**:
  - `number` field: empty iff `min > max`, or `exclusiveMin ≥ max`, or
    `min ≥ exclusiveMax`, or `exclusiveMin ≥ exclusiveMax`.
  - `integer` field: empty iff the interval contains **no integer** (e.g.
    `exclusiveMinimum:1, exclusiveMaximum:2` — nothing strictly between;
    each bound is individually well-formed but no value passes, so we
    reject at load instead). `minimum == maximum` on an integer is the
    one-value case and is fine.
- **[[multipleOf]]**: with a range present, if no multiple of the divisor
  lies in the accepted interval the schema is unsatisfiable → reject
  (detail in [[multipleOf]]).
- **[[type]]**: gates applicability — `maximum` is meaningful only for
  `integer`/`number`; a mismatch is a load reject (**P7.1**). The emitted
  type is `type`'s primitive; `maximum` never narrows it. The integer cap
  (`±(2^53−1)`) is the implicit outer bound on every `integer` field.
- **[[const]] / [[default]] / [[enum]]**: a numeric literal supplied by
  one of these on the **same node** MUST satisfy `maximum` at load time.
  This **closes**, for the numeric constraints, the cross-cutting
  "validate the literal value against constraint keywords" obligation the
  [[const]] and [[default]] specs deferred: e.g. `{type:"integer",
  maximum:5, const:7}` and `{type:"integer", maximum:5, default:9}` are
  now load rejects (the fixed/supplied value can never satisfy the field).

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native. |
| OpenAPI 3.1 | Adopts 2020-12 — `maximum` identical. Native. |
| OpenAPI 3.0 / draft-4 | `maximum` (inclusive) is identical. **But** their `exclusiveMaximum` is a *boolean* modifier of `maximum`, not a number — that rewrite is handled in [[exclusiveMaximum]]. A bare `maximum` needs no change. |
| Swagger 2.0 | Same as OAS 3.0. |

## See also

- [[minimum]] — the paired inclusive lower bound (shares this machinery).
- [[exclusiveMaximum]] — the strict (`<`) upper variant; owns the draft-4
  boolean-form rewrite.
- [[exclusiveMinimum]] — the strict (`>`) lower variant.
- [[multipleOf]] — the other numeric assertion; combines for
  satisfiability.
- [[type]] — supplies the emitted primitive and the integer cap; gates
  applicability.
- [[const]], [[default]], [[enum]] — supplied numeric literals are
  validated against `maximum` at load.
