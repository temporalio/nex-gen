# allOf / anyOf / not / if-then-else — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/allOf.md` — admitted as a **load-time merge/flatten** into one materialized schema; unmergeable branches reject loudly (P6/P7.1); nothing `allOf`-specific survives past the loader.
- `specs/json-schema/features/anyOf.md` — categorically **rejected at load** (P6): inclusive-or has no decidable selector, so no coherent typed lowering.
- `specs/json-schema/features/not.md` — categorically **rejected at load** (P6), with distinct diagnostics for the degenerate `not: {}` / `not: true` (unsatisfiable) and `not: false` (no-op) forms.
- `specs/json-schema/features/if-then-else.md` — the trio **rejected at load** (P6); a stray `then`/`else` with no `if` is a dead keyword and rejects too.
- `specs/json-schema/PRINCIPLES.md` — P1 (polyglot wire + identical accept/reject set), P6, P7.1, P13, P15.

All findings below were reproduced against a locally built `./target/debug/nexgen` (Go/TS/Python/Java) on scratch schemas in `/tmp/nga`.

## Summary

- The **rejection surface for `anyOf`/`not`/`if`/`then`/`else` is genuinely airtight.** I probed 12 nesting positions (top-level def, nested `$defs`, `properties`, `items`, `oneOf` branch, `additionalProperties`, `contains`, `propertyNames`, `contentSchema`, `allOf` branch, plain-file root, service operation input/output) and every one rejected. Only the *diagnostic wording/location* diverges from the specs.
- The **`allOf` merge is broadly correct** for the top-level conjunct list: type intersection, bound tightening, inclusive/exclusive collapse, `multipleOf` LCM, `enum` intersection, `const` consistency, property/`required` union, the closed-object footgun fix, nested flatten, `$ref` folding, cycle detection, and every reject in the spec's negative matrix all behave as specified.
- **The recursive half of the merge is broken (two P0s).** `merge_two` (`src/parser/json_schema.rs:5339`) unconditionally clears `$ref` and never merges `oneOf`. Those two fields are only pre-flattened for the *top-level* conjuncts; when two branches declare the **same property name** (or `items`, or `additionalProperties`), the recursive merge silently drops a `$ref` target's entire schema, and silently drops a branch's `oneOf` (i.e. nullability).
- **The `$ref` branch fold copies too much.** It folds the target's `x-<lang>-name` and its nested `$defs` into the merged type, producing P15 collisions with an *unresolvable* fix-it, and — for `x-go-name` — a **cross-language load disagreement** (Go rejects, TS/Python/Java accept the same schema). That is a direct P1 violation.
- `merge_multiple_of` overflows on LCM (`:5674`): panics in debug, wraps silently in release.
- **Testing:** allOf has ~24 inline `#[test]`s plus one 4-language round-trip fixture (`Widget`/`WidgetBase` in the showcase). But **8 of 16 accepted-matrix rows and 3 of 14 rejected-matrix rows have no test**, and **none of the recursive/nested merge paths are tested at all** — which is exactly where both P0s live.
- **Zero cross-language conformance-manifest coverage for `allOf`**: `samples/conformance/json-schema.json` has 4 cases, none about merging. The four per-language round-trip tests assert the merged bounds independently, with no manifest tying them to one accepted/rejected value set (P1).
- `anyOf`/`not`/`if` reject tests exist at exactly **one** schema position (`properties.value`); no test proves the reject fires in `$defs`, `items`, a `oneOf` branch, `contains`, `propertyNames`, or an operation I/O.
- Two spec-internal issues surfaced: allOf.md's *Loader behavior* bullet (line 256) says reject a "differing `default`", contradicting its own *Merge algorithm* table (line 151, last-wins). The code implements last-wins.
- `deprecated` is OR-merged by the code but appears nowhere in the spec's merge table.

## Implementation divergences

### 1. Recursive merge silently discards a `$ref` in an overlapping subschema — **P0**

- **Spec cite:** allOf.md:196-207 ("`properties`: union of property names; a name present in **both** branches has its two property subschemas merged recursively"), allOf.md:116-117 ("Resolution reuses [[ref]] entirely"), allOf.md:86-90 ("The merge **resolves the ref and folds the target's schema in**").
- **Code cite:** `src/parser/json_schema.rs:5339-5341` — `fn merge_two` opens with `acc.reference = None;` under the comment "`$ref` branches are already flattened away". That is true only for the conjunct list built by `expand_branches` (`:5225-5317`); `merge_properties` (`:5413`), `merge_items` (`:5514`) and `merge_additional_properties` (`:5463`) call `merge_schema_pair` → `merge_two` on **raw, un-flattened** child schemas.
- **What the spec requires:** the two property subschemas are merged as an `allOf` would be, so a `$ref` branch is resolved and folded in.
- **What the code does:** `acc`'s `$ref` is deleted and `branch`'s `$ref` is never read. The referenced type's constraints vanish.
- **Failing input** (loads clean, emits wrong types in all four languages):
  ```yaml
  $defs:
    Base: { type: object, required: [id], properties: { id: { type: string } } }
    M:
      allOf:
        - { type: object, properties: { p: { $ref: '#/$defs/Base' } } }
        - { type: object, properties: { p: { type: object, properties: { extra: { type: string } } } } }
  ```
  Emitted `Mp` has only `extra`; `Base`'s **required** `id` is gone. `{"p":{"extra":"x"}}` is accepted although the authored schema requires `id`.
  A second, more common shape produces a nonsense diagnostic instead: when *both* branches declare `p: {$ref: '#/$defs/Base'}`, the load fails with `$defs.M.properties.p: a leaf schema requires an explicit type; add one …, or supply the shape via oneOf, allOf, or $ref` — the fix-it tells the user to supply the `$ref` they already supplied.
- **Confidence:** high (reproduced; root cause read directly).

### 2. Recursive merge silently discards a branch's `oneOf` (nullability) — **P0**

- **Spec cite:** allOf.md:196-207 (recursive merge of paired subschemas); allOf.md:411-413 (nullability is expressed through `type`/`oneOf`); allOf.md:228-233 (a branch that is a combinator must **reject**, never be dropped).
- **Code cite:** `src/parser/json_schema.rs:5339-5376` — `merge_two` handles `ty`, `title`, `description`, `properties`, `required`, `additional_properties`, `items` and the `extra` map. `Schema::one_of` (`:70-71`) is a struct field and is **never touched**. `reject_combinator_branch` (`:5198`) is called only from `expand_branches` (`:5233`), never from the recursive pair merge.
- **What the spec requires:** either merge the paired subschemas coherently, or reject the combinator branch.
- **What the code does:** `acc.one_of` is retained and `branch.one_of` is **dropped on the floor** — no merge, no reject.
- **Failing input:**
  ```yaml
  $defs:
    M:
      allOf:
        - { type: object, properties: { p: { type: string, minLength: 2 } } }
        - { type: object, properties: { p: { oneOf: [ { type: string }, { type: "null" } ] } } }
  ```
  Emits `P *string` with no null branch; `{"p":null}` is rejected at runtime ("explicit null not allowed") even though the schema declares `p` nullable. Swapping branch order (`oneOf` first) also loses the null branch, because the later branch's `type: string` merges onto the union node.
- **Confidence:** high (reproduced in Go; the loss is in the loader, so all four languages are equally wrong).

### 3. `$ref` branch fold copies the target's `x-<lang>-name` onto the merged type — **P1** (cross-language load disagreement)

- **Spec cite:** allOf.md:264-276 ("Naming follows the ordinary rule … an `allOf` that **is** a named `$defs` entry takes the def name"); allOf.md:121-127 ("the merged type is an independent, standalone type"); PRINCIPLES P15 ("a name synthesized from a member moves with that member"), P1 ("One schema generates models in every supported language").
- **Code cite:** `src/parser/json_schema.rs:5247-5263` folds the whole raw target `Schema` into the branch list; `own_conjunct` (`:5184-5193`) strips only `$ref`/`allOf`/`$comment`/`examples`, so `x-go-name` etc. survive and are copied into the merged node's `extra` by `merge_two` (`:5364-5374`).
- **What the code does:**
  ```yaml
  $defs:
    Base:   { type: object, x-go-name: Renamed, required: [id], properties: { id: { type: string } } }
    Widget: { allOf: [ { $ref: '#/$defs/Base' }, { type: object, required: [name], properties: { name: { type: string } } } ] }
  ```
  `nexgen go` → `identifier collision in go output: type 'Base' and type 'Widget' both map to 'Renamed'`.
  `nexgen typescript` / `python` / `java` on the **same file** → succeed, emitting `Base` and `Widget`.
- **Why P1 not P0:** each individual target is either loud or correct; the violation is that the accept/reject decision differs per language, which P1 forbids. If the merged type also carries its own override the diagnostic becomes `cannot merge differing 'x-go-name' values ("Renamed" vs "W")` — a merge error for a keyword that is not a constraint.
- **Confidence:** high (reproduced across all four backends).

### 4. `$ref` branch fold copies the target's nested `$defs`, duplicating types with an unresolvable fix-it — **P1**

- **Spec cite:** allOf.md:86-90 ("the referenced type's **constraints** are copied"); PRINCIPLES P15 ("Every escape hatch has to reach the name it is offered for: … a fix-it lying about the remedy").
- **Code cite:** same fold at `src/parser/json_schema.rs:5247-5263` + `own_conjunct` `:5184-5193`; the copied `$defs` is then re-walked as real definitions by `normalize_children` (`:5043-5073`) and collected as models by `collect_json_models_from_defs` (`:4782`).
- **Failing input:**
  ```yaml
  $defs:
    Base:
      type: object
      properties: { inner: { $ref: '#/$defs/Base/$defs/Inner' } }
      $defs: { Inner: { type: object, properties: { z: { type: string } } } }
    Widget:
      allOf: [ { $ref: '#/$defs/Base' }, { type: object, properties: { name: { type: string } } } ]
  ```
  → `identifier collision …: type 'Base.Inner' and type 'Widget.Inner' both map to 'Inner'; disambiguate with an x-go-name override` (identical in all four languages). The user cannot apply the override: `Widget.Inner` is not authored anywhere.
- **Confidence:** high (reproduced in all four backends).

### 5. `multipleOf` LCM overflows — **P1** (P0 in a release build)

- **Spec cite:** allOf.md:169-171 ("merge to their **LCM**. All supported divisors are positive integers, so the LCM is a positive integer; no new form appears").
- **Code cite:** `src/parser/json_schema.rs:5650-5676`, the `let lcm = a / gcd * b;` at `:5674` over `i64`.
- **Failing input:** `allOf: [{type: integer, multipleOf: 9007199254740991}, {type: integer, multipleOf: 9007199254740989}]` → `thread 'main' panicked at src/parser/json_schema.rs:5674: attempt to multiply with overflow`. `Cargo.toml` declares no `[profile.release] overflow-checks`, so a release binary wraps instead and emits a **silently wrong divisor**.
- **Secondary (lower confidence, may belong to [[multipleOf]] not [[allOf]]):** even without overflow the LCM can exceed the ±(2^53−1) integer cap — `multipleOf: 2^52` ∩ `multipleOf: 3` merges to `13510798882111488` and is accepted, although no integer field value can satisfy it. The same value authored directly is also accepted, so the missing cap is probably [[multipleOf]]'s gap rather than a merge-specific one.
- **Confidence:** high for the overflow; medium that the cap belongs here.

### 6. `then` / `else` without a sibling `if` get the generic conditional diagnostic, not the spec's dead-keyword one — **P2**

- **Spec cite:** if-then-else.md:70-79 ("A stray `then` or `else` with no sibling `if` is a **dead keyword** — also rejected, with a diagnostic that it has no effect"; loader bullet: "reject as a no-op keyword (it can never fire)").
- **Code cite:** `src/parser/json_schema.rs:1502-1504` maps `"if" | "then" | "else"` to a single string, emitted by the loop at `:1583-1605`.
- **What the code does:** `{type: object, then: {required: [x]}}` → `conditional schemas ('if'/'then'/'else') are not supported; model the alternatives as a 'oneOf'` — never mentions that the keyword can never fire. Note the sibling reject family *does* honour this distinction for `not` (`:1563-1582` produces separate "unsatisfiable" / "no-op" texts), so the omission is inconsistent within the same file.
- **Confidence:** high.

### 7. `anyOf` and `not` diagnostics omit fix-its the specs promise — **P2**

- **Spec cite:** anyOf.md:67-79 ("The diagnostic offers the coherent alternatives: 1. [[oneOf]] … 2. [[allOf]] … 3. A single widened branch"); not.md:78-89 ("1. Positive type/constraints 2. enum/const 3. **The complementary bound** … `not: {maximum: 10}` → [[exclusiveMinimum]] `10`").
- **Code cite:** `src/parser/json_schema.rs:1499-1501` (anyOf: only the `oneOf` alternative) and `:1575-1576` (`not`: only positive type/constraints and `enum`; no complementary-bound hint).
- **Confidence:** high.

### 8. Reject diagnostics from a merged node lose the branch location — **P2**

- **Spec cite:** anyOf.md:68 / not.md:74 / if-then-else.md:76 — "reject with a **located** diagnostic"; allOf.md:241-242 — "recurse into each branch to report the inner fault".
- **Code cite:** `normalize_schema` (`:4954-4962`) folds branches into one node before validation, so `validate_schema_common` (`:1533`) reports the merged node's context.
- **Observed:** `allOf: [{type: object, properties: {a: …}}, {type: object, unevaluatedProperties: false}]` → `$defs.M: 'unevaluatedProperties' is not supported`, not `$defs.M.allOf[1]`. Same for a `then` inside a branch. (Grammar-level faults *are* correctly located, per `all_of_validates_raw_branch_grammar_before_merging` — this only affects post-merge rejects.)
- **Confidence:** high.

### 9. `default` merge: the spec contradicts itself; the code follows the table — **P2**

- **Spec cite:** allOf.md:151 (Merge table: `title`/`description`/`default` are **last-wins**, "Reject when: **never**") vs allOf.md:255-257 (Loader behavior: "Reject the unmergeable pairs: … differing `format`/**`default`**").
- **Code cite:** `src/parser/json_schema.rs:5579` — `"default" | "title" | "description" => Ok(branch.clone())`.
- **Disposition:** the code matches the Merge-algorithm table and the accepted-matrix row at allOf.md:330; the Loader-behavior bullet at :256 is stale and should drop `default`. Recorded as a divergence because one spec statement is currently violated.
- **Confidence:** high.

### 10. `deprecated` is OR-merged but unspecified — **P2**

- **Code cite:** `src/parser/json_schema.rs:5576-5578`.
- The behaviour is sane and tested (`all_of_merges_deprecated_with_or_and_discards_inert_annotations`, `:9764`), but allOf.md's merge tables list no rule for `deprecated`; a reader would expect the catch-all reject at `:5612-5617`. Spec should add the row.
- **Confidence:** high.

### 11. A redundant same-axis bound pair is rejected on a plain node but silently collapsed inside an `allOf` branch — **P2**

- **Spec cite:** allOf.md:153-158 ("the merged result never carries a same-axis pair … so [[maximum]]'s single-node redundancy reject never fires on **merge output**") — the exemption is for merge *output*, not for a pair the author wrote inside one branch. P7.1 says reject ambiguity loudly.
- **Code cite:** `finalize_merged` (`:5730-5732`) runs `collapse_numeric_pair` (`:5756`) on every merged node, before `validate_numeric_constraints` ever sees the branch.
- **Observed:** `{type: integer, minimum: 1, exclusiveMinimum: 5}` on a plain property → `specify exactly one of 'minimum' or 'exclusiveMinimum', not both`. The identical typo inside an `allOf` branch loads clean and emits `must be > 5`.
- **Confidence:** high.

### 12. A `$ref` to a `oneOf` union cannot carry any annotation sibling — **P2** (specs compose into a usability hole)

- **Spec cite:** ref.md:47-49 (siblings are an implicit `allOf`, merged) ∘ allOf.md:228-233 (a branch that is a `oneOf` **rejects**).
- **Code cite:** `normalize_schema:4926` classifies `{$ref: U, description: …}` as ref-with-siblings → `expand_branches:5233` → `reject_combinator_branch:5199`.
- **Observed:** `p: {$ref: '#/$defs/SomeUnion', description: "a union"}` → `an 'allOf' branch cannot be a 'oneOf' …` — an `allOf` diagnostic for an `allOf` the user never wrote, and no way to document a union-typed member. `x-<lang>-name` is exempt (`is_ref_with_name_overrides_only`, `:92-103`); `description`/`deprecated` are not.
- **Confidence:** high on the behaviour; medium that it is a *divergence* rather than an unstated-but-intended consequence — no spec sentence covers it either way.

## Testing gaps

### 1. No test anywhere exercises the recursive (nested) `allOf` merge — **P0-class gap**

- **Untested:** overlapping property names merged recursively, `items` merged recursively, schema-valued `additionalProperties` merged recursively. Every existing test merges only *disjoint* property sets (`all_of_object_base_extension_merges_union`, `:9467`) or scalars.
- **Spec line mandating it:** allOf.md:196-207 and the accepted-matrix row at allOf.md:323 ("Overlapping property merged recursively | `{allOf:[{properties:{n:{minLength:2}}},{properties:{n:{maxLength:8}}}]}`") — the one row that would have caught divergences #1 and #2.
- **Where:** `src/parser/json_schema.rs` inline tests, next to `all_of_object_base_extension_merges_union`.
- **Suggested cases:** (a) the matrix row verbatim, asserting both `minLength` and `maxLength` survive; (b) `p: {$ref: Base}` in one branch + `p: {type: object, properties: {extra}}` in the other, asserting `Base`'s fields and `required` survive; (c) `p: {$ref: Base}` in *both* branches, asserting the `$ref` survives; (d) `p: {oneOf:[{type:string},{type:"null"}]}` in one branch + `p: {type:string,minLength:2}` in the other, asserting either a coherent nullable merge or a reject.

### 2. No cross-language conformance-manifest case for `allOf` — **P0-class gap**

- **Untested:** that all four languages agree on the accepted/rejected value set for a merged schema.
- **Spec line:** allOf.md:74-76 (P1 grounding: "a merged single schema round-trips identically across all four targets") and the runtime-fixture list at allOf.md:351-372.
- **Where:** `samples/conformance/json-schema.json` (4 cases today) + `tests/json_schema_conformance_manifest.rs`.
- **Suggested case:** an `allof-merge` case over the existing `Widget` fixture: accepted `samples/wire/json_schema/showcase/widget.json`; `parse_failures` for `size: 5` and `size: 25` with `expected_paths: ["size"]`; a missing-`name` failure; consumers pointing at the four existing anchors (`TestJSONSchemaShowcaseAllOfMerge`, etc.).

### 3. `anyOf` / `not` / `if` / `then` / `else` rejects are tested at exactly one schema position — **P1**

- **Untested:** the reject firing from inside `$defs`-of-a-`$defs`, `items`, a `oneOf` branch, `additionalProperties`, `contains`, `propertyNames`, a plain-file root schema, and a service operation `input`/`output`. `rejects_structural_keywords_with_fixits` (`:9078-9128`) only places each keyword under `properties.value` of a root object.
- **Spec line:** anyOf.md:68 / not.md:74 / if-then-else.md:76 — "Any `X` present → reject".
- **Where:** `src/parser/json_schema.rs`, alongside `rejects_structural_keywords_with_fixits`.
- **Suggested case:** a table-driven test crossing `["anyOf", "not", "if", "then", "else"]` with ~8 position templates, asserting the load fails and the message names the keyword. (I verified all 40 combinations pass today by hand — this test locks that in, and it is cheap.)

### 4. Missing accepted-matrix rows — **P1**

Rows from allOf.md:311-331 with no test:

| Row (allOf.md line) | Missing assertion |
|---|---|
| Merge lower + upper into an interval (:317) | only the *rejecting* empty-interval case is tested (`:9653`) |
| Tighten across inclusive/exclusive, **lower** side (:316) | only the upper side is tested (`:9519`) |
| String length tighten (:319) | no `minLength`→max / `maxLength`→min test |
| `const` consistent with `enum`/range (:321) | only the two rejecting variants are tested |
| Overlapping property merged recursively (:323) | see gap 1 |
| Nested `allOf` flattened (:328) | no test; `allOf: [true, true]` (`:11848`) is a different case |
| Identity branch dropped (:329) | no `allOf: [{…}, true]` / `[{…}, {}]` accept test |
| Differing metadata annotation, last-wins (:330) | no `default`/`title`/`description` last-wins test |

Where: inline tests next to the existing `all_of_*` block (`:9451-9855`).

### 5. Missing rejected-matrix rows — **P1**

| Row (allOf.md line) | Status |
|---|---|
| `const` violates a sibling, e.g. `{allOf:[{const:5},{maximum:4}]}` (:340) | untested — only `const`-vs-`enum` (`:9812`) is |
| Branch is `anyOf` / `if` (:345) | untested — only `oneOf` (`:9633`) and `not` (`:9790`) are |
| Merged synthesized-name collision, P15 (:349) | untested for the `allOf` path |
| Cyclic `$ref` **through two types** (A→B→A) (:348) | only direct self-merge is tested (`:9834`) |

### 6. No `not` / `if` runtime-negative for the degenerate forms in a *non-leaf* position — **P2**

`rejects_not_empty_unsatisfiable` / `_true_` / `_false_noop` (`:12143-12159`) all use the same leaf position. `{anyOf: []}` and single-branch `{anyOf: [X]}` (anyOf.md:99-100) have no test at all, though they share the blanket reject path.

### 7. `multipleOf` LCM has no overflow or large-value test — **P1**

`all_of_multiple_of_merges_to_lcm` (`:9539`) uses `2` and `3`. Add a case asserting a graceful reject (not a panic) for two coprime near-2^53 divisors, and one asserting the merged divisor is rejected when it exceeds the safe-integer cap.

### 8. `$ref`-fold hygiene is untested — **P1**

No test asserts that folding a `$ref` branch does **not** copy the target's `x-<lang>-name`, `$defs`, or other identity-bearing keys. Add a test with `Base` carrying `x-go-name` + a nested `$defs`, asserting the merged type keeps its own name and declares no duplicate nested defs, and that the same schema loads identically for all four languages.

## Combination gaps

| Feature A × Feature B | Spec says | Tested? | Risk |
|---|---|---|---|
| allOf × ref — overlapping property, one/both branches a `$ref` | recursive merge, target folded in (allOf.md:196-207, :86-90) | **no** | **High — broken (divergence 1); silently loses the target's fields** |
| allOf × oneOf/nullability — branch declares the same property as a nullable union | recursive merge, or reject a combinator branch (allOf.md:196-207, :228-233) | **no** | **High — broken (divergence 2); nullability silently dropped** |
| allOf × ref — target carries `x-<lang>-name` | merged type takes the def name (allOf.md:264-276, P15) | **no** | **High — broken (divergence 3); per-language accept/reject split** |
| allOf × ref — target carries nested `$defs` | only constraints fold in (allOf.md:86-90) | **no** | Med — broken (divergence 4); unresolvable P15 fix-it |
| allOf × multipleOf — large coprime divisors | LCM stays a positive integer (allOf.md:169-171) | **no** | Med — panics/wraps (divergence 5) |
| allOf × oneOf — an `allOf` **inside** a `oneOf` branch | admitted (oneOf branches are normalized; allOf.md:405-410 only rejects the reverse) | **no** | Med — works today (verified by hand, incl. a `const`-tag discriminator produced by the merge), but unpinned |
| allOf × oneOf — branch is a `$ref` to a union def | reject (allOf.md:228-233) | **no** (only a literal inline `oneOf` branch is tested) | Med — works today |
| allOf × unevaluatedProperties / unevaluatedItems / dependentSchemas | stay rejected (allOf.md:414-417) | **no** combination test | Low — works today (rejects both as a sibling and inside a branch) |
| allOf × dependentRequired | per-trigger union of dependent lists (allOf.md:207) | **no** | Med — works today |
| allOf × propertyNames | merge recursively (allOf.md:201-204) | **no** | Med — works today |
| allOf × contains / minContains / maxContains | identical `contains` dedupes, counts tighten; distinct matchers reject (allOf.md:186, :234-237) | reject only (`:11832`) | Med — the tighten path works today |
| allOf × additionalProperties — `false` in one branch, a **value schema** in the other | "if any branch is closed, the merged object is closed to the union" (allOf.md:209-221) | **no** | Med — code returns `false` and drops the value schema; spec does not address this pair |
| allOf × required — a name required by one branch, declared by another | union (allOf.md:206) | partially (`:9467`) | Low — works; the undeclared-name reject also fires correctly |
| allOf × const/enum — `const` vs a merged *numeric/length* bound | reject (allOf.md:147, :340) | **no** | Med — works today (delegated to the const validator) |
| allOf × items | merge the two item schemas (allOf.md:200) | **no** | Med — shares the divergence-1/2 root cause |
| allOf × maximum/minimum — redundant pair authored *inside* one branch | P7.1 loud reject | **no** | Low — silently collapsed (divergence 11) |
| allOf × P15 — two anonymous merges recasing to one name | reject with fix-it (allOf.md:349) | **no** | Low |
| anyOf/not/if × every nesting position | reject everywhere (P6) | one position only | Med — all positions verified by hand today |
| allOf × cross-language wire agreement | P1 | per-language tests only, no manifest case | Med |

## Verified-good

- **Every `anyOf`/`not`/`if`/`then`/`else` position I could construct rejects**: top-level `$defs` entry, nested `$defs` (`$defs.Outer.$defs.Inner`), `properties`, `items`, `oneOf` branch, `additionalProperties` (bool and schema), `contains` matcher (via `validate_schema_common` at `:2543`), `propertyNames` (via the assertion allowlist at `:2869`), `contentSchema` (rejected by the `contentSchema` reject itself, `:1625`), inside an `allOf` branch (`reject_combinator_branch`, `:5198`), a plain-file root schema, and a service operation `input`/`output` (`validate_schema_refs` → `validate_schema_common`, `:3356`).
- `not: {}` / `not: true` → "unsatisfiable", `not: false` → "no-op", and double negation all reject with the spec's distinct texts (`:1563-1582`, tests `:12143-12165`).
- **`allOf` leaves no residue downstream**: `rg allOf|anyOf|not` over `src/generator/json_schema/`, `src/planning/`, `src/spec.rs` returns nothing — matching allOf.md:264-270 ("`allOf` emits nothing of its own").
- Top-level merge, all confirmed by generating Go: same-axis lower-bound tightening (→ `minimum 4`), inclusive/exclusive collapse (→ `exclusiveMaximum 8`), lower+upper interval, `multipleOf` LCM (→ 6), `minLength`/`maxLength` tighten, `minItems`/`maxItems` tighten, `uniqueItems` OR, `minContains`/`maxContains` tighten with an identical `contains`, `enum` intersection (→ `["b","c"]`), `const` consistent with a range, `dependentRequired` per-trigger union, `propertyNames` recursive merge, `title`/`description`/`default` last-wins, `deprecated` OR, `$comment`/`examples` discarded.
- The closed-object footgun fix works exactly as allOf.md:209-221 specifies (closed to the **union** of both branches' properties; the emitted Go drops the `AdditionalProperties` catch-all and rejects unknown keys).
- Every reject in allOf.md's negative matrix that I exercised fires with the right text: disjoint `type` (including a `{type: "null"}` branch), empty numeric interval (delegated to [[maximum]]), empty `enum` intersection, disagreeing `const`, `const` violating a numeric or length sibling, differing `format`, distinct `pattern`s, distinct `contains`, `false` branch, combinator branch (`oneOf`/`anyOf`/`not`/`if`), `allOf: []`, single-branch wrapper, non-schema entry, `allOf: [true, true]`, unresolvable `$ref` branch, direct and A→B→A merge cycles.
- Raw branch grammar is validated **before** the merge can discard a malformed branch (`parse_json_documents:636-642`, test `:9663`), so a typo in a branch that would be overwritten still reports at its authored position.
- `$ref`-with-siblings is merged as the implicit-`allOf` sugar, and an `x-<lang>-name`-only `$ref` sibling is correctly *not* merged — it renames the member while the hoisted inline type keeps its positional name (`is_ref_with_name_overrides_only`, `:92-103`; verified end-to-end in Go).
- A four-language round-trip fixture for base-type extension + bound tightening exists and is real: `samples/schemas/showcase.nexusrpc.yaml:708-736` with `TestJSONSchemaShowcaseAllOfMerge` (`samples/go/tests/json_schema_showcase_test.go:463`) and the TS/Python/Java equivalents, asserting the merged property/required union, the tightened `[10, 20]` bound in both directions, and serialize-side rejection (P12).
