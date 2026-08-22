# contentEncoding / contentMediaType / contentSchema — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/contentEncoding.md` — `base64` / `base64url` are adopted and materialized to a native bytes type (Go `[]byte`, Java `byte[]`, Python `bytes`, TS `Uint8Array`) behind a generator-owned pinned regex; every other encoding rejects at load.
- `specs/json-schema/features/contentMediaType.md` — unsupported, rejected at load in every position (no emit site in the model), including alongside `contentEncoding`.
- `specs/json-schema/features/contentSchema.md` — unsupported, rejected at load; the subschema value is still checked for validity but never lowered.

## Summary

- The **shared gate is sound**: `src/json_schema/content_encoding.rs` is 165 clean lines, the two pinned regexes are identical in all four emitters, and I empirically confirmed all four targets agree on the accept set for alphabet, padding, embedded whitespace/newline and stray characters. The `^…$` anchor-normalization story checks out in all four (Go RE2 `$`, JS `/…$/u`, Python `\Z` rewrite, Java `matches()`).
- **Go emits code that does not compile** whenever a `contentEncoding` property also carries `minLength`/`maxLength`/`pattern`, or is an *optional* `const`/`enum`. `go vet` fails with `cannot indirect m.X (variable of type []byte)`. Only the direct-property path is affected — array items and typed-map members re-encode to a wire string correctly.
- **`const`/`enum` compare on different sides of the codec**: Go compares *decoded bytes*, TS/Python/Java compare the *wire string*. With the spec's own example (`const: "aGk="`), the wire `"aGl="` is **accepted by Go and rejected by the other three** — a P1 accept-set divergence reachable with a canonical schema.
- **`contentEncoding` + `format` is not gated at all.** With a temporal `format` the `contentEncoding` is silently dropped in all four targets (the field becomes a date, never base64-decoded, and Go still emits a dead pinned-regex var). With a non-temporal `format` (e.g. `email`) Go does not compile and the other three emit a field that can never validate.
- **The pinned regex does not pin the canonical form.** It admits non-canonical trailing bits (`"aGl="`, `"AB=="`, base64url `"aGl"`), so a parsed value re-serializes to a *different* wire string. This directly contradicts contentEncoding.md:93-95 ("the wire round-trips byte-identically with no re-canonicalization step"). Verified end-to-end in Go: `{"a":"aGl="}` → `{"a":"aGk="}`.
- **Java `equals`/`hashCode`/`toString` use identity semantics on `byte[]`** — two models parsed from the same payload are not `.equals()`, and `toString()` prints `[B@1b6d…`.
- `contentMediaType` / `contentSchema` reject **correctly in every position** I could reach (root, property, `$defs`, `items`, `additionalProperties`, `oneOf` branch, `contains`, `propertyNames`), including alongside `contentEncoding`. Their *diagnostic text* is wrong, though: `contentMediaType` claims "the string is carried verbatim", which is false (the schema is rejected) and does not match the spec's fix-it.
- The co-occurrence branch inside `validate_content_encoding` is **unreachable dead code** — `validate_schema_common` always rejects `contentMediaType`/`contentSchema` first — and the one test covering it passes vacuously.
- Testing: the cross-language conformance manifest (`samples/conformance/json-schema.json`, 4 cases) has **no content-encoding case at all**, despite base64 being the flagged P1 wire-divergence risk. No runtime test in any language covers the empty string, an embedded newline/space, non-canonical trailing bits, or a `contentEncoding` failure aggregated with a sibling failure.
- The Java/Python integration tests that *do* carry the risky combinations (`contentEncoding` + `minLength`/`pattern`/`const`/`default`) are **text-assertion only** — they never compile or run the output. The Go suite, which *does* compile and run, is the only one missing those combinations, which is exactly why the Go compile break survived.

## Implementation divergences

### 1. Go emits non-compiling code for `contentEncoding` + string constraints or an optional closed value
- **Severity** P0
- **Spec cite** contentEncoding.md:214-218 ("A co-occurring `maxLength`/`minLength`/`pattern` is **not** subsumed by the type … that predicate re-runs over the canonicalized wire string"); Property-testing matrix lines 230-232 list `pattern`, `maxLength` and `const` as accepted shapes.
- **Code cite** `src/generator/json_schema/go.rs:2810-2812` — the shared-`Validate` string-constraint branch guards on `temporal_kind(property).is_none()` but has **no** `content_encoding_kind` guard, so it emits `utf8.RuneCountInString(m.Field)` / `*m.Field` against a `[]byte`. `src/generator/json_schema/go.rs:5696-5703` — `render_go_closed_validate` builds `subject = (*m.Field)` for a non-required field, which is invalid for a slice; the sibling accessor at `go.rs:2571-2578` already knows bytes are nil-able and skips the deref.
- **What the spec requires** The wire predicate re-runs over the canonicalized base64 string before emit (P12), exactly as the temporal path does at `go.rs:2686-2715` (which re-formats to a `wire` local first).
- **What the code does** Emits Go that fails to compile.
- **Concrete failing input**
  ```yaml
  type: object
  properties:
    a: { type: string, contentEncoding: base64, minLength: 4 }
  ```
  → `vet: cannot indirect m.A (variable of type []byte)`. Same for `const`/`enum` on an *optional* property; a *required* `minLength` gives `cannot use m.A (variable of type []byte) as string value in argument to utf8.RuneCountInString`. A required `const`/`enum` is the only compiling case (it takes the `bytes.Equal` branch at `go.rs:5705-5726`) — which is why `tests/generate_go.rs:2542` passes.
- **Secondary consequence** Once the deref is fixed, `MarshalJSON` will double-report: it calls `addViolations(&errs, m.Validate())` *and* re-runs the same length/pattern checks inline over `wire`.
- **Confidence** High — reproduced with `go vet` on generated output for four distinct schemas.

### 2. `const`/`enum` are compared on the decoded bytes in Go and on the wire string in TS/Python/Java
- **Severity** P0 (cross-language accept-set divergence, P1 mandate)
- **Spec cite** PRINCIPLES P1 ("a value one language rejects … must be rejected by all"); contentEncoding.md:277-279 ("a supplied string literal MUST be valid for the declared encoding at load and is stored / echoed in canonical form").
- **Code cite** Go: `src/generator/json_schema/go.rs:5705-5726` (`bytes.Equal(subject, <decoded const var>)`) with the closed-value check reached from `UnmarshalJSON` via `go.rs:3045-3049`. TS: `src/generator/json_schema/typescript.rs:3199-3221` skips the const parser when the field is materialized and instead compares `raw.<field> !== "<literal>"`. Python: `src/generator/json_schema/python.rs` emits `a_value_raw not in ("aGk=",)`. Java: `src/generator/json_schema/java.rs` emits `"aGk=".equals(field.textValue())`.
- **What the spec requires** One accept/reject set across all four targets.
- **What the code does** Go accepts any wire string that *decodes to the same bytes*; the other three require the wire string to be literally equal to the schema literal.
- **Concrete failing input** Schema `{type: string, contentEncoding: base64, const: "aGk="}` (contentEncoding.md:232, verbatim). Wire `{"a":"aGl="}`: Go parses and re-emits `{"a":"aGk="}` with no error (verified by running generated code); TS/Python/Java raise `must equal "aGk="`. The mirror case is worse — with a *non-canonical* literal (`const: "aGl="`, which the loader accepts, see divergence 4) the TS/Python/Java serializers compare the re-encoded `"aGk="` against `"aGl="` and the field becomes **unsatisfiable on serialize**, while Go serializes fine.
- **Confidence** High — generated and executed for Go; read the emitted source for the other three.

### 3. `contentEncoding` alongside `format` is neither rejected nor coherently lowered
- **Severity** P0
- **Spec cite** `src/parser/json_schema.rs:2174-2178` states the design intent verbatim: "reject every other encoding at load, so no `contentEncoding` silently no-ops (P10)". contentEncoding.md's Interactions section (lines 260-287) never mentions `format`, and the reject table (lines 234-242) does not list it — so the spec is silent and the loader is permissive.
- **Code cite** `src/parser/json_schema.rs:2186-2262` — `validate_content_encoding` checks the value, the type, the encoding name, `contentMediaType`/`contentSchema` and the literals, but never `format`. Emitters then resolve the conflict independently: `src/generator/json_schema/go.rs:5157-5177` (`go_materialized_value`) and the type mapper prefer the temporal branch; `src/generator/json_schema/typescript.rs:3199-3203` treats a temporal *or* bytes node as "materialized" but the type mapper picks the temporal.
- **What the spec requires** Either a coherent lowering or a loud load reject (P6 / P7.1).
- **What the code does**
  - `{type: string, contentEncoding: base64, format: date}` → **accepted**; the field is `time.Time` / `LocalDate` / `datetime.date` / TS `string`; the base64 gate and decode are **never emitted** (verified across all four). Go still emits a dead `var sAContentEncoding = regexp.MustCompile(...)`.
  - `{type: string, contentEncoding: base64, format: email}` → **accepted**; the field is bytes, and the email regex is applied to the base64 wire string, so the field can never validate. In Go it does not even compile (`utf8.RuneCountInString(m.B)` on a `[]byte`, same root cause as divergence 1).
- **Confidence** High — generated all four targets and ran `go vet`.

### 4. The pinned regex admits non-canonical trailing bits, so the wire does not round-trip byte-identically
- **Severity** P1
- **Spec cite** contentEncoding.md:87-95 — "each has a single **strict canonical form** (no lenient variants accepted) … Because only the canonical form is accepted, the wire round-trips byte-identically with no re-canonicalization step"; and the runtime-fixture mandate at lines 246-248.
- **Code cite** `src/json_schema/content_encoding.rs:54-59` — the final group is `[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=` (and `[A-Za-z0-9_-]{2,3}` for base64url), which places no constraint on the unused low bits of the last character.
- **What the spec requires** Only the canonical form is accepted; decode→encode is the identity on the wire.
- **What the code does** `"aGl="`, `"AB=="`, `"//9="` and base64url `"aGl"` all match, decode fine in all four targets, and re-encode to a *different* string (`"aGk="`, `"AA=="`, `"//8="`, `"aGk"`). Verified end-to-end in Go generated code: input `{"req":"aGl="}` marshals back as `{"req":"aGk="}`; input `{"optUrl":"aGl"}` marshals back as `"aGk"`.
- **Interaction that turns it into a hard failure** Combined with `pattern` or `maxLength`, parse can succeed and serialize can fail: `{contentEncoding: base64, pattern: "^aGl"}` accepts the wire `"aGl="` on parse and then throws on serialize because the canonicalized wire is `"aGk="`. Combined with `const`/`enum` it produces divergence 2.
- **Fix direction** Restrict the last significant character class — base64 `…{2}[AQgw]==` / `…{3}[AEIMQUYcgkosw048]=`, base64url `…{1}[AQgw]` / `…{2}[AEIMQUYcgkosw048]` — or amend the spec to state that serialize re-canonicalizes and drop the byte-identity claim.
- **Confidence** High — the accept side is a direct reading of the regex; the decode/re-encode behavior was measured in Go, Python, Java and the generated TS codec.

### 5. Java `equals`/`hashCode`/`toString` use reference semantics on `byte[]` fields
- **Severity** P1
- **Spec cite** PRINCIPLES Java §1 ("POJOs … `equals`/`hashCode`/`toString` … boilerplate stays hidden behind a hand-written-feeling API (P2)"); contentEncoding.md:66-71 (a native bytes field is "what a human would have written").
- **Code cite** `src/generator/json_schema/java.rs:2991-2999` (`Objects.equals(this.<field>, that.<field>)` for every non-primitive field), `:3016-3022` (`Objects.hash(...)`), `:3025-3040` (string concatenation in `toString`). No `byte[]` special case anywhere in the file (`rg 'Arrays\.'` in `java.rs` returns nothing).
- **What the spec requires** Value semantics equivalent to what a human would write — `java.util.Arrays.equals` / `Arrays.hashCode` / `Arrays.toString` for arrays (and a deep comparison for `List<byte[]>`).
- **What the code does** `Showcase.equals` compares `byte[]` references, so two instances deserialized from the same payload are unequal and hash differently; `toString()` prints `[B@1b6d3586`.
- **Concrete failing input** `samples/java/.../Showcase.java` — parse `showcase-bytes.json` twice and `assertEquals(a, b)` fails. No existing Java test asserts model equality, so this is unobserved today.
- **Confidence** High — read the generator and the emitted `S.java`.

### 6. `contentMediaType`'s diagnostic is factually wrong and its fix-it does not match the spec
- **Severity** P2
- **Spec cite** contentMediaType.md:61-64 — "reject with a fix-it: remove it; the media type belongs on the transport / payload envelope, not the model"; and lines 39-58 ("Nowhere to emit it", "It cannot become a validator either", "Not a doc comment").
- **Code cite** `src/parser/json_schema.rs:1617-1623` — `"…`contentMediaType` is not supported; the string is carried verbatim (drop it, or validate the media type in application code)"`.
- **What the code does** Says the string "is carried verbatim", which is false — the whole schema is rejected, nothing is carried. The suggested remedy ("validate the media type in application code") is not the spec's remedy (move it to the transport / payload envelope) and reads as if the keyword were accepted-and-ignored.
- **Confidence** High.

### 7. The `contentMediaType`/`contentSchema`-alongside-`contentEncoding` reject is unreachable dead code
- **Severity** P2
- **Spec cite** contentEncoding.md:115-118 ("the reject there wins over materialization here").
- **Code cite** `src/parser/json_schema.rs:2224-2234` loops over `["contentMediaType", "contentSchema"]` and formats a bespoke message; but `validate_schema_node` calls `validate_schema_common` at `:1390` *before* `validate_content_encoding` at `:1401`, and `validate_schema_common` rejects both keywords unconditionally at `:1617` / `:1625`.
- **What the code does** The behavior is correct (the contentMediaType-owned reject wins, as the spec wants) but the branch can never fire, and the test that ostensibly covers it (`src/parser/json_schema.rs:8437-8443`) only asserts `error.contains("contentMediaType")`, which the generic message also satisfies — so the test passes vacuously and would keep passing if the branch were deleted.
- **Confidence** High — confirmed by running the generator on `{contentEncoding: base64, contentMediaType: image/png}` and observing the generic message.

### 8. Violation reasons never truncate a long value
- **Severity** P2
- **Spec cite** contentEncoding.md:198-200 — "The `Violation` `reason` names the encoding and the offending value … truncating a long value."
- **Code cite** Go `src/generator/json_schema/go.rs:4949-4998` (`fmt.Sprintf("must be base64-encoded, got %q", s)`); TS `src/generator/json_schema/typescript.rs:1689-1707` (`JSON.stringify(value)`); Python `src/generator/json_schema/python.rs:1284-1307` with `_quote` at `python.rs:909-915` (plain `json.dumps`); Java `src/generator/json_schema/java.rs:5566-5598` (raw `"…" + value + "…"`).
- **What the code does** A 1 MB base64 blob is echoed in full into the `Violation` reason in every target.
- **Confidence** High.

### 9. Go's closed-value Violation reason quotes the decoded bytes, not the wire string
- **Severity** P2
- **Spec cite** contentEncoding.md:198-200 (reason "names the encoding and the offending value"); the repo convention that a reason must name the offending value in the form the author wrote it.
- **Code cite** `src/generator/json_schema/go.rs:5704` (`go_closed_reason(&values, &subject)` where `subject` is the `[]byte` field) — emits `fmt.Sprintf("must be one of [\"YQ==\",\"Yg==\"], got %q", m.Choices)`.
- **What the code does** Prints `got "a"` (the decoded bytes) against a list of base64 literals. TS/Python/Java all print the wire string.
- **Confidence** High — read from generated `outgo.go`.

### 10. TS emits an unused `<FIELD>_CONST` for a materialized `contentEncoding` const
- **Severity** P2
- **Code cite** `src/generator/json_schema/typescript.rs:2049-2058` unconditionally declares the const, while `:3199-3221` skips `render_const_parser` for a materialized node and inlines the literal instead.
- **What the code does** `const TAG_CONST = "aGk=";` is declared and never referenced. The sample `tsconfig.json` does not set `noUnusedLocals`, so no test catches it; a consumer repo that does will fail to build. It also occupies a P15 identifier slot for nothing.
- **Confidence** High — reproduced (`rg TAG_CONST` finds only the declaration).

## Base64 decoder-strictness matrix

Behavior of the **generated code** (pinned regex, then decode). "gate" = the regex rejects before the decoder is reached. Native-decoder behavior in parentheses where it differs, to show what the gate is buying.

| input case | Go (`StdEncoding` / `RawURLEncoding`) | TS (generator-owned pure-JS) | Python (`b64decode(validate=True)` / `urlsafe_b64decode`) | Java (`getDecoder` / `getUrlDecoder`) | spec says | agree? |
|---|---|---|---|---|---|---|
| `base64` canonical padded `"aGk="` | accept → `hi` | accept → `hi` | accept → `hi` | accept → `hi` | accept | yes |
| `base64` URL-safe alphabet `"Pj4-"` | gate reject (native: reject) | gate reject | gate reject (native: reject) | gate reject (native: reject) | reject | yes |
| `base64` missing padding `"aGk"` | gate reject (native: reject) | gate reject | gate reject (native: reject) | gate reject (native: **accept**) | reject | yes |
| `base64` embedded newline `"aGk=\n"` | gate reject (native: **accept**, ignores `\r\n`) | gate reject (native decoder would yield garbage bytes) | gate reject (native: reject) | gate reject (native: reject) | reject | yes |
| `base64` embedded space `"aG k="` | gate reject (native: reject) | gate reject | gate reject (native: reject) | gate reject (native: reject) | reject | yes |
| `base64` stray `"aGk!"` | gate reject | gate reject | gate reject | gate reject | reject | yes |
| `base64` empty `""` | accept → 0 bytes | accept → 0 bytes | accept → 0 bytes | accept → 0 bytes | accept, 0 bytes | yes |
| `base64` **non-canonical trailing bits** `"aGl="` | **accept → `hi`, re-emits `"aGk="`** | **accept → `hi`, re-emits `"aGk="`** | **accept → `hi`, re-emits `"aGk="`** | **accept → `hi`, re-emits `"aGk="`** | spec claims only the canonical form is accepted and the wire round-trips byte-identically | **no** (divergence 4) |
| `base64` non-canonical `"AB=="` | accept → `0x00`, re-emits `"AA=="` | same | same | same | as above | **no** |
| `base64url` canonical unpadded `"Pj4-"` | accept → `>>>` | accept | accept | accept | accept | yes |
| `base64url` standard alphabet `"Pj4+"` | gate reject | gate reject | gate reject | gate reject | reject | yes |
| `base64url` with padding `"aGk="` | gate reject (native: reject) | gate reject | gate reject (native: **accept**) | gate reject (native: **accept**) | reject | yes |
| `base64url` length ≡ 1 mod 4 `"a"` | gate reject | gate reject | gate reject | gate reject | reject | yes |
| `base64url` non-canonical `"aGl"` | accept → `hi`, re-emits `"aGk"` | same | same | same | canonical only | **no** |
| leading newline `"\naGk="` | gate reject | gate reject | gate reject | gate reject | reject | yes |

Anchor semantics were verified independently: Go RE2 `$` = end-of-text, JS `/…$/u` without `m`, Python's `\Z` rewrite (`src/generator/json_schema/python.rs:1265-1278`), and Java `Matcher.matches()` (`src/generator/json_schema/java.rs:5567`, `:5584`) all reject `"aGk=\n"`. No trailing-newline hole.

## Testing gaps

### 1. No Go test exercises `contentEncoding` with a string constraint or an optional closed value
- **Severity** P0
- **What is untested** The exact combinations that fail to compile (divergence 1). `tests/generate_go.rs:2529-2603` covers only a **required** `enum` and a `default`; `samples/schemas/showcase.nexusrpc.yaml:174-190` has bare `blob`/`urlBlob` with no sibling assertions.
- **Spec line mandating it** contentEncoding.md:230-232 (matrix: "Combined with `pattern`", "Combined with `maxLength`", "`const` base64 literal") and 214-218.
- **Where the test should go** `tests/generate_go.rs`, extending `go_json_materialized_closed_values_and_defaults_stay_native` (it already runs `gofmt` + `go test`, so a compile break is caught immediately).
- **Suggested case** Add optional `const`, optional `enum`, and required + optional `minLength`/`maxLength`/`pattern` on `contentEncoding: base64` properties, then assert the bound counts the **encoded** string — e.g. `maxLength: 4` must accept `"aGk="` (4 encoded chars, 2 decoded bytes) and reject `"aGkhaQ=="` (8 chars, 5 bytes).

### 2. No test asserts `minLength`/`maxLength` count the *encoded* wire, not the decoded bytes
- **Severity** P0
- **What is untested** The interaction's whole point. `tests/generate_typescript.rs:203-212` and `tests/generate_java.rs:157-162` include `minLength` with `contentEncoding`, but both are text-assertion tests and neither uses a value where encoded and decoded lengths differ.
- **Spec line mandating it** contentEncoding.md:274-276 ("independent string assertions over the **encoded wire string** (not the decoded byte length)").
- **Where the test should go** All four per-language round-trip suites plus a conformance-manifest case.
- **Suggested case** `{contentEncoding: "base64", maxLength: 4}`: `"aGk="` (4 encoded chars / 2 decoded bytes) must be **accepted**; `"aGkh"` (4 chars / 3 bytes) accepted; `"aGkhaQ=="` (8 chars / 5 bytes) rejected. A decoded-length reading of the same bound would flip two of the three.

### 3. No conformance-manifest case for `contentEncoding`
- **Severity** P0
- **What is untested** Cross-language agreement on the base64 accept/reject line is asserted only by four independently-written per-language tests that happen to use the same three inputs; nothing in `samples/conformance/json-schema.json` pins it, and `tests/json_schema_conformance_manifest.rs` therefore cannot detect drift.
- **Spec line mandating it** PRINCIPLES P1; contentEncoding.md:244-258 (Runtime fixtures).
- **Where the test should go** `samples/conformance/json-schema.json`, new case `content-encoding-canonical-forms`, with consumer anchors in all four suites.
- **Suggested case** Accept `""`, `"Pj4+"` (base64) / `"Pj4-"` (base64url); `parse_failures` for `"Pj4-"` under base64, `"aGk"` under base64, `"aGk="` under base64url, `"aG k="`, `"aGk=\n"`, `"aGk!"` with `expected_paths` naming the field.

### 4. Non-canonical trailing bits are untested everywhere
- **Severity** P0
- **What is untested** No test in the repo feeds `"aGl="`, `"AB=="` or base64url `"aGl"` to anything. The four inline tests in `src/json_schema/content_encoding.rs:131-164` cover alphabet, padding, whitespace and length but not trailing bits.
- **Spec line mandating it** contentEncoding.md:87-95 (single strict canonical form; byte-identical round trip) and 246-248.
- **Where the test should go** `src/json_schema/content_encoding.rs` (the accept/reject decision) **and** all four round-trip suites (the byte-identity claim).
- **Suggested case** `assert!(!is_valid(Encoding::Base64, "aGl="))` once the regex is tightened; and a round-trip fixture asserting the emitted wire equals the input wire for every accepted value.

### 5. Empty string / zero-length payload has no runtime coverage
- **Severity** P1
- **What is untested** `""` → zero bytes → `""`. Only the Rust `is_valid` unit test (`content_encoding.rs:138`, `:156`) covers it; no Go/TS/Python/Java test round-trips an empty blob. The Go nil-vs-empty distinction is the risk (a zero-length decode must stay non-`nil` or the field would be dropped on serialize).
- **Spec line mandating it** contentEncoding.md:228 ("Empty content `""` → zero-length bytes"), 255 ("Empty string → zero-length bytes, round-trips to `""`").
- **Where the test should go** `samples/*/tests` content-encoding tests for all four languages.
- **Suggested case** Wire `{"blob":"","urlBlob":""}` → model holds a zero-length bytes value that is **present** (not absent/null), and re-serializes to `{"blob":"","urlBlob":""}`. I verified Go passes this today, but nothing pins it.

### 6. `contentEncoding` + `format` has no coverage in any direction
- **Severity** P1
- **What is untested** Neither an accept-and-lower nor a reject; `rg 'contentEncoding' tests/ src/parser` shows no test pairing the two keywords.
- **Spec line mandating it** `src/parser/json_schema.rs:2177` ("so no `contentEncoding` silently no-ops (P10)").
- **Where the test should go** `src/parser/json_schema.rs` inline loader tests, next to `rejects_content_media_type_alongside_content_encoding` (`:8437`).
- **Suggested case** `{type: string, contentEncoding: base64, format: date}` and `{…, format: email}` → both should reject with a fix-it (once the spec picks a disposition).

### 7. No test that a `contentEncoding` failure aggregates with a sibling failure
- **Severity** P1
- **What is untested** P11 one-shot aggregation for the bytes path. Every existing negative test (`samples/go/tests/json_schema_showcase_test.go:319-337`, `samples/typescript/tests/json-schema-showcase.test.ts:1085-1098`, `samples/python/tests/test_showcase.py:955-961`, `samples/java/.../JsonSchemaShowcaseRoundTripTest.java:464-489`) sends **one** bad field and asserts on the message text only.
- **Spec line mandating it** contentEncoding.md:257-258 ("A failing sibling constraint (`maxLength` / `pattern` / another field) → **all** reported in one shot (**P11**)").
- **Where the test should go** All four round-trip suites.
- **Suggested case** One payload with a malformed `blob`, an over-long `maxLength` field, and a missing required key; assert exactly three `Violation`s with the expected paths.

### 8. `default` literal validity is tested only through the nullable-`oneOf` path
- **Severity** P2
- **What is untested** `src/parser/json_schema.rs:2248` checks `const` **and** `default` on the node itself, and `:2253` checks `enum`. There are loader tests for `const` (`:8446`), `enum` (`:8462`) and for a `default` on a nullability `oneOf` wrapper (`:9217-9220`, which reaches the same predicate via the synthesized `schema_with_default` at `:4365-4374`), but none for a plain `{type: string, contentEncoding: …, default: …}`. The base64url arm of the `default` check is also uncovered.
- **Spec line mandating it** contentEncoding.md:119-121, 242.
- **Where the test should go** `src/parser/json_schema.rs` inline tests, next to `rejects_const_violating_content_encoding`.
- **Suggested case** `{type: string, contentEncoding: base64, default: "a-b_"}` and `{…, contentEncoding: base64url, default: "aGk="}` → reject.

### 9. `contentSchema` alongside `contentEncoding` has no loader test
- **Severity** P2
- **What is untested** Only the `contentMediaType` arm is tested (`src/parser/json_schema.rs:8437-8443`); the generic keyword table at `:9109-9114` covers each keyword alone.
- **Spec line mandating it** contentEncoding.md:241; contentMediaType.md:91.
- **Where the test should go** `src/parser/json_schema.rs`, alongside `rejects_content_media_type_alongside_content_encoding`.
- **Suggested case** `{type: string, contentEncoding: base64, contentSchema: {type: object}}` → reject.

### 10. Java model value-equality for `byte[]` is unasserted
- **Severity** P2
- **What is untested** No Java test compares two whole models; `rg 'assertEquals' samples/java` only compares getters.
- **Spec line mandating it** PRINCIPLES Java §1.
- **Where the test should go** `samples/java/src/test/java/jsonschema/JsonSchemaShowcaseRoundTripTest.java`.
- **Suggested case** Parse `showcase-bytes.json` twice, `assertEquals(a, b)` and `assertEquals(a.hashCode(), b.hashCode())`.

### 11. Java and Python integration coverage of the risky combinations is text-only
- **Severity** P2
- **What is untested** `tests/generate_java.rs:614-700` and the Python analogue assert on rendered substrings and never invoke `javac` / import the module, so a semantically broken emission (or a Go-style deref bug) would pass. `tests/generate_typescript.rs:2030-2057` likewise only greps `models.ts`.
- **Where the test should go** Route these schemas through the sample projects (which do compile and run), or add a compile step.

## Combination gaps

| Feature A x Feature B | spec says | tested? | risk |
|---|---|---|---|
| `contentEncoding` x `minLength`/`maxLength` | bound applies to the **encoded** wire string, re-checked on serialize (contentEncoding.md:274-276, 214-218) | partial — TS/Java/Python text-assertion only; **no runtime test, none with differing encoded vs decoded length**; Go untested and non-compiling | **P0** — divergence 1 |
| `contentEncoding` x `pattern` | wire string must satisfy both; reuses the RE2 gate (contentEncoding.md:272-273) | partial — TS/Java text-assertion; Go untested and non-compiling | **P0** |
| `contentEncoding` x `const` (optional field) | accepted; literal validated at load, echoed canonical (contentEncoding.md:232) | no — only *required* `const` is compiled (Go) or text-asserted (TS/Java) | **P0** — non-compiling Go |
| `contentEncoding` x `const`/`enum` (comparison basis) | one accept set across targets (P1) | no | **P0** — divergence 2 |
| `contentEncoding` x `enum` (optional field) | accepted | no | **P0** — non-compiling Go |
| `contentEncoding` x `format` | spec silent | no | **P0** — divergence 3 |
| `contentEncoding` x non-canonical wire | only canonical accepted; byte-identical round trip (contentEncoding.md:87-95) | no | **P1** — divergence 4 |
| `contentEncoding` x `default` (literal validity) | literal must be valid for the encoding at load (contentEncoding.md:119-121) | partial — only via the nullable-`oneOf` wrapper (`:9217-9220`); no direct-property or base64url case | P2 |
| `contentEncoding` x nullability (`oneOf` T/null) | orthogonal; `null` skips check and is not materialized (contentEncoding.md:280-281) | partial — TS text-assertion (`tests/generate_typescript.rs:239-243`); no runtime test in any language | P1 |
| `contentEncoding` x empty string | `""` → zero-length bytes, round-trips to `""` (contentEncoding.md:228, 255) | Rust unit only | P1 |
| `contentEncoding` x sibling failure (P11 aggregation) | all violations in one shot (contentEncoding.md:257-258) | no | P1 |
| `contentEncoding` x `oneOf` sum type (≥2 non-null branches) | deferred, load reject (contentEncoding.md:282-286) | yes — `src/parser/json_schema.rs:10800-10812` | ok |
| `contentEncoding` x `oneOf` nullability wrapper | materializes normally (contentEncoding.md:285-286) | load-level yes (`:9219`); runtime no | P1 |
| `contentEncoding` x `contentMediaType` | reject, owned by contentMediaType (contentEncoding.md:241) | yes (`:8437`) but passes vacuously (divergence 7) | P2 |
| `contentEncoding` x `contentSchema` | reject | no | P2 |
| `contentEncoding` x `contains` matcher | reject with a matcher-specific fix-it (`src/parser/json_schema.rs:2584-2588`) | yes — `:8573-8578` | ok |
| `contentEncoding` x `propertyNames` | reject | yes — matcher allowlist reject verified by running the generator | ok |
| `contentEncoding` x array `items` / typed map values | materialize per element/member; wire checks over the encoded string | yes — Go/TS/Python/Java showcase (`blobs`, `blobIndex`); Go re-encodes to `wire` correctly (`go.rs:3697-3740`, `:4595-4615`) | ok |
| `contentEncoding` x `uniqueItems` over bytes | not specified | accepted, untested | P2 — uniqueness is computed over the raw JSON strings, so two non-canonical spellings of the same bytes count as distinct |
| `contentMediaType` x every position | reject everywhere (contentMediaType.md:60-64) | partial — generic keyword table `:9109-9114` covers a property; I verified root/`$defs`/`items`/`additionalProperties`/`oneOf`/`contains`/`propertyNames` by hand, but no test pins them | P2 |
| `contentSchema` x invalid subschema value | must be a valid JSON Schema; recursed for validity but never lowered (contentSchema.md:66) | partial — `:9113` covers the plain case; recursion into an invalid inner subschema verified by hand only | P2 |
| `contentSchema` x present without `contentMediaType` | reject as inert (contentSchema.md:68) | yes — `:9113-9114` (that case has no `contentMediaType`) | ok |

## Verified-good

- The pinned regexes are byte-identical across all four emitters and are proven to pass the `pattern` RE2 gate at load (`src/json_schema/content_encoding.rs:96-104`).
- End-anchor semantics agree in all four targets: Go RE2 `$`, JS `/…$/u`, Python's `\Z` rewrite (`python.rs:1265-1278`), Java `Matcher.matches()` (`java.rs:5567`, `:5584`). `"aGk=\n"` is rejected everywhere — measured, not assumed.
- The gate genuinely neutralizes every decoder-leniency difference I could find: Go's `StdEncoding` ignoring `\r\n`, Java's basic decoder accepting unpadded input, Java/Python's URL decoders accepting padding. All are rejected before the decoder runs.
- The generator-owned TS codec is correct: no `Buffer`, no `atob`/`btoa` (`typescript.rs:1628-1712`), correct on a 3000-byte round trip, and its 32-bit accumulator never overflows into the output (only the low ≤14 bits are ever read).
- Same-bytes-different-wire is enforced and tested: `">>>"` → `"Pj4+"` (base64) vs `"Pj4-"` (base64url), asserted in all four suites via `showcase-bytes.json`.
- Materialized types match the spec exactly: Go `[]byte`, Java `byte[]`, Python `bytes`, TS `Uint8Array` — confirmed in generated output for direct properties, array items, typed-map members and the nullability `oneOf` wrapper.
- Load rejects that work and are tested: non-string `contentEncoding` value (`:8410`), `contentEncoding` on a non-`string` type (`:8404`), every unsupported encoding name including `base16`/`quoted-printable`/`7bit`/`8bit`/`binary` (`:8419`, plus `content_encoding.rs:107-129`), a `const`/`enum` literal invalid for its encoding (`:8446`, `:8462`), `contentEncoding` on a sum-type branch (`:10800`), `contentEncoding` in a `contains` matcher (`:8573`) and in `propertyNames`.
- `contentMediaType` and `contentSchema` reject in every position I probed, and `contentSchema`'s value is still recursed for validity (an unknown keyword inside `contentSchema` reports the inner diagnostic, per contentSchema.md:66).
- Go's array-item and typed-map paths do the right thing that the direct-property path gets wrong: they re-encode to a `wire` local before applying the string predicates (`go.rs:3697-3740`, `:4595-4615`).
- The Go `<Field>OrDefault()` accessor already special-cases bytes as nil-able and skips the pointer deref (`go.rs:2571-2578`) — the fix template for divergence 1.
