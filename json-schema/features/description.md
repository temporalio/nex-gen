# `description`

Source: JSON Schema 2020-12, Validation vocabulary, §9.1
"A Vocabulary for Basic Meta-Data Annotations → title and description".

Free-form prose explaining the schema it sits on — the primary human
documentation for a generated type, member, service, or operation. In the
spec it is a **pure annotation** — it never affects validation, and it
never affects the emitted *type* or any *identifier*. Its single
operational role is to become the **body of the generated doc comment**
(a Go `//` block, a TS JSDoc, a Python docstring / Pydantic
`Field(description=…)`, a Java Javadoc). Because it is the primary doc
source, this spec **owns the shared doc-comment machinery** — assembly
order, line-wrapping, and per-language escaping — that its sibling
[[title]] defers to it.

## Spec summary

Verbatim (2020-12 validation, §9.1):

> The value of both of these keywords MUST be a string.

> Both of these keywords can be used to decorate a user interface with
> information about the data produced by this user interface. A "title"
> will preferably be short, whereas a description will provide
> explanation for the purpose of the instance described by this schema.

Distilled:
- An **annotation**, not an assertion: per spec it **never changes
  whether an instance validates**. A wire value is judged by the schema's
  assertions alone, `description` present or not.
- Its value **MUST be a string** (the spec's own MUST).
- **Prose, possibly multi-line** — the spec's own role for it
  ("explanation for the purpose"). Unlike [[title]] (a short label,
  restricted to a single line), a `description` may span paragraphs.
- Applies to **any subschema** — a `$defs` type, a property subschema, an
  array `items` subschema — and, on the Nexus envelope, to **services and
  operations** (see [[services]]).

## Support decision

**Support:** yes — as the **doc-comment body only**. It emits no
validator, no adapter behavior, and no type/field name.

The defining choices (citing [[PRINCIPLES.md]]):
- **Annotation, no runtime effect (P10/P12).** `description` contributes
  **no** constraint predicate to the shared `Validate` and **no**
  parse/encode adapter logic. It is inert at runtime in both directions;
  it touches only the *generated source text*.
- **Never an identifier (P13, P15, P1).** Like [[title]], `description`
  never names a type, field, service, or operation — names come from the
  `$defs` key, the [[properties]] resolved policy, and the [[services]]
  keys. Routing prose into the identifier namespace would make a cosmetic
  copy-edit a breaking rename (P13), multiply the collision surface (P15),
  and bypass the case-mapping pipeline that keeps names identical across
  languages (P1). (`description` is even less tempting than `title` here —
  no ecosystem generator names types from it — but the rule is the same.)
- **The doc-comment body (P2).** `description` is the prose body of the
  generated doc comment, rendered to read like a comment a human wrote
  (P2). It composes with [[title]] as **summary line + body** (assembly
  order below). Authors' Markdown is passed through **verbatim** (escaped
  only for the comment block); the generator does not render, reflow, or
  reinterpret it — what the author wrote is what lands in the source (see
  Ecosystem variance).

Loader behavior:
- `description` value **not a string** → **reject** (P7.1; and the spec's
  own MUST). `{description: 42}`, `{description: {...}}`.
- `description` an **empty or whitespace-only** string → **reject** as
  degenerate: it renders an empty doc body, which is dead metadata and
  signals author confusion (P7.1). Drop it, or give it text.
- `description` as a **sibling of `$ref`** is **not** rejected: a
  sibling-bearing `$ref` is rewritten to an implicit `allOf` and **merged**
  (see [[ref]]), so a use-site `description` folds into the merged schema
  under the [[allOf]] rule below — deduped against the target's own
  `description` when identical, and when they differ the use-site
  `description` **wins** (it is last in the rewrite; see [[ref]]). This is
  the idiomatic way to override a shared type's prose at one use site.
- Multiple `description`s applicable to one node after an [[allOf]] merge
  (P6), whether from an explicit `allOf` or a `$ref` sibling →
  **last-wins**: identical values dedup, and when they differ the
  **last-merged** `description` survives (see [[allOf]]). A differing
  description is a deterministic override, never a reject. Mirrors
  [[title]] and [[default]].

Unlike [[title]], a **multi-line** `description` is **accepted** — prose
is its purpose. Embedded blank lines are preserved as paragraph breaks
(see Doc-comment assembly).

## Type mapping

**None.** `description` does not change the emitted type and contributes
no identifier (rationale in Support decision). Its sole materialization is
the **body of the doc comment**, which carries no type information:

| Placement | Where the body lands |
|---|---|
| `$defs` type | doc comment on the generated **type** (Go `type` / TS `interface` / Python class / Java class) |
| property subschema | doc comment on the generated **field/member** |
| service (envelope) | doc comment on the generated **service interface**; see [[services]] |
| operation (envelope) | doc comment on the generated **operation method**; see [[services]] |
| `items` / other inline subschema | doc comment on the nearest generated declaration, if any; otherwise dropped (nowhere to attach) |

Per-language block and placement:

| Language | Doc-comment mechanism |
|---|---|
| Go | `// ` line-comment block above the `type`/field/method, **name-led first line** (see below). |
| TypeScript | `/** … */` JSDoc above the `interface`/field. |
| Python | class **docstring** for a type/service; for a **field**, the native Pydantic `Field(description="…")` argument. |
| Java | `/** … */` Javadoc above the class/getter/method. |

No new identifier is ever synthesized, so `description` has **no P15
collision surface**.

## Doc-comment assembly (shared machinery)

This is the mechanism [[title]] defers to. A generated doc comment is
assembled from up to three parts, in order:

1. **Summary line** — the [[title]], if present.
2. **Body** — the `description`, if present (may be multiple paragraphs).
3. **Tags** — generator-emitted trailers (e.g. an experimental/deprecated
   warning), if any.

Layout rules:
- With **both** a title and a description: summary line, one blank
  comment line, then the body.
- With **only** a description: the body opens the comment (its first line
  becomes the summary in doc tools that treat the first sentence as one —
  Javadoc/JSDoc/PEP 257).
- With **only** a title: just the summary line (see [[title]]).
- **Paragraphs** in the body (blank lines in the source string) are
  preserved as blank comment lines.

**Go — the name-led first line.** Idiomatic Go doc comments begin with the
identifier being described (`godoc`/`golint`: `// User is …`). The
name-led rule governs whatever text **opens** the Go comment:
- title present → the summary line is `// <Name> <title>` (see [[title]]).
- title absent → the identifier is prefixed to the first line of the
  description: `// <Name> <first line of description>`.
- **Stutter guard:** if that opening text already begins with `<Name>`
  (case-insensitively), the identifier is not doubled — the text is
  emitted as-is. Same guard as [[title]].

**Wrapping.** Each part is word-wrapped to the language's format line
length (88 columns) minus the current indent and the comment prefix width
(`// `, ` * `, docstring indent). Wrapping is per source line, so an
author's explicit line breaks and paragraph breaks are kept while long
lines reflow to fit.

**Escaping.** Each part is escaped so the prose cannot break out of the
comment or the host language's lexer:

| Language | Escaping |
|---|---|
| Go | `//` line comments have no terminator to escape; each physical line is prefixed `// `. |
| TypeScript | replace `*/` → `* /` so the body can't close the JSDoc block early. |
| Python | escape `\` → `\\` and `"""` → `\"\"\"` so the body can't terminate the docstring. |
| Java | replace `*/` → `* /`; HTML-escape `&`→`&amp;`, `<`→`&lt;`, `>`→`&gt;` (a Javadoc body is HTML). |

## Validator mapping

`description` emits **no validator** and **no adapter behavior**. It is an
annotation (§9.1): it never appears in the shared `Validate`, never runs in
the parse or encode adapter, and never causes a runtime pass/fail in
either direction. Its entire effect is at generation time, in the emitted
doc comment.

## Property-testing matrix

### Accepted (positive)

| Shape | Example |
|---|---|
| Description on a `$defs` type | `{description:"A registered user account.", type:"object", …}` |
| Description on a property | `properties:{email:{description:"Contact address; used for password resets.", type:"string"}}` |
| Title + description | `{title:"User", description:"A registered user account.", type:"object"}` → summary + body |
| Multi-paragraph description | a `description` with an embedded blank line → two comment paragraphs |
| Description sibling of a `$ref` (merged, last-wins) | `{$ref:"#/$defs/User", description:"Use-site note."}` → the use-site `description` overrides the target's when they differ (see [[ref]]) |
| Differing merged descriptions (last-wins) | `allOf:[{description:"A"},{description:"B"}]` → `"B"` (see [[allOf]]) |
| Description on a service/operation | envelope `services.ChatService.description` → service interface doc comment (see [[services]]) |

### Rejected at load time (negative)

| Reason | Example |
|---|---|
| Non-string value | `{description:42}`, `{description:{...}}` |
| Empty / whitespace-only | `{description:""}`, `{description:"   "}` |

### Runtime fixtures

None. `description` has no runtime behavior — it neither validates nor
(de)serializes. Its only observable output is the generated doc comment,
covered by generation-snapshot tests, not runtime fixtures.

## Interactions

- **[[title]]**: the sibling annotation. `title` is the summary line;
  `description` is the body. This spec owns the shared assembly, wrapping,
  and escaping; [[title]] fills the summary slot.
- **[[properties]]**: owns the field/type naming policy that `description`
  does not participate in. A property's `description` decorates the
  generated member's doc comment; it never renames the member.
- **[[ref]]**: a `description` **sibling** of a `$ref` is not rejected —
  the `$ref` and its siblings are rewritten to an implicit [[allOf]] and
  merged (see [[ref]]); because the rewrite puts the siblings last, a
  use-site `description` **overrides** the referenced target's under the
  last-wins rule below.
- **[[allOf]]**: merges (P6) — explicit or via a `$ref` sibling — can
  bring multiple `description`s onto one node; identical values dedup,
  differing ones resolve **last-wins** (see [[allOf]]), never a reject.
- **[[services]]**: the Nexus envelope decorates services and operations
  with `description` (the envelope's recognized doc member — it uses
  `description`, not `title`); each becomes the doc comment on the
  generated service interface / operation method.
- **[[type]]**, **[[nullability]]**, **[[required]]**: fully orthogonal —
  `description` changes neither the type, its nullability, nor its
  presence.

## Ecosystem variance

| Source dialect | Action |
|---|---|
| JSON Schema 2020-12 | Native annotation (§9.1). Rendered as the doc-comment body. |
| OpenAPI 3.1 | Adopts 2020-12 — `description` native; CommonMark per OAS. |
| OpenAPI 3.0 | `description` present; CommonMark per OAS. |
| Swagger 2.0 / draft-4 | `description` present; same annotation semantics. |

`description` is universal across dialects and needs no rewrite, and —
unlike [[title]] — no generator repurposes it as an identifier, so there
is no naming footgun to guard against.

The one convention to note is **Markdown**: OpenAPI declares `description`
to be CommonMark. We **pass the text through verbatim** into the doc
comment (escaped only for the comment block), and do **not** render it to
another format, reflow it, or strip the markup. This is the P2 choice —
the author's prose lands in the generated source exactly as written, which
is what a hand-written comment would contain; downstream doc tooling
(godoc, TypeDoc, Sphinx, Javadoc) interprets whatever markup it supports.

## See also

- [[title]] — the sibling annotation; supplies the summary line that opens
  the doc comment whose body is `description`.
- [[services]] — the Nexus envelope's service/operation `description`s
  become the doc comments on the generated interface and methods.
- [[properties]] — owns the naming policy `description` does not touch.
- [[ref]] — a `description` sibling of a `$ref` merges via the
  implicit-`allOf` rewrite; use-site value wins (last-wins).
- [[default]] — the other basic-metadata annotation (§9.2); unlike
  `description` it carries operational semantics, whereas `description` is
  purely a doc comment.
