# Generated file layout (cross-cutting design note)

Not a JSON Schema keyword. Specifies how a set of input schema files
becomes generated source in each language: the package structure, file
names, where shared runtime lives, and how reference cycles and name
collisions are handled at the file level. Driven by **P14** (one module
per input file; merge recursion, not files) and local-file-only external
refs; the reference semantics that feed it live in [[ref]].

## Output mirrors the input directory tree (Python / TypeScript / Java)

Each input schema file `<subpath>/<name>.<ext>` becomes a **per-input
module directory** `<subpath>/<name>/` under the output package root,
preserving the input directory structure verbatim — **directories are not
flattened**. Inside each per-input directory:

- `models.<lang>` — the domain types declared in that file (its root type
  and every `$defs`).
- `services.<lang>` — the Nexus service bindings declared in that file,
  emitted **only if it declares any** (see [[services]]).

Schema-independent runtime — `ValidationError`/`Violation`, the
spec-number helpers, the (de)serialize scaffolding — is defined **once**
at the package root (the shared `definitions` file, below), never
duplicated per module.

Nesting works because each of these languages maps an input directory onto
its native module unit:

- **Python** — a subpackage directory carrying an `__init__.py`.
- **TypeScript** — a directory with an `index.ts` barrel.
- **Java** — a package mirroring the directory, one `.java` per public
  class.

Cross-directory mutual references are supported: Python hoists the cyclic
types into `_recursive.py` (Recursion, below), TypeScript relies on
cycle-safe `import type` and call-time live bindings, and Java on object
references. Because each input file's types live in their own module, type
names only need to be unique **within** that module (see Collisions and
[[ref]]).

## Go flattens to one package

**Go is the exception.** Every input file in a generator run collapses
into **one flat package** — nested input directories are not mirrored. A
Go package cannot participate in an import cycle, so if two directories
became two packages a **cross-directory mutual reference** would be an
illegal cyclic import, and Go has no cross-package hoist like Python's
`_recursive`. Flattening to a single package makes every reference —
including a cross-directory cycle — a **within-package** reference, which
the Go compiler resolves natively. It is also what lets the
schema-independent boilerplate be defined **once** in that package instead
of per file.

Within the one package, each input file emits a `<module>.go` whose name
is its flattened path (Module paths, below), plus a shared `definitions.go`.
Because everything shares one namespace, type and module names cannot
collide across files — the loader validates this (see Collisions).

## The single-input special case

When the closure is **exactly one input file**, there is no directory tree
to mirror: the file's output lands **directly at the package root** rather
than in a per-input subdirectory. A single `chat.yaml` → package `chat/`
holding `models.py`, `services.py`, the shared `definitions.py`, and the
`__init__.py` aggregator side by side. No per-input subdirectory, and no
`_recursive` (a cross-file cycle is impossible with one file). The
models/services split and the shared-runtime file are still present in
every language, Go included: Go always emits the one input's `<package>.go`
alongside its own `definitions.go`, the same shared-runtime layout it uses
for multi-input closures.

## Files per language

### Multi-input (≥2 input files in the closure)

Per input file `<subpath>/<name>`:

| Language | Per-input module | Shared runtime (once) | Recursive module | Aggregator |
|---|---|---|---|---|
| **Python** | `<subpath>/<name>/models.py` (+ `services.py` if it declares services) | `definitions.py` (package root) | `_recursive.py` (package root) | `__init__.py` per directory — the per-input directory, every intermediate directory, and the package root |
| **TypeScript** | `<subpath>/<name>/models.ts` (+ `services.ts`) | `definitions.ts` (package root) | — | `index.ts` per directory (barrels chain upward) |
| **Go** | `<module>.go` in the one flat package (`<module>` = flattened path) | `definitions.go` (same package) | — | — (capitalized = exported) |
| **Java** | one `<ClassName>.java` per exported class, in a package mirroring `<subpath>/<name>/` | each runtime class its own file in the root package (`ValidationException.java`, `Violation.java`, `SpecNumbers.java`, …) | — | — (`public` = exported) |

**`_recursive` is Python-only and is a single file at the package root**
(`<package>/_recursive.py`), **never** per-input. It holds every hoisted
cross-file SCC in the whole closure. See Recursion below.

### Single-input (exactly one input file — no cross-file refs possible)

All output lands at the package root (no per-input subdirectory, no
`_recursive`):

| Language | Output |
|---|---|
| **Python** | `models.py` (+ `services.py`), the shared `definitions.py`, and the `__init__.py` aggregator — the same split as multi-input, flattened to the package root |
| **TypeScript** | `models.ts` (+ `services.ts`), `definitions.ts`, `index.ts` |
| **Go** | one `<package>.go` (types and services) + the shared `definitions.go` |
| **Java** | one `.java` per public class + the runtime classes; nothing to aggregate |

## The shared `definitions` file

Holds the schema-independent runtime, defined once per package (`definitions.py`
/ `definitions.ts` / `definitions.go`; Java splits it into one class file each). For
Python/TypeScript/Java it sits at the package root; for Go it sits in the
one flat package, always as its own `definitions.go` file regardless of how
many input files that package aggregates.

- Error types — a **single aggregating error holding a list of
  `Violation { path, reason }`**, identical in spirit across all four
  targets: Python the Pydantic aggregation machinery (`pydantic.ValidationError`);
  Go a `ValidationError` struct implementing `error` over `[]Violation`
  (its `Error()` surfaces every violation — *not* `errors.Join`); TS a
  `ValidationError` class extending `Error` over `Violation[]` (*not* a
  built-in `AggregateError`); Java `ValidationException extends
  JsonMappingException` holding `List<Violation>`. One error type, every
  violation surfaced in one shot (P11).
- Spec-number helpers — `parseSpecInteger` (Go), `SpecInt` /
  `_parse_spec_integer` (Python), `SpecNumbers.specLong` (Java), TS's
  safe-integer check.
- Shared (de)serialize scaffolding — the **P12** three-layer base, the
  Python optional-non-nullable `model_validator` helper. Java's
  collecting (de)serializer stays per-class, but the shared `Violation` /
  `ValidationException` / `SpecNumbers` classes live here.

## Module paths

The **input root** is the absolute path of the directory common to all
resolved input files (their longest common-ancestor directory). `$ref`
paths are resolved to absolute paths first — `..` segments are normalized,
not rejected ([[ref]]) — so a ref that walks upward simply raises the
common root.

**Python / TypeScript / Java** preserve each input file's directory path
**relative to the input root** verbatim under the package root, with the
filename (minus extension) becoming the leaf per-input directory:

```
input root = /abs/schemas   (common ancestor of all resolved inputs)

/abs/schemas/kb.yaml               -> kb/            (models.py, services.py)
/abs/schemas/content/block.json    -> content/block/ (models.py)
/abs/schemas/tree/category.json    -> tree/category/ (models.py)
```

Because the directory structure is mirrored rather than collapsed, there
is no delimiter to escape and no path-encoding scheme: literal
underscores, nested directories, and repeated segments all survive
unchanged.

**Go** flattens each file's path (relative to the input root, minus
extension) into a single `<module>.go` name, replacing the directory
delimiter with `_` and **preserving literal underscores verbatim** — no
escaping. Because module names are relative to the common ancestor, they
never contain `..`:

```
/abs/schemas/full_name.json       -> full_name.go
/abs/schemas/a/b/user.json        -> a_b_user.go
/abs/schemas/billing/invoice.json -> billing_invoice.go
```

Type names are directory-independent (basename-of-root or `$defs` name —
see [[ref]]), so they do not encode the path.

### Why Go does not escape

Identifiers are `[A-Za-z0-9_]`: two structural things (the directory
separator and a literal `_`) must encode into one non-alphanumeric
character. Injective + flat + underscores-preserved is impossible without
escaping. We choose **readable + collision-reject** over an escaping
scheme: keep names clean and reject the rare collision, consistent with
the **P15** "reject loudly, never mangle, offer an override" stance used
everywhere else. Go's flattened `.go` files are internal organization
anyway — consumers import types from the package by name — so a rare
reject costs little.

## Collisions

**Go** — one unified namespace per package holds the **reserved generated
names** (currently `definitions`) plus one entry per input module. Any collision
in that namespace → **load reject** with a fix-it (`x-output-module`
override or rename):

- two inputs flattening to the same module (`full/name` vs `full_name`);
- an input flattening onto a reserved generated name (a root-level
  `definitions.json`);
- (type-name collisions are handled the same way — see [[ref]]);
- a generated **service binding** colliding with a model (or synthesized
  I/O) type — service `ChatService` against a `$defs/ChatService`; see
  [[services]], which shares this one namespace.

**Python / TypeScript / Java** — nesting keeps distinct input files in
distinct modules, so files no longer contend for one flat name. What
remains is a small set of **reserved generated names** per scope; an input
file or directory that maps onto one → load reject with the same fix-it:

- at the **package root**: the shared `definitions` runtime module, `_recursive`
  (Python), and the root aggregator (`__init__` / `index`);
- within a **per-input directory**: `models`, `services`, and that
  directory's own aggregator.

Within a single module's exported namespace the type/service/synthesized-name
collision surface is unchanged (service `ChatService` in `services.py`
against a `$defs/ChatService` in `models.py` share the per-input
directory's one exported namespace — see [[services]]; synthesized names
per [[ref]]).

Like [[properties]], collisions are evaluated **per emitted target only** —
normalization differs per language.

## Recursion: hoist types, not files

This is the file-level realization of **P14**. "Merge on cycle" does
**not** mean merge whole input files — it means hoist only the cyclic
types:

- Build the reference graph and its strongly-connected components
  ([[ref]]). An SCC spanning **≥2 input files** is a cross-file cycle.
- **Python**: the cross-file SCC moves wholesale into `_recursive.py` at
  the package root, where it becomes a within-module cycle (topological
  order + a string forward-ref back-edge + one `model_rebuild()`). It
  imports the leaf, non-cyclic types it needs from the per-input modules;
  those modules and the aggregators import the finished classes back from
  `_recursive.py`, which imports nothing back from them — so the
  cross-module import cycle is gone. A cycle **within** a single file
  stays in its module.
- **TypeScript**: no recursive file. Type references erase
  (`import type` is always cycle-safe) and validator-function imports are
  ESM live bindings resolved at call time, not module-init; generated
  const values are self-contained leaf literals, so there is no
  init-order hazard. Cyclic types stay in their per-input modules.
- **Go**: no recursive file — the single flat package makes every cycle
  (same-file or cross-directory) a within-package reference, which Go
  resolves natively. This is exactly why Go flattens (above).
- **Java**: object references handle cycles natively across packages. No
  recursive file.

`model_rebuild()` is a **cycle** concern, not a same-module concern:
acyclic references emit in topological order with concrete annotations
and need no rebuild.

## Exports / visibility

*Public* = every top-level named type (file roots + all `$defs`,
including dead ones; anonymous types stay nested) plus each file's service
bindings:

- **Python** — each per-input `__init__.py` re-exports its module's public
  types (and services) via `__all__`; each intermediate directory's
  `__init__.py` re-exports its children; the package-root `__init__.py`
  re-exports the whole tree, pulling hoisted types from `_recursive`. The
  shared runtime (`ValidationError`, etc.) is **not** surfaced through the
  aggregators — consumers import it directly from `definitions`.
- **TypeScript** — `index.ts` per directory: per-input barrels
  `export … from './models'` (and `./services`), intermediate barrels
  `export * from './<child>'`, and the root barrel re-exports the tree.
  `ValidationError` is likewise imported directly from `./definitions`, not
  re-exported.
- **Go** — no aggregator; capitalized identifiers are exported from the one
  flat package.
- **Java** — `public` class per file; runtime classes public too.

## Service bindings

A Nexus service ([[services]]) emits into a **`services.<lang>` file in the
same per-input directory** as the models declared in that file (Python/TS),
sharing that directory's one exported namespace and the P15 collision pass
with the file's types and the synthesized operation `<Op>Input`/`<Op>Output`
types. **Go** emits the service into its flat package alongside the models
(Go has no models/services split). **Java** emits each service as its own
`<Service>.java`, like each model class. Aggregators (`index.ts` /
`__init__.py`) re-export services alongside models.

## See also

- [[input-files]] — per-file document modes (Nexus document vs pure JSON
  Schema) and root rules decided before the input-set/module computation
  here.
- [[ref]] — reference semantics, type-name derivation, recursion graph +
  satisfiability, bare-`$ref`-root alias.
- [[services]] — Nexus service/operation bindings placed by these rules.
- [[properties]] — the shared identifier/collision algorithm.
- [[nullability]] — optional/nullable wrapping (a source of cycle
  termination).
- [[PRINCIPLES.md]] — **P14** (one module per input file; merge recursion,
  not files), local-file-only external refs, **P15** (one identifier
  namespace per scope).
