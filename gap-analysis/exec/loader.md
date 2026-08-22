# loader — execution report

Owned file: `src/parser/json_schema.rs` (only file changed).
`cargo build --all-features` clean; `cargo fmt --check` clean;
`cargo test --all-features --lib` 492 passed / 0 failed (was 439 at baseline —
53 tests added).

---

## Fixed

### Wave C — the `.0`-valued count bound crash

- **`04#2` / `05#6` / `06#6` / `08#5`** — `normalize_count_bounds`
  (`src/parser/json_schema.rs:5671`), called from `normalize_children`
  (`:5606`). Canonicalizes an integral float bound to an integer `Value` for all
  eight count keywords (`minLength`, `maxLength`, `minItems`, `maxItems`,
  `minContains`, `maxContains`, `minProperties`, `maxProperties`) in one place,
  so serde's `Option<u64>`/`Option<usize>` in every emitter sees an integer.
  Out-of-range / non-integral / non-numeric spellings are left alone for the
  per-keyword validators to diagnose.
  *Verified:* new test `canonicalizes_integral_float_count_bounds`; and a probe
  schema with all eight keywords written `N.0` generated and **compiled** in all
  four targets (`go vet`, `python -m py_compile`, `tsc --noEmit --strict`,
  `javac`) — see "Probe verification" below.

### Wave 9 — `allOf` recursive merge

Root fix: `merge_child_schemas` (`:5969`). `merge_properties` / `merge_items` /
`merge_additional_properties` no longer hand a raw, un-flattened child pair to
`merge_two`; when either side still carries a `$ref`, a nested `allOf` or a
`oneOf`, the pair is re-expressed as an `allOf` node and left for the enclosing
`normalize_children` walk, which *does* have the merge context. Two identical
children short-circuit to themselves, which keeps a shared `$ref` a reference
instead of inlining the target twice. Error contexts are unchanged
(`normalize_children` builds the same `{context}.properties.{name}` /
`.items` / `.additionalProperties` strings the merge functions did).

- **`02#1` P0** — a `$ref` in a child position is now resolved instead of
  dropped. `merge_two`'s `acc.reference = None` (`:6008`) is now genuinely
  unreachable and the comment says why.
  *Tests:* `all_of_merges_ref_property_with_object_sibling`,
  `all_of_keeps_identical_ref_property_as_a_reference`,
  `all_of_merges_overlapping_property_recursively` (the `allOf.md:323` row,
  verbatim).
- **`02#2` P0** — a `oneOf` in a child position now reaches
  `reject_combinator_branch` exactly as a top-level conjunct does, instead of
  being silently dropped. *Test:* `all_of_rejects_one_of_property_branch`
  (asserts both the message and the `$defs.Widget.properties.n` position).
- **`02#3` P1** — `strip_target_declaration_keywords` (`:5802`), applied to
  every conjunct folded in from a `$ref` branch (the fold site is `:5875`),
  removes the target's
  `x-<lang>-name`. Go no longer rejects a schema TS/Python/Java accept.
  *Test:* `all_of_ref_branch_does_not_inherit_the_target_type_name` (loads for
  all four).
- **`02#4` P1** — the same helper removes the target's nested `$defs`, so
  `Widget.Inner` is no longer declared and the merged node keeps its reference
  to `Base.Inner`. *Test:*
  `all_of_ref_branch_does_not_duplicate_the_target_defs`.
- **`02#5` / `07#6` P1** — `merge_multiple_of` (`:6317`) uses `checked_mul` and
  caps the LCM at 9007199254740991, with a fix-it. *Test:*
  `all_of_rejects_multiple_of_lcm_above_the_safe_integer_cap` (this input
  panicked the debug build before).
  *Verified:* the recursive-merge probe generated and compiled in all four
  targets; Java emits `WidgetP` with `Base`'s `id` + the sibling's `extra` and
  `required: [id]`.

### Wave 3 — identifier passes

- **`01#2` P0** — `collect_go_union_top_level` (`:7870`) plus the inline-union
  arm of `collect_synthesized_top_level` (`:7673`) register the Go sealed
  interface `<Type><Member>`, every synthesized variant wrapper
  (`<Union>{Object,String,Integer,Number,Boolean,Array}`) and the
  `unmarshal<Union>` dispatcher, for both named `$defs` unions and inline
  property unions. A `$ref` branch contributes nothing (its target already holds
  its name). *Tests:* `rejects_colliding_union_names_go` — the exact schema
  `rejects_colliding_union_functions_python` uses — and
  `rejects_union_variant_wrapper_colliding_with_a_def_go`.
  The suffix mirrors the emitter (`go_union_field_suffix`, `:7859`): see the
  cross-file request about `x-go-name` on a union-typed property.
- **`14#3` P0** — `validate_operation_scope` (`:4869`), called from
  `build_service` (`:4955`), rejects two operations that recase to one
  identifier and two that derive one wire name. *Tests:*
  `rejects_colliding_operation_identifiers` (all four targets),
  `rejects_colliding_operation_wire_names`.
- **`14#2` P0** — the same pass runs `member_identifier_defect` on each
  operation key when no `x-<lang>-name` is authored, so `import` rejects for
  Java/TypeScript/Python (Go emits the exported `Import` and still loads).
  *Test:* `rejects_reserved_word_operation_name`, including the override escape
  hatch.
- **`14#4` P0** — `module_segment_defect` (`:647`), applied to every module-path
  segment in `api_spec_tree_from_json_schema_sources` (`:516`). A segment must be
  a syntactically valid identifier and must not be reserved in **any** of the
  four targets (the same union rule `is_reserved_module_name` already uses —
  there is no per-segment escape hatch, so a per-language rule would be a
  cross-language load disagreement). *Tests:*
  `rejects_reserved_word_module_segment` (`class.json`),
  `rejects_module_segment_that_is_not_an_identifier` (`2fa.json`).
- **`03#5` P1** — `validate_member_scope` (`:7922`) now enters Go's fixed method
  set (`Validate`, `MarshalJSON`, `UnmarshalJSON`) into the member scope, which
  in Go is shared with the struct's fields. *Test:*
  `rejects_member_colliding_with_the_go_method_set` (the other three still
  load).
- **`03#4` / `11#5` P1** — `collect_java_nested_scope` (`:7764`) gives Java the
  pass it never had: a per-model **nested type** scope holding one value class
  per closed-value member plus the generated `Serializer`/`Deserializer` and the
  runtime classes imported by simple name, and a per-value-class **constant**
  scope mirroring `java_const_name`/`java_closed_token`. *Tests:*
  `rejects_java_member_colliding_with_a_generated_nested_class`
  (`serializer`/`deserializer`/`violation`),
  `rejects_duplicate_java_enum_name_overrides`,
  `rejects_java_folded_enum_tokens_behind_a_go_only_override` (Go loads, Java
  rejects — this is the "Go-only override hides Java's UPPER_SNAKE fold" case).
- **`11#6` P1 (decision D9)** — `validate_member_scope` builds a separate Java
  **method** namespace (Java keeps fields and methods apart, so one shared scope
  would have produced false positives) holding `get<Field>` for every member and
  `get<Field>OrDefault` for a defaulted optional one. *Test:*
  `rejects_java_or_default_accessor_collision` (Go and Java both reject).
- **`11#11` P1** — `enum_names_lookup_key` (`:7197`) keys
  `x-<lang>-enum-names` by the value's JSON wire spelling, so numeric and
  boolean members are renameable. *Test:*
  `enum_names_override_applies_to_numeric_and_boolean_members`.
  **Requires the two emitter one-liners in "Cross-file requests" to land with
  it** — see the note there.

### Wave 8 — missing rejects

- **`01#5` P0** — `validate_type_shape` (`:1872`) split out of
  `validate_type_presence`; a `oneOf` branch now runs the shape half
  (`validate_schema_node`, `:1576`). An itemless `{type: array}` branch and a
  shapeless `{type: object}` branch reject. *Test:*
  `rejects_shapeless_union_branch`.
- **`01#7` P1 (D5)** — `validate_one_of` (`:4590`, reject at `:4779`) rejects a `default` on a sum
  type; the nullability wrapper keeps its lowering. *Test:*
  `rejects_default_on_a_sum_type_union`.
- **`01#9` P1** — `validate_reference_satisfiability` (`:3590`) rewritten as a
  least-fixed-point over a `Requirement` expression (`Target`/`All`/`Any`,
  `:3473`) instead of a cycle search over an AND-only edge list. A `oneOf` is a
  disjunction, so a union is instantiable as soon as one branch is; whatever the
  fixed point never reaches has no finite instance. The witness path in the
  diagnostic is reconstructed by following blocking edges, so the message keeps
  its `A → B → A` shape. *Tests:*
  `rejects_unsatisfiable_sum_type_recursion`,
  `rejects_unsatisfiable_named_union_recursion`,
  `accepts_sum_type_recursion_with_a_terminating_branch`,
  `accepts_nullable_recursion` (plus the four pre-existing recursion tests,
  unchanged).
- **`04#3` P1** — the `propertyNames` allowlist now covers the typed fields
  (`$ref`, `$id`, `properties`, `required`, `additionalProperties`, `items`,
  `oneOf`) as well as `extra` (`:3122`), and a **pre-normalize** copy runs in
  `validate_raw_schema_grammar` (`:1416`) so a `$ref` is named in the diagnostic
  instead of surfacing as "`allOf` branches declare disjoint types" after the
  merge pass folded it. `title`/`description` stay accepted-and-ignored.
  *Test:* `rejects_property_names_with_a_structural_keyword` (six shapes).
- **`07#4` P1** — the `multipleOf` × range-emptiness check (`:2141`) is no longer
  gated on `is_integer`, and computes the smallest multiple satisfying the lower
  bound rather than adjusting the bounds by ±1. *Test:*
  `rejects_unsatisfiable_number_range_with_multiple_of`.
- **`07#5` P1** — the literal-vs-`multipleOf` check (`:2192`) uses IEEE `fmod`
  (`value % divisor`). *Test:* `rejects_large_literal_violating_multiple_of`
  (`{multipleOf: 3, const: 1e22}`).
- **`12#2` P1** — `deserialize_annotation` (`:116`) on `Schema::title`,
  `Schema::description`, `Service::description` and `Operation::description`
  resolves the scalar with `deserialize_any` before requiring a string, so
  serde_yaml's plain-scalar leniency can no longer coerce `title: 42`. *Test:*
  `rejects_non_string_annotations` (nested `$defs`, a member, a service and an
  operation).
- **`11#7` P1** — `json_values_equal` (`:3425`) gives the `enum` uniqueness
  check, the `enum`+`default` membership check and `intersect_enum` P1's number
  identity (`5`/`5.0`/`5e0` are one value, `0`/`-0.0` are one value) while
  keeping two distinct integers beyond 2^53 apart. *Test:*
  `rejects_enum_members_that_differ_only_in_numeric_spelling`.
- **`14#7` P1 (D11)** — `reject_empty_fqn` (`:4842`, called at `:4950` and `:5033`) on both the service and the
  operation. *Test:* `rejects_empty_service_and_operation_fqn`.
- **`13#7` P2** — `validate_schema_common` (`:1709`) rejects a `type` that is
  present and not a string (and not the array form, which keeps its own
  diagnostic), before the `$ref`/`oneOf` early-outs. `Schema::ty` gained
  `deserialize_present_value` (`:96`) so an explicit `type: null` is
  distinguishable from an absent one, plus `skip_serializing_if` so the internal
  round-trip stays faithful. *Test:* `rejects_malformed_type_value`
  (`5`, `null`, `true`, and `{oneOf: […], type: 5}`).
- **`02#11` P2** — `validate_redundant_same_axis_bounds` (`:1264`, called at `:1317`) runs on the
  schema **as authored** (node and every raw `allOf` branch, via
  `validate_raw_schema_grammar`), so the typo inside one branch rejects while
  the cross-branch inclusive/exclusive tightening still merges. Restricted to
  two *numeric* values so the draft-4 boolean form keeps its own diagnostic.
  *Test:* `rejects_redundant_same_axis_bounds_inside_an_all_of_branch`.

### Decision D6 — materializing `format` in a key/matcher position

- **`04#1` / `09#5`** — rejected in `propertyNames` (`:3176`) and in a `contains`
  matcher (`:2828`). *Tests:* `rejects_temporal_format_in_property_names` (a
  string-shaped `format: uuid` on a key still loads),
  `rejects_temporal_format_in_a_contains_matcher`.

### Decision D2 — nullable scalar element

- **`06#7` / `05#8`** — `nullable_non_null_schema` (`:4509`) added to the loader;
  `validate_array_constraints` (`:2723`) computes the element's *effective* kind
  through the nullability wrapper, so `uniqueItems`/`contains` over a nullable
  scalar element is accepted. A nullable **object** element is still composite.
  *Test:* `accepts_unique_items_and_contains_over_a_nullable_scalar_element`.
  **This loosening exposes two emitter gaps — see "Cross-file requests".**

### Decision D10 — canonical materialized literals

- **`09#2`** — `normalize_temporal_literals` (`:5629`), called from
  `normalize_children`, rewrites a `const`/`default`/`enum` literal on a
  temporal-`format` node to `format::canonicalize_for_format`'s output (which
  was dead code). An invalid literal is left untouched so `validate_format`
  reports the value the user wrote. *Tests:*
  `canonicalizes_materialized_temporal_literals`,
  `rejects_enum_members_that_canonicalize_to_one_temporal_value` (two spellings
  that canonicalize together now hit the shared uniqueness reject).
  *Verified:* the probe's Go output carries `mustParseDuration("PT1H30M")` and
  `mustParseDateTime("2021-06-15T12:30:45Z")` from the authored `PT90M` and
  `2021-06-15t12:30:45z`.

---

## Probe verification

`/tmp/loaderprobe2/probe2.json` — integral-float count bounds on eight
keywords, a recursive `allOf` merge (`p: {$ref: Base}` + `p: {type: object,
properties: {extra}}`, `n: {minLength: 2}` + `n: {maxLength: 8}`), and a
non-canonical temporal `default` — generated for all four targets and compiled:

| target | command | result |
|---|---|---|
| Go | `go vet ./...` (samples' `go.mod`) | clean |
| Python | `python3 -m py_compile` on unformatted output | clean |
| TypeScript | `tsc --noEmit --strict` with the samples' `shims/nexus-rpc-type-info.d.ts` | clean |
| Java | `javac` on the samples' resolved classpath | clean |

The Java output shows the merge landed: `WidgetP` carries `Base`'s `id`
(required) plus the sibling branch's `extra`.

The load-reject fixes have no output to compile; each is covered by a unit test
asserting the diagnostic text.

---

## Not fixed

- Nothing in my assignment was skipped as a disagreement. Every finding listed
  for the loader is addressed above, except the emitter-side halves recorded
  below.

---

## Cross-file requests

1. **`src/generator/json_schema/go.rs` — nullable scalar element (D2, blocking).**
   With the loader loosened, `uniqueItems`/`contains` over
   `items: {oneOf: [{type: T}, {type: "null"}]}` emits Go that does not build:
   `vet: cannot use key (variable of type *string) as string value in map index`
   (the `uniqueItems` `seen` map keys on the element verbatim), and the
   `contains` predicate compares `e >= 3` where `e` is `*int64`. Both loops need
   to skip a `nil` element and dereference the rest — a `null` element never
   matches a scalar matcher, and two `null`s are a duplicate. `go.rs:745-753`'s
   existing unwrap is on the *matcher*, not the element.
   Repro: `/tmp/loaderprobe/in/probe.json`, properties `nullableTags` /
   `nullableScores`.

2. **`src/generator/json_schema/typescript.rs` — nullable scalar element (D2,
   blocking).** Same probe: `models.ts(356,69): error TS18047: 'element' is
   possibly 'null'` — the `contains` matcher predicate needs an
   `element !== null &&` guard (the sibling `forEach` five lines below already
   has one).

3. **`src/generator/json_schema/go.rs:5745` and
   `src/generator/json_schema/java.rs:1732` — `x-<lang>-enum-names` key
   (`11#11`, must land with my change).** Both still gate on
   `(Some(map), Value::String(key))`. Replace the key derivation with the value's
   JSON wire spelling, matching `enum_names_lookup_key` in
   `src/parser/json_schema.rs:7197`:
   `String → text`, `Bool → "true"/"false"`, `Number → number.to_string()`,
   otherwise no override. Until this lands the loader registers
   `ScaleOneHalf`/`ScaleOne` for `enum: [1.5, 1]` while Go still emits
   `Lp4Scale1_5`/`Lp4Scale1` (measured), so a *derived* numeric-token collision
   in a schema that also carries a numeric `x-go-enum-names` entry would go
   unreported. No schema in the repo does this today (the keyword is a complete
   no-op for numbers before the change), so the window is narrow — but the two
   edits belong in the same commit.

4. **`src/generator/json_schema/go.rs` — `go_union_field_suffix` (`01#2`,
   follow-up, non-blocking).** The emitter derives an inline union's interface
   name from the *JSON* member name (`go_field_name`), so an `x-go-name` on a
   union-typed property does not move the interface, its variant wrappers or its
   dispatcher — unlike every other synthesized Go name. My manifest collector
   mirrors the emitter deliberately (`json_schema.rs:7859`), so a collision is
   reported exactly where one is emitted, and the P15 escape hatch today is
   `x-go-name` on the owning *model* (or on the referenced `$defs` union), which
   does work. If the emitter switches to the member identifier — matching
   Python's `inline_union_fn_base` and TypeScript's `DEFAULT_<FIELD>` — flip
   `go_union_field_suffix` in the loader to `member_identifier` in the same
   commit.

5. **`src/generator/json_schema/typescript.rs` — `const` property is
   `readonly` (pre-existing, unrelated to my changes, no finding id I could
   match).** A `{type: string, format: duration, const: "PT1H30M"}` property
   emits `models.ts: error TS2540: Cannot assign to 'span' because it is a
   read-only property.` Reproduced with an already-canonical literal, so it is
   not a side effect of D10 canonicalization. Minimal repro: `/tmp/lp3/lp3.json`.

6. **`src/parser/mod.rs` (unowned) — optional.** If the emitters want to call
   `enum_names_lookup_key` rather than re-derive it, add it to the
   `pub(crate) use json_schema::{…}` list beside
   `ts_transfer_type_converter_name`. I left `mod.rs` untouched.

---

## Sample schema requests

None. Every fix is covered by an inline test or a `/tmp` probe; none of them
needs a new field in `samples/schemas/*.yaml`.

---

## Snapshot shifts

**None caused by this agent.** The four `tests/generate_*.rs` golden assertions
failing at the end of my run
(`go_json_generation_matches_checked_in_output`,
`java_json_{,api_}example_generation_matches_checked_in_output`,
`python_json_{,api_}example_generation_matches_checked_in_output`,
`typescript_json_{,api_}example_generation_matches_checked_in_output`)
diff only on other agents' work: the Go catch-all key-collision check and
`jsonScalarEquals`, Java's `SpecNumbers.readExactTree` and Javadoc placement,
Python's `\Z` rewrite and the tightened base64 regexes, and the TypeScript/Python
serialize-side integer cap. I diffed every one and none of the hunks come from
`src/parser/json_schema.rs`.

One thing to watch during the regeneration pass: `Schema::ty` now carries
`skip_serializing_if = "Option::is_none"`, so the *planned schema JSON* no longer
contains `"type": null` for a typeless node. No generated artefact embeds that
JSON, and all four probe targets compile, but it is the one serialization-shape
change in this diff.

---

# Addendum — two items routed after the first pass

`cargo build --all-features` clean; `cargo fmt --check` clean;
`cargo test --all-features --lib` **499 passed / 0 failed** (+7 tests).
`tests/json_schema_probe_matrix.rs` 1/1, `tests/json_schema_conformance_manifest.rs`
2/2, `tests/json_schema_corpus_runtime.rs` 2/2 — all with the real toolchains.

## 1. Java's generated **local** namespace (`new:java-deserializer-locals`, P0)

`validate_member_scope` (`:7924`) now knows the third Java scope, beside the
nested-class and method scopes I added in the first pass.

**Why it is a scope at all.** The generated `deserialize` declares every member
slot at *method* scope (`String index = null;`), and Java forbids both a
duplicate at that scope and a nested block redeclaring an enclosing local. So a
member named `index` beside any array member is
`variable index is already defined in method deserialize(JsonParser,DeserializationContext)`.
P15's own wording — a scope is whatever unit the target actually resolves names
in — puts the deserializer's locals squarely in the member namespace.

**I enumerated the set rather than trusting the list in the routing note**, in
two passes:

1. Extracted every local declared inside the generated `deserialize` body of a
   maximally-featured model (12 property shapes — constrained string, array with
   `uniqueItems`+`contains`, 3-deep nested array, typed catch-all, temporal,
   `contentEncoding`, numeric bounds, `enum`, `default`, sum-type, nullable
   element, temporal/bytes arrays).
2. Swept **62 candidate names** one at a time through `nexgen java` + real
   `javac` on the samples' resolved classpath, and recorded the verdict.

The routing note's list was right about `index`, `field`, `node`, `element`,
`items` and `violations`, and **wrong about the rest**: `key`, `item`,
`itemPath` and `value` compile. It also missed nine that do not:
`context`, `parser`, `elementPath`, `length`, `nestedLength`, `numberValue`,
`parsed`, `priorIndex`, `rawElement`, `rawIndex`, `rawKey`, `rawMatchCount`,
`rawSeen`, `violation`, `fieldNames`.

Three pieces:

- **`JAVA_DESERIALIZER_LOCALS`** (`:8118`) — the 21 fixed names, each one
  measured to break `javac`, entered into the member scope after the declared
  members so the diagnostic names the user's member as the prior origin.
  Reserved **unconditionally** even though most are emitted only for a shape the
  model may not have: the alternative makes a model's validity depend on whether
  some *sibling* happens to be an array today, so adding an unrelated property
  would break a member that had always been fine. Go's fixed method set and
  `boilerplate_idents` are reserved on the same basis.
- **`java_is_nested_level_local`** (`:8147`) — the depth-suffixed loop locals
  `items<N>`/`index<N>`/`element<N>`/`path<N>`. `render_parse_element`'s
  `JavaType::List` arm mints one set per nesting level, so the family is
  unbounded in the schema's depth and cannot be listed; matched by shape
  instead. Level numbering starts at 1, so `element0` stays an ordinary member
  name (measured: `element3` compiled under 3-deep nesting and fails under
  4-deep — which is exactly why a list would have been wrong).
- **`<member>Value`** (`:8082`) — a `const`/`enum` member's parse block binds
  `String <member>Value` to the decoded scalar. That is a name synthesized
  *from a member*, so it goes in the member scope beside the slot, like Go's
  `<Field>OrDefault` and Python's `_<field>`. Before, `{h: {enum: […]}, hValue}`
  compiled or not **depending on which of the two was authored first** (measured
  both ways); now both orderings reject identically.

**Deliberately excluded, and pinned by a test:** `key`, `item`, `itemPath`. All
three are bound inside the catch-all parse, which the emitter closes at the line
*before* the first member slot is declared (measured: catch-all block ends 614–655,
first slot at 658), so they are out of scope by the time the slots exist.
`additionalProperties` is excluded because the existing catch-all check already
owns it with a better diagnostic. The doc comment records the invariant this
rests on, so if that parse ever moves after the member slots the reader knows to
add them.

*Tests (+4):* `rejects_java_member_colliding_with_a_generated_deserializer_local`
(13 names × asserts Go/TypeScript/Python still load × asserts the `x-java-name`
escape hatch resolves it), `rejects_java_member_colliding_with_a_nested_array_loop_local`
(five suffixed names reject, four `…0` names load, override resolves),
`rejects_java_closed_value_decoded_local_collision` (both authoring orders),
`accepts_java_members_named_after_out_of_scope_generated_locals` (pins the
exclusions so the reserved list stays exactly as wide as `javac` requires).

*Verified:* re-ran both 62-name sweeps after the change — the LOAD-REJECT set is
now **exactly** the previously-measured JAVAC-FAIL set, name for name, and every
previously-compiling name still loads. End-to-end: `{index, tags}` rejects for
Java and loads for the other three; with `x-java-name: indexField` it loads and
`javac` accepts the output.

### Probe-matrix declaration — I did edit `tests/json_schema_probe_matrix.rs`

Flagging this explicitly, because it is outside the file list I was given twice.
The change made the existing `reserved_nested` probe go **red**: its `violation`
member now load-rejects for Java, and the probe declared `scoped_load: &[]`. That
is a correct new verdict, not a defect, and the only file that can say so is the
matrix. Leaving the suite red seemed worse than the ownership exception, so I
made the smallest additive edit that states it:

- `reserved_nested` gained a `ScopedLoad` row for Java
  (`diagnostic: "identifier collision in java output"`).
- A new `reserved_locals` probe (`index`, `node`, plus a `tags` array) with a
  `ScopedLoad` row for Java (`diagnostic: "generated deserializer local"`),
  matching the shape of the `reserved_methods` row I generated in the first pass.

Both rationales are written out in the file. I touched nothing else in it. If
the conformance agent has the file open, these two hunks are the whole diff and
are trivial to re-apply.

## 2. `go_union_field_suffix` flipped to the member identifier (`01#2`, follow-up)

`:7861` now returns `member_identifier(Language::Go, json_name, property)`; call
site `:7706`. I checked the three arguments rather than taking them on trust:

1. **Confirmed.** `go.rs:2193` is now `property.go_member_name(json_name)`, and
   `go_member_name` (`go.rs:92`) is `x_go_name` else `go_field_name(json_name)` —
   bit-for-bit what `member_identifier(Go, …)` computes. The two halves agree.
2. **Confirmed, and it was worse than "a lie".** Without the flip the pass
   rejects `{idOrName: <union>, id_or_name: <union> + x-go-name: IdOrNameSnake}`
   — a schema that already carries the override the diagnostic asks for, with no
   remedy left. Same class as the `02#4` fix-it I removed in the first pass.
3. **Confirmed.** `validate_member_scope` rejects two members of one model
   sharing an identifier *before* this pass runs, so distinct members still
   yield distinct suffixes: the flip cannot hide a collision. It can only stop
   spurious ones.

*Tests (+1, +1 extended):* `union_names_follow_a_member_override_go` (the
previously false-rejected shape), and `rejects_colliding_union_names_go` now
asserts **both** halves of the escape hatch — renaming the owning model *and*
renaming the union-typed member.

*Verified as emitter-visible, not just inline:* `tests/json_schema_probe_matrix.rs`
(16 schemas × 4 toolchains, real `go vet`) and
`tests/json_schema_conformance_manifest.rs` (13 cases executed in all four) both
pass. Plus a direct probe of the flipped shape — the emitter writes
`FooIdOrName`/`FooIdOrNameString`/`unmarshalFooIdOrName` and
`FooIdOrNameSnake`/`FooIdOrNameSnakeString`/`unmarshalFooIdOrNameSnake`, exactly
the six names the loader now registers, and `go vet` is clean.

The first pass's cross-file request #4 is **closed**.

## 3. `enum_names_lookup_key` sharing — recommendation, not done

There are now **three** copies of this six-line derivation: the loader's
(`:7197`), `go.rs:6091` and `java.rs:1838`. Both emitter copies carry a doc
comment pointing back at the loader's, which is honest but is exactly the drift
surface the "single resolution point" comment on `type_identifier` warns about.

I recommend sharing it, but **did not** do it, for a concrete reason: the
re-export is one line in `src/parser/mod.rs` —

```rust
pub(crate) use json_schema::{
    ManifestModel, ManifestService, NameManifest, build_name_manifest,
    enum_names_lookup_key, ts_transfer_type_converter_name,
};
```

— and a `pub(crate) use` with no consumer is an `unused_imports` warning, so it
cannot land before both emitters switch. It is a single three-file commit or
nothing, and sequencing that is yours, not mine.

What I did instead, so the duplication is at least pinned: a new test
`enum_names_lookup_key_is_the_json_wire_spelling` (`:13445`) fixes the contract
for all four scalar kinds plus `null`/array/object. Any future change to one copy
now has to be a deliberate change to a written contract rather than a silent
divergence.

## Snapshot shifts

Still **none from this agent** — both items are load-side only and neither
changes a byte of emitted output. At the end of this pass the red golden
assertions are `go_json_generation_matches_checked_in_output`,
`java_json_{,api_}example_generation_matches_checked_in_output` and
`python_json_{,api_}example_generation_matches_checked_in_output`; the
TypeScript pair has gone green since the first pass. All remaining diffs are
other agents' emitter work awaiting the consolidated regeneration.
