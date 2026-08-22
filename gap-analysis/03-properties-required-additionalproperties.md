# properties / required / additionalProperties — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/properties.md` — typed structs, the 4-stage identifier
  case-mapping pipeline, `x-<lang>-name` overrides, P15 collision policy, inline-object
  hoisting to `<Model><Property>`, per-member validator dispatch.
- `specs/json-schema/features/required.md` — presence-only assertion, loader grammar,
  required-vs-optional emitted forms, required+nullable, serialize-side presence check.
- `specs/json-schema/features/additionalProperties.md` — open-by-default, closed
  (`false`), open (`true`), typed extras, typed maps, the always-named catch-all member,
  per-member `T` validation, unknown-key preservation (P13.2).
- Cross-cutting: `specs/json-schema/PRINCIPLES.md` P1, P7/P7.1, P8, P9, P10–P12, P13,
  P15, plus the TypeScript §2/§4, Python §1/§3, Java §3/§5/§6 and Go §1 sections.

Method: read the three specs + PRINCIPLES, read the loader (`src/parser/json_schema.rs`),
the P15 name manifest, and all four emitters; then drove the built `nexgen` binary over
~30 hand-written schemas and diffed the four languages' output. Every finding below was
reproduced unless explicitly labelled unverified.

## Summary

- **TypeScript emits a syntax error** for the spec's own "closed empty object" positive
  case (`{type:object, additionalProperties:false}`): `if () {`. No test or sample in the
  repo uses that shape, which is why it shipped.
- **TypeScript cannot honour P13.2's "preserved verbatim"** for untyped extras: the
  Temporal converter hands `fromTransferType` an already-`JSON.parse`d value, so
  `9007199254740993` comes back as `…992` where Go (`json.RawMessage`), Java (`JsonNode`)
  and Python (`int`) preserve it. The spec asserts the opposite in prose, and the wire
  fixtures deliberately use `9007199254740992` (safe), so nothing catches it.
- **P15's namespace union is incomplete in two languages.** Go's per-model method set
  (`Validate`) and Java's fixed nested classes (`Deserializer`/`Serializer`) are never
  entered into any namespace, so a member named `validate` (Go) or a `const`/`enum` member
  named `deserializer` (Java) silently generates uncompilable code.
- **Go's untyped open struct is the only one of the four that does not reject a catch-all
  key colliding with a declared property on serialize** — it silently drops the extra.
  The typed-extras path in the same generator does check.
- **The spec's own positive matrix row for `x-py-name`/`x-java-name` on a `class` member
  fails under `nexgen typescript`**: the loader treats every TS keyword as a Stage-3
  rejection, contradicting properties.md §Stage 3/§Stage 4.
- **The serialize-side required-presence check (required.md §Serialize side) exists in no
  target**, and the four disagree on what a missing required value produces on the wire
  (Go/Python emit `null`, Java/TS omit the key).
- **Java does not re-path nested violations on serialize** (`address.zip` → `zip`),
  unlike Go/TS/Python.
- **Stage-1 segmentation is untested end-to-end.** No sample schema or integration test
  uses a JSON member name containing `_`, `-`, a space, or an acronym run — the shared
  segmentation core that makes "one algorithm for all four emitters" true is asserted
  nowhere across languages.
- **Load-time identifier rejection is only ever tested under Python** (`parse`/`doc_reject`
  hard-code `Language::Python`), so the per-target Stage-3 rule is largely unverified.
- **The cross-language conformance manifest (4 cases) contains no object-modeling case at
  all** — no unknown-key preservation, no closed-model rejection, no typed extras, no
  missing-required.

Everything else I probed in the loader — the `required` grammar matrix, the `properties`
reject matrix, the hoist-naming table, inline-vs-`$defs` code identity, the four catch-all
representations, nullable typed maps — matches the specs exactly (see Verified-good).

## Implementation divergences

### 1. **TypeScript emits `if () {` for a closed empty object**
- Severity **P0**
- Spec: `additionalProperties.md:299` (positive row "Closed empty object |
  `{type:object, additionalProperties:false}`") and `:171` (type-mapping row "empty
  `interface`; any member → error").
- Code: `src/generator/json_schema/typescript.rs:3821-3835`
  (`render_closed_object_unknown_key_check`) builds the condition by joining one
  `key !== "<name>"` term per declared field with `" && "`. With zero declared fields the
  join is the empty string, so the emitted line is literally `if () {`.
- Spec requires: a closed object with no `properties` rejects every member.
- Code does: emits TypeScript that does not parse. `tsc`/`node` fail on the model module,
  taking the whole generated package with them.
- Failing input:
  ```yaml
  $schema: https://json-schema.org/draft/2020-12/schema
  type: object
  additionalProperties: false
  ```
  → `models.ts`:
  ```ts
    for (const key of Object.keys(raw)) {
      if () {
        violations.push({ path: key, reason: 'unknown field' });
      }
    }
  ```
  Go, Python and Java all emit correct code for the same input.
- Confidence: **high** (reproduced with the built binary).

### 2. **TypeScript untyped extras lose integer precision the other three preserve**
- Severity **P0** (cross-language wire disagreement)
- Spec: `additionalProperties.md:185-214` ("Why `json.RawMessage`, not `any`") states the
  hazard is Go-specific and explicitly claims "TS `unknown` / Python `Any` / Java `Object`
  don't share the `float64` hazard: their parsers keep an exact numeric/JSON-node
  representation". Also P13.2(b) ("preserved verbatim") and P1.
- Code: `src/generator/json_schema/typescript.rs:3890` lifts `raw[key]` straight into
  `Record<string, unknown>`; `raw` is the value the Temporal converter already produced
  with `JSON.parse` (PRINCIPLES TypeScript §4 places the byte boundary outside the
  converter). By contrast Go keeps `map[string]json.RawMessage`
  (`src/generator/json_schema/go.rs`, untyped branch), Java `Map<String, JsonNode>`, and
  Python the exact `int` from `json.loads`.
- Spec requires: an unmodeled extra round-trips byte/value-faithfully in every language.
- Code does: in TypeScript only, `{"big": 9007199254740993}` on an open struct
  deserializes and re-serializes as `9007199254740992` — a different mathematical value,
  which P1's number-spelling exemption does *not* cover.
- Failing input: any open struct + `{"big": 9007199254740993}`. Verified the JS behaviour
  directly: `node -e 'console.log(JSON.stringify(JSON.parse("{\"big\":9007199254740993}")))'`
  → `{"big":9007199254740992}`.
- Note: the existing fixtures dodge this — `samples/wire/json_schema/showcase/extras.json:3`
  and `showcase-freeform.json:14` both use `9007199254740992`, which is exactly
  representable.
- Confidence: **high** for the divergence; the claim that TS *cannot* fix it without
  owning the JSON parse step is an inference from PRINCIPLES TS §4 (unverified as a
  design constraint).

### 3. **Go silently drops a catch-all key that collides with a declared property (untyped extras only)**
- Severity **P1**
- Spec: `additionalProperties.md:281-285` ("the named-catch-all split keeps the two
  namespaces unambiguous in both directions"); P1 (identical behaviour across targets).
- Code: `src/generator/json_schema/go.rs:2937` gates the collision check on
  `additional_shape.is_some()`; `additional_shape` is computed at `go.rs:2408` from
  `go_typed_additional_properties_shape` (`go.rs:5032`), which returns `None` for an
  untyped catch-all (`additionalProperties` omitted or `true`). The other three emit the
  check unconditionally: `typescript.rs:3056`, `python.rs:3730`, `java.rs:3477`.
- Spec requires: the same in-memory model must serialize the same way in all four.
- Code does: with `{type:object, properties:{id:{type:integer}}}` and
  `AdditionalProperties["id"] = json.RawMessage("5")` in memory, Go's `MarshalJSON`
  succeeds — it writes the extras into `out` first and then `marshalField` overwrites
  `"id"` with the declared value, silently discarding the extra. TypeScript, Python and
  Java all raise `ValidationError`/`ValidationException` on the identical model.
- Failing input: as above.
- Confidence: **high** (read from generated Go for both the untyped and typed cases; the
  typed case *does* emit `Violation{"id", "catch-all key collides with declared property"}`).

### 4. **Java's synthesized nested value class can collide with the generated `Deserializer`/`Serializer`**
- Severity **P1**
- Spec: PRINCIPLES Java §5 ("emitted as `public static final` nested classes
  (`User.Deserializer`/`User.Serializer`) — so the names never collide across models (no
  P15 involvement)"); `properties.md:162-186` (a synthesized type "enters the **same
  per-scope namespace** as the declared names and each other"); P15.
- Code: `src/generator/json_schema/java.rs:1933` names a closed-value class
  `upper_first(&java_name)` and `java.rs:1915` names an inline-union interface
  `json_name.to_upper_camel_case()`. The P15 pass never sees them:
  `src/parser/json_schema.rs:6966-6975` (`collect_synthesized_top_level`) returns early
  for every language except Go, and `validate_member_scope`
  (`src/parser/json_schema.rs:7018-7104`) enters only declared members, the catch-all,
  Python's `_<field>` and Go's `<Field>OrDefault`.
- Spec requires: a coincidence with a generated identifier in the same scope is a load
  reject with a fix-it, never silent output.
- Code does: emits a `.java` file declaring `Deserializer` and `Serializer` twice.
- Failing input:
  ```yaml
  $schema: https://json-schema.org/draft/2020-12/schema
  type: object
  properties:
    deserializer: { type: string, const: x }
    serializer: { type: string, enum: [a, b] }
  ```
  → `Jclash.java` lines 25/213 both declare `public static final class Deserializer`, and
  73/175 both declare `Serializer`. Loads without a diagnostic.
- Confidence: **high** (reproduced).

### 5. **A Go struct field can collide with the generated `Validate` method**
- Severity **P1**
- Spec: P15 ("A **scope** is whatever unit the target actually resolves names in … the
  struct method-set for the Go accessor"); `properties.md:167-171` lists the Go method set
  as one of the per-scope namespaces the single collision pass runs over.
- Code: `src/parser/json_schema.rs:7018-7104` (`validate_member_scope`) enters declared
  members, the catch-all, and `<Field>OrDefault` — but never the fixed methods
  `Validate` / `UnmarshalJSON` / `MarshalJSON` that `go.rs` always emits on every model.
- Spec requires: load reject + fix-it.
- Code does: emits `type Goclash struct { Validate *string … }` alongside
  `func (m Goclash) Validate() error`, which is `type Goclash has both field and method
  named Validate` — uncompilable, no diagnostic.
- Failing input: `{type:object, properties:{validate:{type:string}}}`, target Go.
  (`marshalJSON`/`unmarshalJSON` fold to `MarshalJson`/`UnmarshalJson` under the
  acronym-folding rule, so only `validate` — and case variants of it — trips this.)
- Confidence: **high** (reproduced).

### 6. **TypeScript rejects keyword-named members; the spec says it must not**
- Severity **P1**
- Spec: `properties.md:140-144` ("Go's exported `PascalCase` never collides with Go's
  all-lowercase keywords, and TS interface members permit keywords, so **Python** … and
  **Java** … are the targets that actually hit Stage-3 rejections"); `:151-153` ("a `class`
  member needs `x-py-name` + `x-java-name`; Go/TS need nothing"); positive matrix row
  `:351`.
- Code: `src/parser/json_schema.rs:6416-6430` (`member_identifier_defect`) computes the
  lower-camel base for TypeScript and then calls `ident_is_reserved(Language::TypeScript,
  &base)` (`:6229-6277`, a 40-word keyword list), returning a Stage-3 defect.
  `validate_member_scope` (`:7026-7037`) turns that into a load error.
- Spec requires: the schema in the positive matrix loads for all four targets.
- Code does:
  ```
  $ nexgen typescript --output out class.yaml
  invalid JSON schema in `<json-schema>`: member `Class.class` recases to `class`,
  which is a reserved word in typescript output; add an `x-ts-name` override …
  ```
  for the spec's verbatim example
  `{properties:{class:{type:string, x-py-name:"klass", x-java-name:"klazz"}}}`.
  Go accepts it; Python and Java accept it (their overrides are present).
- Note: Stage 3's normative sentence *does* say "equals a reserved word in that language",
  so the spec is internally inconsistent. One of the two has to move; today the
  spec-documented escape-hatch story is wrong in practice.
- Confidence: **high** (reproduced).

### 7. **The serialize-side required-presence check does not exist in any target, and the four disagree on the resulting wire**
- Severity **P1**
- Spec: `required.md:107-115` — "a required member that is empty in memory (Go `nil`
  pointer · TS `undefined` · Python `None` · Java `null` reference) is a `ValidationError`,
  so `MarshalJSON`/`toTransferType`/`to_transfer_type` fails rather than emitting a
  malformed object. A required member is therefore **never omitted** on serialize".
  P12 explicitly notes in-memory construction is unchecked, which is what gives this
  check its teeth.
- Code: no generator emits it.
  - Go: `marshalField(out, "tags", m.Tags, &errs)` — a `nil` required slice/map marshals
    to `null` (`marshalField` in the generated `definitions.go` just calls
    `json.Marshal`).
  - TypeScript: `out.tags = value.tags;` — `undefined` disappears at `JSON.stringify`.
  - Python: `out["tags"] = value.tags` — `None` becomes `null`.
  - Java: `src/generator/json_schema/java.rs:3600-3612` wraps every field write in
    `if (value.<f> != null) { … }`, so a null required reference is **omitted**.
- Spec requires: one aggregated validation failure before a byte is written.
- Code does: no failure, and two different wire shapes — Go/Python produce
  `{"tags":null}`, Java/TypeScript produce `{}`. Both are then rejected by any
  deserializer, but with different reasons (`explicit null not allowed` vs `required`),
  so the round-trip contract diverges by target.
- Failing input: Go `Reqref{Inner: inner, Name: "x"}` with `Tags` left nil for
  `required: [inner, tags, name]`.
- Confidence: **high** for Go/TS/Python/Java code paths (all read from generated output).

### 8. **Java does not re-path a nested model's violations on serialize**
- Severity **P1**
- Spec: `properties.md:333` — "`path` on a serialize-side failure is the JSON member name,
  identical to deserialize"; P11 (the structured `{path, reason}` shape is the
  cross-language contract).
- Code: `src/generator/json_schema/java.rs:3608` emits
  `gen.writeFieldName(<json>); serializers.defaultSerializeValue(<accessor>, gen);`
  with **no** `try`/`catch`. The deserialize side for the same `JavaType::Ref`
  (`java.rs:3964-3983`) *does* wrap and call `violation.withPathPrefix(<json>)`. Go
  (`mergeNested`), TypeScript (`__nexgenDefinitions.collect(violations, "address", error)`)
  and Python (`_collect(violations, "address", error)`) all prefix on serialize.
- Spec requires: `address.zip` on both directions.
- Code does: the nested `ValidationException` propagates verbatim with path `zip`, and it
  is not merged with the parent's other violations (P11 aggregation is lost for that
  payload). Same omission for a nested model inside the catch-all (`java.rs:4954`).
- Failing input: `{$defs:{Addr:{type:object, properties:{zip:{type:string, minLength:5}}}},
  type:object, properties:{address:{$ref:"#/$defs/Addr"}}}`; serialize an `Addr` with
  `zip = "1"`.
- Confidence: **high** (read from generated Java + the emitter source).

### 9. **Java untyped catch-all is `Map<String, JsonNode>`, not the spec's `Map<String, Object>`**
- Severity **P2**
- Spec: `additionalProperties.md:166` and `:169` both say `Map<String,Object>`;
  `:212-214` reasons about "Java `Object`".
- Code: generated `Open.java` declares `private final Map<String, JsonNode>
  additionalProperties;` (emitter: `src/generator/json_schema/java.rs`, verified in the
  `render_model_file` output).
- The code is *better* than the spec here (a `JsonNode` is exactly the Java analogue of
  Go's `json.RawMessage`), so the spec table is stale, not the code. Worth fixing the spec
  and folding the reasoning into the "Why `json.RawMessage`" section.
- Confidence: **high**.

### 10. **Closed-mode violation reason text does not match the spec's Go/Java rows**
- Severity **P2**
- Spec: `additionalProperties.md:223` (Go: `fmt.Sprintf("unknown property %q", key)`) and
  `:226` (Java: `"unknown property \"" + key + "\""`); Python's row says `"unknown field"`.
- Code: all four emit `unknown field` (`go.rs`, `typescript.rs:3832`, `python.rs`,
  `java.rs`). Consistency across targets is right (P11 says reason text is not part of the
  contract); the spec rows are stale. Note the repo convention that reasons should name
  the offending value — the key is carried in `path`, so this is arguably fine.
- Confidence: **high**.

### 11. **Python closed models inline the key comparison instead of `_<MODEL>_DECLARED`**
- Severity **P2**
- Spec: `additionalProperties.md:225` — "check parsed keys against `_<MODEL>_DECLARED`".
- Code: a closed model emits `if key != "id" and key != "name":` and no frozenset; the
  frozenset is emitted only for open models (which is also why the P15 pass gates its
  insert on `python_open_object`, `src/parser/json_schema.rs:7290-7297`).
- Confidence: **high**.

### 12. **`properties: true` / `false` / `[]` produce raw serde errors, not the located P7.1 diagnostic**
- Severity **P2**
- Spec: `properties.md:57-63` — "A member schema that is empty `{}` / `true` / `false` →
  reject per **P7.1**. Diagnostic names the member and asks for an explicit `type`", and
  "`properties` value not an object → reject".
- Code: only the `{}` case reaches the located check
  (`root schema.properties.a: a leaf schema requires an explicit type; …`). `a: true`,
  `a: false`, `a: 5` and `properties: []` fail earlier in serde with
  `failed to parse JSON schema from '<file>': invalid type: boolean 'true', expected
  struct Schema` — no member name, no fix-it.
- Confidence: **high** (reproduced for all four spellings).

### 13. **TypeScript's `<MODEL>_DECLARED` module-scope const is not in the P15 namespace**
- Severity **P2**
- Spec: P15 (every synthesized module-scope identifier joins the one collision pass);
  the Python emitter's counterpart *is* registered
  (`src/parser/json_schema.rs:7290-7297`, with a comment explaining that
  `to_shouty_snake_case` is non-injective over verbatim overrides —
  "`ContactPy` and `ContactPY` both shout to `CONTACT_PY`").
- Code: `build_name_manifest` registers TS `DEFAULT_*`, `*_CONST` and the transfer-type
  converters (`src/parser/json_schema.rs:6836-6844`) but never `<MODEL>_DECLARED`, which
  the TS emitter binds at module scope for every open model.
- I could not construct an input that escapes — the lower-camel converter name
  (`contactPyTransferTypeConverter`) folds the same pairs and rejects first. Labelled
  **unverified** as an exploitable bug; it is a real asymmetry with Python and a latent
  hazard if the converter naming ever changes.
- Confidence: **medium** (asymmetry confirmed by reading; no failing input found).

## Testing gaps

### 1. **Nothing anywhere uses a closed empty object**
- Severity **P0** (this is why divergence #1 shipped)
- Untested: `{type:object, additionalProperties:false}` with no `properties` — the
  "Closed empty object" positive row and its type-mapping row.
- Spec line: `additionalProperties.md:299` and `:171`.
- Where: `samples/schemas/` (a new `$defs` entry in `showcase.nexusrpc.yaml`) + all four
  round-trip suites + a render assertion in `tests/generate_typescript.rs`.
- Case: load it, assert the emitted TS parses, and assert `{}` round-trips while `{"a":1}`
  yields one `Violation{path:"a"}`.
- Verified absent: no occurrence of `additionalProperties: false` without a sibling
  `properties` in `samples/schemas/*.yaml` or `tests/generate_*.rs`.

### 2. **Go and TypeScript round-trip suites never assert closed-model extra-key rejection**
- Severity **P1**
- Untested: "Closed struct + extra key → one `ValidationError` per extra, aggregated with
  declared-field errors."
- Spec line: `additionalProperties.md:315-316`.
- Where: `samples/go/tests/json_schema_chat_test.go` (chat's models are all closed) and
  `samples/typescript/tests/json-schema-chat.test.ts`.
- Case: the mirror of `samples/python/tests/test_chat.py:70-78` and
  `samples/java/.../JsonSchemaRoundTripTest.java:181-187`.
- Verified absent: `rg "unknown field" samples/go/tests samples/typescript/tests` → no
  matches (Python and Java both have them).

### 3. **The conformance manifest has no object-modeling case**
- Severity **P1**
- Untested cross-language: unknown-key preservation (P13.2b), closed-model extra
  rejection, typed-extras per-member validation, missing-required aggregation.
- Spec line: P1 ("a value one language rejects … must be rejected by all"); the runtime
  fixtures in `properties.md:370-381`, `required.md:139-146`,
  `additionalProperties.md:315-329`.
- Where: `samples/conformance/json-schema.json` (4 cases today, all number/temporal/null).
- Case: at minimum (a) `unknown-key-preservation` — an open model with an extra key,
  `accepted_wire_values` + a byte-faithful round-trip anchor in all four; (b)
  `closed-model-unknown-key` — one `parse_failures` entry with `expected_paths: ["nope"]`;
  (c) `required-missing-aggregates` — two missing required members, both paths expected.

### 4. **Load-time identifier rejection is only exercised under Python**
- Severity **P1**
- Untested: the Stage-3 per-target rule for Go, TypeScript and Java, and the spec's
  explicit "a name that is invalid in a language you are not generating … produces no
  diagnostic".
- Spec line: `properties.md:132-144`.
- Where: `src/parser/json_schema.rs` — the `parse`/`doc_reject` helpers hard-code
  `Language::Python` (`:7456-7462`, `:7767-7775`); `rejects_reserved_member_without_override`
  (`:12116`) and `rejects_invalid_member_identifier` (`:12129`) both go through them.
- Case: loop `rejects_reserved_member_without_override` over all four targets asserting
  that Go **accepts** `{properties:{class:{type:string}}}` while Python and Java reject —
  which is what surfaces divergence #6.

### 5. **The "override admits an otherwise-rejected name" positive row has no test**
- Severity **P1**
- Untested: `{properties:{class:{type:string, x-py-name:"klass", x-java-name:"klazz"}}}`
  loading for all four targets.
- Spec line: `properties.md:351`.
- Where: `src/parser/json_schema.rs` beside `member_override_accepts_and_is_recognized_as_extension`
  (`:10865`).
- Case: `for language in [Go, TypeScript, Python, Java] { parse_for(language, input).unwrap() }`.
  It fails today (divergence #6).

### 6. **No test covers a member colliding with a generated method / nested class**
- Severity **P1**
- Untested: Go member `validate`; Java `const`/`enum` member `deserializer`/`serializer`;
  Java inline-union member of the same names.
- Spec line: `properties.md:162-186` + P15 ("the struct method-set for the Go accessor";
  PRINCIPLES Java §5's "no P15 involvement" claim).
- Where: `src/parser/json_schema.rs`, next to `rejects_or_default_accessor_colliding_with_member_go`
  (`:11379`) and `rejects_member_colliding_with_catch_all` (`:12257`).
- Case: `reject_for(Language::Go, "properties: { validate: { type: string } }")` should
  name the `Validate` method; `reject_for(Language::Java, "properties: { deserializer:
  { type: string, const: x } }")` should name the nested `Deserializer`.

### 7. **No test asserts the serialize-side required-presence check**
- Severity **P1**
- Untested: required.md's explicit "`MarshalJSON`/`toTransferType`/`to_transfer_type`
  fails rather than emitting a malformed object".
- Spec line: `required.md:107-115`.
- Where: all four round-trip suites (chat/showcase already have required members), plus a
  conformance manifest `serialize_failures` entry.
- Case: construct a model with a required reference member left nil/undefined/None/null
  and assert a `ValidationError` at `path = <member>` in each language.

### 8. **No test asserts serialize-side nested path prefixing**
- Severity **P1**
- Untested: a nested model violation raised on *serialize* producing `address.zip`.
- Spec line: `properties.md:325-333`, `:376-377`.
- Where: the four round-trip suites; today the dotted-path assertions I could find are
  deserialize-side.
- Case: mutate a decoded showcase `address.zip` to a too-short string and assert the
  serialize error's path. It fails in Java today (divergence #8).

### 9. **Untyped-extras integer fidelity is only tested at exactly 2^53**
- Severity **P1**
- Untested: 2^53+1 in a catch-all, which is the exact value the spec uses to justify
  `json.RawMessage`.
- Spec line: `additionalProperties.md:193-201`.
- Where: `samples/wire/json_schema/showcase/extras.json` (`count: 9007199254740992`) and
  `showcase-freeform.json` (`big: 9007199254740992`) — both safe values; the Go assertion
  is `samples/go/tests/json_schema_showcase_test.go:666,677`.
- Case: change the fixture to `9007199254740993` and add it to the conformance manifest.
  It fails in TypeScript today (divergence #2).

### 10. **Stage-1 segmentation is not exercised by any generated-code test**
- Severity **P1**
- Untested: separator consumption (`user_id`, `kebab-case`, `with space`), the
  "boundary before the final capital of an uppercase run" rule (`HTTPServer` →
  `HttpServer`), digit attachment (`oauth2`), and the guarantee that the **wire name is
  pinned** for such a member in all four languages.
- Spec line: `properties.md:101-118`, positive rows `:349-350`.
- Where: no `samples/schemas/*.yaml` property key contains `_`, `-`, a space or an acronym
  run (verified by grep), and no `tests/generate_*.rs` schema does either. The only
  `user_id` in the repo is inside a *rejection* test
  (`src/parser/json_schema.rs:10891`).
- Case: add a `Naming` model to `showcase.nexusrpc.yaml` with `user_id`, `httpServer`,
  `XMLHttpRequest`, `oauth2`, `kebab-case` and assert the four emitted identifiers plus
  the four preserved wire keys in the round-trip suites. (I ran this manually: all four
  agree today — `UserId`/`userId`/`user_id`, `HttpServer`, `XmlHttpRequest`, `Oauth2`,
  `KebabCase` — so this locks in behaviour rather than exposing a bug.)

### 11. **Typed extras alongside `properties`, and explicit `additionalProperties: true` alongside `properties`, are absent from the sample corpus**
- Severity **P2**
- Untested at runtime: the "Typed extras + `properties`" and "Open struct (explicit)"
  positive rows. They *are* covered by render-level assertions
  (`tests/generate_go.rs:197-206`, `tests/generate_java.rs:226-232`,
  `tests/generate_typescript.rs:352-359`, `tests/generate_python.rs:169-172`), but no
  cross-language wire fixture exercises the Go helper-struct re-marshal path or the
  declared-key collision check on serialize.
- Spec line: `additionalProperties.md:296-298`.
- Where: a new `$defs` entry in `showcase.nexusrpc.yaml` + a wire fixture.

### 12. **The `properties: true`/`false`/`[]` reject rows are untested**
- Severity **P2**
- Spec line: `properties.md:357-359`.
- Where: `src/parser/json_schema.rs` alongside the other loader rejects.
- Case: assert the diagnostic names the member (it does not today — divergence #12).

### 13. **`required` positive rows are only implicitly covered**
- Severity **P2**
- Untested explicitly: `required: []` accepted as a vacuous no-op (`required.md:125`),
  and "required nullable member absent → still `required property "<name>" is missing`"
  (`required.md:143-144`). The negative rows all have dedicated tests
  (`:11738`, `:11745`, `:11752`, `:11759`); the positive ones do not.

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| `additionalProperties:false` x no `properties` | closed empty object; any member → error (`additionalProperties.md:171,299`) | **no** — absent from samples and all `tests/generate_*.rs` | **P0**: TS emits `if () {` |
| `additionalProperties` untyped x declared-key collision on serialize | namespaces unambiguous in both directions (`:281-285`) | Go: **no**; TS/Py/Java: yes (`generate_typescript.rs:1949`, `generate_python.rs:287`, `generate_java.rs:643`) | **P1**: Go silently drops the extra |
| `additionalProperties` untyped x integers > 2^53 | preserved verbatim (`:193-201`, P13.2) | **no** — fixtures stop at 2^53 | **P0**: TS corrupts |
| `properties` x TS reserved word | Stage 3 does not fire for TS (`:140-144`) | **no** — all Stage-3 tests run under Python only | **P1**: documented positive case rejects |
| `properties` x `x-py-name`+`x-java-name` on a `class` member | accepted for all four (`:351`) | **no** | **P1**: fails under TS |
| `properties` x generated Go method set | member scope includes the method set (P15) | **no** | **P1**: `validate` → uncompilable Go |
| `const`/`enum` member x Java nested `Deserializer`/`Serializer` | synthesized type joins the same scope (`:162-186`) | **no** | **P1**: duplicate nested classes |
| `required` x serialize-side empty value | one `ValidationError`, never omitted (`required.md:107-115`) | **no** in any language | **P1**: silent `null` (Go/Py) vs silent omission (Java/TS) |
| nested model x serialize-side violation path | `path` identical to deserialize (`properties.md:333`) | **no** | **P1**: Java yields `zip`, others `address.zip` |
| Stage-1 segmentation x all four emitters | one shared algorithm (`:101-118`) | **no** end-to-end test; only a rejection test uses `user_id` | **P1**: silent drift between the loader's `recase_member` and the four emitters' own field-name helpers |
| open struct x extra key preservation, both directions | P13.2(b) verbatim round-trip | per-language yes (showcase/chat); **manifest: no** | P1: no cross-language lock |
| closed struct x extra key rejection | one Violation per extra, aggregated (`:315`) | Python/Java yes; **Go/TS no** | P1 |
| typed extras (`{type:T}`) x `properties` | supported everywhere (`:298`) | render-level yes (all four); **runtime/wire: no** | P2 |
| typed extras x nullable member (`oneOf` + null) | null kept as a null member, `map[string]*T` etc. (`:260-264`) | yes — showcase `Nicknames`, all four suites | ok |
| typed map x member constraint violation, both directions | `path = key` on parse and serialize (`:318-321`) | yes — showcase `Quotas`/`Tokens` | ok |
| inline object x `$defs`+`$ref` | identical emitted code (`:246-249`) | **no explicit test**; I verified it byte-for-byte for Go/TS/Python | P2 |
| inline object x `x-<lang>-name` on the member | member moves, hoisted type keeps the position name (`:279-283`) | yes (`src/parser/json_schema.rs:10491`; showcase `ledger`) | ok |
| hoisted name x `$defs` entry / file-root type | load reject with fix-it (`:288-291`) | yes (`:11134`, `:11188`, `:11248`) | ok |
| `required` x nullable | required+nullable accepted, emits `null` never absent | yes (showcase; Go emits `out["x"] = null`) | ok |
| `required` x `default` | rejected (default.md) | yes (`:9255`) | ok |
| `additionalProperties` x `minProperties`/`maxProperties` | counts include preserved extras (`:344-345`) | yes — showcase `Extras`/`Attributes`; verified Go/TS/Py count `out`/`raw` | ok |
| declared member named `additionalProperties` x open/closed | reject when open, accept when closed (`:179-183`) | yes (`:12257`, `:12271`) | ok |

## Verified-good

Checked against the implementation and found correct **and** covered:

- The whole `required` loader matrix: non-array, object, non-string element, boolean
  element, duplicate, name-not-in-`properties`, `required: []` — each with a precise
  fix-it diagnostic (`src/parser/json_schema.rs`, tests `:11738`–`:11759`). Also rejects
  `required` on a map-shaped object with no `properties`.
- The `properties` reject matrix: member `{}` (P7.1, named + fix-it), member missing
  `type`, `properties` without `type: object` (both at the root and at a member),
  `properties` beside `type: string`, name collision after recasing, leading-digit and
  empty member names, override that is not a legal identifier, non-string override value.
- `additionalProperties` rejects: `{}` ("use `true`"), `"yes"`, `1`, and
  `{type: object}` with no shape — all with the spec's wording.
- The catch-all name-collision reject and its `x-<lang>-name` escape hatch, including the
  correct per-language catch-all identifier (`AdditionalProperties` / `additionalProperties`
  / `additional_properties`).
- The inline-object hoist naming table, verified end-to-end: `OrderAddress`,
  `OrderAddressGeo` (fixpoint), `OrderRowsItem`, `OrderGridItemItem`, `OrderValue`,
  `OrderFreeform` (free-form objects are named too), and hoist-name collisions with a
  `$defs` entry and with the file-root type both reject with the documented fix-it.
- Inline shape vs `$defs` + `$ref` emit **identical** code (diffed Go/TS/Python; only the
  root model name differs).
- An `x-<lang>-name` on a member moves the member identifier and does **not** move the
  hoisted type's position-derived name.
- Case mapping agrees across Go/TypeScript/Python/Java for 13 name shapes I tried
  (`user_id`, `httpServer`, `HTTPServer2`, `oauth2`, `kebab-case`, `with space`,
  `XMLHttpRequest`, `a1b2`, `ID`, `_leading`, `trailing_`, `__dunder__`, `ALLCAPS`), and
  the original wire key is pinned in all four (`json:"…"`, the TS/Python converter key,
  and Java's tree key).
- The named-catch-all representation in all four languages, for all five shapes: open
  untyped (`map[string]json.RawMessage` / `Record<string, unknown>` /
  `dict[str, typing.Any]` / `Map<String, JsonNode>`), typed extras + properties, typed map
  wrapper, open opaque map wrapper, and closed (no catch-all field at all).
- Nullable typed map (`additionalProperties: {oneOf:[{T},{null}]}`) →
  `map[string]*T` / `Record<string, T | null>` / `dict[str, T | None]` /
  `Map<String, @Nullable T>`, with the null member kept rather than dropped and the
  present member still constrained — in both directions.
- Typed-extras per-member validation with the key as the violation path, including a
  `$ref` element (Go's helper struct uses `mergeNested(k, …)` → `home.street`; Python
  materializes and re-encodes through the referenced model's converter).
- `required` + nullable: Go emits `out["x"] = null` rather than omitting; required
  non-nullable rejects an explicit `null` in all four.
- `minProperties`/`maxProperties` count the full member set including preserved extras, on
  both parse and serialize (verified Go/TS/Python).
- Python §1 shapes: open-model `__init__` initializes an omitted `additional_properties`
  to a fresh dict; a default-bearing property emits `init=False` + a `_<field>` slot
  (`repr=False`, still a comparison field) + property/setter/deleter.
- Per-target scoping of the load: `{properties:{class:{type:string}}}` is accepted when
  generating Go and rejected when generating Python — the spec's "rejection is per emitted
  target" rule.
- P15 collision coverage that *is* present and correct: model type idents, Go closed-value
  defined types and value constants, service idents, runtime boilerplate idents, TS
  `DEFAULT_*` / `*_CONST` / transfer-type converters, Python converter classes /
  declared-key frozensets / union functions / pattern constants / converter-body locals.
