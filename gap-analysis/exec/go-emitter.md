# go-emitter — execution report

Files touched: `src/generator/json_schema/go.rs`, `src/generator/go.rs`,
`tests/generate_go.rs`. **`src/planning/reachability.rs` is unchanged** — see
`14#1` below for why the fix landed in the generator instead.

Verification protocol: every claim below was generated into `/tmp`, given a
`go.mod`, and run through `go build` + `go vet`; behavioural claims were run
through a real `go test`. The four checked-in sample packages were regenerated
into `/tmp/samplecheck`, built, vetted, and the checked-in
`samples/go/tests/*_test.go` round-trip suite was executed against them — green.

---

## Fixed

### Wave 1 — nullability unwrap

New helper `property_shape` (`src/generator/json_schema/go.rs:5652`) returns the
non-null branch of a `oneOf:[T, null]` wrapper, carrying the property-level
`x-go-name` / `x-go-const-name` / `x-go-enum-names` across. `nullable_non_null_schema`
(`:5628`) was **tightened first**: it used to return the first non-null branch of
*any* `oneOf`, so a sum type such as the showcase's `mode`
(`oneOf:[{string,enum},{integer}]`) would have been misread as a nullable enum.
It now requires a null branch and exactly one non-null branch, matching
`typescript.rs:4332`.

- **`13#1` (P0, also `07#1`, `06#4`)** — parse dispatcher. `render_property_unmarshal`
  (`go.rs:3362`) now dispatches on the shape and threads a single `nullable` flag
  into every `parse*Field` call; the trailing `allows_null → parseStringField`
  arm is gone. `parseIntegerField`/`parseNumberField` had `false` hard-coded for
  `nullable` — also fixed. Verified: `/tmp/probe1` went from 4 `cannot use &v
  (value of type *string) as *int64` errors to a clean `go build`, and a runtime
  test round-trips `{"count":null,…}`.
- **`13#2` (P0, also `07#1`, `11#1`)** — `render_validate` (`go.rs:2829`) reads the
  shape, and `by_value` (`required && !nullable`) replaced every
  `required_fields(schema).contains(json_name)` pointer test. `render_marshal_json`
  (`go.rs:4174`) got the same treatment. `render_const_discriminators` (`go.rs:1731`)
  unwraps too, so a nullable `enum` now emits its closed defined type (P13.1) —
  `Kind *NullBagKind`, not `*string`. `go_property_type` (`:5290`) likewise.
  Verified at runtime: `minLength`/`maxLength`/`pattern`/`minimum`/`maximum`/
  `multipleOf`/`minItems`/`enum` all reject on a nullable member.
  Side effect visible in the samples: a nullable `$ref` property now gets its
  `mergeNested(&errs, …, m.X.Validate())` (kb `Block.page`, showcase `Showcase.audit`).
- **`13#6` (P1)** — required+nullable array is `[]T`, not `*[]T`
  (`go_property_type`, `go.rs:5303`); parse leaves the slice nil for a wire
  `null`, marshal writes `null` for a nil slice.
- **Test** `go_json_nullable_non_string_properties_keep_their_shape_and_constraints`
  (`tests/generate_go.rs`) — asserts the emitted types and runs the accept/reject
  matrix through real Go.

### Wave 2 — materialized types

- **`08#4` / `10#1` (P0)** — `utf8.RuneCountInString` over a `[]byte`. The
  shared-`Validate` string branch is now gated on `content_encoding_kind(...).is_none()`
  and a dedicated branch encodes first (`wire<Field> := encodeBase64(m.Blob)`) and
  checks the **wire string** (`go.rs:2895`). The duplicate copy of those checks in
  `render_content_encoding_property_marshal` was removed — `MarshalJSON` already
  runs `Validate`, so keeping both would double every violation.
- **`10#1` (P0)** — `render_go_closed_validate` derefed a slice (`(*m.Field)`).
  Rewritten (`go.rs:5990`): a `[]byte` field is never behind a pointer.
- **`05#1` (P0)** — `uniqueItems` map key type. `render_go_array_checks`
  (`go.rs:693`) now keys on the **canonical wire string** for a materialized
  element (**D10**): `formatDateTime(e)` / `formatDuration(e)` / `encodeBase64(e)`.
  (`[]byte` is not a legal Go map key at all.) The deserialize side already keyed
  on the wire JSON, so the two directions now agree.
- **`09#1` / `04#1` (P0)** — guard/body mismatch. `Schema::has_string_constraints`
  (`go.rs:106`) now counts a `format` only when `format::check_for` yields a
  runtime check, so a `time`/`duration` element no longer emits a loop scaffold
  with an empty body (`declared and not used: v0`). The `contentEncoding` element
  branch only materializes `wire{level}` when something reads it. Same guard fix
  in `render_go_property_name_checks` (`go.rs:922`) for `propertyNames`
  (the loader now also rejects that shape per D6 — the guard stays as the
  emitter's own invariant). Verified on `/tmp/probe2`/`/tmp/probe3`.
- **`10#2` / `09#2` (P0) + `10#9` / `09#14`** — `const`/`enum` over a materialized
  value now compares the canonical wire string in both directions (**D10**), and
  the reason string prints the wire form:
  `wireWhen := formatDateTime((*m.When)); if wireWhen != "2021-06-15T12:30:45Z"`.
  Was `bytes.Equal` over decoded bytes / `!=` over the native `time.Time`.
- **`09#3` (P0)** — serialize-side temporal predicates. Four new runtime helpers
  in the temporal support blob (`go.rs:5060`): `checkDateTime`, `checkDate`,
  `checkTime`, `checkDuration`, plus `checkTemporalYear` (1..9999) and
  `checkTemporalOffset` (whole minutes), mirroring Python's
  `_check_date_time`/`_check_time`/`_check_duration`. They replace the three
  `Year() < 1` sites (property, array element, typed-map member) and
  `schema_requires_go_validation` now admits all four temporal kinds. Measured:
  `-90m → "a duration cannot be negative"`, `500ms → "cannot carry a fraction of
  a second"`, a 30-second offset → "not a whole number of minutes", year 10000 →
  "year must be <= 9999". Reason text now matches Java's shape
  (`must be a valid date-time, got …: year must be >= 0001`).
- Also fixed while in there: a required+nullable `contentEncoding` or
  materialized-element array emitted nothing for `nil` instead of `null`.
- **Test** `go_json_materialized_values_are_checked_and_compared_on_the_wire`.

### Wave 4 — value equality

- **`07#2` (P0)** — `isJSONMultiple` (exact `big.Rat` over the shortest decimal
  spelling) is **deleted**, along with the `math/big` import. A `number` field's
  `multipleOf` is now `math.Mod(float64(v), m) != 0` (`go.rs:214`, `:620`).
  Runtime-verified against the report's table: `1e23 % 5` rejects, `1e300 % 3`
  accepts, `1e22 % 3` rejects, `-0` accepts.
  **Test** `go_json_number_multiple_of_is_ieee_fmod` re-pins `fmod` (the semantics
  commit `e2b8de6` un-pinned).
- **`01#4` (P0)** — discriminator dispatch. The `switch string(bytes.TrimSpace(discRaw))`
  is now `switch { case jsonScalarEquals(discRaw, "<literal>"): … }`, backed by a
  new runtime helper that compares the two **parsed** JSON values (`go.rs:1658`).
  Runtime-verified: `{"kind":1}`, `{"kind":1.0}` and `{"kind":1e0}` all select the
  `const: 1` branch; `{"kind":{}}` is a clean violation, not a panic.
- **`06#3` (P0)** — an `integer` matcher over `number` elements now also requires
  `e >= -integerCap && e <= integerCap` (`go.rs:578`). Runtime-verified:
  `[1e300]` and `[9007199254740993]` no longer match; `[9007199254740991]` does.
  Pinned in `go_json_scalar_matchers_have_runtime_type_and_decimal_semantics`.

### Wave 6 — cross-module resolution

- **`01#1` (P0)** — cross-file `$ref` union branches. `generate_branch_tree`
  collects every JSON model in the closure (`collect_tree_json_models`,
  `src/generator/go.rs:652`) and hands it to the JSON backend
  (`adopt_tree_models`), which keeps them as `foreign_json_models` — read-only.
  `render_external_models` now takes both lists and builds **two** union maps:
  `declared_unions` (this file's, the only ones emitted) and `unions` (those plus
  the closure's, used for every type/dispatch decision). Verified: a named
  cross-file `oneOf` went from `type Shape struct{}` to the sealed interface with
  a working dispatcher, and the package builds.
- **`01#6` (P1)** — a cross-module `$ref` to a named union now emits `Shape`, not
  `*Shape`. Verified end-to-end. While proving it I found and fixed the
  **same-module** variant: `property_union_name` resolved the reference with an
  *empty* name map, so an `x-go-name` on a union produced `Shape *ShapeGo` —
  `m.Shape.Validate undefined (type *ShapeGo is pointer to interface)`,
  reproduced with the Go compiler. It now resolves through the manifest
  (`go.rs:2176`); `render_marshal_json` gained the `model_names` parameter it needed.
  **Test** `go_json_renamed_union_reference_binds_the_interface`.
- **`14#1` (P0)** — service-only module. Reproduced exactly: no `var Svc`, WIT
  operation funcs, and a `go.temporal.io/sdk/workflow` import in
  definitions-only output. **I did not change `reachability.rs`**: dropping the
  foreign declaration is correct and load-bearing (Go flattens the closure into
  one package, so keeping it would re-declare `Page`), and the pass is shared by
  all four backends. The bug is that `GoExternalModels::new` inferred
  "JSON plan" from `spec.types` alone. It now also asks the service operations'
  own type expressions (`plan_uses_json_models`, `src/generator/go.rs:640`), and
  `json::ModelBackend::is_active` accounts for a model-less JSON service.
  Verified: correct `var Svc = struct{…nexus.OperationReference[Page, nexus.NoValue]…}`,
  `Page` still declared exactly once, package builds. The repo's own fixture
  `go_json_service_module_without_own_types_does_not_redeclare_refs` now asserts
  all three properties instead of only counting `type Page struct {`.

### Wave 7 — discrete Go bugs

- **`05#5` (P0)** — a required non-nullable array with a `nil` slice emitted
  `{"tags":null}`, which this package's own decoder rejects. It now emits `[]`
  (`go.rs:4260`, items.md:193-202). Runtime-verified round-trip.
  **Test** `go_json_required_array_emits_empty_array_for_a_nil_slice`.
- **`11#3` (P1)** — `enum` + `default` did not compile. `render_default_accessors`
  (`go.rs:2709`) returns the closed defined type and the named constant:
  `func (m Defaulted) StatusOrDefault() DefaultedStatus { … return DefaultedStatusActive }`.
- **`03#3` / `04#11` (P1)** — the catch-all/declared key-collision check is now
  gated on `is_open_object(schema)` rather than on a *typed* catch-all
  (`go.rs:3156`). Runtime-verified: an untyped open struct with a colliding extra
  now raises, matching TS/Python/Java.
- **`04#4` (P2)** — `minProperties`/`maxProperties`/`dependentRequired` moved
  **into** the shared `Validate` on declared-property models, over a `present`
  member set that reproduces exactly what `MarshalJSON` writes
  (`render_go_present_member_set`, `go.rs:3199`). The duplicate copy in
  `MarshalJSON` was removed, so violations are still reported exactly once
  (asserted). `UnmarshalJSON` keeps its wire-side count over `all`.
- **`08#7` / `09#11` (P1)** — compile-once. Both offenders are hoisted to
  package-level vars: `contains` matchers via `render_go_matcher_vars`
  (`<model><position>ContainsPattern` / `…ContainsFormat`) and `propertyNames`
  via the existing `render_go_string_vars` at a new `propertyName` position. The
  test that asserted the inline `regexp.MustCompile(...).MatchString(e)` now
  asserts the hoisted var **and** that the inline form is absent.

---

## Not fixed

- **`05#8` / `06#7` (D2, nullable element type)** — still load-rejected as of this
  writing (`uniqueItems: true` / `contains` "over a composite element type is not
  yet supported"), so the path is unreachable and untestable. **When the loader
  loosens it, `render_go_array_checks`'s `uniqueItems` block needs one more
  case**: a nullable element is a `*T`, so `seen[e]` would key on the pointer.
  It needs `if e == nil { … }` tracking a single null index and `KEY(*e)`
  otherwise. I deliberately did not write that code blind.
- **`10#2` residual** — Go now compares the canonical wire string, so a *non-canonical*
  wire value that decodes to the const's bytes (`"aGl="` against `const: "aGk="`)
  is still Go-accepted **unless** the base64 regex tightening (**D1**,
  shared-helpers) rejects it at parse. That is the agreed mechanism; nothing more
  is needed here once D1 lands. Please confirm it did.
- **`09#2` residual** — same shape: Go compares `formatDuration(v)` against the
  authored literal, so a non-canonical literal (`const: "PT90M"`) needs the
  loader-side canonicalization to match. Loader agent's item.

---

## Cross-file requests

None. Everything I needed was inside my four files.

---

## Sample schema requests

`samples/schemas/showcase.nexusrpc.yaml`, under `$defs.Showcase.properties`
(each of these fails in at least one backend today and is generated by **no**
sample schema):

```yaml
      nullableCount:
        description: A nullable integer with bounds on the non-null branch.
        oneOf:
          - { type: integer, minimum: 1, maximum: 10 }
          - { type: "null" }
      nullableRatio:
        description: A nullable number with a divisor on the non-null branch.
        oneOf:
          - { type: number, multipleOf: 2 }
          - { type: "null" }
      nullableFlag:
        oneOf:
          - { type: boolean }
          - { type: "null" }
      nullableTags:
        description: >-
          A nullable array. Optional + nullable, so the Go field is `[]string`
          and a wire `null` decodes to a nil slice.
        oneOf:
          - { type: array, items: { type: string }, minItems: 1 }
          - { type: "null" }
      nullableMode:
        description: A nullable closed value set - the closed type survives the wrapper.
        oneOf:
          - { type: string, enum: [auto, manual] }
          - { type: "null" }
      nullableName:
        oneOf:
          - { type: string, minLength: 3, maxLength: 8, pattern: "^[a-z]+$" }
          - { type: "null" }
```

and, for the matcher/number divergences (`07#2`, `06#3`):

```yaml
      integralMeasurements:
        description: An integer matcher over number elements - the +/-(2^53-1) cap applies.
        type: array
        items: { type: number }
        contains: { type: integer }
      byFive:
        description: A number-field divisor - IEEE fmod, not decimal arithmetic.
        type: number
        multipleOf: 5
```

Wire fixtures worth adding alongside: `{"integralMeasurements":[1e300]}` and
`{"byFive":1e23}` as `parse_failures`; `{"byFive":1e300}`-style acceptance for
`multipleOf: 3`.

---

## Snapshot shifts

`tests/generate_go.rs::go_json_generation_matches_checked_in_output` fails until
the consolidated regeneration. The regenerated output was built + vetted and the
checked-in `samples/go/tests` round-trip suite passes against it. Expected diffs:

| File | Change |
|---|---|
| all four `definitions.go` | `math/big` import and `isJSONMultiple` removed; `jsonScalarEquals` added |
| `showcase.go`, `chat.go` | discriminator `switch` → `switch { case jsonScalarEquals(…) }` |
| `showcase.go`, `temporal.go` | the six `check*` temporal helpers added; `Year() < 1` sites → `checkDateTime`/`checkDate`/`checkTime`/`checkDuration` with Java-shaped reasons |
| `showcase.go` | `Showcase.audit` (nullable `$ref`) gains `mergeNested(&errs, "audit", …)`; `Contact.Validate` gains `present`/`minProperties`/`maxProperties`/`dependentRequired`, and `Contact.MarshalJSON` loses its copy |
| `kb/content_block.go` | `Block.page` (nullable `$ref`) gains `mergeNested(&errs, "page", …)` |
| `kb/tree_category.go` | same, on the nullable back-reference |
| every open struct with declared properties | gains the catch-all/declared collision check |

`tests/generate_go.rs` itself: `go_json_renders_complete_contains_matchers` and
`go_json_service_module_without_own_types_does_not_redeclare_refs` were updated
in place; five tests were added.

---

# Round 2 — coordinator follow-up

Same protocol: generated into `/tmp`, `go build` + `go vet`, and a real `go test`
for every behavioural claim. All three harnesses
(`json_schema_conformance_manifest`, `json_schema_probe_matrix`,
`json_schema_corpus_runtime`) are green, and the four sample packages were
regenerated, built, vetted and run through the checked-in
`samples/go/tests` round-trip suite.

## Fixed

### 1. `new:go-numeric-accepts-quoted-token` (P0)

`json.Number` is a string type, so `encoding/json` decodes the quoted token
`"7"` into it **without error** (confirmed with a standalone Go program:
`"7" -> n="7" err=<nil>`), and `parseSpecInteger`/`Float64` then see a
perfectly good number. The token kind has to be read off the wire bytes.

New runtime predicate `isJSONNumberToken` (`src/generator/json_schema/go.rs:1685`):
a JSON number starts with `-` or a digit and nothing else does. It guards
`parseIntegerField` and `parseNumberField` before the decoder runs, and the
**raw `contains` integer scan** (`go.rs:844`), which reads wire elements
independently of the typed slice and would otherwise have counted `["1"]` as an
integer match.

Not affected, checked: the union token dispatcher (`switch trimmed[0]`) already
routes `"7"` to the string branch; `jsonScalarEquals` compares dynamic types so
`"1"` never equals `1`; `render_go_raw_array_checks`' `number` branch unmarshals
into a `float64`, which rejects a string.

Verified against the harness's own case: the manifest now reports
`numeric-bounds: every target now agrees, so its expected_divergence … is stale`
and `integer-semantics: expected_divergence matches nothing any more:
["parse_failures[2]"]`. **The conformance agent has since unpinned both**
(`numeric-bounds.expected_divergence` is `null`; `integer-semantics` keeps only
`13#4` / `parse_failures[5]`), and the harness is green.

### 2. `05#8` / `06#7` — nullable elements, now that D2 has landed

Reproduced the break the coordinator described
(`cannot use e (variable of type *string) as string value in map index`,
plus `invalid operation: e >= 2 (mismatched types *int64 and untyped int)` —
`render_go_array_checks` also read `items.type` off the wrapper, so the matcher's
element kind was `None`).

`render_go_array_checks` (`go.rs:661`) now unwraps the wrapper for the element
kind and asks the new `go_element_is_nullable_pointer` (`go.rs:5439`, mirrors
`go_element_type_annotation` without needing the name map) whether the Go element
is a `*T`:

- **`uniqueItems`**: the loop dereferences for the map key and tracks `nullIndex`
  on the side, so **two `null` elements are a duplicate** (`uniqueItems.md:188-190`)
  without needing `null` to be a key of the branch's key type.
- **`contains`**: `if e == nil { continue }` before the predicate, so a `null`
  element **never matches a scalar matcher** (`contains.md`, Interactions →
  nullability).

The deserialize side already agreed and is unchanged: the raw `uniqueItems` key
is `json.Marshal` of the decoded `any`, so two wire `null`s key to `"null"`, and
every raw matcher branch fails to decode `null`.

Runtime-verified in **both directions** with the same six cases:
`["a",null,"b"]` ok, `[null,null]` duplicate, `["a","a"]` duplicate,
`[null,3]` matches, `[null,1]` does not, `[null]` does not — parse and serialize
give identical verdicts. `[]byte` elements are deliberately excluded from the
pointer path: a materialized `contentEncoding` is already nil-able, so `null` and
`""` are indistinguishable in memory there (a materialization collapse, not a
`uniqueItems` question).

### 3. `11#11` — `x-go-enum-names` over numeric and boolean members

`go_value_constant_override` (`go.rs:6059`) keyed the map with
`Value::String(key)`, so a numeric or boolean member could never be renamed —
P15's only escape hatch for a value-constant collision did not exist for exactly
the values most likely to collide. It now derives the key from the member's
canonical **wire spelling**, matching the loader's `enum_names_lookup_key`.

Verified: `enum: [1, 2]` with `x-go-enum-names: {"1": TierBronzeGo, …}` emits
`TierBronzeGo BagTier = 1`, and `enum: [true, false]` emits
`FlagOnGo BagFlag = true`; both are used by the parse `switch` and the shared
`Validate`.

I duplicated the three-line key derivation locally rather than reach into
`src/parser/mod.rs` (not mine) to re-export the loader's function — see the
cross-file request below.

### 4. `go_union_field_suffix` — **decided: fix it** (needs the loader half)

An inline property union's sealed interface is `<Model><Member>`; it derived
`<Member>` from the JSON name, so `x-go-name` on the property moved the field
but not the union. That contradicts this emitter's own documented rule for the
sibling name — `const_type_name`: *"built from the **emitted member identifier**
so an `x-go-name` override on the declaring property moves it: a name
synthesized from the member follows the member (P15)"*.

Worse, it makes the loader's fix-it a lie. Measured, before the change:

```
$defs.Bag.properties:
  idOrName:   { x-go-name: Chosen, oneOf: [ {string}, {integer} ] }
  id_or_name: { oneOf: [ {boolean}, {number} ] }
```
→ `identifier collision in go output: 'Bag.idOrName' union interface and
'Bag.id_or_name' union interface both map to 'BagIdOrName'; disambiguate with an
'x-go-name' override` — on a schema that **already carries** the `x-go-name` the
fix-it asks for. P15 forbids a fix-it the user cannot apply.

`go_union_field_suffix` now takes the property and returns
`property.go_member_name(json_name)` (`go.rs:2182`). Verified: `BagChosen`,
`BagChosenString`, `BagChosenInteger`, `unmarshalBagChosen`; the wire name
(`json:"idOrName"`) is untouched; the package builds. No checked-in sample
shifts — none of them puts `x-go-name` on a union property.

Note the derivation is also *strictly safer*: `<Model><EmittedMember>` cannot
collide across two members, because the member-identifier pass already rejects
two members mapping to one identifier (measured: `member 'Bag.idOrName' and
member 'Bag.chosen' both map to 'Chosen'`). The old JSON-name derivation could
collide where the emitted members did not.

### 5. Residuals from round 1 — both closed

With D1 and `canonicalize_for_format` landed, verified end-to-end against
`const: "aGk="` / `const: "PT90M"` / `const: "2021-06-15t12:30:45z"`:

| wire | verdict |
|---|---|
| `{"blob":"aGl="}` | rejected — `must be base64-encoded, got "aGl="` (D1's tightened regex catches it at parse, before the wire-string comparison) |
| `{"dur":"PT90M"}` | accepted — the literal canonicalized to `PT1H30M` at load and the emitted check is `wireDur != "PT1H30M"` |
| `{"when":"2021-06-15t12:30:45z"}` | accepted — canonicalized literal |
| `{"dur":"PT2H"}` | rejected — `must equal "PT1H30M"` |

Go now matches TS/Python/Java on both. `10#2` and `09#2` are fully closed.

## Cross-file requests

1. **`src/parser/json_schema.rs` (loader) — must land with my item 4.**
   `go_union_field_suffix` (`json_schema.rs:7859`) still mirrors the *old*
   emitter and its doc comment says so explicitly. Change:
   ```rust
   fn go_union_field_suffix(json_name: &str) -> String { recase_member(Language::Go, json_name) }
   ```
   to take the property and return the **emitted member identifier** — the same
   `member_identifier` the closed-value pass beside it already uses — and update
   the call at `json_schema.rs:7706`
   (`format!("{type_ident}{}", go_union_field_suffix(json_name))`).
   The doc comment's rationale ("mirroring the emitter") now points the other way.
   **Until it lands there is a narrow window**: the P15 union pass guards
   `<Type><JsonName>` while the emitter writes `<Type><EmittedMember>`, so the
   `idOrName`/`id_or_name` schema above is still falsely rejected. No *missed*
   collision, though — the member-identifier pass covers that axis.
2. **`src/parser/mod.rs` (nobody's list)** — re-exporting
   `json_schema::enum_names_lookup_key` as `pub(crate)` would let the Go and Java
   emitters share the one key derivation instead of each carrying a copy. Low
   priority; my copy is three lines with a pointer to the original.
3. **`src/generator/json_schema/java.rs` (java-emitter)** — `ClosedNameOverrides::get`
   (`java.rs:1821-1829`) still gates on `Value::String`, so `x-java-enum-names`
   over a numeric/boolean member is still a no-op. Same one-line shape as my
   item 3; `11#11` is only half closed without it.

## Snapshot shifts (round 2 additions)

No new *sample* shifts beyond round 1 — the checked-in schemas have no quoted
numeric fixtures, no nullable elements, no numeric/boolean `x-go-enum-names`, and
no `x-go-name` on a union property. `definitions.go` in all four packages gains
`isJSONNumberToken` and the two `expected integer`/`expected number` guards.

Three tests added to `tests/generate_go.rs`:
`go_json_numeric_members_reject_a_quoted_token`,
`go_json_nullable_elements_are_unique_and_never_match_a_matcher`,
`go_json_synthesized_names_follow_the_emitted_member`.
