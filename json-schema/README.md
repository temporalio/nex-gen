# nex-gen — JSON Schema → Nexus code generator

`nex-gen` generates idiomatic, statically-typed model code **and runtime
validators** for Go, Java, Python, and TypeScript from
[JSON Schema 2020-12](https://json-schema.org/draft/2020-12/json-schema-validation)
input, plus optional Nexus **service bindings**. Output feeds the Temporal
Nexus API SDK ecosystem.

> ⚠️ **UNDER DEVELOPMENT**
>
> This generator is a work in progress and has **not** released a stable
> version. The supported schema subset, command-line options, and emitted
> code may all change in incompatible ways until it is marked stable. The
> samples in this repository are illustrative of the intended output, not a
> compatibility promise.

## What it does

Unlike a plain "schema → struct" converter, `nex-gen` emits a **typed model
plus a shared validator** for every type. The same constraints are enforced
on the way in (deserialize) and on the way out (serialize), and violations
aggregate into one language-native error that a Nexus handler can map onto a
`BAD_REQUEST`. The generator implements a **strict subset** of JSON Schema:
anything ambiguous or not cleanly lowerable to all four languages is
**rejected at generation time** with a fix-it-style diagnostic rather than
producing silently-incorrect code.

## Installation

While under development, `nex-gen` is distributed as a single prebuilt
binary attached to each [GitHub release](https://github.com/temporalio/nexus-api-gen/releases).
Download the archive for your platform, extract it, and put `nex-gen` on
your `PATH`:

```bash
# macOS / Linux — adjust VERSION and the platform suffix to taste.
VERSION=0.1.0
curl -fsSL -o nex-gen.tar.gz \
  "https://github.com/temporalio/nexus-api-gen/releases/download/v${VERSION}/nex-gen-${VERSION}-$(uname -s)-$(uname -m).tar.gz"
tar -xzf nex-gen.tar.gz
sudo mv nex-gen /usr/local/bin/
nex-gen --help
```

There is no package-manager distribution yet; releases are the only
supported install channel for now.

## Usage

Generate TypeScript from a definition file:

```bash
nex-gen --lang ts --out-dir ./generated ./chat.nexusrpc.yaml
```

Generate all four languages from the same input by running once per target:

```bash
nex-gen --lang go   --out-dir ./gen/go     ./chat.nexusrpc.yaml
nex-gen --lang java --out-dir ./gen/java   ./chat.nexusrpc.yaml
nex-gen --lang py   --out-dir ./gen/python ./chat.nexusrpc.yaml
nex-gen --lang ts   --out-dir ./gen/ts     ./chat.nexusrpc.yaml
```

See [samples](samples) for what the output looks like in each language for a
single, feature-diverse input.

<!-- BEGIN GENERATED HELP -->
```
Synopsis

  $ nex-gen --lang LANG [--out-dir DIR | --out-file FILE] SCHEMA_FILE|DIR ...

  LANG ... go|java|py|ts

Description

  Generate model code and validators from JSON Schema 2020-12 definition
  files, including Nexus service bindings declared in a Nexus document.

Options

 -h, --help          Display help.
 --lang string       The target language: go | java | py | ts.
 --out-dir string    Output directory. Mutually exclusive with --out-file.
 --out-file string   Output file (single-file targets only). Mutually
                     exclusive with --out-dir.
 --dry-run           Print every file that would be written to stdout
                     instead of writing it.
```

Some languages have additional options. Run `nex-gen --help` for the full
list.
<!-- END GENERATED HELP -->

`cs` (C#) is **not yet a supported target.**

## How it works — the flow

`nex-gen` is two stages: a **language-agnostic loader** that runs once and
produces a shared type model, and a **per-language generator** that emits
that model. Everything that decides whether an input is *legal* happens in
the loader, so all four targets accept exactly the same schemas.

```
  one or more JSON Schema files (2020-12)
                 │
   ┌─────────────▼──────────────────────────────────────────────┐
   │ LOADER  (runs once, language-agnostic)                      │
   │                                                             │
   │  1. Parse            decide per-file mode from the root:    │
   │                      Nexus document (has `nexusrpc`) vs     │
   │                      pure JSON Schema; check root rules.    │
   │  2. Resolve $ref     local files & named ($defs) targets    │
   │                      only; normalize paths; compute the     │
   │                      input set + common input root.         │
   │  3. Strict-subset    reject unsupported keywords/shapes     │
   │     gate             with located, fix-it diagnostics.      │
   │  4. Reference graph   find cycles; reject unsatisfiable     │
   │                      ones (no terminating edge).            │
   │  5. Identifier pass  case-map JSON names to idiomatic       │
   │                      identifiers; reject collisions.        │
   └─────────────┬───────────────────────────────────────────────┘
                 │
          shared type model (IR)
        shape · optional vs nullable · constraints · const/default
                 │
   ┌─────────────▼───────────────┐
   │ GENERATOR (per target)      │
   │   Go   Java   Python   TS   │
   └─────────────┬───────────────┘
                 │
   package per language (nested tree mirroring inputs; Go flattens):
   models + one shared validator core + (for ≥2 inputs) an aggregator
```

1. **Parse.** Each input file is read in exactly one of two modes,
   decided *before* any schema processing by the presence of a root
   `nexusrpc` property. A **Nexus document** (`nexusrpc: "1.0.0"`) is an
   envelope whose root carries `services` and `$defs` but is *not itself a
   type*; a **pure JSON Schema** file's root *is* a type. The
   `nexusrpc` / `$schema` root rules are checked here. See
   [input-files.md](input-files.md).
2. **Resolve `$ref`.** References resolve to **local files and named
   targets only** (`#/$defs/Name`, `other.json#/$defs/Name`) — no HTTP, no
   `$id` re-basing, no pointers into the middle of a schema. Paths are
   normalized and the transitive closure of reachable files becomes the
   *input set*; their common-ancestor directory is the *input root*. See
   [features/ref.md](features/ref.md).
3. **Strict-subset gate.** Anything outside the supported subset is
   rejected here with a diagnostic that names the location and the fix —
   e.g. an array `type`, a missing `type`, a bare `{type: object}` with no
   shape, an `anyOf`/`not`, or an unmergeable `allOf` (contradictory
   branches). (A mergeable `allOf` — and the `$ref`-with-siblings sugar —
   is instead flattened into one schema here.)
   This is the core principle: **reject loudly, never emit something
   subtly wrong.**
4. **Reference graph.** A recursion cycle is kept only if it can
   terminate (an optional, nullable, or collection-wrapped edge);
   an otherwise-unsatisfiable cycle is rejected.
5. **Identifier pass.** JSON member, type, service, and operation names
   are mapped to each language's idiomatic identifier (Go `PascalCase`,
   TS/Java `camelCase`, Python `snake_case`) by one shared algorithm; the
   original JSON name is always pinned on the wire. Two names that collide
   after mapping are rejected (with an `x-<lang>-name` override as the
   escape hatch) rather than silently mangled.

The **type model** records each field's shape, whether it is *optional*
(may be absent) vs *nullable* (may be `null`) — two distinct axes — plus
constraints, `const`, and `default`. The generator lowers that model to one
**package per language** — a nested tree mirroring the input directories,
except Go which flattens every input into a single package (its packages
cannot form import cycles): the model types, a single shared validator
core (error types + spec-number helpers + the (de)serialize scaffolding),
and — when more than one input file is involved — an aggregator that
re-exports everything.

For the full design, see [pipeline.md](pipeline.md),
[generated-file-layout.md](generated-file-layout.md), and the per-keyword
specs under [features/](features).

### Runtime validators

Every generated (de)serializer is three layers wrapped around **one shared
`Validate(model)`** that is identical in both directions:

```
 wire JSON ─▶ parse adapter ─▶┐                  ┌─▶ encode adapter ─▶ wire JSON
   (spec-number parse,        │  shared          │   (omit vs emit-null,
    null rules, absence→       ├▶ Validate(model)─┤    default omission)
    required, unknown keys)    │  constraints,    │
                               └  const, counts  ─┘
                                       │
                                       └─▶ violations ─▶ aggregated error ─▶ BAD_REQUEST
```

The parse adapter classifies the wire bytes (spec-compliant integer
parsing, explicit-`null` handling, presence checks); the shared `Validate`
runs the constraint predicates over the decoded model; the encode adapter
decides per field whether an empty value is omitted or emitted as `null`.
A serialize fails *before* a byte is written if the in-memory model is
invalid.

## Definition files

Definition files are YAML or JSON. There are two flavors, chosen by the
root:

### Pure JSON Schema

No `nexusrpc` marker — the file is an ordinary type schema whose root *is*
a type (named from the file basename), exactly as JSON Schema 2020-12
describes. Reusable types live under `$defs` and are referenced with
`$ref`.

```yaml
$schema: https://json-schema.org/draft/2020-12/schema
type: object
additionalProperties: false
properties:
  userId: { type: string }
  email:
    oneOf:                       # nullable: optional & may be null
      - { type: string }
      - { type: "null" }
required: [userId]
```

### Nexus document

Opt in with a root `nexusrpc: "1.0.0"` marker. This — and **only** this —
enables the `services` extension. The root becomes an *envelope*: its
recognized members are `nexusrpc`, `services`, `$defs`, and optionally
`$schema` / `description`. It is not itself a type; put types in `$defs`.

```yaml
nexusrpc: "1.0.0"
services:
  ChatService:
    fqn: example.chat.v1.ChatService   # optional wire name; defaults to the key
    description: Send and receive chat messages.
    operations:
      sendMessage:
        description: Post a message to a room.
        input:  { $ref: '#/$defs/SendMessageInput' }
        output: { $ref: '#/$defs/SendMessageOutput' }
$defs:
  SendMessageInput:
    type: object
    additionalProperties: false
    properties:
      roomId: { type: string }
      message: { type: string }
    required: [roomId, message]
  SendMessageOutput:
    type: object
    additionalProperties: false
    properties:
      messageId: { type: string }
    required: [messageId]
```

A full, feature-diverse Nexus document is at
[samples/chat.nexusrpc.yaml](samples/chat.nexusrpc.yaml).

### Operation input/output types

Operation `input` and `output` are each optional and, when present, **must
be an object type** — a `$ref` to an object `$defs` entry, or an inline
object that is promoted to a synthesized `<Operation>Input` /
`<Operation>Output` type. Object-only is deliberate: only an object can
gain a new optional field later without breaking existing peers
(forward compatibility). Wrap a scalar in a single-field object if you need
one. An omitted side means *no* I/O (Go `nexus.NoValue` / TS `void` /
Python `None` / a Java `void` return or no-arg method).

### The supported subset

The supported keywords (and the ones deliberately rejected) are documented
per keyword under [features/](features). Highlights of the current subset (WIP):

| Area | Supported | Notes |
|---|---|---|
| `type` | single string only | array `type` and missing `type` are rejected |
| `properties` / `required` | yes | typed structs; presence enforced at runtime |
| `additionalProperties` | yes | structs are **open by default** (extras preserved); `false` closes them; a typed value makes a typed map |
| `type: array` / `items` | yes | homogeneous lists (`[]T` / `T[]` / `list[T]` / `List<T>`); `items` is required; tuples (`prefixItems`) are rejected |
| nullability | `oneOf: [{T}, {null}]` | the array form `["T","null"]` and OAS 3.0 `nullable` are rejected with a fix-it |
| `oneOf` | selector-separable unions | closed sum type (Go sealed interface, TS/Python native union, Java interface). Branches of disjoint JSON kinds separate by the wire token (mixed kinds OK, e.g. `object \| string`; a `null` branch makes it a nullable union); two+ object branches separate by a shared required `const`-tag (discriminated/tagged union). Only the OpenAPI `discriminator` object is deferred; `integer`+`number` overlap rejected |
| `allOf` | load-time merge/flatten | branches fold into one materialized schema (no combinator kept, no new type): same-axis numeric bounds **tighten**, `multipleOf` → LCM, value sets intersect, object/array subschemas merge recursively; unmergeable branches (disjoint `type`, disagreeing `const`, empty `enum`, distinct `pattern`/`format`/`contains`, a `false`/combinator branch) reject; `$ref` branches fold in (flatten, not subtype — the base-extension idiom) |
| `const` | scalar | the wire discriminator; emitted as the underlying primitive |
| `default` | scalar | off-the-wire, materialized on read; never echoed back |
| `$ref` / `$defs` | named, local-file targets only | no `$id`, no remote refs; siblings are the implicit-`allOf` sugar and are merged (see `allOf`) |
| count assertions | `minProperties` / `maxProperties`; `minItems` / `maxItems` | member counts over distinct wire keys; element counts over array size |
| `uniqueItems` | scalar elements | element distinctness for scalar `items`; composite (object/array) elements deferred; `false` is a no-op |
| `contains` | scalar matcher | existential "≥ 1 element matches" over scalar `items`; composite matchers/elements deferred |
| `minContains` / `maxContains` | yes | bound the number of `contains` matches; `minContains:0` relaxes the existential (needs `maxContains`) |
| `format` | curated subset | `uuid` / `ipv4` / `ipv6` / `hostname` / `email` / `uri` / `uri-reference` asserted as validated strings; `date-time` / `date` / `time` / `duration` **materialized** as native typed fields (offset & precision preserved, no truncation; `date-time` round-trip lossy only at a target type's limit — a bounded P1 exception; TS repr for all temporals via `--js-temporal-repr` = `string` (default) / `date` / `temporal`; string opt-out); `idn-*` / `iri*` / niche + unknown formats rejected (deferred) |
| `services` | yes | Nexus documents only |

### IDE support

A JSON Schema for the `*.nexusrpc.yaml` document format will ship alongside
the generator so editors can offer autocomplete and validation. (Wiring
instructions will be added once the schema file is published.)

## Samples

[samples/](samples) contains one feature-diverse input
([chat.nexusrpc.yaml](samples/chat.nexusrpc.yaml)) and its generated output
in all four languages. See [samples/README.md](samples/README.md) for a
guided tour of which schema feature produces which piece of code.
