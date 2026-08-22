# const / enum / default — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/const.md` — single fixed scalar value; closed type + value constant per language (P13.1), pure `Validate` assertion (P12), P15 synthesized names + `x-<lang>-const-name`.
- `specs/json-schema/features/enum.md` — the multi-value sibling; same closed machinery, per-member value constants, `x-<lang>-enum-names`, load-time member checks (uniqueness, homogeneity, sibling constraints).
- `specs/json-schema/features/default.md` — off-the-wire / materialize-on-read annotation; omit-unset encode adapter, Go `<Field>OrDefault()` / TS `DEFAULT_<FIELD>` / Python `_<field>` slot + property, and (per spec) *no* Java-synthesized name.

Cross-checked against `specs/json-schema/PRINCIPLES.md` (P1, P7.1, P9, P10–P13.1, P15, Python §1/§3, Java §1/§4/§5/§6, Go §1).

Method: read the loader/planning/emitter code, then drove the built `nexgen` binary over ~45 probe schemas in all four languages and compiled the Go output (`go build`) to confirm each claim. Probes live in `/tmp/nexprobe/cases`.

## Summary

- **Loader coverage is genuinely strong.** Every rejection row in all three property-testing matrices that I probed fires with an accurate, fix-it-bearing diagnostic (type mismatch, sibling-constraint violation, `const`+`default`, `const`+`enum`, `null`, composite, non-ASCII/whitespace, empty token, empty/duplicate/mixed enum, default-on-required, object/array/null default, enum-default-not-in-set, encoding fold). Load-time is not where the problems are.
- **The two P0s are in the emitters, both around *positions* and *number spellings*.** A `const`/`enum` behind the nullability pattern (`oneOf: [X, null]`) loses its assertion entirely in Go and Java while TS/Python still enforce it — a straight cross-language wire disagreement. And an integer-typed `const`/`enum` authored with a non-integral spelling (`1.0`) makes Java emit the constant `0L`.
- **Go breaks outright on `enum` + `default`** — the exact combination `enum.md` lists as accepted-positive. `<Field>OrDefault()` returns the raw primitive while the field is the closed defined type, so the generated package does not compile. No test anywhere exercises the pair without a `format`/`contentEncoding` that sidesteps the closed type.
- **Java's closed-value constants are named from the *member*, not the *value*** (`Kind.KIND`, `Status.STATUS_INACTIVE`, `Tier.TIER_1`), contradicting both spec tables (`USER`, `INACTIVE`, `V_1`) and even the showcase schema's own description text. The `V_` leading-letter rule is consequently dead code.
- **Java's nested value classes never enter any P15 namespace.** `collect_synthesized_top_level` is Go-only. Four distinct schemas I probed produce non-compiling Java that loads clean: duplicate `x-java-enum-names` targets, two members that fold together under Java's UPPER_SNAKE but were skipped because a *Go* override was present, a member named `deserializer`/`serializer`, and a member named `violation` (which shadows the imported runtime `Violation`).
- **Java also synthesizes a `get<Field>OrDefault()` the spec says it does not** (`default.md` says the default folds into the plain getter, and its P15 table says Java adds no name). It is absent from `validate_member_scope`, so `{a: default} + {aOrDefault}` emits two identical Java methods — Go correctly rejects the same schema.
- **Numeric identity is by JSON representation, not mathematical value.** `enum: [1, 1.0]` passes the uniqueness check (serde `Number` equality distinguishes int/float storage) and yields duplicate Go `switch` cases → compile error. `[5, 5.0, 5e0]` is caught only because YAML happens to fold two of the three.
- **A single-element `enum` is not normalized to `const`.** `enum.md` mandates it; the code only special-cases it in the oneOf discriminator lookup. Observable: TS emits no `<FIELD>_CONST`, Python omits the dataclass default, and TS/Python report `must be one of ["v1"], got …` where Go/Java report `must equal "v1"`.
- **A `$defs`-named scalar `const`/`enum` is rejected outright** ("must be `type: object`, a `oneOf` union, or a bare `$ref`"), so the whole "reuse the `$defs` name" branch of both specs — and `x-<lang>-const-name` on a `$defs` node — is unreachable.
- **Closedness is only expressed for direct scalar properties in Go/Java.** In array-item, typed-map-value, and nullable positions Go emits `[]string`/`map[string]string`/`*string` and Java `List<String>`/`String`, while TS/Python keep the literal union. (In the array/map case the runtime check survives; in the nullable case it does not — see P0-1.)
- **Numeric/boolean members can never be overridden**: `x-<lang>-enum-names` lookups gate on `Value::String`, so `{"1": "One"}` and `{"true": "Yes"}` are silently ignored, contradicting `enum.md`'s explicit key rules.

## Implementation divergences

### 1. **Nullable `const`/`enum` loses its assertion in Go and Java**
- Severity **P0**
- Spec: `enum.md` Interactions → *"[[nullability]]: a `null` member is rejected; a nullable enum is the [[nullability]] pattern wrapping a non-null enum. Otherwise orthogonal."*; `const.md` Interactions → *"[[nullability]]: orthogonal."*; P1 (*"a value one language rejects … must be rejected by all"*), P13.1.
- Code: `src/generator/json_schema/go.rs:2631` (the `render_validate` property loop uses the raw `property`) and `src/generator/json_schema/go.rs:2720` / `go.rs:3131` (`is_closed_value_schema(property)` — never `nullable_non_null_schema(property)`); `src/generator/json_schema/java.rs:1901-1907` (`closed_values` reads `property.const_value` / `property.enum_values` directly). Contrast `go.rs:3489` where `schema_requires_go_validation` *does* unwrap, so the gate says "validate" and the emitter emits nothing.
- Spec requires: the membership/equality predicate runs in both directions in all four languages whenever the value is present.
- Code does: TypeScript and Python emit the check; Go and Java emit **no check at all**.
- Failing input:
  ```yaml
  type: object
  properties:
    a:
      oneOf: [{ type: string, enum: [a, b] }, { type: "null" }]
  ```
  Wire `{"a":"purple"}` → TS/Python raise `must be one of ["a", "b"], got "purple"`; Go accepts (`Validate()` body is empty, `UnmarshalJSON` only calls `parseStringField`) and Java accepts (only `expected string`). Identical result for `const`, and for required+nullable.
- Confidence: **high** (generated Go/Java inspected end to end).
- Note: the same unwrap gap drops *all* sibling constraints on a nullable scalar in Go/Java (`minLength: 3` behind `oneOf:[…, null]` is enforced only in TS) — likely a shared root cause other auditors will also see.

### 2. **Java emits the wrong constant value for an integer-typed `const`/`enum` written in a non-integral spelling**
- Severity **P0**
- Spec: P1 *"`5`, `5.0`, and `5e0` are the same mathematical number"*; `const.md` Float exactness (*"an integer-valued number such as `1.0` is normalized to an integer const"*); PRINCIPLES Java §4 (`specLong` accepts `1.0`).
- Code: `src/generator/json_schema/java.rs:5184-5191`
  ```rust
  JavaType::Long => format!("{}L", value.as_i64().unwrap_or_default()),
  ```
  `serde_json::Value::Number(1.0f64).as_i64()` is `None`, so the fallback `0` is emitted.
- Spec requires: the Java value class holds the same mathematical value the other three targets compare against.
- Code does: emits `new A(0L)`, `if (value == 0L) return A;` and `throw … "must equal 1.0, got " + value`.
- Failing input: `{ "a": { "type": "integer", "const": 1.0 } }` (required). Wire `{"a":1}` → accepted by Go/TS/Python, **rejected by Java**; wire `{"a":0}` → rejected by Go/TS/Python, **accepted by Java**. Same for `enum: [1, 1.0]` → `A_1 = new A(1L)`, `A_1_0 = new A(0L)`.
- Confidence: **high** (generated Java inspected).

### 3. **Go `enum` + `default` generates code that does not compile**
- Severity **P1** (loud, but breaks the whole Go package)
- Spec: `enum.md` Property-testing matrix → Accepted (positive): `enum + default (member) | {type:"string", enum:["a","b"], default:"a"}`; `enum.md` Interactions → *"[[default]]: **compatible**"*.
- Code: `src/generator/json_schema/go.rs:2537-2585` (`render_default_accessors`) and `src/generator/json_schema/go.rs:2591-2610` (`go_default_type_and_literal`) — the return type is derived from the declared `type` and never consults `is_closed_value_schema`, while the field is `*<Model><Member>` (the closed defined type).
- Spec requires: `<Field>OrDefault()` returns the field's value or the default literal.
- Code does: emits `func (m M) AOrDefault() string { if m.A != nil { return *m.A }; return "a" }` against a `A *MA` field.
- Failing input:
  ```yaml
  type: object
  properties:
    a: { type: string, enum: [a, b], default: a }
  ```
  `go build` → `cannot use *m.A (variable of string type MA) as string value in return statement`. Identical for integer/number/boolean enums. Java handles the same schema correctly (`return a != null ? a : A.A_A;`).
- Confidence: **high** (compiled).

### 4. **Java closed-value constants are named from the member, not from the value**
- Severity **P1**
- Spec: `const.md` "Naming and encoding" table (`string → Java USER`, `integer → V_3`, `number → V_3_14`, `boolean → TRUE/FALSE`); *"in **Java** the constant is purely the encoded value with no member-derived component"*; *"**Java leading-letter guarantee.** … A token that does not start with an ASCII letter … is prefixed `V_`"*; `enum.md` "Naming and collisions" (`Palette.Color.RED`).
- Code: `src/generator/json_schema/java.rs:5208-5222`
  ```rust
  let field_upper = shouty(field_java_name);
  if values.len() == 1 { field_upper } else { format!("{field_upper}_{}", java_closed_token(value)) }
  ```
  with the comment *"the field name supplying the leading letter so no `V_` digit-guard prefix is needed"* — an acknowledged deviation.
- Spec requires: `Showcase.Kind.SHOWCASE`, `Showcase.Status.{ACTIVE,INACTIVE,PENDING}`, `Showcase.Tier.{V_1,V_2,V_3}`, `Showcase.Scale.{V_1_5,V_2_5}`.
- Code does (`samples/java/.../Showcase.java:873-1122`): `Kind.KIND`, `Status.{ACTIVE_JAVA,STATUS_INACTIVE,STATUS_PENDING}`, `Tier.{TIER_1,TIER_2,TIER_3}`, `Scale.{SCALE_1_5,SCALE_2_5}`. The `V_` prefix path does not exist anywhere in the codebase.
- Corroboration: `samples/schemas/showcase.nexusrpc.yaml:88-96` describes the enum override as renaming *"the `active` value's emitted constant to the value name plus a per-language suffix (… Java `ACTIVE_JAVA`)"* — i.e. the sample's own prose assumes the value-derived name the spec mandates.
- Confidence: **high**. Either the spec or the emitter must move; note that fixing the emitter re-opens the `V_` rule *and* makes the P15 gap in #5 worse (`"user"`/`"USER"` would fold to a bare `USER` twice).

### 5. **Java nested value classes and their constants never enter a P15 namespace → four non-compiling-Java schemas that load clean**
- Severity **P1** (loud at `javac`, but P15 mandates a load reject with a fix-it)
- Spec: **P15** *"each enters the same per-scope identifier set as the declared names and as each other. The generator runs **one collision pass** over that union"*; `enum.md` *"two members whose encodings fold to the same identifier … are a load reject"*; PRINCIPLES Java §5 (`Deserializer`/`Serializer` "sit visibly with the type they serve").
- Code: `src/parser/json_schema.rs:6966-6975` — `collect_synthesized_top_level` returns immediately for any language but Go; `src/parser/json_schema.rs:7015-7104` (`validate_member_scope`) covers members, the catch-all, Python `_<field>` and Go `<Field>OrDefault` — nothing Java-side. `src/parser/json_schema.rs:6507` (`value_has_constant_override`) is a Go-**or**-Java disjunction, and the fold check at `src/parser/json_schema.rs:3096-3117` uses Go's `to_upper_camel_case` token, so Java's UPPER_SNAKE equivalence classes are never modelled.
- Failing inputs (all four: `load ACCEPT` in Java, duplicate declarations emitted):
  1. `enum: ["x","y"]` + `x-java-enum-names: {x: SAME, y: SAME}` → `public static final A SAME` twice. (Go's mirror of this **is** rejected.)
  2. `enum: ["user-admin","user_admin"]` + `x-go-enum-names: {user_admin: UserAdminAlt}` → both members skip the fold check because a *Go* override exists, and Java emits `ROLE_USER_ADMIN` twice. **This is verbatim the schema in `src/parser/json_schema.rs:11119-11128`** (`value_constant_collision_resolved_by_enum_names_override`), which only parses it for Go.
  3. `{deserializer: {type: string, const: q}}` → `public static final class Deserializer` (value class) *and* `public static final class Deserializer extends JsonDeserializer<…>` in the same class body.
  4. `{violation: {type: string, const: q}}` → nested `class Violation` shadows the imported runtime `Violation`, so every `new Violation(path, reason)` in the same file resolves to a private 1-arg constructor.
- Confidence: **high** for (1)–(3) (duplicate declarations inspected in the generated files); **high** for (4) by Java scoping rules (not compiled).

### 6. **Java synthesizes `get<Field>OrDefault()`, which the spec says it does not, and it is outside P15**
- Severity **P1**
- Spec: `default.md` Type mapping → *"| Java | `null` field + `@JsonInclude(NON_NULL)` | **native** — the generated **getter** returns the default when the backing field is `null` (`return nickname != null ? nickname : "anon";`)"*; P15 table → *"| Java | none (default folds into the existing getter) | — | — |"*; *"Java adds no name, so it carries no default-specific collision."*
- Code: `src/generator/json_schema/java.rs:2914-2921` emits a second accessor `get<Field>OrDefault()`; `getA()` stays `@Nullable`. Nothing registers it in `validate_member_scope` (`src/parser/json_schema.rs:7015`).
- Spec requires: no synthesized Java identifier.
- Code does: emits one, and a sibling member spelled `aOrDefault` collides silently.
- Failing input:
  ```yaml
  type: object
  properties:
    a: { type: string, default: "x" }
    aOrDefault: { type: string }
  ```
  Go rejects (`member … and … OrDefault accessor both map to AOrDefault`); Java emits `public String getAOrDefault()` and `public @Nullable String getAOrDefault()` in the same class.
- Confidence: **high**. (The extra accessor is arguably *better* than the spec's design — it preserves the absent-vs-default distinction — but then the spec and the P15 pass both need updating.)

### 7. **`enum` uniqueness is by JSON representation, not mathematical value**
- Severity **P1**
- Spec: `enum.md` Loader behavior → *"**Duplicate members** → reject as redundant (the spec's SHOULD-unique tightened to MUST)"*; P1 → *"`5`, `5.0`, and `5e0` are the same mathematical number, and positive/negative zero compare equal"*.
- Code: `src/parser/json_schema.rs:3090-3097`
  ```rust
  if members[..index].contains(value) { … }
  ```
  `serde_json::Number` `PartialEq` distinguishes `PosInt(1)` from `Float(1.0)`.
- Spec requires: reject `enum: [1, 1.0]`.
- Code does: accepts. Go then emits two constants of equal value and `switch v { case A1, A1_0: }` → `duplicate case A1_0 (constant 1 of int64 type …) in expression switch` — the package does not compile. `enum: [0, -0.0]` behaves the same (Go untyped constants have no signed zero). `[5, 5.0, 5e0]` is only caught because YAML folds `5.0`/`5e0` first.
- Confidence: **high** (compiled).

### 8. **A single-element `enum` is not normalized to `const`**
- Severity **P1**
- Spec: `enum.md` Loader behavior → *"**Single-element** `enum` (`enum: ["v1"]`) → normalized to the [[const]] representation; `const` is the canonical spelling for the one-value case"*; matrix row *"Single-element → const"*; Ecosystem variance (OAS 3.0 / draft-4 const idiom).
- Code: no normalization exists. `discriminator_const` (`src/parser/json_schema.rs:3599-3610`) is the only place a one-member enum is treated as a const, and it is used solely for `oneOf` branch selection. `validate_const_enum` (`src/parser/json_schema.rs:2980`) leaves the keyword as authored.
- Spec requires: `{type: string, enum: ["v1"]}` emits exactly what `const: "v1"` emits.
- Code does: Go/Java coincidentally match (both key on `values.len() == 1`), but
  - TypeScript emits **no** `<FIELD>_CONST` module binding (`collect_ts_const_constants` gates on `property.extra.contains_key("const")`, `src/parser/json_schema.rs:7220`) and reports ``must be one of ["v1"], got …`` where `const` reports ``must equal "v1"``;
  - Python emits `tag: typing.Literal["v1"]` with **no dataclass default**, where `const` emits `tag: typing.Literal["v1"] = "v1"`, and reports the `must be one of` form.
- Failing input: `{ tag: { type: string, enum: ["v1"] } }`, required — verified in all four outputs.
- Confidence: **high**.

### 9. **A `$defs`-named scalar `const`/`enum` is rejected, making both specs' `$defs` naming branch unreachable**
- Severity **P1**
- Spec: `const.md`/`enum.md` "Naming and collisions" → *"reuse the `$defs` name when the const is a **named** definition"*; `enum.md` Interactions → *"[[ref]]: a `$defs`-named enum's synthesized type … reuses the `$defs` name and enters the same per-package namespace (P15)"*; both rejection matrices list *"a `$defs`-named const reusing an existing type name"*; `const.md` Overriding → *"a **`$defs`**-named const has no declaring member at all"* (the stated motivation for `x-<lang>-const-name` existing).
- Code: `src/parser/json_schema.rs:1374` — `"{context} must be \`type: object\`, a \`oneOf\` union, or a bare \`$ref\`"`.
- Failing input: `$defs: { Color: { type: string, enum: [red, green, blue] } }` → rejected in all four languages.
- Confidence: **high**. Either the `ref`/`properties` policy needs to admit scalar defs, or `const.md`/`enum.md` need their `$defs` paragraphs removed.

### 10. **Go and Java do not express closedness outside direct scalar properties**
- Severity **P1**
- Spec: P13.1 → *"The emitted type expresses that closedness in each language's idiom"*; `enum.md` Type mapping table (Go defined type + one const per value; Java value class), *"for **every scalar kind**"*.
- Code: `src/generator/json_schema/go.rs:1567-1660` (`render_closed_value_types`) iterates a model's `properties` only; `src/generator/json_schema/java.rs:1901` likewise builds `closed_values` from top-level properties. `src/parser/json_schema.rs:6976` (`collect_synthesized_top_level`) mirrors the same restriction.
- Spec requires: a closed type wherever the closed value set appears.
- Code does, for
  ```yaml
  tags: { type: array, items: { type: string, enum: [a, b] } }
  m:    { type: object, additionalProperties: { type: string, enum: [c, d] } }
  ```
  Go `Tags []string` / `map[string]string`, Java `List<String>` / `Map<String,String>` — no defined type, no value class, no way to name a value. TS `("a"|"b")[]` and Python `list[Literal["a","b"]]` stay closed. (The runtime membership check *is* emitted in these two positions in all four languages; the nullable position is the one that loses it — see #1.)
- Confidence: **high**.

### 11. **`x-<lang>-enum-names` silently ignores numeric and boolean members**
- Severity **P1**
- Spec: `enum.md` Overriding value constants → *"Keys are the member's canonical JSON string — the string itself for string members, the shortest round-trippable decimal for numbers, `"true"`/`"false"` for booleans."*; P7.1 (reject ambiguity loudly).
- Code: `src/parser/json_schema.rs:6487`, `src/generator/json_schema/go.rs:5602`, `src/generator/json_schema/java.rs:1616` — all three gate on `Value::String(key)`, so a `Number`/`Bool` member never matches a map entry.
- Spec requires: `enum: [1,2]` + `x-go-enum-names: {"1": One}` → Go constant `One`.
- Code does: emits `MA1`/`MA2`; the override is dropped without a diagnostic. Same for `{"true": …}`. Consequently the only escape hatch for a numeric/boolean value-constant collision does not exist.
- Related (same severity band, **P2**): an override key that matches no member (`x-go-enum-names: {zzz: Nope}`) is accepted and ignored; `x-go-const-name` on a schema with no `const` is accepted and ignored.
- Confidence: **high** (probed).

### 12. **`const`'s violation reason omits the offending value**
- Severity **P2**
- Spec: `const.md` Validator mapping (`fmt.Sprintf("must equal %q, got %q", …)`, `` `must equal "user", got ${JSON.stringify(v)}` ``) and Runtime fixtures (*"one `ValidationError` naming the expected and actual value (`must equal "user", got "admin"`)"*).
- Code: `src/generator/json_schema/go.rs:5661-5679` (`values.len() == 1` → a static string), `src/generator/json_schema/typescript.rs:1072`, `src/generator/json_schema/python.rs:2062`, `src/generator/json_schema/java.rs:5242-5253`.
- Code does: `must equal "user"` with no `, got …` — while the multi-value `enum` path *and* Java's `@JsonCreator` (`java.rs:5099-5117`) *and* Java's materialized-scalar path (`java.rs:3417`) all do append `, got …`. So Java is internally inconsistent across its two decode paths.
- Note: this behaviour is locked in by `samples/python/tests/test_showcase.py` and the Java sample tests, so the spec text and the tests disagree. Either is a one-line fix; pick one.
- Confidence: **high**.

### 13. **`propertyNames` + `enum` emits a reason that names neither the set nor the value in Go and Python**
- Severity **P2**
- Spec: `enum.md` → *"The reason string names the **expected set and the offending value** … never a bare keyword"*; `enum.md` Interactions → *"[[propertyNames]]: `enum` is reused as a **key** assertion … same closed machinery"*.
- Code: `src/generator/json_schema/go.rs:914` and `src/generator/json_schema/python.rs:1810` both emit `invalid property name %q: must equal an allowed value`. TypeScript (`typescript.rs:661`) and Java (`java.rs:668` → `render_java_closed_string_checks`) emit the informative `must be one of [...], got …`.
- Confidence: **high**.

### 14. **Number→identifier encoding keeps a lowercase `e` for exponent form**
- Severity **P2**
- Spec: `const.md` Naming and encoding → *"A magnitude that canonicalizes to exponent form encodes `e` as `E` … (`1e-7` → `Ratio1ENeg7`)"*.
- Code: `src/parser/json_schema.rs:3142-3146`, `src/generator/json_schema/go.rs:5586`, `src/generator/json_schema/java.rs:5195-5201` — all do `number.to_string().replace('-', …).replace('.', '_')` with no case fold.
- Code does: `{type: number, const: 1.0e-7}` → Go `NExpConstA1eNeg7` (spec: `…A1ENeg7`). Harmless but off-spec, and the loader's collision token and the emitters' tokens agree, so no collision escapes.
- Confidence: **high**.

### 15. **`value_has_constant_override` is a Go-or-Java disjunction, so the empty-token fix-it can be bypassed per-language**
- Severity **P2**
- Spec: `const.md` → *"**Empty or illegal** encodings are **rejected at load** by Stage 3 … with a diagnostic pointing at the `x-<lang>-const-name` override"*.
- Code: `src/parser/json_schema.rs:6507-6511`.
- Code does: `{const: "-", x-java-const-name: DASH}` (no Go override) passes the language-agnostic empty-token gate. Go then names the constant `<Type>` + `""` = the defined type's own name, and the Go P15 pass rejects with a *collision* diagnostic ("closed-value type and value constant … both map to `MA`") rather than the specified "this value cannot name a Go/Java constant; add `x-go-const-name`". Under the current Java naming (#4) the Java side is unaffected either way, so today it only costs diagnostic quality — but it becomes a real hole the moment #4 is fixed.
- Confidence: **high** (probed).

### 16. **Required + `const` is a constructor parameter in Java, not a `final`-initialized field**
- Severity **P2**
- Spec: `const.md` Serialize-side → *"`private final UserEventKind kind = UserEventKind.USER;` for required+const, getter only."*
- Code: `samples/java/.../Showcase.java:1496` — `public Showcase(Kind kind, Revision revision, Enabled enabled, …)`.
- Consequence: a caller can pass `null` (the class is `@NullMarked` but that is compile-time advisory only), so the spec's *"cannot be wrong in memory"* claim is weaker in Java than stated.
- Confidence: **high**.

### 17. **`default.md`'s `$ref`-sibling last-wins example is unreachable**
- Severity **P2**
- Spec: `default.md` Accepted (positive) → *"Differing merged defaults (last-wins) | … `{$ref:"#/$defs/X", default:"local"}` overrides X's default"*.
- Code: `src/parser/json_schema.rs:1374` rejects any scalar `$defs` entry, and an object/array `default` is rejected separately, so a `$ref` use-site `default` can never legally override a target's `default`. The `allOf` half of the same row **does** work (`allOf: [{default: "a"}, {default: "b"}]` → `"b"`, verified in all four).
- Confidence: **high**.

## Testing gaps

1. **`enum` + `default` is untested in every language** — Severity **P0-adjacent** (it hides divergence #3). Untested: the accepted-positive row `{type:"string", enum:["a","b"], default:"a"}`. Spec line: `enum.md` Property-testing matrix → "enum + default (member)". The two places that come close both sidestep the closed type: `tests/generate_java.rs:163-166` (`day`) and `tests/generate_python.rs:138-146` carry a `format: date`, which routes through `go_materialized_value` and never reaches the defined type. Where: add `enumDefault: { type: string, enum: [a, b], default: a }` and `enumIntDefault: { type: integer, enum: [1,2], default: 1 }` to `GO_WAVE3_MATRIX_SCHEMA` (`tests/generate_go.rs:112`) — the test already runs `go build`/`go test`, so it fails immediately.
2. **Nullable `const`/`enum` runtime behaviour is untested anywhere** — Severity **P0-adjacent** (hides divergence #1). Spec line: `enum.md` Interactions "[[nullability]] … Otherwise orthogonal". Where: a new conformance case in `samples/conformance/json-schema.json` with a `parse_failures` entry, plus a showcase property `maybeStatus: {oneOf: [{type: string, enum: [active, inactive]}, {type: "null"}]}` and a rejection fixture for `"purple"` in all four sample suites.
3. **Integer-typed `const`/`enum` authored as `1.0`/`1e0` is untested** — Severity **P0-adjacent** (hides divergence #2). Spec line: P1 "`5`, `5.0`, and `5e0` are the same mathematical number"; `const.md` "an integer-valued number such as `1.0` is normalized to an integer const". The existing `mathematical-number-equality` conformance case covers *wire* spellings, never *schema-literal* spellings. Where: `samples/conformance/json-schema.json` — extend the case with a schema-side `const: 1.0` on an `integer` member, or add a Rust unit test asserting the emitted Java literal.
4. **`enum` members that are mathematically equal but lexically distinct** — Severity **P1** (hides divergence #7). Untested: `enum: [1, 1.0]`, `enum: [0, -0.0]`. Spec line: `enum.md` "Duplicate members → reject". Where: `src/parser/json_schema.rs` next to `rejects_duplicate_enum_members` (line 9410).
5. **Java value-class constant naming has no assertion at all** — Severity **P1** (hides divergence #4, and the samples lock in the divergent names). Spec line: `const.md` "Naming and encoding" table + the `V_` leading-letter rule. Where: `tests/generate_java.rs` — a rendered-output assertion on `Status.INACTIVE` / `Tier.V_1` / `Scale.V_1_5`.
6. **Java-side P15 for nested value classes** — Severity **P1** (hides divergence #5, four distinct non-compiling schemas). Spec line: P15 "one collision pass over that union". Where: `src/parser/json_schema.rs` tests, mirroring `value_constant_collision_resolved_by_enum_names_override` (line 11099) but `reject_for(Language::Java, …)`; add cases for duplicate `x-java-enum-names` targets, a member named `deserializer`, and a member named `violation`.
7. **Java `get<Field>OrDefault` P15 collision** — Severity **P1** (hides divergence #6). Spec line: `default.md` P15 table. Where: `src/parser/json_schema.rs` beside `rejects_or_default_accessor_colliding_with_member_go` (line 11379) — the identical schema under `Language::Java`.
8. **Single-element `enum` normalization** — Severity **P1** (hides divergence #8). Spec line: `enum.md` "Single-element `enum` … → normalized to the [[const]] representation". Where: `tests/generate_typescript.rs` (assert a `TAG_CONST` binding is emitted) and `tests/generate_python.rs` (assert the dataclass default).
9. **P9 "explicitly set to the default value stays on the wire" is only tested in Python** — Severity **P1**. Spec line: `default.md` Runtime fixtures → "Member **explicitly set to the default value** → marked set, **emitted** (no deep-equals strip)". Covered at `samples/python/tests/test_chat.py:110-115` (set-to-default emits, `del` restores unset). No Go, TypeScript or Java equivalent. Where: `samples/go/tests/json_schema_chat_test.go`, `samples/typescript/tests/json-schema-chat.test.ts`, `samples/java/.../JsonSchemaRoundTripTest.java` — set `priority` to `0` (its default) and assert the key is present in the re-marshalled bytes.
10. **Numeric/boolean `x-<lang>-enum-names`** — Severity **P1** (hides divergence #11). Spec line: `enum.md` "the shortest round-trippable decimal for numbers, `"true"`/`"false"` for booleans". Where: `src/parser/json_schema.rs` + `tests/generate_go.rs` rendered-output assertion.
11. **Closedness in array-item / typed-map-value positions for Go and Java** — Severity **P1** (hides divergence #10). Spec line: P13.1 "The emitted type expresses that closedness in each language's idiom". Where: `tests/generate_go.rs` / `tests/generate_java.rs` — assert a defined type / value class for `items: {enum: […]}`.
12. **`$defs`-named `const`/`enum`** — Severity **P1** (hides divergence #9). Spec line: `enum.md` Interactions "[[ref]]". Where: a `src/parser/json_schema.rs` test asserting the *current* reject with a spec cross-reference, or the feature.
13. **`x-<lang>-const-name` rescuing an unencodable value end to end** — Severity **P2**. Spec line: `const.md` Accepted (positive) → `{const:"-", x-go-const-name:"Dash", x-java-const-name:"DASH"}`, and Runtime fixtures → *"wire value `"-"` round-trips … the override renames the constant, not the compared value"*. `rejects_unencodable_const_value` (line 9398) covers only the reject half. Where: `tests/generate_go.rs`/`tests/generate_java.rs`.
14. **`const`/`enum` of every scalar kind in the Python and Java integration matrices** — Severity **P2**. Go (`tests/generate_go.rs:112`) and TypeScript (`tests/generate_typescript.rs:265`) both carry a full `constString/constInteger/constNumber/constBoolean/zeroConst/enumString/enumInteger/enumNumber/enumBoolean` matrix; the Python and Java matrices carry only `enum:[0]`, `enum:[true]`, and format/contentEncoding-flavoured consts. Where: port the Wave-3 matrix into `tests/generate_python.rs` and `tests/generate_java.rs`.
15. **Serialize-side mutation of an optional `const`/`enum`** — Severity **P2**. Spec line: `const.md`/`enum.md` Runtime fixtures → "Serialize after mutating an optional+const to a wrong value → rejected before emit (**P12**)". `tests/generate_go.rs:2716` mutates a **required** const only; no optional-field mutation test in any language, and no TS/Python/Java serialize-side mutation test at all.
16. **Go zero-value bypassed required const** — Severity **P2**. Spec line: `const.md` Runtime fixtures → "Serialize of a Go zero-value / bypassed required const (`Kind == UserEventKind("")`) → rejected **loudly**". Only the wrong-non-zero-value case is tested.
17. **Non-ASCII / whitespace `enum` **member**** — Severity **P2**. `rejects_non_ascii_const` / `rejects_whitespace_const` (lines 9422, 9428) cover `const` only; the `enum` rows of the matrix share the code path but have no test.
18. **No cross-language conformance case for closed value sets or defaults** — Severity **P2**. `samples/conformance/json-schema.json` has 4 cases; none exercises `const`/`enum`/`default`. Spec line: P1 (identical accepted-and-rejected value set across targets) — the manifest is the only place that is checked mechanically across all four. Where: add a `closed-value-sets` case (parse_failures for an off-set value, including behind nullability) and a `scalar-defaults` case (absent→omitted, explicit-default→emitted).
19. **Go/Python `propertyNames` + `enum` reason text** — Severity **P2** (hides divergence #13). Where: `tests/generate_go.rs` / `tests/generate_python.rs` rendered-output assertions.

## Combination gaps

| Feature A × Feature B | spec says | tested? | risk |
|---|---|---|---|
| `enum` × `default` | enum.md: accepted-positive, default must be a member | **no** (only with `format`/`contentEncoding`, which bypasses the closed type) | **Go does not compile** (div. #3) |
| `const`/`enum` × nullability | enum.md/const.md: orthogonal; the nullability pattern wraps a non-null enum | **no** | **Go+Java emit no assertion** (div. #1) |
| `const`/`enum` × `type: integer` with a `1.0` literal | P1: `1`/`1.0`/`5e0` are one number | **no** | **Java constant = `0L`** (div. #2) |
| `enum` × mathematically-equal members | enum.md: duplicates reject | **no** | Go duplicate `switch` case → compile error (div. #7) |
| `enum` × `x-<lang>-enum-names` × Java fold | enum.md: folding members reject; overrides participate in P15 | Go only (`src/parser/json_schema.rs:11099`) | **duplicate Java constant** from the repo's own test schema (div. #5.2) |
| `const`/`enum` × Java nested-class namespace (`Deserializer`/`Serializer`/`Violation`) | P15 + Java §5 | **no** | non-compiling / shadowed Java (div. #5.3, #5.4) |
| `default` × Java member namespace | default.md: Java synthesizes no name | **no** | duplicate `get<X>OrDefault()` (div. #6) |
| `const`/`enum` × array `items` / typed-map values | P13.1: closed type per language | runtime check only | Go `[]string` / Java `List<String>` (div. #10) |
| `const`/`enum` × `$defs` (named definition) | const.md/enum.md: reuse the `$defs` name | **no** | feature rejected at load (div. #9) |
| single-element `enum` × `const` normalization | enum.md: normalize to `const` | **no** | TS/Python emit different type + reason than `const` (div. #8) |
| `default` × explicit-set-to-default (P9) | default.md: emitted, no deep-equals | Python only | Go/TS/Java unverified |
| `default` × `allOf` last-wins | default.md: last-merged wins | rendered-output only (`d_allof` probe; no repo test found) | low — verified correct in all four |
| `default` × `$ref` use-site sibling | default.md: overrides the target's default | n/a | path unreachable (div. #17) |
| `default` × nullability | default.md: composes; default applies to absence | Go/TS wave-3 (`optionalNullableDefault`), Python/Java matrices | good |
| `const`/`enum` × `oneOf` discriminator | const.md: `const` is the selector | showcase (`Circle`/`Square`/`TextNote`/`LinkNote`) + `discriminator_const` tests | good |
| `enum` × `propertyNames` | enum.md: key assertion, same machinery | all four matrices | reason text bare in Go/Python (div. #13) |
| `const`/`enum` × `contains` | enum.md: sibling constraints checked per member | Go/TS/Python/Java matrices | good |
| `const`/`enum`/`default` × sibling constraints at load | each keyword owns its half | thorough (`rejects_const_violating_bound`, `…_pattern`, `…_format`, `…_content_encoding`, and enum/default twins) | good |
| `const`/`enum` × `format`/`contentEncoding` materialization | const.md: value must satisfy the format | Python/Java matrices + `accepts_materializable_temporal_const_literals` | good |
| `const` × `x-<lang>-name` (type moves with member) | P15: a name synthesized from a member moves with it | `tests/…` + probe (`YOverrideCategory`, `Category`, `CAT_CONST`) | good |
| TS `DEFAULT_<FIELD>` / `<FIELD>_CONST` × model-name qualification | default.md/const.md P15 | `default_constant_collision_resolved_by_override`, `const_constant_collision_rejects_and_is_resolved_by_override` | good |

## Verified-good

- Every load-time rejection I probed from all three matrices fires with the specified diagnostic: `const` type mismatch / `minLength` / `minimum` / `multipleOf` / `pattern`, `const: null`, composite `const`, non-ASCII, whitespace, empty token, `const`+`default`, `const`+`enum`; empty `enum`, duplicate members, mixed types, `null` member, composite member, encoding fold (`"user"`/`"USER"` → `User`), default-not-in-set; `default` on required, `default` type mismatch, `default` violating a constraint, object/array `default`, `default: null`.
- Go closed-value emission for direct scalar properties: defined type + one typed constant per value for string / integer / number / boolean, each with the mandated name-led doc comment, and the fallback doc line (`… is the closed value set for M.f.`) when no `title`/`description` (`samples/go/showcase/showcase.go:51-109`).
- TypeScript closed literal / literal union for all four scalar kinds, plus the module-scope `<FIELD>_CONST` binding (`samples/typescript/showcase/models.ts:6-12, 209-238`).
- Python `Literal[…]` for string/integer/boolean, plain `float` for the number case (PEP 586 exception), and the `const` carried as the dataclass default including for floats (`a: float = 1.5`) — `samples/python/showcase/models.py:3655-3682`.
- Python default machinery: private `_<field>` slot with `repr=False`, materializing property, setter, deleter, and `to_transfer_type` reading the slot (never the property) — `samples/python/showcase/models.py:3754-3768, 4133-4162, 3133-3140`; round-tripped in `samples/python/tests/test_chat.py:88-126` including the P9 set-to-default case.
- Go `<Field>OrDefault()` and TS `DEFAULT_<FIELD>` for the plain-scalar case, including on optional+nullable members (`tests/generate_go.rs:2660-2666`).
- P15 collision passes that *are* implemented all work end to end and reject with actionable fix-its: Go closed-value type vs. a declared top-level type; Go value constant vs. value constant (incl. via overrides); Go `<Field>OrDefault` vs. a declared member; TS `DEFAULT_<FIELD>` and `<FIELD>_CONST` qualification and clash; Python `_<field>` backing slot vs. an `x-py-name`-renamed sibling.
- `x-go-name`/`x-java-name`/`x-ts-name`/`x-py-name` correctly move the synthesized closed-value type, the Java nested class, and the TS `_CONST`/`DEFAULT_` bindings with the member.
- `x-go-const-name` / `x-java-const-name` / `x-go-enum-names` / `x-java-enum-names` reach the emitted identifier for **string** members, and leave the compared wire value untouched (`samples/go/showcase/showcase.go:82`, `samples/java/.../Showcase.java:922, 1012`).
- `allOf` last-wins for differing `default`s (`allOf: [{default: a}, {default: b}]` → `"b"` in all four).
- Mathematical number equality on the **wire** for closed values: `zeroConst: {type:number, const:0}` accepts `-0` and preserves the sign bit in memory (`tests/generate_go.rs:2669`), and `enum` numeric members accept `5e0` spellings.
- Java `@JsonCreator` fail-fast factory vs. the collecting deserializer's non-throwing lookup, per PRINCIPLES Java §5 (`src/generator/json_schema/java.rs:5023-5070`, `5267+`).
- `const`/`enum` inside `oneOf` sum-type branches (not the nullable collapse) is enforced in all four languages (`samples/…/showcase` `mode`, `shapeOrName`).
