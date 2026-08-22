# Wave 11 — spec corrections (agent: specs)

Scope: `specs/**/*.md` only. No source, test, sample or corpus file touched.
Nothing to compile; `specs/**/*.md` is read by no test (only the corpora
`.json`/`.body` files are `include_str!`'d, and those are shared-helpers').

## Fixed

- `03#2` — `features/additionalProperties.md:185-244`. Deleted the false
  parenthetical claiming TS/Python/Java all keep an exact numeric
  representation. Python (`int`) and Java (`JsonNode`) now stated correctly;
  new subsection **"TypeScript untyped extras: a bounded exception to P13.2"**
  states that PRINCIPLES TypeScript §4 puts `JSON.parse` outside the converter,
  so `9007199254740993` arrives as `…992` with no interception point, names
  what *is* preserved, and points out that declaring the field does not rescue
  the value (the `±(2^53−1)` cap rejects it) — model it as a `string`.
  Consistent with **D8** (document, don't change the converter boundary).
  Verified: `node -e` on `JSON.parse` behaviour; PRINCIPLES TS §4 re-read.

- `13#4` — `features/type.md:146-162`, `:213-217`, `:302-321`.
  (a) The TS bullet no longer claims `Number.isSafeInteger` is "complete and
  sound": it is now stated as sound **for magnitude** and explicitly *not* a
  fractional-part check, with the worked case
  `JSON.parse("9007199254740991.1") === 9007199254740991`.
  (b) The printed Java `specLong` is marked **normative**, calling out
  `n.decimalValue()` vs the `n.doubleValue()` shortcut — the java-emitter agent
  is moving the code to match.
  (c) New fixture bullet **"Fractional literals in the `[2^52, 2^53)` band"**:
  the normative rule (written fractional part must be zero) plus which targets
  can enforce it (Go via `json.Number` text, Java via `BigDecimal`) and which
  cannot (TS / Python, both handed an already-parsed double). Documented as a
  bounded limitation of the two parse-boundary targets — **see "Decisions
  wanted" below**, this is the one place I state a residual P1 divergence.

- `09#3` — `features/format.md:421-457`. Deleted "Other native values are valid
  by construction (`time.Duration` always represents a supported time-only
  duration…)". Replaced with the six serialize-side obligations the Go and Java
  agents are implementing: calendar floor (`year < 1`), calendar ceiling
  (`year > 9999`, the pinned grammar is a 4-digit year), sub-minute UTC offset,
  negative duration (must not emit `"PT-1H-30M"`), sub-second duration (must not
  fold to `"PT0S"`), calendar duration components. Plus: a temporal held as a
  `string` (TS default mode, Java `time`) must re-run the pinned predicate
  before emit.

- `09#15` — `features/format.md:495-513`. Dropped the nonexistent "duration 68"
  corpus and the "materialize-duration corpora"; the surviving citations now
  match the on-disk row counts (124/56/41/72) and the actual
  `format_materialize_clock` sections. Added an explicit **"Planned, not yet
  present"** paragraph (duration corpus, `uri-reference` corpus, per-language
  runner) and the honest statement that the corpora are Rust-only data today,
  so a row is a specification, not a measurement. Verified counts by parsing
  each `corpus.json`.
  (I checked `format.md` for the stale "temporals are load-rejected" claim the
  plan flags in `format.rs`/`json_schema.rs` — `format.md` does **not** carry
  it; every load-reject it states is a real one.)

- `08#12` — `features/pattern.md:12-14`, `:104-131`, `:295-312`, `:341-349`.
  Dropped "all verified — 13 original divergences → 0 after rewrite" and the
  hard-coded "83-pair" counts (the corpus is now 140 pairs). The placement
  rules now read as "the rewrite is defined for every placement, and the corpus
  pins each one" — which shared-helpers has made true (`[\s.]`, `[^\s]`,
  `[\S]`, `[^\S]`, `[\S.]`, `[\S\d]` are all present, verified). Added that a
  row's expectation is data, never a special case in a test's source
  (`case-inline-flag` now carries `expect_gate_reject`, verified), and a
  "Planned" note for the per-language corpus runner.

- `02#9` — `features/allOf.md:254-258`. Dropped `default` from the unmergeable
  pairs bullet and stated positively that a differing
  `title`/`description`/`default` is a last-wins override, matching `:151` and
  the accepted-matrix row.

- `02#10` — `features/allOf.md:152` + accepted matrix. Added the `deprecated`
  **OR**-merge row (verified against `src/parser/json_schema.rs:5695-5697`),
  with the rationale that a later branch must not silence a deprecation, and a
  positive-matrix example.

- `03#6` — `features/properties.md:131-152` + accepted matrix. Resolved the
  contradiction **in the spec's favour**: Stage 3's reserved-word test is now
  explicitly against the words a language reserves *in the position the
  identifier is emitted into*, and TS interface members reserve nothing, so a
  keyword-named property is not a TS Stage-3 rejection. Added a positive matrix
  row pinning it. **The loader's current behaviour is therefore a bug** — see
  Cross-file requests.

- `01#11` — `features/oneOf.md:626-627`, `:775`, `:794`, `:798`. Added
  `type: string` to every tagged-union `const` tag, including the matrix rows,
  so the examples are loadable per `type.md:51`.

- `13#8` — `features/type.md:56`, `:277`. Removed `patternProperties` from the
  object-shape resolutions (loader behaviour bullet and the negative-matrix
  row), matching the emitted diagnostic; the row now says outright that
  `patternProperties` does not supply a shape.

- `13#9` — `nullability.md:355-362`. Java serialize is now described as the
  nested `Serializer` writing fields in code (honoring `@JsonInclude`
  *semantics*, no annotation on the POJO), matching PRINCIPLES Java §6 and the
  generated output.

- `10#6` — `features/contentMediaType.md:65-72`. The spec side already said
  "reject"; added a normative constraint on the **diagnostic**: it must say the
  schema is rejected and name the envelope remedy, and must not describe the
  keyword as ignored/tolerated/"carried verbatim", nor suggest validating the
  media type in application code. Code-side wording filed below.

- `12#9` — `features/readOnly.md:91-97`, `features/writeOnly.md:67-71`. Kept
  the four distinct rejects (recommended disposition) and made the requirement
  explicit: one shared message is insufficient; `readOnly/writeOnly: false`
  must be told to *delete* a dead annotation, never to split the type by
  direction; the contradictory pair names both keywords. Code change filed
  below.

- `05#9`, `04#7` (**D4**) — `features/minItems.md`, `maxItems.md`,
  `minProperties.md`, `maxProperties.md`. All eleven reason-string occurrences
  moved to the emitted text: `must have at least N items, got M` /
  `must have at most N items, got M` /
  `must have at least N properties, got M` /
  `must have at most N properties, got M`. Verified against all four emitters
  (`go.rs:620,630,746,752,838,848`, `python.rs:1684,1693,1737,1746`,
  `java.rs:626,631,909,914`, and the TS equivalents). The `contains` family
  (`too few/many matching items: …`) is deliberately left alone;
  `minContains.md`/`maxContains.md` still cross-reference the count family for
  the *convention* (name the bound and the count), which still reads correctly.

- `09#8` — `features/format.md:337-346`, matrix row `:481`. The temporal
  `string` opt-out is marked **Status: unimplemented** in a blockquote (no
  keyword, no mode, no accessor), stating that every opt-out sentence in the
  spec is intended design, and that **P1 exception (b) is conditioned on it**.
  I did **not** edit `PRINCIPLES.md` — see "Decisions wanted".

- `01#8` — `features/ref.md:249-254`, matrix row. Bare-`$ref`-root alias marked
  **Status: unimplemented**, including that such a file's root is not a model
  and a `$ref` to it fails to resolve.

- `11#9` — `features/const.md:221-229`, `:299-303`; `features/enum.md:211-219`,
  `:410-415`. Added a shared blockquote: a `$defs` entry must be `type: object`,
  a `oneOf` union, or a bare `$ref`, so a `$defs`-named scalar `const`/`enum` is
  a load reject and the `$defs`-name-reuse branch, its P15 row, and
  `x-<lang>-const-name` on a def are unreachable. `const.md`'s
  "a `$defs`-named const has no declaring member" is kept as the motivation but
  marked as not yet loadable.

- `14#11` — `generated-file-layout.md:206`. The fix-it is now **"rename the
  input file or directory"** (matching `json_schema.rs:454-461`);
  `x-output-module` is labelled a possible future escape hatch that is **not
  implemented**, with the note that there is no per-module escape hatch at all.

- `09#12` — `features/format.md:78-84`. The `$vocabulary:
  {format-assertion: true}` IDE-support schema is marked **Status:
  unimplemented** (no such artifact is published); assertion semantics still
  hold in generated code, only the machine-readable declaration is missing.

- `11#17` — `features/default.md:223`. The `$ref`-sibling half of the last-wins
  row is marked **unreachable today**, with the two reasons (scalar `$defs`
  rejects; object/array `default` rejects separately). The `allOf` half is left
  as the working example.

- `14#14` — `generated-file-layout.md:169-179`. Documented that "minus
  extension" strips `.json`/`.yaml`/`.yml` **and then a trailing `.nexusrpc`
  infix** (`chat.nexusrpc.yaml` → module `chat`), that the same stripping feeds
  the derived root type name, and that `chat.yaml` + `chat.nexusrpc.yaml` in one
  closure therefore collide. **Verified by running the built CLI** over a /tmp
  probe with both files: `invalid JSON schema in `chat`: duplicate JSON schema
  module path`.

- `14#10` — `generated-file-layout.md:270-286`, `:323-329`. Corrected the wrong
  side: hoisted cyclic types are re-exported by the **package-root
  `__init__.py` only**; a per-input module or its `__init__.py` importing them
  back would recreate the very cycle the hoist breaks. Added the concrete
  consequence (`from kb import Page`, never `from kb.content.page import
  Page`), and excluded hoisted types from the per-input `__all__` in the
  aggregator section.

- `14#8` (**D11**) — `services.md:118-125`. Documented `fqn: ""` as a load
  reject on a service or an operation (P7.1), with the rationale ("arbitrary
  characters" is not "no characters"; it is indistinguishable from omitting the
  key) and the required diagnostic.

- `05#8`, `06#7` (**D2**) — verified `uniqueItems.md:188-190` and
  `contains.md:241-244` are already correct and complete for the loosened
  loader (`null` is one value for uniqueness, two `null`s duplicate; a `null`
  element never matches a scalar matcher). Added a positive accepted-matrix row
  to each pinning the nullable-element shape, and confirmed neither rejected
  matrix lists it.

## Not fixed (deliberately)

- **`features/default.md:128` and `:202` still say Java uses
  `@JsonInclude(NON_NULL)`** and describe the default folding into the existing
  getter. This is the same drift as `13#9` and is factually wrong today (the
  generated POJO has no annotation; `getPriorityOrDefault()` exists alongside a
  `@Nullable getPriority()` — inspected in
  `samples/java/src/main/java/json_schema/definitions/chat/Message.java:109-115`).
  I left both rows untouched because **D9 is deferred**: the `get<Field>OrDefault`
  design decision and the P15 registration land together, and this table row is
  the thing that will be rewritten then. Recommend folding the `@JsonInclude`
  correction into that change rather than half-editing the row now.

## Cross-file requests

1. **`src/parser/json_schema.rs` (loader agent) — TypeScript keyword-named
   members must not reject.** `member_identifier_defect`
   (`src/parser/json_schema.rs:6688-6702`) applies `ident_is_reserved(language,
   &base)` uniformly, so every TS reserved word rejects as a property name. Per the now-explicit
   `properties.md` Stage-3 rule, TS's reserved set *in member position* is
   empty (`interface X { class: string }` compiles; so does `x.class`), and
   `properties.md`'s positive matrix now pins `{properties:{class:{type:
   string}}}` as accepted when generating Go + TS only. Requested change: drop
   the reserved-word check for TypeScript member identifiers (keep the
   syntactic checks — empty, leading digit, illegal character). Python and Java
   are unchanged.

2. **`src/parser/json_schema.rs:1617-1623` (loader agent) —
   `contentMediaType` diagnostic wording.** Current text says "the string is
   carried verbatim (drop it, or validate the media type in application code)",
   which is false (the schema is rejected) and gives the wrong remedy.
   Requested text, per `contentMediaType.md`:
   `` `contentMediaType` is not supported; the schema is rejected. Remove it — a media type belongs on the transport / payload envelope (HTTP `Content-Type`, Temporal payload metadata), not on the model. ``

3. **`src/parser/json_schema.rs:1609-1616` (loader agent) — split the
   `readOnly`/`writeOnly` diagnostic.** One message currently covers four spec
   rejects. Minimum acceptable split (per the tightened `readOnly.md` /
   `writeOnly.md`):
   - `readOnly: true` / `writeOnly: true` → the existing "model it on the
     output/input type" fix-it (name which keyword was seen);
   - `readOnly: false` / `writeOnly: false` → "…is the default and has no
     effect; remove it" — **must not** suggest splitting the type;
   - non-boolean → name the keyword and the offending value;
   - both `true` on one node → name the contradiction and both keywords.
   Note `rejects_read_only_false` (`:9130-9137`) pins the current shared
   message and will need updating with it.

4. **`src/parser/json_schema.rs` (loader agent) — empty `fqn`.** Confirming the
   D11 rule is now written at `services.md:118-125`: reject `fqn: ""` on both
   services and operations; the diagnostic should name the service/operation and
   offer "remove the `fqn` to take the default, or give it a non-empty value".

## Decisions wanted

1. **P1 exception (b) vs. the missing `string` opt-out (`09#8`).** P1's bounded
   exception for temporal round-trip loss is written as conditional on the loss
   being "recoverable through a per-field `string` opt-out". That opt-out does
   not exist, so **as written the exception is not satisfied on its own terms**
   — Python's sub-µs truncation and the legacy TS `date` fold are currently
   unrecoverable. I flagged this loudly in `format.md` but did **not** touch
   `PRINCIPLES.md`. My recommendation: **do not weaken P1.** Add a one-clause
   forward reference instead, e.g. after the condition: *"(the opt-out is
   specified in [[format]] and not yet implemented; until it ships the
   exception is provisional)"* — that keeps the principle honest without
   licensing an unrecoverable loss. Say the word and I will make that edit; it
   is the only `PRINCIPLES.md` change I would recommend.

2. **The fractional-band divergence (`13#4`).** After the java-emitter fix,
   `{"count": 9007199254740991.1}` will be **rejected** by Go and Java and
   **accepted** (as `9007199254740991`) by TypeScript and Python — a P1
   accept/reject split with no fix available to the two parse-boundary targets
   (neither owns `JSON.parse` / `json.loads`). I wrote it up in `type.md` as a
   documented, bounded limitation of TS and Python. The only alternative that
   closes it is **loosening Go and Java to accept**, i.e. having them judge the
   rounded value rather than the token — which contradicts the spec's own
   "written fractional part is zero" rule and would delete Go's existing
   assertion (`tests/generate_go.rs:1998`). I recommend keeping the documented
   split; if you prefer uniformity, that is a code decision for the Go and Java
   agents plus a `type.md` follow-up.

## Sample schema requests

None.

## Snapshot shifts

None from this agent — prose only.
