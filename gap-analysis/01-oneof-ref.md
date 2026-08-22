# oneOf & $ref — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/oneOf.md` — the retained **closed sum type**: accepted only with a decidable selector (JSON type token across disjoint kinds, plus a shared required `const`-tag among 2+ object branches); covers branch naming/hoisting, element-position unions, nullable unions, per-language emission, and the branch-constraint / serialize-side validator contract.
- `specs/json-schema/features/ref.md` — `$ref` to **named targets only, local files only, no `$id`**; `$defs` naming, the input-set closure, type-name derivation + P15 collisions, the recursion/SCC story (P14), satisfiability, and the bare-`$ref`-root alias.
- `specs/json-schema/PRINCIPLES.md` — P1 (identical accept/reject set + wire round-trip across Go/TS/Python/Java), P6, P7/P7.1, P11, P12, P13/P13.1, P14, P15.

Every finding below was reproduced against a locally built `./target/debug/nexgen` for all four targets on scratch schemas in `/tmp/nxg`; Go findings were additionally confirmed with `go build`.

## Summary

- **The loader's `oneOf` acceptance surface is in very good shape.** All 17 rows of the spec's negative matrix reject, with matching diagnostics, identically in all four languages; the positional hoist names (`BagValuesItem`, `BagNestedItemItem`, `EntriesValue`, `UniArrayItem`, `BagValuesItemObject`) all match the spec's table, including the fixpoint.
- **Cross-file union branches are silently broken in three of four languages (P0).** Go, TypeScript and Python resolve a branch's `$ref` only against the *current module's* model list, so a branch pointing into another input file is **dropped from the dispatcher**. A named cross-file union def becomes `type Shape struct{}` in Go; a property-level one becomes the first branch's concrete type. Java is correct because it resolves against the whole model set.
- **Go's synthesized union identifiers are absent from the P15 collision pass (P0).** Two anonymous unions that derive the same Go name are silently *merged* — `Foo.barBaz` declared `string | integer` binds to a `string | boolean` interface. The exact schema an existing test (`json_schema.rs:11561`) asserts Python must reject is accepted for Go with wrong output.
- **Java's discriminated-union dispatcher only understands string tags (P0).** `disc.isTextual()` gates the switch, so an integer/boolean `const` discriminator — which the loader accepts and Go/TS/Python route correctly — never selects a branch in Java.
- **Go matches the discriminator on raw JSON *text* (P0/P1).** `{"kind": 1.0}` against an integer tag, or `{"kind":"cat"}` against `"cat"`, matches in TS/Python (and Java for strings) and misses in Go — a direct P1 "same mathematical number / same JSON value" violation.
- **Union branches skip the `type: object`/`type: array` shape check.** `validate_type_presence` is not called for branches, so an itemless `{type: array}` branch loads and Java infers `List<String>` where Go/TS/Python infer `any`/`unknown`/`Any` — `[1,2]` accepted by three targets, rejected by Java (P0).
- **`default` on a sum-type union is neither validated nor lowered (P1).** `default: true` on a `string|integer` union loads; Go emits `FOrDefault() bool { return *m.F }` (does not compile), TS emits a mistyped `DEFAULT_F`, Java emits nothing.
- **The bare-`$ref`-root alias is entirely unimplemented (P1).** No `type A = Main` in Go/TS/Python, and a cross-file `$ref` at such a file rejects with "does not resolve to a known JSON model" — the accepted-matrix row is unreachable.
- **The unsatisfiable-recursion check is blind to sum types (P1).** `collect_mandatory_targets` treats *any* `oneOf` edge as terminating, so a required non-nullable union of two mutually recursive objects loads.
- **Testing:** the loader half is well covered (~45 inline `#[test]`s across `oneOf`/`$ref`) and the showcase gives all four languages a real round-trip over 12 union shapes. But there is **no test anywhere** for a cross-file union branch, a cross-module `$ref` to a union, a non-string discriminator, the Go P15 union-name collision, a nullable union with 3+ kinds at runtime, a boolean branch, or the bare-`$ref` root — and the cross-language conformance manifest has **zero** union cases.

## Implementation divergences

### 1. Cross-file `$ref` union branches are silently dropped in Go / TypeScript / Python — **P0**

- **Spec cite:** oneOf.md §"Support decision" ("every branch declares a single recognized [[type]] … or via a `$ref` to a named typed definition"), §Interactions → `[[ref]]` ("a branch may be a `$ref` to a named typed definition; the resolved type supplies the kind"); ref.md §"Accepted ref forms" (`<relative-path>#/$defs/<Name>`), §"Type-name derivation" ("A type's emitted name is resolved once for the **whole input closure**").
- **Code cite:**
  - Go: `src/generator/json_schema/go.rs:1790-1800` (`find_ref_model` searches the `models: &[&PlannedJsonType]` slice it is handed), `go.rs:1815-1821` (`.unwrap_or_else(|| branch.clone())` when not found), `go.rs:1914` (`_ => {}` — a `$ref`-only schema has no `ty`, so the branch is dropped), `go.rs:1917` (`if variants.len() < 2 { return None }` — the union disappears). The slice comes from `render_external_models(&self.local_json_models…)` (`go.rs:1071`, `go.rs:1389`), which holds the current module's models only.
  - TypeScript: `src/generator/json_schema/typescript.rs:2184-2196`, `:2216-2221`, same `_ => {}` drop.
  - Python: `src/generator/json_schema/python.rs:2604-2614`, `:2628-2632`.
  - Java is correct: `src/generator/json_schema/java.rs:1409` takes `all_models: &BTreeMap<String, PlannedJsonType>` (the whole plan) and `:1418-1422` resolves through it.
- **What the spec requires:** a `$ref` branch resolves to its target wherever the target is declared; Go emits `func (Circle) isShape() {}`, TS narrows on `Circle | Square`, Python delegates to their converters.
- **What the code does:** the branch is silently discarded from the dispatcher, with two distinct failure shapes.
- **Failing input** — `shapes.yaml`:
  ```yaml
  $defs:
    Circle: { type: object, required: [kind], properties: { kind: { type: string, const: circle }, r: { type: number } } }
    Square: { type: object, required: [kind], properties: { kind: { type: string, const: square }, s: { type: number } } }
  ```
  (a) property-level union — `main.yaml`:
  ```yaml
  type: object
  properties:
    shape:
      oneOf:
        - { $ref: 'shapes.yaml#/$defs/Circle' }
        - { $ref: 'shapes.yaml#/$defs/Square' }
        - { type: string }
  ```
  Go emits `Shape *Circle` and unmarshals every value into `Circle` (a `Square` payload fails on the `kind` const; a string fails to unmarshal). TS declares `shape?: Circle | Square | string` but its converter only handles `typeof raw === 'string'` and pushes `expected one of: string` for an object. Python's `_main_shape_from_transfer_type` is likewise string-only. Java is correct (full three-way dispatch).
  (b) named union def — `main2.yaml` with `$defs.Shape` holding the same three branches: Go emits **`type Shape struct {}`**, a closed empty struct whose `UnmarshalJSON` reports `unknown field` for every key; TS's `shapeTransferTypeConverter` accepts only strings; Python likewise. Both compile, so the failure is silent.
- **Confidence:** high (reproduced in all four targets; root cause read directly; Java's contrasting signature confirms the diagnosis).

### 2. Go's P15 pass does not know about union identifiers — silent union merge and package redeclarations — **P0**

- **Spec cite:** oneOf.md §"Loader behavior" ("A synthesized branch name already declared in `$defs`, or colliding with another emitted type → reject per **P15**"; "Synthesized union type name collides after case-mapping → reject per **P15**"); oneOf.md §"Rejected at load time" row "Synthesized union name collision (P15) | two anonymous unions recasing to the same Go type name (fix: `x-go-name`)"; PRINCIPLES P15.
- **Code cite:** `src/parser/json_schema.rs:6963-7011` — `collect_synthesized_top_level` is the *only* Go-specific contributor to the package namespace and registers just the closed-value defined types + value constants. Nothing registers the union interface (`<Model><Property>`), the variant wrappers (`<Union>String|Integer|Number|Boolean|Array|Object`) or the dispatcher `unmarshal<Union>`, all emitted at package scope by `src/generator/json_schema/go.rs:1979-2005` / `:2041-2110`. Contrast `collect_python_module_idents` (`json_schema.rs:7262-7339`), which *does* register Python's union function names.
- **What the spec requires:** a load reject with `x-go-name` offered as the escape hatch.
- **What the code does:** two things, both bad.
  (a) **Silent merge.** Two property-level unions whose synthesized names coincide produce one interface; the later model's branch set wins for both.
  ```yaml
  # this is verbatim the schema `rejects_colliding_union_functions_python` (json_schema.rs:11561) asserts Python must reject
  type: object
  properties:
    u: { $ref: "#/$defs/FooBar" }
    f: { $ref: "#/$defs/Foo" }
  $defs:
    FooBar: { oneOf: [ { type: string }, { type: integer } ] }
    Foo:
      type: object
      additionalProperties: false
      properties:
        bar: { oneOf: [ { type: string }, { type: boolean } ] }
  ```
  Python rejects (`_foo_bar_from_transfer_type` collision). Go accepts and emits `Bar FooBar` where `FooBar` is `string | integer` — so Go rejects `{"bar": true}` and accepts `{"bar": 1}`, exactly inverting the schema. TS emits the correct `bar?: string | boolean`. That is a P1 wire disagreement *and* a load-time disagreement.
  Same class, different derivation (`Foo.barBaz` and `FooBar.baz` both → `FooBarBaz`):
  ```yaml
  $defs:
    Foo:    { type: object, properties: { barBaz: { oneOf: [{ type: string }, { type: integer }] } } }
    FooBar: { type: object, properties: { baz:    { oneOf: [{ type: string }, { type: boolean }] } } }
  ```
  → one `type FooBarBaz interface` (string|boolean) used by both fields. Compiles; silently wrong.
  (b) **Package redeclaration.** A variant wrapper colliding with an authored `$defs` entry breaks the build:
  ```yaml
  $defs:
    FooBarBaz: { type: object, properties: { z: { type: string } } }
    Foo: { type: object, properties: { barBaz: { oneOf: [{ type: string }, { type: integer }] }, other: { $ref: '#/$defs/FooBarBaz' } } }
  ```
  `go build` → `FooBarBaz redeclared in this block` + `invalid receiver type FooBarBaz (pointer or interface type)`. Note the *object*-branch analogue **is** caught (`hoist_inline_object_shapes`, `json_schema.rs:3768-3776`, tested at `json_schema.rs:10186`); only the scalar/array wrappers and the interface itself escape.
- **Confidence:** high (reproduced; `go build` output captured).

### 3. Java's discriminated-union dispatch requires a *textual* discriminator — **P0**

- **Spec cite:** oneOf.md §"Discriminated object unions — the `const`-tag" ("has a scalar **`const`**"), §Validator mapping Java row ("peeks the discriminator node and dispatches to the matching POJO's collecting deserializer"). The loader admits any scalar tag: `discriminator_const` (`src/parser/json_schema.rs:3600-3611`) accepts anything `scalar_value_kind` accepts.
- **Code cite:** `src/generator/json_schema/java.rs:2186-2190` —
  ```rust
  "{indent}    if (disc == null || !disc.isTextual()) {{ … \"discriminator {discriminant:?} is required\" … }}\n"
  …
  output.push_str(&format!("{indent}    switch (disc.textValue()) {{\n"));
  ```
  and `:2196-2204` renders the `case` labels via `java_string_literal(&text)`, so an integer tag `1` becomes `case "1":`.
- **What the spec requires:** the same accepted value set in all four targets (P1).
- **What the code does:** for a non-string tag, Java always takes the `disc == null || !disc.isTextual()` arm.
- **Failing input:**
  ```yaml
  $defs:
    Cat: { type: object, required: [kind], properties: { kind: { type: integer, const: 1 }, meow: { type: string } } }
    Dog: { type: object, required: [kind], properties: { kind: { type: integer, const: 2 }, bark: { type: string } } }
    Animal: { oneOf: [{ $ref: '#/$defs/Cat' }, { $ref: '#/$defs/Dog' }] }
    Holder: { type: object, properties: { f: { $ref: '#/$defs/Animal' } } }
  ```
  `{"f":{"kind":1,"meow":"x"}}` → Go/TS/Python bind a `Cat`; Java reports `discriminator "kind" is required`. Boolean tags fail the same way.
- **Confidence:** high (generated `Animal.java` inspected; Go/TS/Python dispatchers inspected side by side).

### 4. Go matches the discriminator against raw JSON *text*, not the JSON value — **P0**

- **Spec cite:** PRINCIPLES P1 ("JSON identity does not include … a number's lexical spelling: `5`, `5.0`, and `5e0` are the same mathematical number"); oneOf.md §"Discriminated object unions" ("reads the **discriminator property's value** and maps it to the branch bearing that `const`"); const.md §Loader behavior (value equality is JSON equality).
- **Code cite:** `src/generator/json_schema/go.rs:2256-2264` —
  ```rust
  output.push_str("\t\tswitch string(bytes.TrimSpace(discRaw)) {\n");
  …
  let literal = serde_json::to_string(value).unwrap_or_default();
  output.push_str(&format!("\t\tcase {}:\n", go_string_literal(&literal)));
  ```
  `discRaw` is the member's `json.RawMessage`, i.e. the verbatim wire bytes.
- **What the spec requires:** value comparison.
- **What the code does:** byte comparison against the canonical serde spelling.
- **Failing inputs:** with the integer-tagged `Animal` of §3, `{"kind":1.0}` and `{"kind":1e0}` are `unknown discriminator kind 1.0` in Go, while TypeScript (`switch (raw["kind"])`, `case 1:` — `1.0 === 1`) and Python (`if tag == 1:` — `1.0 == 1`) both bind `Cat`. With the string-tagged `Cat|Dog`, `{"kind":"cat"}` matches in TS/Python/Java (all compare the *parsed* string) and misses in Go.
- **Confidence:** high for the mechanism (code read; Go emits `case "1"` / `case "\"cat\""`); the runtime behaviour follows directly from Go's `json.RawMessage` semantics. Not executed end-to-end.

### 5. `oneOf` branches skip the object/array shape check — itemless array branch diverges across languages — **P0**

- **Spec cite:** type.md §Loader behavior — "`type: object` with no `properties`, `patternProperties`, or `additionalProperties` → reject (P7.1)" and "`type: array` … needs an explicit element type; add `items: {…}`"; items.md:63 (same parallel).
- **Code cite:** `src/parser/json_schema.rs:1408-1410` —
  ```rust
  if !is_union_branch {
      validate_type_presence(path, schema, context)?;
  }
  ```
  with `validate_type_presence` (`:1642`, object arm `:1668-1673`, array arm `:1674-1680`) carrying the shape rejects. The doc comment at `:1640` says branches are exempt because "their kind is checked by the sum-type pass" — but `one_of_branch_kind` (`:3551`) only classifies the *kind*, never the shape.
  Downstream, the itemless array element type is guessed per language: Go `"any"` (`go.rs:1902`), Java **`JavaType::String`** (`java.rs:1505-1509`), TS `unknown`, Python `typing.Any`.
- **What the spec requires:** a load reject.
- **What the code does:** accepts, and the four targets disagree on the element type.
- **Failing input:**
  ```yaml
  $defs:
    Foo: { oneOf: [ { type: array }, { type: string } ] }
    H: { type: object, properties: { f: { $ref: '#/$defs/Foo' } } }
  ```
  Emits `[]any` (Go), `unknown[]` (TS), `list[typing.Any]` (Python), **`List<String>`** (Java). `{"f":[1,2]}` is accepted by Go/TS/Python and rejected by Java with `expected string` at `f[0]`/`f[1]`.
  The object twin (`{ oneOf: [ { type: object }, { type: string } ] }`) also loads, silently becoming the free-form object in all four — consistent output, but still an accept-that-should-reject.
- **Confidence:** high (reproduced; both branches of the emitter read).

### 6. A cross-module `$ref` to a named union emits `*Foo` (pointer to interface) in Go — **P1**

- **Spec cite:** oneOf.md §"Nullable unions" table — Go row: "`Foo` (interface) | already nilable — `nil` = `null`; **no `*Foo` wrapper**"; §Validator mapping Go row (the container's `UnmarshalJSON` must route through the union's dispatcher).
- **Code cite:** `src/generator/json_schema/go.rs:2020-2037` — `property_union_name` consults the module-local `unions` map built by `collect_go_unions(models, …)` (`go.rs:1979-2005`, `:1389`); a `$ref` into another input file is not in it, so the property falls through to the ordinary named-model path.
- **What the spec requires:** `F Foo` plus `unmarshalFoo(...)` dispatch.
- **What the code does:** `F *Foo` with `var tmp Foo; json.Unmarshal(*raw, &tmp)`.
- **Failing input:** `u.yaml` = `$defs: { Foo: { oneOf: [{type: string},{type: integer}] } }`; `m.yaml` = `type: object` / `properties: { f: { $ref: 'u.yaml#/$defs/Foo' } }`.
  `go build` → `m.F.Validate undefined (type *Foo is pointer to interface, not interface)`. TypeScript, Python and Java are all correct here (TS imports `fooTransferTypeConverter` from the other module).
- **Confidence:** high (`go build` output captured).

### 7. `default` on a sum-type union is unvalidated and lowers incoherently — **P1**

- **Spec cite:** default.md §Interactions → `[[type]]` ("the default value must be valid for the declared type (enforced at load, P7.1)"); PRINCIPLES P7.1. oneOf.md's §Interactions has no `[[default]]` bullet at all, and neither spec defines a union default — so at minimum this needs a reject.
- **Code cite:**
  - Loader: `src/parser/json_schema.rs:4357-4375` — the `default` cross-check runs only when `branches.len() == 2 && non_null == 1` (the nullability pattern). A sum type's `default` is never checked against any branch.
  - Go: `src/generator/json_schema/go.rs:2536-2582` (`render_default_accessors`) emits `*m.<Field>` for every non-`contentEncoding` default, and `go_default_type_and_literal` (`:2588-2600`) derives the return type from the *literal's* own kind whenever `property.ty` is absent — a comment at `:2586-2589` says this fallback exists "for a typeless (nullable `oneOf`) member", but it also catches sum types.
- **What the spec requires:** reject an out-of-branch default; and, for an in-branch one, a coherent per-language lowering.
- **What the code does:**
  ```yaml
  $defs:
    H: { type: object, properties: { f: { default: true, oneOf: [ { type: string }, { type: integer } ] } } }
  ```
  loads in all four targets even though `true` satisfies neither branch. Go emits
  ```go
  func (m H) FOrDefault() bool { if m.F != nil { return *m.F }; return true }
  ```
  → `go build`: `invalid operation: cannot indirect m.F (variable of interface type HF)`. `default: "hello"` on the same union yields the same compile error with `string`. TypeScript emits `export const DEFAULT_F = "hello"` beside a `string | number` field; Python emits a `_f` slot + property; Java emits nothing.
- **Confidence:** high (reproduced across all four; `go build` output captured).

### 8. The bare-`$ref`-root alias is not implemented, and such a file cannot be referenced — **P1**

- **Spec cite:** ref.md §"Bare-`$ref`-root alias" (Go `type A = Main`, TS `export type A = Main`, Python `A = Main`, Java none) and the accepted-matrix row "Bare-`$ref` root | file root `{"$ref":"#/$defs/Main"}` → alias (Go/TS/Py), `Main` (Java)"; the Java note ("every reference to the bare-ref root resolves directly to the target `Main`") presupposes such references resolve.
- **Code cite:** `src/parser/json_schema.rs:686` — `if root_is_schema_shaped(&doc.root) && !doc.root.is_bare_ref()` guards the *only* place a root model is inserted; the same guard appears at `:669` (validation), `:3699` (hoist), `:4754`, `:4866`. `is_bare_ref` is `json_schema.rs:77-83`. No emitter has an alias path for JSON-Schema models (`grep -rn "type .* = " src/generator/json_schema/*.rs` finds none; the `ExternalTypeSpec::Alias` machinery in `src/planning/*` is the WIT path).
- **What the spec requires:** the file's derived root name is emitted as an alias in Go/TS/Python and resolves to the target everywhere.
- **What the code does:** the bare-`$ref` root is not a model, so nothing is emitted and nothing can point at it.
- **Failing input:** `q10.yaml` = `{ $ref: '#/$defs/Main', $defs: { Main: {…} } }` → no `Q10` identifier appears anywhere in the Go/TS/Python output. And `alias.yaml` = `{$ref: 'core.yaml#/$defs/Main'}` referenced from `use.yaml` → **all four** reject with ``$ref `alias.yaml` does not resolve to a known JSON model``.
- **Confidence:** high (reproduced; guard read).

### 9. The unsatisfiable-recursion check treats every `oneOf` edge as terminating — **P1**

- **Spec cite:** ref.md §"Recursion & satisfiability" — an edge terminates only when it is optional, **required + nullable** (the nullability `oneOf` pattern), or collection-wrapped; "If **every** edge in a cycle is mandatory-and-single-valued … → **load reject**".
- **Code cite:** `src/parser/json_schema.rs:3184-3187` —
  ```rust
  if !required.contains(name.as_str()) || property.one_of.is_some() {
      // Optional, or a nullable/union `oneOf` edge — the chain can terminate.
      continue;
  }
  ```
  The comment conflates the nullability wrapper with a sum type. `collect_mandatory_targets` also returns early for a model that *is* a union (`:3171-3173`, `ty != "object"`), so a union def contributes no edges either.
- **What the spec requires:** a required, non-nullable sum-type edge terminates only if at least one of its branches does.
- **What the code does:** every union edge is assumed terminating, so the cycle is invisible.
- **Failing input** (accepted by all four; no finite instance exists):
  ```yaml
  $defs:
    A:   { type: object, required: [n], properties: { n: { oneOf: [ { $ref: '#/$defs/Cat' }, { $ref: '#/$defs/Dog' } ] } } }
    Cat: { type: object, required: [kind, a], properties: { kind: { type: string, const: cat }, a: { $ref: '#/$defs/A' } } }
    Dog: { type: object, required: [kind, a], properties: { kind: { type: string, const: dog }, a: { $ref: '#/$defs/A' } } }
  ```
- **Confidence:** high (reproduced; code read). Note the spec's own "conservative — it never rejects a satisfiable schema" clause explains the *direction* of the shortcut but does not license missing this case.

### 10. Violation reason strings are not identical across targets — **P2**

- **Spec cite:** oneOf.md §Validator mapping, closing paragraph: "no decidable branch → `expected one of: <labels>` … an object whose discriminator value matches no branch → `unknown discriminator <field> <value>: expected one of [...]`. **Both strings are identical in all four targets.**"
- **Code cite:** `src/generator/json_schema/go.rs:2270-2276` vs `src/generator/json_schema/java.rs:2213-2218`; label construction at `go.rs:1905` (`format!("[]{item}")`), `java.rs:1521` (`"array"`), `typescript.rs:2280-2290`, `python.rs` equivalents.
- **What the code does:** for `Cat|Dog`, Go/TS/Python emit `expected one of ["cat", "dog"]` while Java emits `expected one of [cat, dog]`. For a `Widget | number[]` union, the no-branch label is `Widget, []float64` (Go) / `Widget, number[]` (TS) / `Widget, list[float]` (Python) / `Widget, array` (Java).
- Not a wire-behaviour bug (PRINCIPLES P11 explicitly does *not* hold reason text byte-identical), so this is a **spec/impl reconciliation item**: either soften oneOf.md's claim to "same shape, language-native labels", or align Java's rendering.
- **Confidence:** high.

### 11. oneOf.md's own examples use a `type`-less `const` tag, which the loader rejects — **P2**

- **Spec cite:** oneOf.md:626-627 (the `Cat`/`Dog` running example), :775 ("Object tagged union (`const`-tag) … with `kind:{const:…}` required in each"), :794, :797-798 — all write `kind: { const: cat }` with no `type`.
- **Code cite:** `src/parser/json_schema.rs:1654-1658` (via `validate_type_presence`) — the property is a leaf, so it rejects with "a leaf schema requires an explicit `type`". This is correct per type.md:51, but it makes every tagged-union example in oneOf.md unloadable verbatim.
- **Fix:** write `kind: { type: string, const: cat }` throughout oneOf.md (and in the matrix rows).
- **Confidence:** high (reproduced — the examples were copy-pasted and rejected in all four targets).

### 12. `1.5` against `string | integer` reports "no branch matched" in TS/Python — **P2**

- **Spec cite:** oneOf.md §"Runtime fixtures": "`1.5` against a `string | integer` union → **routed to the integer branch**, rejected by the spec-number rule (**P12** parse adapter), not truncated."
- **Code cite:** TS folds the token test and the integral test into one guard — `else if (typeof raw === 'number' && Number.isSafeInteger(raw))` — so `1.5` falls to the `else` (`expected one of: string, integer`); Python's classification arm is `not isinstance(value, bool) and isinstance(value, (int, float)) and abs(value) <= 9007199254740991 and float(value).is_integer()`, same effect. Go (`go.rs`, `case '-','0'…'9'` → `parseSpecInteger`) and Java (`node.isNumber()` → `SpecNumbers.specLong`) do route-then-reject.
- The accept/reject set is identical, so P1 holds; only the `reason` differs (and the integer-cap overflow message is likewise lost in TS/Python). Low impact, but it means a conformance case asserting the spec's wording would fail in two targets.
- **Confidence:** high (generated code read for all four).

## Testing gaps

### 1. Cross-file `$ref` union branch — **P0**

- **Untested:** no schema anywhere in the repo puts a `$ref` to another *input file* inside a sum-type `oneOf` branch. `samples/schemas/kb/content/block.json:17` is the closest, but it is the two-branch *nullability* pattern (`[{$ref:"page.json"},{type:"null"}]`), which takes a different code path.
- **Mandated by:** oneOf.md §Interactions → `[[ref]]`; ref.md §"Accepted ref forms" + §"Type-name derivation" ("resolved once for the **whole input closure**").
- **Where:** a loader/emission test in `src/parser/json_schema.rs` won't catch it (the loader is fine); it needs a generator-level test — `tests/generate_go.rs` / `generate_typescript.rs` / `generate_python.rs` asserting the union interface/alias is emitted, plus a new `samples/conformance/json-schema.json` case.
- **Suggested case:** add a `shapes.yaml`/`main.yaml` pair to `samples/schemas/kb` (or a new multi-file sample) with `oneOf: [{$ref:'shapes.yaml#/$defs/Circle'}, {$ref:'shapes.yaml#/$defs/Square'}, {type:string}]`, and a manifest case asserting `{"shape":{"kind":"square","s":2}}` and `{"shape":"x"}` both round-trip in all four.

### 2. Cross-module `$ref` to a named union — **P0**

- **Untested:** a property whose type is `$ref: 'other.yaml#/$defs/SomeUnion'`.
- **Mandated by:** oneOf.md §Type mapping ("a named `$defs` union reuses the def name"), §"Nullable unions" Go row (no `*Foo`).
- **Where:** `tests/generate_go.rs` (assert the field is `F Foo`, not `*Foo`) plus the same conformance case as gap 1.
- **Suggested case:** `u.yaml` declaring `$defs.Foo: {oneOf:[{type:string},{type:integer}]}`, `m.yaml` with `f: {$ref: 'u.yaml#/$defs/Foo'}`; assert the emitted Go compiles.

### 3. Non-string `const` discriminator — **P0**

- **Untested:** every discriminated union in the repo (`showcase.nexusrpc.yaml` `Shape`, `Note`, `Choices`, `shapeOrName`; the inline tests at `json_schema.rs:9904`, `:10106`, `:10717`) uses a **string** tag.
- **Mandated by:** oneOf.md §"Discriminated object unions" ("has a scalar **`const`**") — `discriminator_const` (`json_schema.rs:3600`) accepts any scalar, so integer/boolean tags are in-subset.
- **Where:** a new `$defs` in `samples/schemas/showcase.nexusrpc.yaml` (e.g. `Level`/`LevelOne`/`LevelTwo` tagged by `code: {type: integer, const: 1|2}`) with a conformance-manifest case, plus a `tests/generate_java.rs` assertion that the dispatcher does not gate on `isTextual`.
- **Suggested case:** wire `{"kind":1,"meow":"x"}` accepted in all four; `{"kind":3}` → one violation naming `[1, 2]`; `{"kind":1.0}` accepted in all four (covers gap 4 / divergence 4 too).

### 4. Go P15 collision over synthesized union identifiers — **P0**

- **Untested:** `rejects_colliding_union_functions_python` (`src/parser/json_schema.rs:11561`) covers exactly this for Python and has no Go sibling; `rejects_inline_object_one_of_branch_name_clashing_with_a_definition` (`:10186`) covers only the *object-branch hoist* name.
- **Mandated by:** oneOf.md §"Rejected at load time" row "Synthesized union name collision (P15)" and §"Loader behavior" ("colliding with another emitted type → reject per **P15**").
- **Where:** `src/parser/json_schema.rs` inline tests, beside `rejects_colliding_union_functions_python`.
- **Suggested cases:** (a) the *same* schema as the Python test, asserted to reject for `Language::Go`; (b) `$defs.FooString` object + a `Foo` union with a string branch → reject naming `FooString`; (c) `Foo.barBaz` + `FooBar.baz` both deriving `FooBarBaz` → reject, with `x-go-name` on one union proving the escape hatch works.

### 5. Union branch shape checks (itemless array / bare object) — **P0**

- **Untested:** no test asserts a `{type: array}` or bare `{type: object}` *branch* rejects; `rejects_typeless_branch_union` (`:10650`) covers only a wholly absent `type`.
- **Mandated by:** type.md:51 and its "`type: object` needs an explicit shape" / "`type: array` needs an explicit element type" loader bullets, which oneOf.md inherits via §Interactions → `[[type]]` ("every branch must declare one recognized `type`").
- **Where:** `src/parser/json_schema.rs` inline tests next to `rejects_typeless_branch_union`.
- **Suggested case:** `{oneOf:[{type:array},{type:string}]}` → reject; `{oneOf:[{type:object},{type:string}]}` → reject with the three-way fix-it.

### 6. Nullable union (`null` among 3+ kinds) has no runtime coverage in any language — **P1**

- **Untested at runtime:** `accepts_nullable_multi_kind_union` (`src/parser/json_schema.rs:10746`) is a loader-only test. Every `oneOf` containing `type: "null"` in `samples/schemas/` is the two-branch nullability pattern, so no sample exercises `oneOf: [{$ref:X},{type:array,…},{type:"null"}]`.
- **Mandated by:** oneOf.md §"Null branches — nullable unions" and the §"Nullable unions" per-language table; §Runtime fixtures ("`null` against a nullable union … accepted as the null state … `null` against a non-nullable union → one `Violation`").
- **Where:** a new `$defs` in `samples/schemas/showcase.nexusrpc.yaml` + all four round-trip suites + a conformance case (this is precisely a `permitted_presence_nullability_collapse` scenario, so the manifest is the right home).
- **Suggested case:** `WidgetOrList: { oneOf: [{$ref:'#/$defs/Circle'},{type:array,items:{type:number}},{type:"null"}] }` as a **required + nullable** member; assert explicit `null` survives in TS and collapses in Go/Java/Python.

### 7. Zero union coverage in the cross-language conformance manifest — **P1**

- **Untested:** `samples/conformance/json-schema.json` has 4 cases (`recursive-collections`, `mathematical-number-equality`, `year-zero-rejection`, `optional-null-presence-collapse`); none exercises union selection, and each language's union assertions live only in its own suite with no shared accepted/rejected value set.
- **Mandated by:** PRINCIPLES P1 (the manifest is the mechanism that enforces it) plus oneOf.md §Runtime fixtures, ~15 bullets of which are per-language-only today.
- **Where:** `samples/conformance/json-schema.json`, with consumers in all four suites.
- **Suggested cases:** `union-token-selection` (string/integer/array/object/null tokens over the showcase's `idOrName`, `measurements`, `payload`, `shapeOrName`), and `union-discriminator` (unknown tag, absent tag, non-string tag) with `expected_paths` for each.

### 8. `default` on a union member — **P1**

- **Untested:** `rejects_nullable_default_invalid_for_non_null_branch` (`:9195`) covers the two-branch nullability case only; nothing covers a sum type.
- **Mandated by:** default.md §Interactions → `[[type]]` (P7.1 load-time validity).
- **Where:** `src/parser/json_schema.rs` inline tests.
- **Suggested case:** `{default: true, oneOf:[{type:string},{type:integer}]}` → reject (no branch admits it); `{default: "ab", oneOf:[{type:string,minLength:5},{type:integer}]}` → reject; decide and test whether an in-branch default on a sum type is supported at all (today Go does not compile).

### 9. Bare-`$ref` root alias — **P1**

- **Untested:** no test in `src/parser/json_schema.rs` or `tests/generate_*.rs` mentions a bare-`$ref` root; the accepted-matrix row is unimplemented and unasserted.
- **Mandated by:** ref.md §"Bare-`$ref`-root alias" + the accepted-matrix row.
- **Where:** `tests/generate_{go,typescript,python,java}.rs`.
- **Suggested case:** `alias.yaml` = `{$ref: 'core.yaml#/$defs/Main'}`, referenced from a third file; assert `type Alias = Main` / `export type Alias = Main` / `Alias = Main` / (Java) every site typed `Main`.

### 10. Unsatisfiable cycle through a sum-type union — **P1**

- **Untested:** `rejects_unsatisfiable_self_reference` (`:12191`) and `rejects_unsatisfiable_mutual_recursion` (`:12206`) use plain `$ref` edges; `accepts_optional_recursion` (`:12242`) and `accepts_array_wrapped_recursion` (`:12226`) cover the terminating forms.
- **Mandated by:** ref.md §"Recursion & satisfiability".
- **Where:** `src/parser/json_schema.rs`, beside the two existing reject tests.
- **Suggested cases:** the `A/Cat/Dog` schema from divergence 9 → reject; the same with a `{type:string}` third branch → **accept** (one branch terminates), which is the positive control the fix must not break.

### 11. Boolean union branch — **P2**

- **Untested:** no `oneOf` in `samples/schemas/` has a `type: boolean` branch; the loader tests never exercise the boolean token. Python's classification order matters here (`isinstance(True, int)` is `True`), and the emitter does get it right — but nothing pins it.
- **Mandated by:** oneOf.md §"Support decision" (boolean is one of the six selector kinds); §Validator mapping (`true`/`false` → the boolean branch; Python "`bool`→boolean").
- **Where:** a `boolOrCount: {oneOf:[{type:boolean},{type:integer}]}` member on the showcase + all four suites.
- **Suggested case:** `true` → boolean branch; `1` → integer branch; `"x"` → one violation. (Regression guard for the Python `bool`-before-`int` ordering.)

### 12. Discriminator-absent fixture exists only in Go — **P2**

- **Untested in 3 of 4:** `samples/go/tests/json_schema_showcase_test.go:604` is the only absent-discriminator assertion; TS/Python/Java suites assert the *unknown*-tag case but not the absent one.
- **Mandated by:** oneOf.md §Runtime fixtures ("Object with the discriminator **absent** → one `Violation` (it is `required`); the deserializer never falls back to trial-matching branches").
- **Where:** the three sibling suites, or better, promote it to a conformance case (see gap 7).
- **Suggested case:** `{"shape":{"radius":1}}` → one violation at `shape`.

### 13. Materialized values inside a union's array branch — **P2**

- **Untested:** the spec explicitly says a union's *array* branch "uses the ordinary [[items]] parser and mapper in both directions … temporals, and binary values therefore materialize normally", but no test covers `{type:array, items:{type:string, format:date-time}}` or `contentEncoding: base64` as a branch — the very shapes the §Deferred rule refuses at branch level.
- **Where:** showcase + conformance.
- **Suggested case:** `{oneOf:[{type:array,items:{type:string,format:date-time}},{type:integer}]}` round-tripping in all four (I verified emission is currently correct and consistent: `[]time.Time` / `string[]` (TS's canonical-string temporal repr) / `list[datetime.datetime]` / `List<OffsetDateTime>`).

### 14. Non-canonical discriminator spellings — **P2**

- **Untested:** nothing asserts `{"kind":1.0}` ≡ `{"kind":1}` or a `\u`-escaped string tag. This is what would have caught divergence 4.
- **Mandated by:** PRINCIPLES P1 ("`5`, `5.0`, and `5e0` are the same mathematical number").
- **Where:** the `mathematical-number-equality` conformance case is the natural home — extend it with a discriminator payload.

## Combination gaps

| Feature A × Feature B | Spec says | Tested? | Risk |
|---|---|---|---|
| `oneOf` × `$ref` (same file) | branch `$ref` supplies the kind; named type implements the marker directly | **yes** — `accepts_discriminated_object_union_def` (`:9904`), showcase `Shape`/`shapeOrName` in all four | low |
| `oneOf` × `$ref` (**cross-file** branch) | same, resolved over the whole input closure (ref.md §Type-name derivation) | **no** | **P0 — divergence 1** |
| `oneOf` × `$ref` (**cross-module** union reference) | Go `F Foo`, no `*Foo` | **no** | **P0 — divergence 6** |
| `oneOf` × `$ref` (union inside a cross-file SCC → `_recursive.py`) | P14 hoist | **no** (kb sample's cycle is object-only) | medium — verified working by hand, unpinned |
| `oneOf` × `$ref` (recursive union: branch refs the enclosing model) | supported; recursion bounded by data | **no** | medium — verified loading + emitting by hand |
| `oneOf` × `$ref` (**unsatisfiable** cycle via a required union edge) | load reject | **no** | **P1 — divergence 9** |
| `oneOf` × `const` (string tag) | closed value set, unknown rejected (P13.1) | **yes** — showcase, all four | low |
| `oneOf` × `const` (**integer/boolean tag**) | any scalar `const` qualifies | **no** | **P0 — divergence 3** |
| `oneOf` × `const` (non-canonical spelling on the wire) | JSON-value equality (P1) | **no** | **P0 — divergence 4** |
| `oneOf` × `required` (discriminator absent) | one violation | **Go only** | P2 — gap 12 |
| `oneOf` × `nullability` (2-branch) | owned by nullability | **yes** — showcase `middleName`/`category`/`slots`/`audit`, manifest case 4 | low |
| `oneOf` × `nullability` (**null among 3+ kinds**) | nullable union; per-language nullable channel over the union type | **loader only** (`:10746`) | **P1 — gap 6** |
| `oneOf` × `items` (union as array element, named + inline) | hoist to `<Enclosing>Item`; elementwise dispatch with indexed paths | **yes** — showcase `shapes`/`tags`, `hoists_inline_union_inside_items` (`:10208`) | low |
| `oneOf` × `items` (nested array of unions, `…ItemItem`) | hoist to a fixpoint | **loader only** | P2 |
| `oneOf` × `additionalProperties` (union as map member) | hoist to `<Enclosing>Value`; key in the path | **yes** — showcase `Choices` | low |
| `oneOf` × `additionalProperties` (free-form object branch) | stays inline; verbatim members, `<Union>Object` in Go/Java | **yes** — showcase `payload`, incl. large-int preservation | low |
| `oneOf` × `properties` (inline structured branch → `<Union>Object`) | emitted as the named model a `$defs` entry would give | **yes** — showcase `detail`, `names_inline_structured_object_one_of_branch` (`:10013`) | low |
| `oneOf` × `properties` (2+ inline branches, `x-<lang>-name`) | each must self-name | **yes** — showcase `Note`, `:10106`/`:10163` | low |
| `oneOf` × `P15` (object-branch hoist name clash) | reject | **yes** — `:10186`, `:10317` | low |
| `oneOf` × `P15` (**Go interface / variant-wrapper / dispatcher names**) | reject | **no** (Python analogue only, `:11561`) | **P0 — divergence 2** |
| `oneOf` × `type` (branch with no classifiable kind) | reject | **yes** — `:10650`, `:10666` | low |
| `oneOf` × `type` (**branch is a shapeless `object`/`array`**) | reject (type.md) | **no** | **P0 — divergence 5** |
| `oneOf` × `format`/`contentEncoding` (on the branch) | deferred reject | **yes** — `:10783`, `:10800`, and the nullable exemption `:10816` | low |
| `oneOf` × `format`/`contentEncoding` (**inside an array branch's items**) | materializes normally | **no** | P2 — gap 13 (verified correct by hand) |
| `oneOf` × `enum` (closed value set on a branch) | literal union / `Literal` / membership check | **yes** — showcase `mode` | low |
| `oneOf` × `allOf` (`allOf` branch is a `oneOf`) | reject | **yes** — `:9633`, and `$ref`-with-siblings at a union target rejects the same way | low |
| `oneOf` × `default` | *unspecified in both specs* | **no** | **P1 — divergence 7** |
| `oneOf` × `services` (operation I/O is a union) | reject | **yes** — `:12038`, `:12066` | low |
| `oneOf` × `minItems`/`uniqueItems`/`contains` on an array branch | branch constraints run under the union's path | `minItems`/`uniqueItems` **yes** (showcase `measurements`); `contains` **no** | P2 |
| `$ref` × `$defs` (nested `$defs` chain, `~1` escape, `..` normalisation, dead `$defs`, cross-file root) | all accepted | **yes** — `:12304`, `:12403`, `:8961`, and reproduced by hand | low |
| `$ref` × `P15` (two files declaring `Page`) | per-target reject; Java per-module OK | **yes** — reproduced; Go/TS/Python reject, Java accepts | low |
| `$ref` × root-name identity (`thing.yaml` + `$defs.Thing`) | reject for **every** target | **yes** — `:11189` | low |
| `$ref` × bare-`$ref` root | alias in Go/TS/Py, none in Java | **no** — unimplemented | **P1 — divergence 8** |

## Verified-good

Checked directly (loader + emitted code for all four targets, unless noted) and found correct **and** covered:

- The full negative matrix of oneOf.md §"Rejected at load time" — all 17 rows reject, with identical diagnostics in all four languages: single-branch wrapper, empty array, typeless/`true`/`{}`/nested-combinator branch, no shared discriminator, non-`required` tag, non-distinct tag values, ambiguous (2+) tags, 2+ inline object branches without `x-<lang>-name`, `<Union>Object` clash with `$defs`, materialized temporal `format`, materialized `contentEncoding`, two same-scalar-kind branches, `integer`+`number` overlap, duplicate `null` branches, and the OpenAPI `discriminator` object (`json_schema.rs:1056`).
- The element-position naming table (oneOf.md §"Unions in element positions") reproduces exactly: `BagValuesItem`, `BagNestedItemItem`, `EntriesValue`, `UniArrayItem`, and the fixpoint `BagValuesItemObject`.
- The nullability `oneOf` is correctly *not* hoisted, and the object inside a nullability wrapper takes the position's own name (`ShowcaseAudit`), so adding/removing nullability never renames the type.
- Free-form object branch: TS `Record<string, unknown>`, Python `dict[str, typing.Any]`, Go/Java a `<Union>Object` wrapper over the verbatim member map — matching oneOf.md §Type mapping, with large-integer preservation tested in all four (`showcase.payload`).
- Same-file discriminated unions: layered selection (token → tag), unknown-tag rejection naming the admissible values, extra keys preserved inside the selected branch (P13), and per-branch constraint enforcement in both directions — tested end-to-end in all four suites.
- `1.5` / `1.0` handling on a numeric union branch: all four agree on the accepted set (`1.0` accepted, `1.5` rejected, cap enforced); only the reason text differs (divergence 12).
- Boolean/integer branch ordering in Python (`isinstance(value, bool)` tested before the numeric arm) is correct.
- `$ref` reject surface: pointer into non-`$defs`, `$id` (root and nested), HTTP/URI ref, `$anchor`, `$dynamicRef`/`$dynamicAnchor`, unresolvable target, `definitions` instead of `$defs`, malformed RFC-6901 escapes, root/`$defs` name coincidence, and two-inputs-one-`Page` (Go/TS/Python reject, Java accepts per its per-module scope) — all reject identically across targets and are covered by inline tests.
- Recursion: optional self-ref (linked list), array-wrapped self-ref (tree), and the cross-file SCC hoist to Python's `_recursive.py` — including when the cyclic edge runs through a `oneOf` (the SCC pass *does* see union branch edges, even though the satisfiability pass does not).
- Dead `$defs` are emitted and exported; `..` path segments are normalised and raise the common input root as specified.
- The Go doc-comment mandate (PRINCIPLES Go §1) is honoured for unions: the interface gets `// Foo is one of: …` and each variant gets `// FooString wraps a string value admissible in the Foo union.`
