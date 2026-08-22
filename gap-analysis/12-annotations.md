# Annotations (title / description / $comment / examples / deprecated / readOnly / writeOnly) — gap analysis

## Scope (specs covered, one line each)

- `specs/json-schema/features/title.md` — short label → doc-comment summary line; never an identifier; reject non-string / empty / multi-line.
- `specs/json-schema/features/description.md` — prose → doc-comment body; **owns** the shared doc-comment assembly, wrapping (88 cols) and per-language escaping.
- `specs/json-schema/features/comment.md` — `$comment` accepted, string-checked, silently dropped; must never reach generated source.
- `specs/json-schema/features/examples.md` — the canonical accept-and-ignore (P7.1 exception); fully inert, array-MUST deliberately unenforced.
- `specs/json-schema/features/deprecated.md` — boolean; lowers to a native marker per target (Go `// Deprecated:` paragraph, TS `@deprecated`, Java `@Deprecated` + `@deprecated`, Python PEP 702 `@deprecated(..., category=None)`); merges by OR.
- `specs/json-schema/features/readOnly.md` / `writeOnly.md` — rejected wholesale at load (directional, no single-type lowering), with four enumerated reject reasons each.
- `specs/json-schema/PRINCIPLES.md` — P1 (polyglot parity), P2 (idiomatic output), P7.1 (loud reject; inert-annotation exception), P10/P12 (annotations are runtime-inert), P13/P15 (never an identifier), **Go §1** (every exported declaration carries a name-led doc comment, with a synthesized fallback).

## Summary

- **One P0**: the Python docstring escaper handles `\` and `"""` but not a *trailing* `"`. A single-line `title`/`description` whose last character is a double quote emits `"""He said "hi""""` — a hard `SyntaxError`. Reproduced on model class docstrings, attribute docstrings, service docstrings and operation docstrings. The shared helper is also used by the WIT/proto front-end.
- **Non-string `title`/`description` are silently coerced, not rejected**, everywhere except the document's flattened root schema: `$defs.R.title: 42` renders `// R 42`, `services.Svc.description: 42` renders `// 42`. The two existing unit tests only exercise the root position, which is why this has gone unnoticed. Applies to `.json` inputs too.
- **Go §1 is not met for the service/operation/client bindings**: when the envelope supplies a `description`, the emitted godoc is the bare prose (`// Send messages and look up rooms.` above `var ChatService`), not name-led. Only the *fallback* path is name-led. `ServiceName`, `Violation.Path/Reason` and `ValidationError.Violations` carry no doc comment at all.
- **Java puts the prose Javadoc on the private field and the `@deprecated` tag on the getter** — the two halves of one doc comment are split across two declarations, and `javadoc` skips private members, so authored prose never reaches published docs.
- **Python drops both `title` and `deprecated` on a named `oneOf` union alias**; Go, TypeScript and Java all emit them. This is a P1 parity break in the one keyword the spec insists is "a compile-/lint-time signal in every target".
- Escaping is otherwise solid and well tested: `tests/doc_rendering.rs` compiles/typechecks/parses hostile `*/`, `<`, `&`, `\`, `"""` fixtures in all four languages, and the checked-in `samples/{go,python,typescript,java}` trees are **golden snapshots asserted by `cargo test`** (`tests/generate_*.rs`), so ordinary doc-comment output is regression-locked.
- `$comment` and `examples` are genuinely inert: I probed 12 positions (type, property, `items`, inline object, `oneOf` branch, `allOf` branch, `additionalProperties`, `propertyNames`, `contains`, alongside `default`) across all four backends and found **zero** leakage. But there is **no test** asserting this at the generation level, which `comment.md` explicitly promises.
- `readOnly`/`writeOnly` reject broadly and correctly (verified in 8 nesting positions), but a **single generic diagnostic** covers all four spec-enumerated reject reasons, including the `false` no-op case the spec says should get its own "has no effect" note.
- Cross-language `deprecated` placement diverges on synthesized declarations: Go additionally marks the synthesized closed-value *type* as deprecated when only the *property* is; Java/TS/Python mark only the member.
- All four emitters `trim()` every description line, so Markdown indentation (nested lists, indented/fenced code) is silently reflowed — contrary to `description.md`'s "passed through **verbatim**".

## Implementation divergences

### 1. Python docstring escaping misses a trailing `"` — generated Python does not parse
- **Severity** P0
- **Spec cite** `description.md:169` — "Python | escape `\` → `\\` and `\"\"\"` → `\\\"\\\"\\\"` so the body can't terminate the docstring."
- **Code cite** `src/generator/python.rs:7013-7017` (`python_docstring_literal_text`) and the single-line emission path `src/generator/python.rs:6959-6963`.
- **What the spec requires** The body must be escaped so it cannot terminate the docstring.
- **What the code does** Only `\` and the literal three-quote sequence are escaped. In the single-line branch the docstring is emitted as `"""` + text + `"""` with no separating newline, so a body ending in one `"` produces four consecutive quotes.
- **Concrete failing input**
  ```yaml
  $defs:
    R:
      description: Type ends with a quote "
      type: object
      additionalProperties: false
      properties:
        a: { description: 'Trailing quote "', type: string }
  ```
  emits
  ```python
  class R:
      """Type ends with a quote """"

      a: str | None = None
      """Trailing quote """"
  ```
  `python3 -c "import ast; ast.parse(open('models.py').read())"` → `SyntaxError: unterminated string literal`. Reproduced identically for a service/operation `description` in `services.py`. Two-quote endings are benign (implicit concatenation); the one-quote ending is fatal. Multi-paragraph bodies are safe because the terminator lands on its own line.
- **Blast radius** `render_python_docstring` is the shared helper (`src/generator/python.rs:6858`), so the WIT/proto front-end is affected too.
- **Confidence** High (reproduced end-to-end).

### 2. Non-string `title` / `description` silently coerce instead of rejecting
- **Severity** P1
- **Spec cite** `title.md:78-80` and matrix `title.md:192`; `description.md:67-68` and matrix `description.md:198` — non-string → **reject** (the spec's own MUST).
- **Code cite** `src/parser/json_schema.rs:63-64` (`title: Option<String>`, `description: Option<String>`); `validate_annotations` at `src/parser/json_schema.rs:2358-2407` performs no value-shape check for these two, relying entirely on the serde type. `Service.description` / `Operation.description` at `src/parser/json_schema.rs:37,48` have the same shape.
- **What the code does** For any nested `Schema` (a `$defs` entry, a `properties` member, `items`, …) and for the envelope's service/operation `description`, serde_yaml's plain-scalar leniency hands the raw scalar text to the `String` visitor, so `42`/`true` become `"42"`/`"true"` and flow straight into the doc comment. Only the **document root** schema rejects, because `Document`'s `#[serde(flatten)] root: Schema` (`src/parser/json_schema.rs:31`) routes those values through serde's strict `Content` buffer. Sequences and mappings still reject.
- **Concrete failing input** (`nexgen go`, exit 0)
  ```yaml
  nexusrpc: "1.0.0"
  services: { Svc: { fqn: a.b.Svc, description: 42, operations: { run: {...} } } }
  $defs:
    R: { title: 42, type: object, additionalProperties: false, properties: { xs: { type: string, description: 7 } } }
  ```
  → `// 42` above `var Svc`, `// R 42`, `// Xs 7`. Identical behavior for a `.json` input file (the loader parses JSON through serde_yaml).
- **Why the tests miss it** `rejects_non_string_title` / `rejects_non_string_description` (`src/parser/json_schema.rs:11895-11904`) build the schema via `numeric_reject`, which places the keyword on the **root** schema — the one position that is strict.
- **Confidence** High (reproduced for YAML and JSON, at `$defs`, property and envelope positions).

### 3. Go service / operation / client doc comments are not name-led when prose is authored
- **Severity** P1
- **Spec cite** `PRINCIPLES.md` Go §1 — "Every exported type, struct field, function, method, var, and const the generator emits … gets a `//` doc comment whose opening line leads with the identifier itself, **never a bare, unattributed sentence**"; `description.md:140-151` (the name-led rule governs whatever text opens the comment).
- **Code cite** `src/generator/json_schema/go.rs:5837-5843` — `render_go_doc_comment(output, indent, doc, fallback)` applies the name-led prefix **only** to `fallback`; an authored `doc` is emitted verbatim. Call sites: `:1145` (service var), `:1163` (operation field), `:1254` (`<Service>Client` type), `:1287` (client operation method). Contrast `render_go_schema_doc` at `:5854-5911`, which does apply `name_led`.
- **Evidence in checked-in output** `samples/go/chat/chat.go:11` → `// Send messages and look up rooms.` above `var ChatService`; `samples/go/chat/chat.go:14` → `// Post a message to a room.` above `SendMessage`. `golint`/`revive` emit "comment on exported var ChatService should be of the form \"ChatService ...\"".
- **Confidence** High.

### 4. Java emits the description/title Javadoc on the private field, and the `@deprecated` tag on the getter with no body
- **Severity** P1
- **Spec cite** `description.md:111` — "Java | `/** … */` Javadoc above the class/**getter**/method"; `title.md:123`; `deprecated.md:145` — "The tag renders directly below the `description` body."
- **Code cite** `src/generator/json_schema/java.rs:2858-2865` (`render_javadoc(output, "    ", field.doc)` immediately above `private final …`) versus `src/generator/json_schema/java.rs:2880-2895`, which emits a Javadoc containing **only** the `@deprecated` tag above the getter (the comment there even records the choice: "the rationale lives on the field above").
- **What the code does** Splits one logical doc comment across two declarations. `javadoc` excludes private members at the default `-protected` visibility, so the authored prose never reaches the generated documentation, and the getter's `@deprecated` tag is orphaned from the rationale the spec says it must sit under.
- **Concrete input** `properties: { kind: { description: "A closed value set.", deprecated: true, type: string, const: text } }` →
  ```java
  /** A closed value set. */
  private final @Nullable Kind kind;
  ...
  /** @deprecated This field is deprecated. */
  @Deprecated
  public @Nullable Kind getKind() { ... }
  ```
- **Confidence** High.

### 5. Python drops `title` and `deprecated` on a named `oneOf` union alias
- **Severity** P1
- **Spec cite** `deprecated.md:120` (a `$defs` type → the generated type, Python row `deprecated.md:133`); `title.md:110`; `PRINCIPLES.md` P1 (identical treatment across targets) and `deprecated.md:74-76` ("a compile-/lint-time signal in **every** target").
- **Code cite** `src/generator/json_schema/python.rs:688-706` — the `TypeAlias` emission passes only `schema.description` to `render_python_docstring` and never consults `schema.title` or `schema.deprecated`.
- **Concrete input**
  ```yaml
  $defs:
    U: { title: UNION TITLE, description: UNION BODY, deprecated: true, oneOf: [{type: string},{type: integer}] }
  ```
  - Go `src/generator/json_schema/go.rs:2054`: `// U UNION TITLE` + `// Deprecated: This type is deprecated.`
  - TypeScript: `/** UNION TITLE\n *\n * UNION BODY\n *\n * @deprecated */`
  - Java: `/** UNION TITLE … @deprecated This type is deprecated. */ @Deprecated`
  - Python: `U: typing.TypeAlias = str | int` + `"""UNION BODY"""` — **no title line, no marker**.
- **Note** A `TypeAlias` cannot carry a decorator (the code comment at `:683` says so), but `typing.Annotated[str | int, typing_extensions.deprecated(...)]` is available, and the title is unconditionally droppable prose with no such excuse.
- **Confidence** High (reproduced in all four languages).

### 6. Go stutter guard is a raw prefix match and suppresses the name lead spuriously
- **Severity** P2
- **Spec cite** `title.md:143-147` / `description.md:152-154` (stutter guard) read against `PRINCIPLES.md` Go §1 ("leads with the identifier **itself**").
- **Code cite** `src/generator/json_schema/go.rs:5864-5870` — `text.to_lowercase().starts_with(&name.to_lowercase())`, with no word-boundary check.
- **What the code does** Any opening text whose *first word merely begins with* the identifier suppresses the prefix, producing a comment that is not name-led.
- **Concrete inputs** field `Id` + `title: "Identifier of the room"` → `// Identifier of the room`; field `Name` + `description: "Names the thing…"` → `// Names the thing…`; union type `U` + `title: "UNION TITLE"` → `// UNION TITLE`. All three fail `golint`'s "comment should be of the form `<Name> …`".
- **Confidence** High on behavior; medium on classification (the spec sentence is literally satisfied, the Go §1 mandate is not).

### 7. Generator-owned Go runtime exported struct fields carry no doc comment
- **Severity** P2
- **Spec cite** `PRINCIPLES.md` Go §1 — "Every exported type, **struct field**, function, method, var, and const the generator emits — schema-derived or **generator-owned runtime alike**" and it names `Violation` / `ValidationError` / "service/operation client bindings" explicitly.
- **Code cite** `src/generator/json_schema/go.rs:1479` (`type Violation struct {\n\tPath   string\n\tReason string\n}`), `:1490` (`type ValidationError struct {\n\tViolations []Violation\n}`), `:1158` (`\tServiceName string` inside the service var's anonymous struct).
- **Evidence** `samples/go/chat/definitions.go:19,20,35`; `samples/go/chat/chat.go:13`. A scan of every generated `.go` file under `samples/go` found these three field kinds to be the *only* undocumented exported declarations — every type, func, method, var and const is otherwise covered.
- **Confidence** High.

### 8. All four emitters trim leading whitespace from each description line, reflowing Markdown
- **Severity** P2
- **Spec cite** `description.md:60-64` — "Authors' Markdown is passed through **verbatim** (escaped only for the comment block); the generator does not render, reflow, or reinterpret it"; `description.md:156-160` — wrapping is "per source line, so an author's explicit line breaks and paragraph breaks are kept".
- **Code cite** `src/generator/go.rs:3310`, `src/generator/typescript.rs:4771`, `src/generator/java.rs:348`, `src/generator/python.rs:6871` — each does `line.trim()` before wrapping (and then re-joins on `split_whitespace()`).
- **Concrete input** a description containing `  - indented item` and a fenced ```` ```go ```` block indented under a list renders with all leading indentation stripped in every target, changing the Markdown nesting level that godoc/TypeDoc/Sphinx/Javadoc will render. Verified in all four outputs.
- **Confidence** High.

### 9. `readOnly` / `writeOnly` emit one generic diagnostic for four distinct spec reject reasons
- **Severity** P2
- **Spec cite** `readOnly.md:81-90` (directional / non-boolean / `false` no-op with "Diagnostic notes it has no effect" / contradictory pair), matrix `readOnly.md:105-110`; `writeOnly.md:56-65`, matrix `writeOnly.md:80-85`.
- **Code cite** `src/parser/json_schema.rs:1609-1616` — a single `contains_key` check over both keywords producing one message regardless of the value or which keyword was present.
- **What the code does** `{readOnly: false}`, `{readOnly: "true"}`, `{readOnly: true, writeOnly: true}` and `{writeOnly: true}` all yield ``readOnly`/`writeOnly` is not supported; a directional annotation has no single-type lowering…`` — the `false` case is told to "split the type into request/response shapes" when the correct fix-it is "delete it, it does nothing", and the contradictory-pair case gets no mention of the contradiction. The reject *behavior* is correct in all cases (verified in 8 nesting positions), only the diagnostics diverge. Note `rejects_read_only_false` (`src/parser/json_schema.rs:9130-9137`) deliberately pins the current shared message.
- **Confidence** High.

### 10. `$comment` on a Nexus envelope root rejects with a misleading "root is not a model" diagnostic
- **Severity** P2
- **Spec cite** `comment.md:33` ("It may appear on any subschema — … the document root"), `comment.md:37` (accepted and dropped).
- **Code cite** `src/parser/json_schema.rs:6088-6096` (`root_is_schema_shaped`) exempts only `description` from the schema-shaped test; anything in `extra` — including `$comment`, `examples`, `deprecated` and `title` — makes the envelope root look like a model.
- **Concrete failing input** a `nexusrpc: "1.0.0"` document with a top-level `$comment: internal note` → `a Nexus JSON schema document root is an envelope, not a model; move the model into $defs`.
- **Confidence** High on behavior; medium on whether the envelope root counts as "the document root" for `comment.md`'s purposes.

### 11. A `$ref`-sibling `description` lands on the synthesized hoisted type, not the declaring member
- **Severity** P2
- **Spec cite** `description.md:99` (property subschema → doc comment on the generated **field/member**) and `description.md:74-78` / `:215-219` (use-site sibling wins).
- **Code cite** the `$ref`+sibling rewrite (`src/parser/json_schema.rs:5224` `expand_branches`) merges into a new inline object which `hoist_inline_object_shapes` then names `<Model><Property>`; the merged `description` travels with the hoisted type.
- **Concrete input** `a: { $ref: "#/$defs/Named", description: USE-SITE WINS }` →
  ```go
  // A corresponds to the "a" JSON property.     <- member falls back to the placeholder
  A *Ra `json:"a,omitempty"`
  ...
  // Ra TARGET TITLE
  //
  // USE-SITE WINS                                <- prose landed on the hoisted type
  type Ra struct { … }
  ```
  The last-wins resolution itself is correct; the placement is arguably not.
- **Confidence** Medium (this is entangled with `[[ref]]`'s hoist behavior, which is out of this group's scope).

### 12. Go marks a synthesized closed-value *type* deprecated when only the *property* is
- **Severity** P2
- **Spec cite** `deprecated.md:122` — a property subschema marks "the generated **field / getter / member**"; P1 parity.
- **Code cite** `src/generator/json_schema/go.rs:1645-1654` passes the *property* schema to `render_go_schema_doc` with `kind = "type"` for the synthesized `<Model><Field>` closed-value type.
- **Concrete input** `kind: { deprecated: true, type: string, const: text, description: "A closed value set." }` → Go emits `// RootKind A closed value set.` + `// Deprecated: This type is deprecated.` on `type RootKind string` **and** the field marker, so `staticcheck` SA1019 fires on every mention of the type. Java's nested `Kind` value class, the TS `"text"` literal and Python's `Literal["text"]` carry no marker. (Conversely, Go's synthesized *inline union* interface `RootUn` inherits neither the property's prose nor its marker — `src/generator/json_schema/go.rs:2054` looks the schema up by model name and falls back to `Schema::default()` for inline unions, so Go is internally inconsistent too.)
- **Confidence** High.

## Testing gaps

### 1. No test for a doc string that *ends* in a double quote (the P0 above)
- **Severity** P0
- **Untested** Python docstring termination safety for bodies ending in `"`.
- **Spec line** `description.md:169` (Python escaping must prevent the body terminating the docstring).
- **Where** `tests/doc_rendering.rs` — the `HOSTILE_DOCUMENTATION_SCHEMA` (`tests/doc_rendering.rs:12-50`) contains `"""` mid-line but every string ends in `safely.`/`paragraph.`, so the boundary case is never hit.
- **Suggested case** Add `title: 'Ends in a quote "'` and `description: 'Body ends in a quote "'` on the type, a property, the service and the operation; `python_hostile_documentation_is_wrapped_escaped_and_parses` will then fail until the escaper is fixed.

### 2. No generation-level test that `$comment` / `examples` never reach the emitted source
- **Severity** P1
- **Untested** That the keywords leave no trace in generated Go/TS/Python/Java source. Only the loader is tested (`src/parser/json_schema.rs:9292-9296` non-string `$comment`, `:9299-9320` accepts-annotations, `:9764-9786` allOf discards them from the merged schema).
- **Spec line** `comment.md:101-102` — "a generation-snapshot test confirms it leaves the emitted source unchanged"; `examples.md:103` — "accepted, dropped, no output".
- **Where** `tests/doc_rendering.rs` (new test) or as an assertion block in `tests/generate_{go,typescript,python,java}.rs`.
- **Suggested case** One schema carrying uniquely-tokenized `$comment: NEXGEN_COMMENT_LEAK` / `examples: [NEXGEN_EXAMPLE_LEAK]` at every position (type, property, `items`, inline object, `oneOf` branch, `allOf` branch, `additionalProperties`, `propertyNames`, `contains`, next to a `default`); assert the joined output of all four backends contains neither token. *(I ran exactly this probe manually: it currently passes — the test is missing, not the behavior.)*

### 3. `deprecated` on a named `oneOf` union is untested in every language
- **Severity** P1
- **Untested** That the marker reaches a union alias/interface — which is why divergence #5 (Python drops it) survives.
- **Spec line** `deprecated.md:120` (`$defs` type → the generated type).
- **Where** `tests/generate_oneof.rs`, plus the four language suites.
- **Suggested case** `$defs: { U: { title: T, description: D, deprecated: true, oneOf: [{type: string},{type: integer}] } }`; assert Go `// Deprecated: This type is deprecated.`, TS `@deprecated`, Java `@Deprecated`, Python `typing_extensions.deprecated("This type is deprecated.", category=None)` and the title in all four.

### 4. No test that Python's `deprecated` marker has no runtime effect
- **Severity** P1
- **Untested** That instantiating a deprecated type or reading a deprecated field raises no `DeprecationWarning`.
- **Spec line** `deprecated.md:216-218` — "The Python snapshot asserts the `category=None` form, i.e. that accessing a deprecated field or instantiating a deprecated type raises **no** `DeprecationWarning`"; `deprecated.md:74-80` (parity with the other three, P1).
- **Where** `samples/python/tests/test_showcase.py`.
- **Suggested case** `with warnings.catch_warnings(): warnings.simplefilter("error"); Showcase(..., legacy_id_py="x").legacy_id_py`. Related: **no round-trip test in any language touches the deprecated `legacyId` member at all** (grepping `samples/*/tests` for `legacy_id_py|legacyIdTs|LegacyIdGo|legacyIdJava` returns nothing), so the "deprecated is wire-inert" claim (`deprecated.md:177-181`) is unexercised.

### 5. Non-string `title` / `description` are only tested in the one position that rejects
- **Severity** P1
- **Untested** `title: 42` / `description: 42` on a `$defs` entry, a nested property, `items`, or an envelope service/operation.
- **Spec line** `title.md:192`, `description.md:198`.
- **Where** `src/parser/json_schema.rs` inline tests, next to `rejects_non_string_title` (`:11894`).
- **Suggested case** A `structural_reject`-style helper that plants the keyword under `$defs.<X>.properties.<y>`, plus a `doc_reject` case for `services.Svc.description: 42` and `…operations.run.description: 7`.

### 6. Go §1 is not asserted for service/operation/client declarations
- **Severity** P1
- **Untested** That an authored service/operation description produces a *name-led* Go comment.
- **Spec line** `PRINCIPLES.md` Go §1.
- **Where** `tests/generate_go.rs` (near `go_json_emits_deprecated_services_and_operations`, `:2391`) or `tests/doc_rendering.rs`.
- **Suggested case** Assert `rendered.contains("// ChatService Send messages and look up rooms.")` and, more generally, add a lint-style assertion that every generated Go line matching `^(type|func|var|const) [A-Z]` / `^\t[A-Z]\w* ` in a struct body is preceded by a `//` line starting with that identifier. That single assertion would catch findings #3, #6 and #7 at once.

### 7. Go stutter guard has no test
- **Severity** P2
- **Untested** Both arms: the intended suppression (`Email` + `"Email address"`) and the over-trigger (`Id` + `"Identifier …"`).
- **Spec line** `title.md:143-147`, `description.md:152-154`.
- **Where** `tests/generate_go.rs`.
- **Suggested case** A schema with `email: {title: "Email address"}`, `id: {title: "Identifier of the room"}`, `name: {description: "Names the thing."}`; assert `// Email address`, `// Id Identifier of the room`, `// Name Names the thing.`.

### 8. `title`/`description` last-wins under `allOf` and `$ref`-siblings is untested
- **Severity** P2
- **Untested** The matrix rows `allOf:[{title:"A"},{title:"B"}] → "B"` and `{$ref: …, description: "Use-site note."}` → use-site wins.
- **Spec line** `title.md:186,185`; `description.md:191,190`. (`deprecated`'s OR merge *is* tested, `src/parser/json_schema.rs:9763-9786`.)
- **Where** `src/parser/json_schema.rs` inline tests, beside `all_of_merges_deprecated_with_or_and_discards_inert_annotations`.
- **Suggested case** `model_schema` assertions on the merged `title`/`description`, plus one generated-output assertion covering the `$ref`-sibling path (which would surface divergence #11).

### 9. `readOnly` / `writeOnly` value-shape and contradiction cases are untested
- **Severity** P2
- **Untested** `{readOnly: "true"}`, `{readOnly: 1}`, `{writeOnly: 0}`, and `{readOnly: true, writeOnly: true}`; also every nesting position other than a top-level property (`oneOf` branch, `allOf` branch, `contains`, `additionalProperties`, `items`, a second `$defs`).
- **Spec line** `readOnly.md:105-110`, `writeOnly.md:80-85`.
- **Where** `src/parser/json_schema.rs`, extending the table at `:9106-9107` and `rejects_read_only_false` at `:9130`.
- **Suggested case** A parameterized table over `(keyword, value, position)`; behavior is currently correct in all of them, so the tests lock it in (and would force the diagnostics of #9 to be differentiated if the assertions name the reason).

### 10. No non-ASCII doc-comment test, and no `Deprecated:`-in-prose injection test
- **Severity** P2
- **Untested** (a) UTF-8 prose (`café`, `日本語`, `🎉`) surviving all four emitters and their compilers/parsers — relevant because Java compiles with the platform default encoding unless `-encoding UTF-8` is set; (b) a `description` paragraph that *begins* with `Deprecated:`, which godoc/`staticcheck` will read as a real deprecation marker on a non-deprecated symbol.
- **Spec line** `description.md:162-170` (escaping so prose "cannot break out of the comment or the host language's lexer"); `deprecated.md:131` (Go's marker *is* just a doc paragraph).
- **Where** `tests/doc_rendering.rs`, extending `HOSTILE_DOCUMENTATION_SCHEMA`.
- **Suggested case** Add a UTF-8 property description and one reading `Deprecated: this is prose, not a marker.` as its own paragraph; the Java compile step already in that test would catch an encoding regression.

### 11. `deprecated: false` is not asserted to be output-inert
- **Severity** P2
- **Untested** That an explicit `deprecated: false` emits no marker in any language (only that it *loads*, `src/parser/json_schema.rs:9314-9316`).
- **Spec line** `deprecated.md:202` — "accepted, no marker, no diagnostic".
- **Where** `tests/generate_{go,typescript,python,java}.rs`.
- **Suggested case** A model with `a: {deprecated: false, type: string}` and nothing else deprecated; assert the rendered output contains no `Deprecated`/`@deprecated`/`@Deprecated`.

### 12. Java Javadoc placement (getter vs private field) is unasserted
- **Severity** P2
- **Untested** That the prose Javadoc lands where consumers can see it.
- **Spec line** `description.md:111`, `title.md:123`, `deprecated.md:145`.
- **Where** `tests/doc_rendering.rs::java_hostile_documentation_is_wrapped_escaped_and_compiles` — it asserts the field-level Javadoc text (`tests/doc_rendering.rs:273-276`) without checking which declaration it precedes, which is exactly why divergence #4 is invisible.
- **Suggested case** Assert the full slice `"     */\n    @Deprecated\n    public"` is preceded by the description body, i.e. that the getter's Javadoc contains the prose.

### 13. Markdown indentation preservation is untested
- **Severity** P2
- **Untested** That indented list items / fenced code blocks in a `description` survive.
- **Spec line** `description.md:60-64`, `description.md:156-160`.
- **Where** `tests/doc_rendering.rs`.
- **Suggested case** A description with a two-space-indented nested bullet; assert the indent survives in all four outputs (this currently fails — see divergence #8).

### 14. Escape-expansion is not counted against the 88-column budget in Python
- **Severity** P2
- **Untested** A line whose escaping (`\` → `\\`) pushes it past the format width.
- **Spec line** `description.md:156-160` (wrap to 88 columns).
- **Where** `tests/doc_rendering.rs::assert_hostile_comment_lines_fit`.
- **Code note** Java and TypeScript escape *before* wrapping (`src/generator/java.rs:385-389`, `src/generator/typescript.rs:4821`); Python escapes *after* (`src/generator/python.rs:6951/6970`), so only Python can overflow.
- **Suggested case** A description line of ~80 backslashes.

## Combination gaps

| Feature A × Feature B | Spec says | Tested? | Risk |
|---|---|---|---|
| `title` × `description` (both present) | summary, blank line, body (`description.md:127-128`) | Yes — `tests/doc_rendering.rs:152,178,237,268` all four langs | Low |
| `title` absent × `description` present (Go name-led) | identifier prefixed to the description's first line (`description.md:145-146`) | Partially — sample snapshots (`samples/go/showcase/showcase.go:2508`) but no targeted assertion | Low |
| neither × Go §1 fallback | synthesized name-led line (`description.md:147-151`) | Yes — sample snapshots (`samples/go/chat/chat.go:33,35`) | Low |
| `description` × `*/` (TS/Java block comment) | `*/` → `* /` (`description.md:168,170`) | Yes — `tests/doc_rendering.rs:178,187,268,279` incl. compile/typecheck | Low |
| `description` × `"""` / `\` (Python) | escape both (`description.md:169`) | Yes — `tests/doc_rendering.rs:238-240` | Low |
| `description` × **trailing `"`** (Python) | must not terminate the docstring | **No** | **P0 — generated Python does not parse** |
| `description` × `&`/`<`/`>` (Java HTML) | HTML-escape (`description.md:170`) | Yes — `tests/doc_rendering.rs:268,280` | Low |
| `description` × non-ASCII | verbatim pass-through (`description.md:60-64`) | **No** | Medium — Java default-encoding compile risk |
| `description` × leading `@` / `{@link}` | no rule; Javadoc/JSDoc will read it as a tag | No | Low (doc-tool only, not a compile error) |
| `description` × paragraph starting `Deprecated:` | Go's marker *is* a doc paragraph (`deprecated.md:131`) | **No** | Medium — false SA1019 on a non-deprecated symbol |
| `description` × Markdown indentation | verbatim (`description.md:60-64`) | **No** | Medium — currently reflowed in all four languages |
| `description` × long text / wrapping | 88 cols minus indent+prefix (`description.md:156-160`) | Yes — `tests/doc_rendering.rs:120-146` | Low |
| `title`/`description` × non-string value | reject (`title.md:192`, `description.md:198`) | **Root schema only** | **High — silent coercion elsewhere** |
| `title` × `allOf` last-wins | last-merged wins (`title.md:186`) | **No** (only `deprecated`'s OR is tested) | Medium |
| `description` × `$ref` sibling | use-site wins (`description.md:190`) | **No** at output level | Medium — lands on the hoisted type, member falls back |
| `deprecated` × `$defs` type | marker on the type (`deprecated.md:120`) | Yes — `tests/doc_rendering.rs:156,182,272`; Python via `src/generator/json_schema/python.rs:3206` | Low |
| `deprecated` × property | marker on field/getter (`deprecated.md:122`) | Yes — all four | Low |
| `deprecated` × service / operation | marker on interface/method (`deprecated.md:123`) | Yes — `tests/generate_go.rs:2433`, `tests/generate_python.rs:1669`, `tests/generate_java.rs:709`, `tests/generate_typescript.rs:2083` | Low |
| `deprecated` × named `oneOf` union | marker on the type | **No** | **P1 — Python emits nothing** |
| `deprecated` × synthesized closed-value type | property marks the *member* (`deprecated.md:122`) | **No** | Medium — Go also marks the type; others don't |
| `deprecated` × `allOf` merge (OR) | any true ⇒ deprecated (`deprecated.md:99-106`) | Yes — `src/parser/json_schema.rs:9763` | Low |
| `deprecated` × `$ref` sibling | OR through the rewrite (`deprecated.md:233-235`) | **No** | Low (shares the `allOf` code path) |
| `deprecated: false` × output | no marker, no diagnostic (`deprecated.md:202`) | Loader only (`src/parser/json_schema.rs:9314`) | Low |
| `deprecated` × `required` / `default` / `const` / nullability | orthogonal (`deprecated.md:108-110`) | Yes — showcase snapshot combines deprecated + `x-<lang>-name`; `tests/generate_python.rs:2145` combines deprecated + default | Low |
| `deprecated` × runtime warning (Python) | `category=None`, no `DeprecationWarning` (`deprecated.md:216-218`) | **No** | Medium — P1 parity claim unverified |
| `$comment` × emitted output | never present (`comment.md:101`) | **No** (loader only) | Medium — behavior verified manually, unlocked |
| `$comment` × `$ref` sibling / `allOf` | merged then dropped (`comment.md:66-68`) | Loader (`src/parser/json_schema.rs:9764`) | Low |
| `$comment` × envelope root | valid location, dropped (`comment.md:33`) | **No** | Low — currently rejects with a misleading message |
| `examples` × emitted output / malformed value | inert, array-MUST unenforced (`examples.md:103-104`) | Loader only (`src/parser/json_schema.rs:9311`) | Low |
| `readOnly`/`writeOnly` × nesting (`items`, `oneOf`, `allOf`, `contains`, `additionalProperties`, unreferenced `$defs`) | reject everywhere (`readOnly.md:82`) | **No** (only a top-level property) | Low — verified correct manually |
| `readOnly` × `writeOnly` both `true` | reject as contradictory (`readOnly.md:88-90`) | **No** | Low — rejects, but with the wrong reason |
| `readOnly`/`writeOnly` × non-boolean / `false` | distinct diagnostics (`readOnly.md:83-87`) | `false` only, pinned to the generic message (`src/parser/json_schema.rs:9130`) | Low |
| `title`/`description` on `items` / inline subschema | dropped when there is nowhere to attach (`description.md:102`) | **No** | Low — verified correct in all four |
| annotations × runtime validation | zero effect (`description.md:172-178`, `deprecated.md:176-188`) | Implicitly (no validator emitted) | Low |

## Verified-good

- **Escaping (except the Python trailing quote).** `*/` → `* /` in TS (`src/generator/typescript.rs:4821`) and Java (`src/generator/java.rs:389`); Java HTML-escapes `&`/`<`/`>` *before* wrapping so the width budget accounts for the expansion; Python escapes `\` and `"""` (`src/generator/python.rs:7013-7017`). `tests/doc_rendering.rs` doesn't just diff strings — it runs `gofmt`, `tsc --noEmit`, `ast.parse` and a real `javac` release-8 compile over the hostile output.
- **`$comment` and `examples` are truly inert.** Dropped at `src/parser/json_schema.rs:5104-5105` (normalize) and `:5190-5191` (`own_conjunct`, so an inert difference never becomes a merge conflict). I probed 12 authoring positions × 4 backends and found no leakage into any emitted artifact.
- **`readOnly`/`writeOnly` reject everywhere.** `src/parser/json_schema.rs:1609-1616` fires from a per-node structural walk; confirmed at `items`, nested `properties`, an unreferenced `$defs`, `additionalProperties`, `contains`, a `oneOf` branch, an `allOf` branch, and for `false`/non-boolean/both-true. Nothing reaches codegen.
- **Value-shape checks that do work.** Empty/whitespace `title` and `description`, multi-line `title`, non-boolean `deprecated`, non-string `$comment` all reject with fix-it-bearing messages (`src/parser/json_schema.rs:2358-2407`), at any nesting depth, and for envelope service/operation `description` too (`tests/generate_java.rs:753`).
- **Merge semantics.** `title`/`description` last-wins (`src/parser/json_schema.rs:5344-5349`, `:5579`) and `deprecated` ORs (`src/parser/json_schema.rs:5576-5578`) — `true` in one branch plus `false` in a later one still yields deprecated, as `deprecated.md:104-106` requires.
- **88-column wrapping in all four languages** (`GO_DOC_COMMENT_LINE_LENGTH`/`TYPESCRIPT_FORMAT_LINE_LENGTH`/`JAVA_FORMAT_LINE_LENGTH`/`PYTHON_FORMAT_LINE_LENGTH` are all `88`), with the comment prefix and indent subtracted, paragraphs preserved as blank comment lines, and per-source-line wrapping.
- **`title` never becomes an identifier** — no `schema.title` read anywhere in `src/planning/` or the name manifest; names come from the `$defs` key and the `properties` pipeline.
- **Assembly order** summary → body → tags, in all four languages, including the Go `Deprecated:` paragraph placed last.
- **Go §1 fallback lines** are synthesized and correct for types (`<Name> is generated from the corresponding JSON Schema definition.`), fields (`<Name> corresponds to the "<json>" JSON property.`), closed-value types and constants, and union wrappers — and a full scan of `samples/go` found every generated top-level exported declaration documented (the three runtime struct-field exceptions are listed in divergence #7).
- **TypeScript / Python / Java correctly emit *no* doc comment** when neither `title` nor `description` is authored (`samples/typescript/chat/models.ts:9-11`), per `description.md:134-136`.
- **`items`-level `title`/`description` are dropped** consistently in all four languages when there is no declaration to attach to.
- **The checked-in `samples/**` trees are golden snapshots asserted by `cargo test`** (`tests/generate_go.rs:813`, and the equivalents in the other three), so doc-comment output for the showcase/chat/kb/temporal schemas is regression-locked even where no hand-written assertion names it.
